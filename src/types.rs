use std::ops::Deref;

use chrono::NaiveDateTime;
use serde::{de::Error, Deserialize, Serialize};

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

impl<'de, T> Deserialize<'de> for ApiResponse<T>
where T: Deserialize<'de>
{
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de> {

		let root = serde_json::Value::deserialize(deserializer).expect("Invalid JSON received!");
		
		// Try parse error
		if let Some(bunq_error) = root.get("Error") {
			
			let error_descriptions_value = bunq_error
				.as_array()
				.ok_or_else(|| D::Error::custom("Error in ApiResponse was not an array"))?;

			let error_descriptions = error_descriptions_value.iter().map(|description_value| {
					ApiErrorDescription::deserialize(description_value.clone())
						.map_err(|e| D::Error::custom(format!("ApiErrorDescription: {}", e.to_string())))
				}).collect::<Result<Vec<ApiErrorDescription>, D::Error>>()?;

			return Ok(ApiResponse::Err(error_descriptions));
		}

		// Otherwise, parse data
		let data = T::deserialize(root)
			.map_err(|e| D::Error::custom(format!("ApiResponse: {}", e.to_string())))?;

		return Ok(ApiResponse::Ok(data));
	}
}

// //////////////////////////

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

impl<'de, T> Deserialize<'de> for Multiple<T>
where T: Deserialize<'de> {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de> {
		let root = serde_json::Value::deserialize(deserializer).expect("Failed to parse JSON");

		// Get pagination details
  		let pagination_value = root.get("Pagination").ok_or_else(|| D::Error::custom("Failed"))?;
		let pagination = Pagination::deserialize(pagination_value.clone()).map_err(|e| D::Error::custom(format!("Failed: {e}")))?;

		// Get data
		let data_value = root.get("Response").ok_or_else(|| D::Error::custom("Failed"))?;
		let data_value_array = data_value.as_array().ok_or_else(|| D::Error::custom("Wansnt an array!"))?;
		let data: Vec<T> = data_value_array.iter().map(|value| {
			T::deserialize(value.clone()).map_err(|e| D::Error::custom(format!("Error: {e}")))
		}).collect::<Result<Vec<T>, D::Error>>()?;

		return Ok(Self {
			data,
			pagination
		});
	}
}

#[derive(Debug, Serialize, Clone)]
pub struct Single<T>(pub T);

impl<'de, T> Deserialize<'de> for Single<T>
where T: Deserialize<'de> {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de> {

		let root = serde_json::Value::deserialize(deserializer).expect("Failed to parse to JSON");

		// Expect Response, but no Pagination!
		let response_field = root.get("Response").expect("No 'Response' field available").clone();

		let data: Vec<T> = Vec::<T>::deserialize(response_field)
			.map_err(|e| D::Error::custom(format!("Single: {}", e.to_string())))?;

		if data.len() > 1 {
			return Err(D::Error::custom(format!("Received more than a single!")));
		}

		let single_data = data.into_iter().next().expect("No single data was available");

		Ok(Self(single_data))
	}
}

impl<T> Deref for Single<T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

#[derive(Debug, Clone)]
pub struct BunqMeTabWrapper(pub BunqMeTab);

impl<'de> Deserialize<'de> for BunqMeTabWrapper {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de> {

		let value = serde_json::Value::deserialize(deserializer).expect("Failed to deserialize BunqMeTabWrapper");

		let tab = value.get("BunqMeTab").ok_or_else(|| D::Error::custom(format!("BunqMeTab not present in wrapper")))?;

		let tab = BunqMeTab::deserialize(tab).map_err(|e| D::Error::custom(format!("BunqMeTab: {}", e.to_string())))?;

		Ok(Self(tab))
	}
}

impl Deref for BunqMeTabWrapper {
	type Target = BunqMeTab;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

// Parse the string into a NaiveDateTime
fn deserialize_date<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
where
    D: serde::Deserializer<'de>,
{
	let s = String::deserialize(deserializer)?;
	NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f")
		.map_err(|e| D::Error::custom(format!("Incorrect datetime {s}: {}", e.to_string())))
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BunqMeTab {
	pub id: u32,
	// #[serde(with = "bunq_datetime_parser")]
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

#[derive(Debug, Serialize, Clone)]
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

impl<'de> Deserialize<'de> for BunqMeTabStatus {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		let s = String::deserialize(deserializer)?;
		match s.as_str() {
			"WAITING_FOR_PAYMENT" => Ok(BunqMeTabStatus::WaitingForPayment),
			"CANCELLED" => Ok(BunqMeTabStatus::Cancelled),
			"EXPIRED" => Ok(BunqMeTabStatus::Expired),
			"PAID" => Ok(BunqMeTabStatus::Paid),
			other => Err(D::Error::custom(
				format!("BunqMeTabStatus: {} is invalid variant", other)
			)),
		}
	}
}