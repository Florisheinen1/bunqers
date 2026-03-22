//! Custom [`Deserialize`] implementations for types whose
//! JSON shape does not map cleanly to a Rust struct.
//!
//! Bunq uses a fixed envelope format for all responses:
//!
//! ```json
//! { "Response": [ { "TypeKey": { ...fields... } }, ... ], "Pagination": {...} }
//! ```
//!
//! Standard `#[derive(Deserialize)]` cannot handle this without custom logic,
//! so the impls in this module manually walk the JSON value tree using
//! `serde_json::Value` and `serde_path_to_error` for precise error messages.

use std::any::type_name;

use chrono::NaiveDateTime;
use serde::{Deserialize, de::Error};

use crate::types::*;

/// Deserialises [`ApiResponseBody<T>`] by checking whether the top-level
/// JSON object contains an `"Error"` key (API error) or a `"Response"` key
/// (success payload).
impl<'de, T> Deserialize<'de> for ApiResponseBody<T>
where
	T: Deserialize<'de>,
{
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		let root = serde_json::Value::deserialize(deserializer)?;

		if let Some(errors) = root.get("Error") {
			let errors: Result<Vec<ApiErrorDescription>, _> =
				serde_path_to_error::deserialize(errors);

			match errors {
				Ok(errors) => return Ok(ApiResponseBody::Err(errors)),
				Err(parse_error) => {
					return Err(D::Error::custom(format!("Errors: {parse_error}")));
				}
			}
		}

		let content: Result<T, _> = serde_path_to_error::deserialize(root);
		match content {
			Ok(content) => return Ok(ApiResponseBody::Ok(content)),
			Err(parse_error) => return Err(D::Error::custom(format!("Response: {parse_error}"))),
		}
	}
}

/// Deserialises [`Multiple<T>`] by extracting the `"Response"` array and the
/// `"Pagination"` object from the envelope.
impl<'de, T> Deserialize<'de> for Multiple<T>
where
	T: Deserialize<'de>,
{
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		let root = serde_json::Value::deserialize(deserializer).expect("Failed to parse JSON");

		let pagination_value = root
			.get("Pagination")
			.ok_or_else(|| D::Error::custom("Missing 'Pagination' in response"))?;
		let pagination = Pagination::deserialize(pagination_value.clone())
			.map_err(|e| D::Error::custom(format!("Failed to parse Pagination: {e}")))?;

		let data_value = root
			.get("Response")
			.ok_or_else(|| D::Error::custom("Missing 'Response' in response"))?;
		let data_value_array = data_value
			.as_array()
			.ok_or_else(|| D::Error::custom("'Response' was not an array"))?;
		let data: Vec<T> = data_value_array
			.iter()
			.map(|value| {
				T::deserialize(value.clone()).map_err(|e| D::Error::custom(format!("{e}")))
			})
			.collect::<Result<Vec<T>, D::Error>>()?;

		Ok(Self { data, pagination })
	}
}

/// Deserialises [`Single<T>`] by extracting the one-element `"Response"` array
/// from the envelope.
///
/// Returns an error if the `Response` array contains more than one element.
impl<'de, T> Deserialize<'de> for Single<T>
where
	T: Deserialize<'de>,
{
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		let root = serde_json::Value::deserialize(deserializer).expect("Failed to parse to JSON");

		let response_field = root
			.get("Response")
			.ok_or_else(|| D::Error::custom("Missing 'Response' field in single-item response"))?
			.clone();

		let response: Result<Vec<T>, _> = serde_path_to_error::deserialize(response_field);

		let response = match response {
			Ok(parse_success) => parse_success,
			Err(parse_error) => {
				println!(
					"Parsing of {} failed. Path: {}, error: {}",
					type_name::<T>(),
					parse_error.path(),
					parse_error
				);
				return Err(D::Error::custom(format!(
					"Failed to parse single response item: {parse_error}"
				)));
			}
		};

		if response.len() > 1 {
			return Err(D::Error::custom(format!(
				"Expected a single-item response but received {} elements",
				response.len()
			)));
		}

		let single_data = response
			.into_iter()
			.next()
			.ok_or_else(|| D::Error::custom("'Response' array was empty"))?;

		Ok(Self(single_data))
	}
}

/// Deserialises [`Installation`] by manually walking its heterogeneous
/// `Response` array: `[{Id}, {Token}, {ServerPublicKey}]`.
impl<'de> Deserialize<'de> for Installation {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		let root = serde_json::Value::deserialize(deserializer).expect("Failed to parse to JSON");

		let response_field = root
			.get("Response")
			.expect("No 'Response' field in Installation")
			.clone();
		let response_elements = response_field
			.as_array()
			.expect("'Response' field was not an array");

