use std::{fmt::Display, ops::Deref};

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::deserialization::{deserialize_date, deserialize_string_to_f32};

////// General Api Response types ///////

#[derive(Debug, Serialize, Clone)]
pub enum ApiResponseBody<T> {
	Ok(T),
	Err(Vec<ApiErrorDescription>)
}

impl<T> ApiResponseBody<T> {
	pub fn into_result(self) -> Result<T, Vec<ApiErrorDescription>> {
		match self {
			ApiResponseBody::Ok(v) => Ok(v),
			ApiResponseBody::Err(api_error_descriptions) => Err(api_error_descriptions),
		}
	}
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ApiErrorDescription {
	#[serde(rename = "error_description")]
	pub description: String,
	#[serde(rename = "error_description_translated")]
	pub translated: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Pagination {
	pub future_url: Option<String>,
	pub newer_url: Option<String>,
	pub older_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Multiple<T> {
	pub data: Vec<T>,
	pub pagination: Pagination
}

#[derive(Debug, Serialize, Clone)]
pub struct Single<T>(pub T);

impl<T> Deref for Single<T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

///////////// Installation ////////////////

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateInstallation {
	pub client_public_key: String
}

#[derive(Debug)]
pub struct Installation {
	pub id: BunqId,
	pub token: InstallationToken,
	pub bunq_public_key: String,
}

#[derive(Debug, Deserialize)]
pub struct InstallationToken {
	pub id: u32,
	#[serde(deserialize_with = "deserialize_date")]
	pub created: NaiveDateTime,
	#[serde(deserialize_with = "deserialize_date")]
	pub updated: NaiveDateTime,
	pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct BunqId {
	pub id: u32
}

///////////// Device Server ////////////

#[derive(Debug, Serialize)]
pub struct CreateDeviceServer {
	#[serde(rename = "secret")]
	pub bunq_api_key: String,
	pub description: String,
	pub permitted_ips: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceServerWrapper {
	#[serde(rename = "DeviceServer")]
	pub device_server: DeviceServer
}
impl Deref for DeviceServerWrapper {
	type Target = DeviceServer;

	fn deref(&self) -> &Self::Target {
		&self.device_server
	}
}

#[derive(Debug, Deserialize)]
pub struct DeviceServer {
	pub id: u32,
	#[serde(deserialize_with = "deserialize_date")]
	pub created: NaiveDateTime,
	#[serde(deserialize_with = "deserialize_date")]
	pub updated: NaiveDateTime,
	pub description: String,
	pub ip: String,
	pub status: DeviceServerStatus
}

#[derive(Debug)]
pub struct DeviceServerSmall {
	pub id: u32
}

#[derive(Debug, Deserialize)]
pub enum DeviceServerStatus {
	#[serde(rename = "ACTIVE")]
	Active,
	#[serde(rename = "BLOCKED")]
	Blocked,
	#[serde(rename = "NEEDS_CONFIRMATION")]
	NeedsConfirmation,
	#[serde(rename = "OBSOLETE")]
	Obsolete
}

///////////// Session ////////////

#[derive(Debug, Serialize)]
pub struct CreateSession {
	#[serde(rename = "secret")]
	pub bunq_api_key: String
}

#[derive(Debug)]
pub struct Session {
	pub id: u32,
	pub token: SessionToken,
	pub user_person: UserPerson,
}

#[derive(Debug, Deserialize)]
pub struct SessionToken {
	pub id: u32,
	#[serde(deserialize_with = "deserialize_date")]
	pub created: NaiveDateTime,
	#[serde(deserialize_with = "deserialize_date")]
	pub updated: NaiveDateTime,
	pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct UserPerson {
	pub id: u32,
	// pub status: UserStatus,
	#[serde(deserialize_with = "deserialize_date")]
	pub created: NaiveDateTime,
	#[serde(deserialize_with = "deserialize_date")]
	pub updated: NaiveDateTime,
	pub public_uuid: String,
	pub session_timeout: i32,
	// pub daily_limit_without_confirmation_login: Amount,
	pub legal_name: String,
	pub public_nick_name: String,
	pub display_name: String,
	pub first_name: String,
	pub last_name: String,
	pub middle_name: String,
	// pub address_main: Address,
	pub date_of_birth: String,
	pub nationality: String,
	// pub alias: Vec<BunqPointer>,
	// pub sub_status: UserSubStatus,
	// pub avatar: Avatar,
	// pub notification_filters: Vec<NotificationFilter>,
}


////////////////// UserListing ////////////////

#[derive(Debug, Deserialize)]
pub struct User {
	#[serde(rename = "UserPerson")]
	pub user_person: UserPerson
}

////////////////// Monetary Account ////////////////
#[derive(Debug, Deserialize)]
pub struct MonetaryAccountBankWrapper {
	#[serde(rename = "MonetaryAccountBank")]
	pub monetary_account_bank: MonetaryAccountBank
}
impl Deref for MonetaryAccountBankWrapper {
	type Target = MonetaryAccountBank;

	fn deref(&self) -> &Self::Target {
		&self.monetary_account_bank
	}
}

#[derive(Debug, Deserialize)]
pub struct MonetaryAccountBank {
	pub currency: String,
	pub id: u32,
	pub balance: Amount,
	pub description: String,
	pub status: MonetaryAccountBankStatus,
	// pub alias: Vec<BunqPointer>,
}

#[derive(Debug, Deserialize)]
pub struct Amount {
	#[serde(deserialize_with = "deserialize_string_to_f32")]
	pub value: f32,
	pub currency: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum MonetaryAccountBankStatus {
	#[serde(rename = "ACTIVE")]
	Active,
	#[serde(rename = "BLOCKED")]
	Blocked,
	#[serde(rename = "CANCELLED")]
	Cancelled,
	#[serde(rename = "PENDING_REOPEN")]
	PendingReopen,
	#[serde(other)]
	Unknown, // Catch for new status we do not know yet
}

////////////////// BunqMeTab ////////////////

#[derive(Debug, Deserialize, Clone)]
pub struct BunqMeTabWrapper(pub BunqMeTab);

impl Deref for BunqMeTabWrapper {
	type Target = BunqMeTab;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

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
	pub bunqme_tab_share_url: String,

}

#[derive(Debug, Deserialize, Serialize, Clone)]
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


