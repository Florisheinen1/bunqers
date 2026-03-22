//! Rate-limited wrapper around [`Client`].
//!
//! Bunq imposes default rate limits of **3 GET requests per second** and **1
//! POST request per second** per device. Exceeding these limits results in a
//! HTTP 429 response.
//!
//! [`ClientRateLimited`] wraps a [`Client`] with two separate
//! [`RateLimiter`] instances — one for GET
//! requests and one for POST/PUT requests — sourced from the
//! [`ritlers`](https://crates.io/crates/ritlers) crate. Each method queues its
//! request through the appropriate limiter. If Bunq responds with 429, the
//! task is automatically re-queued as a **priority task** and retried without
//! any extra configuration. The `on_response` callback is only called once the
//! request succeeds.
//!
//! The `on_response` callback is **spawned onto a new Tokio task** so it does
//! not hold the rate-limiter slot while the callback is running. This keeps
//! throughput high even when callbacks perform slow operations.
//!
//! # Example
//!
//! ```rust,no_run
//! use std::{sync::Arc, time::Duration};
//! use bunqers::client_rate_limited::ClientRateLimited;
//! use ritlers::async_rt::RateLimiter;
//!
//! # #[tokio::main]
//! # async fn main() {
//! # let client: bunqers::client::Client = todo!();
//! let client_rl = Arc::new(ClientRateLimited {
//!     client,
//!     ratelimiter_get:  RateLimiter::new(3, Duration::from_secs(1)).unwrap(),
//!     ratelimiter_post: RateLimiter::new(1, Duration::from_secs(1)).unwrap(),
//! });
//!
//! client_rl.get_user_ratelimited(|response| async move {
//!     let user = response.into_result().expect("API error");
//!     println!("Hello, {}!", user.user_person.display_name);
//! }).await;
//! # }
//! ```

use std::{
	future::Future,
	pin::Pin,
	sync::{Arc, atomic::AtomicU32},
	time::Duration,
};

use ritlers::{TaskResult, async_rt::RateLimiter};
use rust_decimal::Decimal;

use crate::{client::Client, messenger::ApiResponse, types::*};

/// A type-erased, heap-allocated future that resolves to `()`.
///
/// Used internally to store callbacks without knowing their concrete type.
pub type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// A type-erased callback invoked with the successful API response.
type OnResponse<T> = Arc<dyn Fn(ApiResponse<T>) -> BoxFuture + Send + Sync>;

/// A type-erased closure that, when called, produces a future that fetches
/// data from the API. Called repeatedly on retry.
type FetchFn<T> =
	Arc<dyn Fn() -> Pin<Box<dyn Future<Output = ApiResponse<T>> + Send + 'static>> + Send + Sync>;

/// A [`Client`] with separate rate limiters for GET and POST/PUT requests.
///
/// Construct this directly and wrap it in an [`Arc`] to share across tasks:
///
/// ```rust,no_run
/// # use std::{sync::Arc, time::Duration};
/// # use bunqers::client_rate_limited::ClientRateLimited;
/// # use ritlers::async_rt::RateLimiter;
/// # let client: bunqers::client::Client = todo!();
/// let client_rl = Arc::new(ClientRateLimited {
///     client,
///     ratelimiter_get:  RateLimiter::new(3, Duration::from_secs(1)).unwrap(),
///     ratelimiter_post: RateLimiter::new(1, Duration::from_secs(1)).unwrap(),
/// });
/// ```
pub struct ClientRateLimited {
	pub client: Client,
	/// Rate limiter for read (GET) requests.
	pub ratelimiter_get: RateLimiter,
	/// Rate limiter for write (POST) requests.
	pub ratelimiter_post: RateLimiter,
	/// Rate limiter for write (PUT) requests.
	pub ratelimiter_put: RateLimiter,
	/// The maximum amount of retries a single task can have
	pub max_retries: u32,
}

/// Schedules a single API fetch through the given `ratelimiter`.
///
/// On a 429 response the task returns [`TaskResult::TryAgain`], which causes
/// `ritlers` to re-queue it as a priority task. On success, `on_response` is
/// spawned onto a new Tokio task so the rate-limiter slot is freed immediately.
async fn schedule<T: Send + 'static>(
	ratelimiter: &RateLimiter,
	fetch: FetchFn<T>,
	on_response: OnResponse<T>,
	max_retries: u32,
) -> Duration {
	let retries = Arc::new(AtomicU32::new(0));

	ratelimiter
		.schedule_task_with_retry(move || {
			let fetch = Arc::clone(&fetch);
			let on_response = Arc::clone(&on_response);
			let retries = retries.clone();
			async move {
				let response = fetch().await;
				if response.is_rate_limited() {
					let prev = retries.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
					if prev < max_retries {
						TaskResult::TryAgain
					} else {
						TaskResult::Done
					}
				} else {
					// Spawn the callback on a separate task so the
					// rate-limiter slot is released right away rather than
					// waiting for the callback to finish.
					tokio::spawn(on_response(response));
					TaskResult::Done
				}
			}
		})
		.await
}

impl ClientRateLimited {
	/// Fetches the user account associated with the current session.
	///
	/// `on_response` is called (on a spawned task) once Bunq returns a
	/// successful response. 429 responses are retried automatically.
	pub async fn get_user_ratelimited<F, Fut>(self: &Arc<Self>, on_response: F) -> Duration
	where
		F: Fn(ApiResponse<Single<User>>) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = ()> + Send + 'static,
	{
		let c = Arc::clone(self);
		let fetch: FetchFn<Single<User>> = Arc::new(move || {
			let c = Arc::clone(&c);
			Box::pin(async move { c.client.get_user().await })
		});
		schedule(
			&self.ratelimiter_get,
			fetch,
			Arc::new(move |r| Box::pin(on_response(r))),
			self.max_retries,
		)
		.await
	}

