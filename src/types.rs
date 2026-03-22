//! Data types for all Bunq API requests and responses.
//!
//! # Response wrappers
//!
//! Bunq wraps every response in a top-level `{"Response": [...]}` JSON object.
//! This library exposes two wrappers to match the two shapes:
//!
//! - [`Single<T>`] — the `Response` array has exactly one element.
//! - [`Multiple<T>`] — the `Response` array has zero or more elements, with an
//!   accompanying `Pagination` object.
//!
//! Both implement [`Deref`] so you can access the inner value
//! directly without manually unwrapping.

use std::ops::Deref;

use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::deserialization::deserialize_date;

// =============================================================================
// Generic response wrappers
// =============================================================================

/// The raw parsed body of a Bunq API response.
///
/// Bunq signals errors by returning an `Error` key instead of a `Response` key
/// at the top level. This enum captures both cases. Prefer using
/// [`ApiResponse::into_result`](crate::messenger::ApiResponse::into_result)
/// rather than matching on this directly.
#[derive(Debug, Serialize, Clone)]
pub enum ApiResponseBody<T> {
	Ok(T),
	Err(Vec<ApiErrorDescription>),
}

impl<T> ApiResponseBody<T> {
	/// Converts into a `Result`, discarding HTTP status information.
	///
	/// Prefer [`ApiResponse::into_result`](crate::messenger::ApiResponse::into_result)
	/// which also includes the status code in the error.
	pub fn result(self) -> Result<T, Vec<ApiErrorDescription>> {
		match self {
			ApiResponseBody::Ok(body) => Ok(body),
			ApiResponseBody::Err(api_error_descriptions) => Err(api_error_descriptions),
		}
	}
}

/// A single error description returned by the Bunq API.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ApiErrorDescription {
	#[serde(rename = "error_description")]
	pub description: String,
	#[serde(rename = "error_description_translated")]
	pub translated: String,
}

/// Pagination cursor URLs returned alongside list endpoints.
///
/// Each field is a full URL that can be used to retrieve the next/previous page
/// of results, or `None` if that direction does not exist.
#[derive(Debug, Deserialize, Clone)]
pub struct Pagination {
	pub future_url: Option<String>,
	pub newer_url: Option<String>,
	pub older_url: Option<String>,
}

/// A paginated list of items returned by a Bunq list endpoint.
///
/// Obtained when calling an endpoint that returns multiple items (e.g.
/// [`Client::get_monetary_accounts`](crate::client::Client::get_monetary_accounts)).
#[derive(Debug, Clone)]
pub struct Multiple<T> {
	pub data: Vec<T>,
	pub pagination: Pagination,
}

/// A single item returned by a Bunq endpoint.
///
/// Bunq always wraps its responses in a `Response` array even for endpoints
/// that return one item. `Single<T>` extracts that item and exposes it via
/// [`Deref`].
#[derive(Debug, Serialize, Clone)]
pub struct Single<T>(pub T);

impl<T> Deref for Single<T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

// =============================================================================
// Installation
// =============================================================================

/// Request body for `POST /installation`.
#[derive(Debug, Deserialize, Serialize)]
pub struct CreateInstallation {
	pub client_public_key: String,
}

/// Parsed response from `POST /installation`.
#[derive(Debug)]
pub struct Installation {
	pub id: BunqId,
	pub token: InstallationToken,
	/// Bunq's RSA public key in PEM format.
	pub bunq_public_key: String,
}

/// The token object returned by the `/installation` endpoint.
#[derive(Debug, Deserialize)]
pub struct InstallationToken {
	pub id: u32,
	#[serde(deserialize_with = "deserialize_date")]
	pub created: NaiveDateTime,
	#[serde(deserialize_with = "deserialize_date")]
	pub updated: NaiveDateTime,
	/// The token string used as `X-Bunq-Client-Authentication` during device
	/// registration.
	pub token: String,
}

/// A generic `{"id": N}` object used by multiple Bunq endpoints.
#[derive(Debug, Deserialize)]
pub struct BunqId {
	pub id: u32,
}

// =============================================================================
// Device server
// =============================================================================

