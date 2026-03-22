use std::{future::Future, pin::Pin, sync::Arc};

use openssl::pkey::{PKey, Private, Public};
use reqwest::Method;
use ritlers::async_rt::RateLimiter;
use rust_decimal::Decimal;

use crate::{
	client_builder::{ClientBuilder, Registered},
	messenger::{ApiResponse, Messenger},
	types::*,
};

#[derive(Clone)]
pub struct SessionContext {
	pub owner_id: u32,
	pub session_token: String,
	pub registered_device_id: u32,
	pub bunq_api_key: String,
	pub installation_token: String,
	pub bunq_public_key: PKey<Public>,
}

pub type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type OnResponse<T> = Arc<dyn Fn(ApiResponse<T>) -> BoxFuture + Send + Sync>;
type OnRateLimit = Arc<dyn Fn() -> BoxFuture + Send + Sync>;
type FetchFn<T> =
	Arc<dyn Fn() -> Pin<Box<dyn Future<Output = ApiResponse<T>> + Send + 'static>> + Send + Sync>;

pub struct ClientRateLimited {
	pub client: Client,
	pub ratelimiter_get: RateLimiter,
	pub ratelimiter_post: RateLimiter,
}

fn schedule<T: Send + 'static>(
	client: Arc<ClientRateLimited>,
	ratelimiter: fn(&ClientRateLimited) -> &RateLimiter,
	fetch: FetchFn<T>,
	on_response: OnResponse<T>,
	on_rate_limit: OnRateLimit,
	retry_on_limit: bool,
) -> BoxFuture {
	Box::pin(async move {
		let task_client = Arc::clone(&client);
		ratelimiter(&client)
			.schedule_task(async move {
				let response = fetch().await;
				if response.is_rate_limited() {
					on_rate_limit().await;
					if retry_on_limit {
						schedule(
							task_client,
							ratelimiter,
							fetch,
							on_response,
							on_rate_limit,
							retry_on_limit,
						)
						.await;
					}
				} else {
					on_response(response).await;
				}
			})
			.await;
	})
}

impl ClientRateLimited {
	// async fn ensure_session_rt(self: &Arc<Self>) {}

	pub async fn get_user_ratelimited<F, Fut, RL, RLFut>(
		self: &Arc<Self>,
		on_response: F,
		on_rate_limit: RL,
		retry_on_limit: bool,
	) where
		F: Fn(ApiResponse<Single<User>>) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = ()> + Send + 'static,
		RL: Fn() -> RLFut + Send + Sync + 'static,
		RLFut: Future<Output = ()> + Send + 'static,
	{
		let c = Arc::clone(self);
		let fetch: FetchFn<Single<User>> = Arc::new(move || {
			let c = Arc::clone(&c);
			Box::pin(async move { c.client.get_user().await })
		});
		schedule(
			Arc::clone(self),
			|c| &c.ratelimiter_get,
			fetch,
			Arc::new(move |r| Box::pin(on_response(r))),
			Arc::new(move || Box::pin(on_rate_limit())),
			retry_on_limit,
		)
		.await;
	}

	pub async fn get_monetary_accounts_ratelimited<F, Fut, RL, RLFut>(
		self: &Arc<Self>,
		on_response: F,
		on_rate_limit: RL,
		retry_on_limit: bool,
	) where
		F: Fn(ApiResponse<Multiple<MonetaryAccountBankWrapper>>) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = ()> + Send + 'static,
		RL: Fn() -> RLFut + Send + Sync + 'static,
		RLFut: Future<Output = ()> + Send + 'static,
	{
		let c = Arc::clone(self);
		let fetch: FetchFn<Multiple<MonetaryAccountBankWrapper>> = Arc::new(move || {
			let c = Arc::clone(&c);
			Box::pin(async move { c.client.get_monetary_accounts().await })
		});
		schedule(
			Arc::clone(self),
			|c| &c.ratelimiter_get,
			fetch,
			Arc::new(move |r| Box::pin(on_response(r))),
			Arc::new(move || Box::pin(on_rate_limit())),
			retry_on_limit,
		)
		.await;
	}

