//! HTTP layer: request signing and response signature verification.
//!
//! All communication with the Bunq API goes through [`Messenger`]. It is
//! responsible for:
//!
//! - Building requests with the correct `User-Agent`, `Cache-Control`, and
//!   authentication headers.
//! - Signing the request body with the client's RSA private key and attaching
//!   it as `X-Bunq-Client-Signature`.
//! - Attaching the current session (or installation) token as
//!   `X-Bunq-Client-Authentication`.
//! - Verifying the `X-Bunq-Server-Signature` header on every response.

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

/// An API-level error returned by Bunq (non-2xx status with an `Error` body).
#[derive(Debug)]
pub struct ApiErrorResponse {
	/// The HTTP status code of the response.
	pub status_code: StatusCode,
	/// Human-readable error descriptions from the response body.
	pub reasons: Vec<ApiErrorDescription>,
}

/// A parsed HTTP response from the Bunq API.
///
/// Call [`into_result`](ApiResponse::into_result) to unwrap the successful
/// body or surface the error. Use [`is_rate_limited`](ApiResponse::is_rate_limited)
/// to check for 429 responses before consuming the value.
#[derive(Debug)]
pub struct ApiResponse<T> {
	body: ApiResponseBody<T>,
	status_code: StatusCode,
}

impl<T> ApiResponse<T> {
	/// Returns `true` if Bunq responded with HTTP 429 Too Many Requests.
	pub fn is_rate_limited(&self) -> bool {
		self.status_code == StatusCode::TOO_MANY_REQUESTS
	}

	/// Converts the response into a `Result`.
	///
	/// Returns `Ok(T)` for a successful response or
	/// `Err(`[`ApiErrorResponse`]`)` for any API error.
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

/// Errors that can occur while sending or receiving a message.
#[derive(Debug)]
pub enum MessageError {
	/// The response had no body (only the status code is available).
	NoResponseBody(StatusCode),
	/// The response body could not be deserialised. A `data_dump.json` file
	/// is written to the working directory for debugging.
	BodyParseError,
	/// The HTTP request could not be sent (e.g. network error).
	RequestSendError,
	/// The `X-Bunq-Server-Signature` header is missing, malformed, or does not
	/// match the response body.
	InvalidServerSignature {
		reason: String,
		/// Raw API response body, for debugging.
		api_response: String,
	},
}

/// Handles all HTTP communication with the Bunq API.
///
/// Attach to a [`crate::client_builder::ClientBuilder`] via
/// [`Messenger::new`]. The authentication token and Bunq's public key are
/// updated as the builder advances through its state machine.
pub struct Messenger {
	base_url: String,
	app_name: String,
	http_client: reqwest::Client,
	/// RSA private key used to sign outgoing request bodies.
	private_sign_key: PKey<Private>,
	/// Bunq's RSA public key used to verify incoming response signatures.
	/// `None` before the `/installation` step completes.
	bunq_public_sign_key: Option<PKey<Public>>,
	/// Token sent as `X-Bunq-Client-Authentication`.
	/// `None` before the first endpoint is called.
	authentication_token: Option<String>,
}

impl Messenger {
	/// Creates a new `Messenger`.
	///
	/// `bunq_public_sign_key` and `authentication_token` may be `None` for the
	/// very first request (`/installation`), which is sent before Bunq's key is
	/// known. Update them with [`set_bunq_public_sign_key`](Self::set_bunq_public_sign_key)
	/// and [`set_authentication_token`](Self::set_authentication_token) once
	/// they are available.
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

	/// Sets the token sent as `X-Bunq-Client-Authentication`.
	pub fn set_authentication_token(&mut self, authentication_token: Option<String>) {
		self.authentication_token = authentication_token;
	}

	/// Sets Bunq's RSA public key used to verify response signatures.
	pub fn set_bunq_public_sign_key(&mut self, bunq_public_sign_key: Option<PKey<Public>>) {
		self.bunq_public_sign_key = bunq_public_sign_key;
	}

	/// Signs `body` with the client's RSA private key (SHA-256) and returns
	/// the result as a Base64-encoded string.
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

		general_purpose::STANDARD.encode(signature)
	}

	/// Writes raw bytes to `path`. Called when response parsing fails so the
	/// raw JSON can be inspected.
	fn dump_json_to_file(contents: &[u8], path: &str) -> std::io::Result<()> {
		let mut file = File::create(path)?;
		file.write_all(contents)?;
		Ok(())
	}

	/// Sends a request **without** verifying the response signature.
	///
	/// Only used for the `/installation` endpoint, which is called before
	/// Bunq's public key is known. All other requests should go through
	/// [`send`](Self::send).
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
			.map_err(|_| MessageError::NoResponseBody(response_code))?;

		let response_body: ApiResponseBody<T> =
			serde_json::from_slice(&response_body).map_err(|error| {
				println!("Encountered parsing error: {error}");
				println!("Dumping file to: data_dump.json");
				Self::dump_json_to_file(&response_body, "data_dump.json")
					.expect("Failed to dump JSON to file");
				MessageError::BodyParseError
			})?;

		Ok(ApiResponse {
			body: response_body,
			status_code: response_code,
		})
	}

	/// Verifies that `signature` (Base64-encoded) matches `body` using Bunq's
	/// public key.
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

		verifier
			.verify(&decoded_signature)
			.expect("Failed to check API response's signature")
	}

	/// Sends a request and verifies the `X-Bunq-Server-Signature` on the
	/// response.
	///
	/// Returns [`MessageError::InvalidServerSignature`] if the header is
	/// missing or the signature does not match.
	pub async fn send<T>(
		&self,
		method: Method,
		endpoint: &str,
		body: Option<String>,
	) -> Result<ApiResponse<T>, MessageError>
	where
		T: DeserializeOwned + std::fmt::Debug,
	{
		let unverified_response = self
			.send_http_request(method, endpoint, body.clone())
			.await?;

		let server_signature = unverified_response
			.headers()
			.get("X-Bunq-Server-Signature")
			.cloned();
		let response_code = unverified_response.status();
		let response_body = unverified_response
			.bytes()
			.await
			.map_err(|_| MessageError::NoResponseBody(response_code))?;

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

		// Verify the response signature before returning.
		let body_signature = server_signature
			.ok_or_else(|| MessageError::InvalidServerSignature {
				reason: "No X-Bunq-Server-Signature header in response".to_string(),
				api_response: format!("{:?}", api_response),
			})?
			.to_str()
			.map_err(|_| MessageError::InvalidServerSignature {
				reason: "X-Bunq-Server-Signature header contained non-ASCII bytes".to_string(),
				api_response: format!("{:?}", api_response),
			})?
			.to_string();

		if !self.verify_body_signature(&body_signature, &response_body) {
			return Err(MessageError::InvalidServerSignature {
				reason: "X-Bunq-Server-Signature did not match the response body".to_string(),
				api_response: format!("{:?}", api_response),
			});
		}

		Ok(api_response)
	}

	/// Builds and executes the raw HTTP request, returning the unprocessed
	/// response.
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

		// Sign the body and attach the signature header.
		if let Some(body) = body {
			let body_signature = self.sign_body(&body);
			request = request
				.header("X-Bunq-Client-Signature", body_signature)
				.body(body);
		}

		// Attach the authentication token if one is available.
		if let Some(authentication_token) = &self.authentication_token {
			request = request.header("X-Bunq-Client-Authentication", authentication_token);
		}

		let request = request.build().expect("Failed to build HTTP request");

		self.http_client
			.execute(request)
			.await
			.map_err(|_| MessageError::RequestSendError)
	}
}