/// Request body for `POST /device-server`.
#[derive(Debug, Serialize)]
pub struct CreateDeviceServer<'a> {
	/// The Bunq API key.
	#[serde(rename = "secret")]
	pub bunq_api_key: &'a str,
	/// A human-readable label for this device.
	pub description: &'a str,
	/// IP addresses allowed to use this device registration. An empty list
	/// means the current IP is used.
	pub permitted_ips: Vec<String>,
}

/// Full device server object (not currently used by any endpoint method).
#[derive(Debug, Deserialize)]
pub struct DeviceServerWrapper {
	#[serde(rename = "DeviceServer")]
	pub device_server: DeviceServer,
}
impl Deref for DeviceServerWrapper {
	type Target = DeviceServer;

	fn deref(&self) -> &Self::Target {
		&self.device_server
	}
}

/// Full device server object returned by the device listing endpoint.
#[derive(Debug, Deserialize)]
pub struct DeviceServer {
	pub id: u32,
	#[serde(deserialize_with = "deserialize_date")]
	pub created: NaiveDateTime,
	#[serde(deserialize_with = "deserialize_date")]
	pub updated: NaiveDateTime,
	pub description: String,
	pub ip: String,
	pub status: DeviceServerStatus,
}

/// Minimal device server object returned by `POST /device-server`.
///
/// Only the ID is extracted; the full object can be fetched separately if
/// needed.
#[derive(Debug)]
pub struct DeviceServerSmall {
	pub id: u32,
}

/// Registration status of a device server.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub enum DeviceServerStatus {
	#[serde(rename = "ACTIVE")]
	Active,
	#[serde(rename = "BLOCKED")]
	Blocked,
	#[serde(rename = "NEEDS_CONFIRMATION")]
	NeedsConfirmation,
	#[serde(rename = "OBSOLETE")]
	Obsolete,
}

// =============================================================================
// Session
// =============================================================================

/// Request body for `POST /session-server`.
#[derive(Debug, Serialize)]
pub struct CreateSession {
	#[serde(rename = "secret")]
	pub bunq_api_key: String,
}

/// Parsed response from `POST /session-server`.
#[derive(Debug)]
pub struct Session {
	pub id: u32,
	pub token: SessionToken,
	pub user_person: UserPerson,
}

/// The token object returned by `/session-server`.
#[derive(Debug, Deserialize)]
pub struct SessionToken {
	pub id: u32,
	#[serde(deserialize_with = "deserialize_date")]
	pub created: NaiveDateTime,
	#[serde(deserialize_with = "deserialize_date")]
	pub updated: NaiveDateTime,
	/// The session token used as `X-Bunq-Client-Authentication` for subsequent
	/// API requests.
	pub token: String,
}

// =============================================================================
// User
// =============================================================================

/// A personal Bunq user account.
#[derive(Debug, Deserialize, Serialize)]
pub struct UserPerson {
	pub id: u32,
	#[serde(deserialize_with = "deserialize_date")]
	pub created: NaiveDateTime,
	#[serde(deserialize_with = "deserialize_date")]
	pub updated: NaiveDateTime,
	pub public_uuid: String,
	/// How long (in seconds) until the session expires.
	pub session_timeout: i32,
	pub legal_name: String,
	pub public_nick_name: String,
	pub display_name: String,
	pub first_name: String,
	pub last_name: String,
	pub middle_name: String,
	pub date_of_birth: String,
	pub nationality: String,
}

/// Top-level wrapper for a user returned by `GET /user`.
///
/// Bunq returns a tagged union here; this library currently only handles the
/// `UserPerson` variant.
#[derive(Debug, Deserialize, Serialize)]
pub struct User {
	#[serde(rename = "UserPerson")]
	pub user_person: UserPerson,
}

// =============================================================================
// Monetary account
// =============================================================================

/// JSON wrapper returned in list responses for monetary accounts.
#[derive(Debug, Deserialize)]
pub struct MonetaryAccountBankWrapper {
	#[serde(rename = "MonetaryAccountBank")]
	pub monetary_account_bank: MonetaryAccountBank,
}
impl Deref for MonetaryAccountBankWrapper {
	type Target = MonetaryAccountBank;

	fn deref(&self) -> &Self::Target {
		&self.monetary_account_bank
	}
}

/// A Bunq bank account.
#[derive(Debug, Deserialize)]
pub struct MonetaryAccountBank {
	pub currency: String,
	pub id: u32,
	pub balance: Amount,
	pub description: String,
	pub status: MonetaryAccountBankStatus,
}