	pub async fn get_monetary_account_ratelimited<F, Fut, RL, RLFut>(
		self: &Arc<Self>,
		bank_account_id: u32,
		on_response: F,
		on_rate_limit: RL,
		retry_on_limit: bool,
	) where
		F: Fn(ApiResponse<Single<MonetaryAccountBankWrapper>>) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = ()> + Send + 'static,
		RL: Fn() -> RLFut + Send + Sync + 'static,
		RLFut: Future<Output = ()> + Send + 'static,
	{
		let c = Arc::clone(self);
		let fetch: FetchFn<Single<MonetaryAccountBankWrapper>> = Arc::new(move || {
			let c = Arc::clone(&c);
			Box::pin(async move { c.client.get_monetary_account(bank_account_id).await })
		});
		schedule(
			Arc::clone(self),
			|c| &c.ratelimiter_get,
			fetch,
			Arc::new(move |r| Box::pin(on_response(r))),
			Arc::new(move || Box::pin(on_rate_limit())),
			retry_on_limit,
		)
		.await;
	}

	pub async fn get_payment_request_ratelimited<F, Fut, RL, RLFut>(
		self: &Arc<Self>,
		monetary_account_id: u32,
		payment_request_id: u32,
		on_response: F,
		on_rate_limit: RL,
		retry_on_limit: bool,
	) where
		F: Fn(ApiResponse<Single<BunqMeTabWrapper>>) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = ()> + Send + 'static,
		RL: Fn() -> RLFut + Send + Sync + 'static,
		RLFut: Future<Output = ()> + Send + 'static,
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
			Arc::clone(self),
			|c| &c.ratelimiter_get,
			fetch,
			Arc::new(move |r| Box::pin(on_response(r))),
			Arc::new(move || Box::pin(on_rate_limit())),
			retry_on_limit,
		)
		.await;
	}

	pub async fn create_payment_request_ratelimited<F, Fut, RL, RLFut>(
		self: &Arc<Self>,
		monetary_account_id: u32,
		amount: Decimal,
		description: String,
		redirect_url: String,
		on_response: F,
		on_rate_limit: RL,
		retry_on_limit: bool,
	) where
		F: Fn(ApiResponse<Single<CreateBunqMeTabResponseWrapper>>) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = ()> + Send + 'static,
		RL: Fn() -> RLFut + Send + Sync + 'static,
		RLFut: Future<Output = ()> + Send + 'static,
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
			Arc::clone(self),
			|c| &c.ratelimiter_post,
			fetch,
			Arc::new(move |r| Box::pin(on_response(r))),
			Arc::new(move || Box::pin(on_rate_limit())),
			retry_on_limit,
		)
		.await;
	}

	pub async fn close_payment_request_ratelimited<F, Fut, RL, RLFut>(
		self: &Arc<Self>,
		monetary_account_id: u32,
		payment_request_id: u32,
		on_response: F,
		on_rate_limit: RL,
		retry_on_limit: bool,
	) where
		F: Fn(ApiResponse<Single<CreateBunqMeTabResponseWrapper>>) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = ()> + Send + 'static,
		RL: Fn() -> RLFut + Send + Sync + 'static,
		RLFut: Future<Output = ()> + Send + 'static,
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
			Arc::clone(self),
			|c| &c.ratelimiter_post,
			fetch,
			Arc::new(move |r| Box::pin(on_response(r))),
			Arc::new(move || Box::pin(on_rate_limit())),
			retry_on_limit,
		)
		.await;
	}
}

pub struct Client {
	pub api_base_url: String,
	pub app_name: String,
	pub private_key: PKey<Private>,
	pub messenger: Messenger,
	pub context: SessionContext,
}

