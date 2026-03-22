use std::{future::Future, pin::Pin, sync::Arc};

use ritlers::{TaskResult, async_rt::RateLimiter};
use rust_decimal::Decimal;

use crate::{client::Client, messenger::ApiResponse, types::*};

pub type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type OnResponse<T> = Arc<dyn Fn(ApiResponse<T>) -> BoxFuture + Send + Sync>;
type FetchFn<T> =
	Arc<dyn Fn() -> Pin<Box<dyn Future<Output = ApiResponse<T>> + Send + 'static>> + Send + Sync>;

pub struct ClientRateLimited {
	pub client: Client,
	pub ratelimiter_get: RateLimiter,
	pub ratelimiter_post: RateLimiter,
}

async fn schedule<T: Send + 'static>(
	ratelimiter: &RateLimiter,
	fetch: FetchFn<T>,
	on_response: OnResponse<T>,
) {
	ratelimiter
		.schedule_task_with_retry(move || {
			let fetch = Arc::clone(&fetch);
			let on_response = Arc::clone(&on_response);
			async move {
				let response = fetch().await;
				if response.is_rate_limited() {
					TaskResult::TryAgain
				} else {
					tokio::spawn(on_response(response));
					TaskResult::Success
				}
			}
		})
		.await;
}

impl ClientRateLimited {
	pub async fn get_user_ratelimited<F, Fut>(self: &Arc<Self>, on_response: F)
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
		)
		.await;
	}

	pub async fn get_monetary_accounts_ratelimited<F, Fut>(self: &Arc<Self>, on_response: F)
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
		)
		.await;
	}

	pub async fn get_monetary_account_ratelimited<F, Fut>(
		self: &Arc<Self>,
		bank_account_id: u32,
		on_response: F,
	) where
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
		)
		.await;
	}

	pub async fn get_payment_request_ratelimited<F, Fut>(
		self: &Arc<Self>,
		monetary_account_id: u32,
		payment_request_id: u32,
		on_response: F,
	) where
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
		)
		.await;
	}

	pub async fn create_payment_request_ratelimited<F, Fut>(
		self: &Arc<Self>,
		monetary_account_id: u32,
		amount: Decimal,
		description: String,
		redirect_url: String,
		on_response: F,
	) where
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
		)
		.await;
	}

	pub async fn close_payment_request_ratelimited<F, Fut>(
		self: &Arc<Self>,
		monetary_account_id: u32,
		payment_request_id: u32,
		on_response: F,
	) where
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
			&self.ratelimiter_post,
			fetch,
			Arc::new(move |r| Box::pin(on_response(r))),
		)
		.await;
	}
}
