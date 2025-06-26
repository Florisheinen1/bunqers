use chrono::NaiveDateTime;
use serde::{Deserialize, de::Error};

use crate::types::*;


impl<'de, T> Deserialize<'de> for ApiResponse<T>
where T: Deserialize<'de>
{
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de> {

		let root = serde_json::Value::deserialize(deserializer)?;

		if let Some(errors) = root.get("Error") {
			let errors: Result<Vec<ApiErrorDescription>, _> = serde_path_to_error::deserialize(errors);
			
			match errors {
				Ok(errors) => return Ok(ApiResponse::Err(errors)),
				Err(parse_error) => {
					return Err(D::Error::custom(format!("Errors: {parse_error}")));
				},
			}
		}
		
		let content: Result<T, _> = serde_path_to_error::deserialize(root);
		match content {
			Ok(content) => return Ok(ApiResponse::Ok(content)),
			Err(parse_error) => return Err(D::Error::custom(format!("Response: {parse_error}"))),
		}
	}
}

impl<'de, T> Deserialize<'de> for Multiple<T>
where T: Deserialize<'de> {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de> {
		let root = serde_json::Value::deserialize(deserializer).expect("Failed to parse JSON");

		// Get pagination details
  		let pagination_value = root.get("Pagination").expect("Failed here");
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

impl<'de, T> Deserialize<'de> for Single<T>
where T: Deserialize<'de> {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de> {

		let root = serde_json::Value::deserialize(deserializer).expect("Failed to parse to JSON");

		// Expect Response, but no Pagination!
		let response_field = root.get("Response").expect("No 'Response' field available").clone();

		let response: Result<Vec<T>, _> = serde_path_to_error::deserialize(response_field);

		let response = match response {
			Ok(parse_success) => parse_success,
			Err(parse_error) => {
				println!("Error with parsing. Path: {}, error: {}", parse_error.path(), parse_error);
				return Err(D::Error::custom("Some error happened. Look above"));
			},
		};

		if response.len() > 1 {
			return Err(D::Error::custom(format!("Received more than a single!")));
		}

		let single_data = response.into_iter().next().expect("No single data was available");

		Ok(Self(single_data))
	}
}

// impl<'de> Deserialize<'de> for BunqMeTabWrapper {
// 	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
// 	where
// 		D: serde::Deserializer<'de> {

// 		let value = serde_json::Value::deserialize(deserializer).expect("Failed to deserialize BunqMeTabWrapper");

// 		let tab = value.get("BunqMeTab").ok_or_else(|| D::Error::custom(format!("BunqMeTab not present in wrapper")))?;

// 		let tab: BunqMeTab = BunqMeTab::deserialize(tab).map_err(|e| D::Error::custom(format!("BunqMeTab: {}", e.to_string())))?;

// 		Ok(Self(tab))
// 	}
// }


// Parse the string into a NaiveDateTime
pub fn deserialize_date<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
where
    D: serde::Deserializer<'de>,
{
	let s = String::deserialize(deserializer)?;
	NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f")
		.map_err(|e| D::Error::custom(format!("Incorrect datetime {s}: {}", e.to_string())))
}