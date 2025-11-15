use std::{fs::File, io::Write};

use base64::{Engine, engine::general_purpose};
use openssl::{
	hash::MessageDigest,
	pkey::{PKey, Private, Public},
	sign::{Signer, Verifier},
};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;

use crate::types::{ApiErrorDescription, ApiResponseBody};

#[derive(Debug)]
pub struct ApiErrorResponse {
	pub status_code: StatusCode,
	pub reasons: Vec<ApiErrorDescription>,
}
pub struct ApiResponse<T> {
	body: ApiResponseBody<T>,
	status_code: StatusCode,
}

impl<T> ApiResponse<T> {
	pub fn into_result(self) -> Result<T, ApiErrorResponse> {
		match self.body {
			ApiResponseBody::Ok(body) => Ok(body),
			ApiResponseBody::Err(api_error_response) => Err(ApiErrorResponse {
				status_code: self.status_code,
				reasons: api_error_response,
			}),
		}
	}
}

#[derive(Debug)]
pub enum MessageError {
	NoResponseBody,
	BodyParseError,
	RequestBuildError,
	RequestSendError,
	InvalidServerSignature { reason: String },
}

pub struct Messenger {
	base_url: String,
	app_name: String,
	http_client: reqwest::Client,

	private_sign_key: PKey<Private>,
	bunq_public_sign_key: Option<PKey<Public>>,
	authentication_token: Option<String>,
}

impl Messenger {
	pub fn new(
		base_url: String,
		app_name: String,
		private_sign_key: PKey<Private>,
		bunq_public_sign_key: Option<PKey<Public>>,
		authentication_token: Option<String>,
	) -> Self {
		Self {
			base_url,
			app_name,
			http_client: reqwest::Client::new(),
			private_sign_key,
			bunq_public_sign_key,
			authentication_token,
		}
	}

	/// Signs the provided request body with the private key
	fn sign_body(&self, body: &str) -> String {
		let mut signer = Signer::new(
			openssl::hash::MessageDigest::sha256(),
			&self.private_sign_key,
		)
		.expect("Failed to create Signer with key");

		signer
			.update(body.as_bytes())
			.expect("Failed to add body to signer");

		let signature = signer.sign_to_vec().expect("Failed to sign body");

		let signature_base64 = general_purpose::STANDARD.encode(signature);

		return signature_base64;
	}

	/// Dumps given error to the file
	fn dump_json_to_file(contents: &[u8], path: &str) -> std::io::Result<()> {
		let mut file = File::create(path)?;
		file.write_all(contents)?;
		Ok(())
	}

	/// Sends the request, and returns the parsed expected response
	/// without verifying it's signature
	pub async fn send_unverified<T>(
		&self,
		method: Method,
		endpoint: &str,
		body: Option<String>,
	) -> Result<ApiResponse<T>, MessageError>
	where
		T: DeserializeOwned,
	{
		let unverified_response = self.send_http_request(method, endpoint, body).await?;

		let response_code = unverified_response.status();
		let response_body = unverified_response
			.bytes()
			.await
			.map_err(|_| MessageError::NoResponseBody)?;

		let response_body: ApiResponseBody<T> =
			serde_json::from_slice(&response_body).map_err(|error| {
				println!("Encountered parsing error: {error}");
				println!("Dumping file to: data_dump.json");
				Self::dump_json_to_file(&response_body, "data_dump.json")
					.expect("Failed to dump JSON to file");
				MessageError::BodyParseError
			})?;

		let api_response = ApiResponse {
			body: response_body,
			status_code: response_code,
		};

		Ok(api_response)
	}

	/// Verifies the signature of given body
	fn verify_body_signature(&self, signature: &str, body: &[u8]) -> bool {
		let decoded_signature = general_purpose::STANDARD
			.decode(signature)
			.expect("Failed to decode Bunq's signature");

		let mut verifier = Verifier::new(
			MessageDigest::sha256(),
			self.bunq_public_sign_key
				.as_ref()
				.expect("Missing Bunq's public key to verify signature"),
		)
		.expect("Failed to create signature verifier");

		verifier
			.update(body)
			.expect("Failed to pass response body to signature verifier");

		let is_valid = verifier
			.verify(&decoded_signature)
			.expect("Failed to check API response's signature");

		return is_valid;
	}

	/// Builds and sends given request.
	/// Returns raw HTTP response
	pub async fn send<T>(
		&self,
		method: Method,
		endpoint: &str,
		body: Option<String>,
	) -> Result<ApiResponse<T>, MessageError>
	where
		T: DeserializeOwned,
	{
		let unverified_response = self
			.send_http_request(method, endpoint, body.clone())
			.await?;

		// Verify response signature
		let body_signature = unverified_response
			.headers()
			.get("X-Bunq-Server-Signature")
			.ok_or_else(|| MessageError::InvalidServerSignature {
				reason: format!("No key available in Bunq's response"),
			})?
			.to_str()
			.map_err(|_| MessageError::InvalidServerSignature {
				reason: format!("Failed to parse Bunq's signature to a string"),
			})?
			.to_string();

		let response_code = unverified_response.status();
		let response_body = unverified_response
			.bytes()
			.await
			.map_err(|_| MessageError::NoResponseBody)?;

		if !self.verify_body_signature(&body_signature, &response_body) {
			Err(MessageError::InvalidServerSignature {
				reason: format!("Incorrect signature in Bunq's response"),
			})?;
		}

		let api_response_body: ApiResponseBody<T> = serde_json::from_slice(&response_body)
			.map_err(|error| {
				println!("Encountered parsing error: {error}");
				println!("Dumping file to: data_dump.json");
				Self::dump_json_to_file(&response_body, "data_dump.json")
					.expect("Failed to dump JSON to file");

				MessageError::BodyParseError
			})?;

		let api_response = ApiResponse {
			body: api_response_body,
			status_code: response_code,
		};

		Ok(api_response)
	}

	// Builds and sends the request.
	// Returns the raw HTTP response.
	async fn send_http_request(
		&self,
		method: Method,
		endpoint: &str,
		body: Option<String>,
	) -> Result<reqwest::Response, MessageError> {
		let url = format!("{}/{}", self.base_url, endpoint);
		let mut request = self
			.http_client
			.request(method, url)
			.header("User-Agent", self.app_name.clone())
			.header("Cache-Control", "no-cache");

		// Attach body and corresponding signature
		if let Some(body) = body {
			let body_signature = self.sign_body(&body);

			request = request
				.header("X-Bunq-Client-Signature", body_signature)
				.body(body);
		}

		// Add authentication header
		if let Some(authentication_token) = &self.authentication_token {
			request = request.header("X-Bunq-Client-Authentication", authentication_token)
		}

		let request = request
			.build()
			.map_err(|_| MessageError::RequestBuildError)?;

		// And send it
		let response = self
			.http_client
			.execute(request)
			.await
			.map_err(|_| MessageError::RequestSendError)?;

		return Ok(response);
	}
}
