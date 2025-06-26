use std::ops::Deref;

use chrono::NaiveDateTime;
use serde::{de::Error, Deserialize, Serialize};

use crate::deserialization::deserialize_date;

////// General Api Response types ///////

#[derive(Debug, Serialize, Clone)]
pub enum ApiResponse<T> {
	Ok(T),
	Err(Vec<ApiErrorDescription>)
}

impl<T> ApiResponse<T> {
	pub fn into_result(self) -> Result<T, Vec<ApiErrorDescription>> {
		match self {
			ApiResponse::Ok(v) => Ok(v),
			ApiResponse::Err(api_error_descriptions) => Err(api_error_descriptions),
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

#[derive(Debug, Deserialize)]
pub struct Installation {
	pub id: u32,
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

///////////// Device Server ////////////

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

#[derive(Debug, Deserialize)]
pub enum DeviceServerStatus {
	Active,
	Blocked,
	NeedsConfirmation,
	Obsolete
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


