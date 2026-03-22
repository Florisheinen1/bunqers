use openssl::pkey::{PKey, Private, Public};
use reqwest::Method;
use rust_decimal::Decimal;

use crate::{
	client_builder::{ClientBuilder, Registered},
	messenger::{ApiResponse, Messenger},
	types::*,
};

/// Credentials that are valid for the lifetime of a single Bunq session.
///
/// A session is created by [`ClientBuilder::create_session`] and remains valid
/// until it expires (default: 1 hour) or is explicitly closed. The session
/// token is sent as the `X-Bunq-Client-Authentication` header on every
/// subsequent request.
#[derive(Clone)]
pub struct SessionContext {
	/// Numeric user ID of the account that owns this session.
	pub owner_id: u32,
	/// Token for authenticating subsequent API requests.
	pub session_token: String,
	/// Device ID assigned during registration.
	pub registered_device_id: u32,
	/// Bunq API key used to create the session.
	pub bunq_api_key: String,
	/// Installation token from the `/installation` step; kept for re-auth.
	pub installation_token: String,
	/// Bunq's RSA public key used to verify response signatures.
	pub bunq_public_key: PKey<Public>,
}

/// A ready-to-use Bunq API client with an active session.
///
/// Obtain a `Client` via [`crate::create_client`] or by driving
/// [`crate::client_builder::ClientBuilder`] through its typestate chain.
///
/// Every endpoint method returns [`ApiResponse<T>`]. Call
/// [`.into_result()`](ApiResponse::into_result) on the response to convert it
/// into a `Result`, or check [`.is_rate_limited()`](ApiResponse::is_rate_limited)
/// first when using the client without the rate-limiting wrapper.
pub struct Client {
	pub api_base_url: String,
	pub app_name: String,
	pub private_key: PKey<Private>,
	pub messenger: Messenger,
	pub context: SessionContext,
}

impl Client {
	/// Verifies that the current session is still valid and, if not, creates a
	/// new one.
	///
	/// Returns `Ok(Client)` with a guaranteed-valid session. Returns
	/// `Err(Registered)` if session creation itself fails (e.g. the device
	/// registration was revoked), giving back the registration context so the
	/// caller can decide how to proceed.
	pub async fn ensure_session(self) -> Result<Self, Registered> {
		// Reuse the ClientBuilder logic to verify the session.
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
				// Session token is invalid; create a new session from the
				// existing registration.
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

	// =========================================================================
	// Endpoints
	// =========================================================================

	/// Returns the user account associated with the current session.
	///
	/// Bunq API: `GET /user`
	pub async fn get_user(&self) -> ApiResponse<Single<User>> {
		self.messenger
			.send(Method::GET, "user", None)
			.await
			.expect("Failed to send request to Bunq")
	}

	/// Returns all monetary accounts for the session's user.
	///
	/// Bunq API: `GET /user/{userId}/monetary-account-bank`
	pub async fn get_monetary_accounts(&self) -> ApiResponse<Multiple<MonetaryAccountBankWrapper>> {
		let endpoint = format!("user/{}/monetary-account-bank", self.context.owner_id);
		self.messenger
			.send(Method::GET, &endpoint, None)
			.await
			.expect("Failed to send request to Bunq")
	}

	/// Returns a single monetary account by ID.
	///
	/// Bunq API: `GET /user/{userId}/monetary-account-bank/{accountId}`
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

	/// Returns a single bunq.me payment request (BunqMeTab) by ID.
	///
	/// Bunq API: `GET /user/{userId}/monetary-account/{accountId}/bunqme-tab/{tabId}`
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

	/// Creates a new bunq.me payment request (BunqMeTab).
	///
	/// `amount` is always interpreted as EUR. The returned response contains
	/// the ID of the newly created tab.
	///
	/// Bunq API: `POST /user/{userId}/monetary-account/{accountId}/bunqme-tab`
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
					currency: "EUR".to_string(),
				},
				description,
				redirect_url,
			},
		};

		let body = serde_json::to_string(&body)
			.expect("Failed to serialize create_payment_request body");

		self.messenger
			.send(Method::POST, &endpoint, Some(body))
			.await
			.expect("Failed to send request to Bunq")
	}

	/// Cancels an open bunq.me payment request (BunqMeTab).
	///
	/// Bunq API: `PUT /user/{userId}/monetary-account/{accountId}/bunqme-tab/{tabId}`
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
		let body = serde_json::to_string(&body)
			.expect("Failed to serialize close_payment_request body");
		self.messenger
			.send(Method::PUT, &endpoint, Some(body))
			.await
			.expect("Failed to send request to Bunq")
	}
}
