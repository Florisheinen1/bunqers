use std::any::type_name;

use chrono::NaiveDateTime;
use serde::{de::Error, Deserialize};

use crate::types::*;


impl<'de, T> Deserialize<'de> for ApiResponseBody<T>
where T: Deserialize<'de>
{
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de> {

		let root = serde_json::Value::deserialize(deserializer)?;

		if let Some(errors) = root.get("Error") {
			let errors: Result<Vec<ApiErrorDescription>, _> = serde_path_to_error::deserialize(errors);
			
			match errors {
				Ok(errors) => return Ok(ApiResponseBody::Err(errors)),
				Err(parse_error) => {
					return Err(D::Error::custom(format!("Errors: {parse_error}")));
				},
			}
		}
		
		let content: Result<T, _> = serde_path_to_error::deserialize(root);
		match content {
			Ok(content) => return Ok(ApiResponseBody::Ok(content)),
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
				println!("Parsing of {} failed. Path: {}, error: {}", type_name::<T>(), parse_error.path(), parse_error);
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

impl<'de> Deserialize<'de> for Installation {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de> {

		let root = serde_json::Value::deserialize(deserializer).expect("Failed to parse to JSON");

		let binding = root.get("Response")
			.expect("No 'Response' field in Installation")
			.clone();
  		let response_field = binding
			.as_array()
			.expect("'Response' field was not an array");

		let mut response_iter = response_field.iter();

		let id: BunqId = match serde_path_to_error::deserialize(response_iter.next()
			.expect("Not enough elements in Installation response")
			.get("Id")
			.expect("No 'Id' in Installation response")) {
			Ok(id) => id,
			Err(er) => {
				println!("Failed to parse: {er}");
				return Err(D::Error::custom("Failed to parse Installation"));
			},
		};

		let token_value = response_iter.next()
			.expect("Not enough elements in installation response 2")
			.get("Token")
			.expect("No 'Token' object in Installation response");

		let token: InstallationToken = match serde_path_to_error::deserialize(token_value) {
			Ok(token) => token,
			Err(e) => {
				println!("Failed to parse: {e}");
				return Err(D::Error::custom("Failed to parse Installation"));
			},
		};

		let bunq_public_key = response_iter.next()
			.expect("Not enough elements in installation response 3")
			.get("ServerPublicKey")
			.expect("No 'ServerPublicKey' in Response elements of Installation")
			.get("server_public_key").expect("No actual key")
			.as_str().expect("Failed to parse this to string").to_string();

		Ok(Self {
			id,
			token,
			bunq_public_key,
		})
	}
}

impl<'de> Deserialize<'de> for DeviceServerSmall {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de> {
		
		let root = serde_json::Value::deserialize(deserializer).map_err(|e| D::Error::custom(format!("Failed: {e}")))?;

		let id = root.get("Id")
			.ok_or_else(|| D::Error::custom("No 'Id' available in DeviceServerSmall"))?
			.get("id")
			.ok_or_else(|| D::Error::custom("No 'id' in Id of DeviceServerSmall"))?
			.as_u64()
			.ok_or_else(|| D::Error::custom("Invalid type of 'id' in DeviceServerSmall"))?
			as u32;

		Ok(DeviceServerSmall { id: id })
	}
}

impl<'de> Deserialize<'de> for Session {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de> {
		let root = serde_json::Value::deserialize(deserializer)
			.map_err(|e| D::Error::custom(format!("Failed: {e}")))?;

		let response_elements = root.get("Response")
			.ok_or_else(|| D::Error::custom("No 'Response' in Session response"))?
			.as_array()
			.ok_or_else(|| D::Error::custom("Response field in Session was not an array"))?;
		let mut response_iter = response_elements.into_iter();

		let id = response_iter.next()
			.ok_or_else(|| D::Error::custom("Not enough elements in Session for Id"))?
			.get("Id")
			.ok_or_else(|| D::Error::custom("First element in Session did not have 'Id'"))?
			.get("id")
			.ok_or_else(|| D::Error::custom("Id in session did not have 'id'"))?
			.as_u64()
			.ok_or_else(|| D::Error::custom("'id' in Id in Session was invalid type"))?
			as u32;

		let token = serde_path_to_error::deserialize(
			response_iter.next()
				.ok_or_else(|| D::Error::custom("Not enough elements for 'Token' in Session"))?
				.get("Token")
				.ok_or_else(|| D::Error::custom("'Token' was not second element in Session response"))?
		).map_err(|e| D::Error::custom(format!("Failed to parse Token in Session: {e}")))?;

		let user_person = serde_path_to_error::deserialize(
			response_iter.next()
				.ok_or_else(|| D::Error::custom("Not enough elements in Session for UserPerson"))?
				.get("UserPerson")
				.ok_or_else(|| D::Error::custom("Third element in Session did not have 'UserPerson'"))?
		).map_err(|e| D::Error::custom(format!("Failed to parse UserPerson in Session: {e}")))?;

		Ok(Session {
			id,
			token,
			user_person,
		})
	}
}

/// Parse the string into a NaiveDateTime
pub fn deserialize_date<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
where
    D: serde::Deserializer<'de>,
{
	let s = String::deserialize(deserializer)?;
	NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f")
		.map_err(|e| D::Error::custom(format!("Incorrect datetime {s}: {}", e.to_string())))
}