impl Client {
	/// Checks if current session is still valid
	/// If not, it will try to create a new one
	pub async fn ensure_session(self) -> Result<Self, Registered> {
		// Reuse the ClientBuilder logic to verify the session
		let unchecked_session = ClientBuilder::from_unchecked_session(
			self.context.into(),
			self.api_base_url.clone(),
			self.app_name.clone(),
			self.private_key.clone(),
		);

		match unchecked_session.check_session().await {
			Ok(checked_session) => {
				return Ok(checked_session.build());
			}
			Err(error) => {
				// The session token is invalid. Remove it and create a new one
				let new_session_builder = ClientBuilder::from_registration(
					error.context.into(),
					self.api_base_url,
					self.app_name,
					self.private_key,
				);
				match new_session_builder.create_session().await {
					Ok(checked_session) => {
						return Ok(checked_session.build());
					}
					Err(error) => Err(error.context),
				}
			}
		}
	}

	//@===@===@===@===@// ENDPOINTS //@===@===@===@===@//

	/// Fetches the user data of this session (GET)
	pub async fn get_user(&self) -> ApiResponse<Single<User>> {
		self.messenger
			.send(Method::GET, "user", None)
			.await
			.expect("Failed to send request to Bunq")
	}

	/// Fetches a list of monetary accounts (GET)
	pub async fn get_monetary_accounts(&self) -> ApiResponse<Multiple<MonetaryAccountBankWrapper>> {
		let endpoint = format!("user/{}/monetary-account-bank", self.context.owner_id);
		self.messenger
			.send(Method::GET, &endpoint, None)
			.await
			.expect("Failed to send request to Bunq")
	}

	/// Fetches a list of monetary accounts (GET)
	pub async fn get_monetary_account(
		&self,
		bank_account_id: u32,
	) -> ApiResponse<Single<MonetaryAccountBankWrapper>> {
		let endpoint = format!(
			"user/{}/monetary-account-bank/{}",
			self.context.owner_id, bank_account_id
		);
		self.messenger
			.send(Method::GET, &endpoint, None)
			.await
			.expect("Failed to send request to Bunq")
	}

	/// Fetches the payment request with given id (GET)
	pub async fn get_payment_request(
		&self,
		monetary_account_id: u32,
		payment_request_id: u32,
	) -> ApiResponse<Single<BunqMeTabWrapper>> {
		let endpoint = format!(
			"user/{}/monetary-account/{monetary_account_id}/bunqme-tab/{payment_request_id}",
			self.context.owner_id
		);
		self.messenger
			.send(Method::GET, &endpoint, None)
			.await
			.expect("Failed to send request to Bunq")
	}

	/// Creates a new payment request (POST)
	pub async fn create_payment_request(
		&self,
		monetary_account_id: u32,
		amount: Decimal,
		description: String,
		redirect_url: String,
	) -> ApiResponse<Single<CreateBunqMeTabResponseWrapper>> {
		let endpoint = format!(
			"user/{}/monetary-account/{monetary_account_id}/bunqme-tab",
			self.context.owner_id
		);

		let body = CreateBunqMeTabWrapper {
			bunqme_tab_entry: CreateBunqMeTab {
				amount_inquired: Amount {
					value: amount,
					currency: format!("EUR"),
				},
				description,
				redirect_url,
			},
		};

		let body = serde_json::to_string(&body)
			.expect("Failed to serialize body of create payment request");

		self.messenger
			.send(Method::POST, &endpoint, Some(body))
			.await
			.expect("Failed to send request to Bunq")
	}

	/// Closes the given payment request
	pub async fn close_payment_request(
		&self,
		monetary_account_id: u32,
		payment_request_id: u32,
	) -> ApiResponse<Single<CreateBunqMeTabResponseWrapper>> {
		let endpoint = format!(
			"user/{}/monetary-account/{monetary_account_id}/bunqme-tab/{payment_request_id}",
			self.context.owner_id
		);
		let body = AlterBunqMeTabRequest {
			status: Some(BunqMeTabStatus::Cancelled),
		};
		let body =
			serde_json::to_string(&body).expect("Failed to serialize AlterBunqMeTab request body");
		self.messenger
			.send(Method::PUT, &endpoint, Some(body))
			.await
			.expect("Failed to send request to Bunq")
	}
}
