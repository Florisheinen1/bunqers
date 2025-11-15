use openssl::pkey::{PKey, Private, Public};
use reqwest::Method;
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