	/// Fetches all monetary accounts for the session's user.
	///
	/// `on_response` is called (on a spawned task) once Bunq returns a
	/// successful response. 429 responses are retried automatically.
	pub async fn get_monetary_accounts_ratelimited<F, Fut>(
		self: &Arc<Self>,
		on_response: F,
	) -> Duration
	where
		F: Fn(ApiResponse<Multiple<MonetaryAccountBankWrapper>>) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = ()> + Send + 'static,
	{
		let c = Arc::clone(self);
		let fetch: FetchFn<Multiple<MonetaryAccountBankWrapper>> = Arc::new(move || {
			let c = Arc::clone(&c);
			Box::pin(async move { c.client.get_monetary_accounts().await })
		});
		schedule(
			&self.ratelimiter_get,
			fetch,
			Arc::new(move |r| Box::pin(on_response(r))),
			self.max_retries,
		)
		.await
	}

	/// Fetches a single monetary account by ID.
	///
	/// `on_response` is called (on a spawned task) once Bunq returns a
	/// successful response. 429 responses are retried automatically.
	pub async fn get_monetary_account_ratelimited<F, Fut>(
		self: &Arc<Self>,
		bank_account_id: u32,
		on_response: F,
	) -> Duration
	where
		F: Fn(ApiResponse<Single<MonetaryAccountBankWrapper>>) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = ()> + Send + 'static,
	{
		let c = Arc::clone(self);
		let fetch: FetchFn<Single<MonetaryAccountBankWrapper>> = Arc::new(move || {
			let c = Arc::clone(&c);
			Box::pin(async move { c.client.get_monetary_account(bank_account_id).await })
		});
		schedule(
			&self.ratelimiter_get,
			fetch,
			Arc::new(move |r| Box::pin(on_response(r))),
			self.max_retries,
		)
		.await
	}

	/// Fetches a single bunq.me payment request (BunqMeTab) by ID.
	///
	/// `on_response` is called (on a spawned task) once Bunq returns a
	/// successful response. 429 responses are retried automatically.
	pub async fn get_payment_request_ratelimited<F, Fut>(
		self: &Arc<Self>,
		monetary_account_id: u32,
		payment_request_id: u32,
		on_response: F,
	) -> Duration
	where
		F: Fn(ApiResponse<Single<BunqMeTabWrapper>>) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = ()> + Send + 'static,
	{
		let c = Arc::clone(self);
		let fetch: FetchFn<Single<BunqMeTabWrapper>> = Arc::new(move || {
			let c = Arc::clone(&c);
			Box::pin(async move {
				c.client
					.get_payment_request(monetary_account_id, payment_request_id)
					.await
			})
		});
		schedule(
			&self.ratelimiter_get,
			fetch,
			Arc::new(move |r| Box::pin(on_response(r))),
			self.max_retries,
		)
		.await
	}

	/// Creates a new bunq.me payment request.
	///
	/// `amount` is always interpreted as EUR.
	///
	/// `on_response` is called (on a spawned task) once Bunq returns a
	/// successful response. 429 responses are retried automatically, which
	/// means `fetch` — and therefore the POST — may be called more than once.
	pub async fn create_payment_request_ratelimited<F, Fut>(
		self: &Arc<Self>,
		monetary_account_id: u32,
		amount: Decimal,
		description: String,
		redirect_url: String,
		on_response: F,
	) -> Duration
	where
		F: Fn(ApiResponse<Single<CreateBunqMeTabResponseWrapper>>) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = ()> + Send + 'static,
	{
		let c = Arc::clone(self);
		let fetch: FetchFn<Single<CreateBunqMeTabResponseWrapper>> = Arc::new(move || {
			let c = Arc::clone(&c);
			let description = description.clone();
			let redirect_url = redirect_url.clone();
			Box::pin(async move {
				c.client
					.create_payment_request(monetary_account_id, amount, description, redirect_url)
					.await
			})
		});
		schedule(
			&self.ratelimiter_post,
			fetch,
			Arc::new(move |r| Box::pin(on_response(r))),
			self.max_retries,
		)
		.await
	}

	/// Cancels an open bunq.me payment request.
	///
	/// `on_response` is called (on a spawned task) once Bunq returns a
	/// successful response. 429 responses are retried automatically.
	pub async fn close_payment_request_ratelimited<F, Fut>(
		self: &Arc<Self>,
		monetary_account_id: u32,
		payment_request_id: u32,
		on_response: F,
	) -> Duration
	where
		F: Fn(ApiResponse<Single<CreateBunqMeTabResponseWrapper>>) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = ()> + Send + 'static,
	{
		let c = Arc::clone(self);
		let fetch: FetchFn<Single<CreateBunqMeTabResponseWrapper>> = Arc::new(move || {
			let c = Arc::clone(&c);
			Box::pin(async move {
				c.client
					.close_payment_request(monetary_account_id, payment_request_id)
					.await
			})
		});
		schedule(
			&self.ratelimiter_put,
			fetch,
			Arc::new(move |r| Box::pin(on_response(r))),
			self.max_retries,
		)
		.await
	}
}