/// A monetary amount with a currency code (ISO 4217).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Amount {
	pub value: Decimal,
	pub currency: String,
}

/// Status of a monetary account.
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum MonetaryAccountBankStatus {
	#[serde(rename = "ACTIVE")]
	Active,
	#[serde(rename = "BLOCKED")]
	Blocked,
	#[serde(rename = "CANCELLED")]
	Cancelled,
	#[serde(rename = "PENDING_REOPEN")]
	PendingReopen,
	/// Catch-all for statuses introduced after this library was written.
	#[serde(other)]
	Unknown,
}

// =============================================================================
// BunqMeTab (payment requests)
// =============================================================================

/// JSON wrapper returned for payment request (BunqMeTab) responses.
#[derive(Debug, Deserialize, Clone)]
pub struct BunqMeTabWrapper {
	#[serde(rename = "BunqMeTab")]
	bunqme_tab: BunqMeTab,
}
impl Deref for BunqMeTabWrapper {
	type Target = BunqMeTab;

	fn deref(&self) -> &Self::Target {
		&self.bunqme_tab
	}
}

/// A bunq.me payment request (BunqMeTab).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BunqMeTab {
	pub id: u32,
	#[serde(deserialize_with = "deserialize_date")]
	pub created: NaiveDateTime,
	#[serde(deserialize_with = "deserialize_date")]
	pub updated: NaiveDateTime,
	#[serde(deserialize_with = "deserialize_date")]
	pub time_expiry: NaiveDateTime,
	pub monetary_account_id: u32,
	pub status: BunqMeTabStatus,
	/// The shareable bunq.me URL to send to the payer.
	pub bunqme_tab_share_url: String,
	/// Payments received against this request.
	pub result_inquiries: Vec<BunqMeTabInquiry>,
}

/// Lifecycle status of a BunqMeTab payment request.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub enum BunqMeTabStatus {
	#[serde(rename = "WAITING_FOR_PAYMENT")]
	WaitingForPayment,
	#[serde(rename = "CANCELLED")]
	Cancelled,
	#[serde(rename = "EXPIRED")]
	Expired,
	#[serde(rename = "PAID")]
	Paid,
}

/// Request body wrapper for `POST /bunqme-tab`.
#[derive(Debug, Serialize, Clone)]
pub struct CreateBunqMeTabWrapper {
	pub bunqme_tab_entry: CreateBunqMeTab,
}

/// The inner body for creating a bunq.me payment request.
#[derive(Debug, Serialize, Clone)]
pub struct CreateBunqMeTab {
	/// Amount to request. Currency is always EUR.
	pub amount_inquired: Amount,
	pub description: String,
	/// URL to redirect the payer to after payment.
	pub redirect_url: String,
}

/// Request body for `PUT /bunqme-tab/{id}` (e.g. to cancel a request).
#[derive(Debug, Serialize)]
pub struct AlterBunqMeTabRequest {
	pub status: Option<BunqMeTabStatus>,
}

/// Unused inner body for altering a BunqMeTab (reserved for future use).
#[derive(Debug, Serialize)]
pub struct AlterBunqMeTab {
	pub amount_inquired: Option<Amount>,
	pub description: Option<String>,
	pub redirect_url: Option<String>,
}

/// Response from `POST /bunqme-tab` or `PUT /bunqme-tab/{id}`.
///
/// Contains only the ID of the created or modified tab.
#[derive(Debug, Deserialize)]
pub struct CreateBunqMeTabResponseWrapper {
	#[serde(rename = "Id")]
	pub id: BunqId,
}

/// A single payment received against a BunqMeTab request.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BunqMeTabInquiry {
	pub id: u32,
	pub payment: PaymentWrapper,
}

/// JSON wrapper for a payment object.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PaymentWrapper {
	#[serde(rename = "Payment")]
	pub payment: Payment,
}

/// A payment made in response to a BunqMeTab request.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Payment {
	pub id: u32,
	#[serde(deserialize_with = "deserialize_date")]
	pub created: NaiveDateTime,
	#[serde(deserialize_with = "deserialize_date")]
	pub updated: NaiveDateTime,
	pub counterparty_alias: Alias,
}

/// An alias (IBAN + display name) identifying a payment counterparty.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Alias {
	pub iban: String,
	pub display_name: String,
	pub country: String,
}