		let mut response_iter = response_elements.iter();

		let id: BunqId = match serde_path_to_error::deserialize(
			response_iter
				.next()
				.expect("Not enough elements in Installation response")
				.get("Id")
				.expect("No 'Id' in Installation response"),
		) {
			Ok(id) => id,
			Err(er) => {
				println!("Failed to parse: {er}");
				return Err(D::Error::custom("Failed to parse Installation Id"));
			}
		};

		let token_value = response_iter
			.next()
			.expect("Not enough elements in installation response")
			.get("Token")
			.expect("No 'Token' object in Installation response");

		let token: InstallationToken = match serde_path_to_error::deserialize(token_value) {
			Ok(token) => token,
			Err(e) => {
				println!("Failed to parse: {e}");
				return Err(D::Error::custom("Failed to parse Installation Token"));
			}
		};

		let bunq_public_key = response_iter
			.next()
			.expect("Not enough elements in installation response")
			.get("ServerPublicKey")
			.expect("No 'ServerPublicKey' in Installation response")
			.get("server_public_key")
			.expect("No 'server_public_key' inside ServerPublicKey")
			.as_str()
			.expect("'server_public_key' was not a string")
			.to_string();

		Ok(Self {
			id,
			token,
			bunq_public_key,
		})
	}
}

/// Deserialises [`DeviceServerSmall`] from the `{"Id": {"id": N}}` shape
/// returned by `POST /device-server`.
impl<'de> Deserialize<'de> for DeviceServerSmall {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		let root = serde_json::Value::deserialize(deserializer)
			.map_err(|e| D::Error::custom(format!("Failed to parse DeviceServerSmall: {e}")))?;

		let id = root
			.get("Id")
			.ok_or_else(|| D::Error::custom("No 'Id' in DeviceServerSmall response"))?
			.get("id")
			.ok_or_else(|| D::Error::custom("No 'id' inside 'Id' in DeviceServerSmall"))?
			.as_u64()
			.ok_or_else(|| D::Error::custom("'id' in DeviceServerSmall was not an integer"))?
			as u32;

		Ok(DeviceServerSmall { id })
	}
}

/// Deserialises [`Session`] by manually walking its heterogeneous `Response`
/// array: `[{Id}, {Token}, {UserPerson}]`.
impl<'de> Deserialize<'de> for Session {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		let root = serde_json::Value::deserialize(deserializer)
			.map_err(|e| D::Error::custom(format!("Failed to parse Session: {e}")))?;

		let response_elements = root
			.get("Response")
			.ok_or_else(|| D::Error::custom("No 'Response' in Session response"))?
			.as_array()
			.ok_or_else(|| D::Error::custom("'Response' in Session was not an array"))?;
		let mut response_iter = response_elements.iter();

		let id = response_iter
			.next()
			.ok_or_else(|| D::Error::custom("Not enough elements in Session for 'Id'"))?
			.get("Id")
			.ok_or_else(|| D::Error::custom("First element in Session did not have 'Id'"))?
			.get("id")
			.ok_or_else(|| D::Error::custom("'Id' in Session did not have 'id'"))?
			.as_u64()
			.ok_or_else(|| D::Error::custom("'id' in Session was not an integer"))?
			as u32;

		let token = serde_path_to_error::deserialize(
			response_iter
				.next()
				.ok_or_else(|| D::Error::custom("Not enough elements in Session for 'Token'"))?
				.get("Token")
				.ok_or_else(|| {
					D::Error::custom("Second element in Session did not have 'Token'")
				})?,
		)
		.map_err(|e| D::Error::custom(format!("Failed to parse Token in Session: {e}")))?;

		let user_person = serde_path_to_error::deserialize(
			response_iter
				.next()
				.ok_or_else(|| {
					D::Error::custom("Not enough elements in Session for 'UserPerson'")
				})?
				.get("UserPerson")
				.ok_or_else(|| {
					D::Error::custom("Third element in Session did not have 'UserPerson'")
				})?,
		)
		.map_err(|e| D::Error::custom(format!("Failed to parse UserPerson in Session: {e}")))?;

		Ok(Session {
			id,
			token,
			user_person,
		})
	}
}

/// Parses a Bunq date-time string (`"YYYY-MM-DD HH:MM:SS.f"`) into a
/// [`NaiveDateTime`].
pub fn deserialize_date<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
where
	D: serde::Deserializer<'de>,
{
	let s = String::deserialize(deserializer)?;
	NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f")
		.map_err(|e| D::Error::custom(format!("Invalid date-time '{}': {}", s, e)))
}
