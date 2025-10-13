use std::{fs::File, io::Write, thread::sleep, time::Duration};

use base64::{Engine, engine::general_purpose};
use openssl::{
	hash::MessageDigest,
	pkey::{PKey, Private, Public},
	sign::{Signer, Verifier},
};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;

use crate::types::ApiResponseBody;

#[derive(Debug)]
pub struct Response<T> {
	pub body: ApiResponseBody<T>,
	pub code: StatusCode,
}

#[derive(Debug)]
pub struct NotVerified;
#[derive(Debug)]
pub struct Verified {
	bunq_public_key: PKey<Public>,
}

pub struct Messenger<V> {
	base_url: String,
	app_name: String,
	http_client: reqwest::Client,

	private_sign_key: PKey<Private>,
	authentication_token: Option<String>,

	verification_data: V,
}

impl<V> Messenger<V> {
	pub fn set_authentication_token(&mut self, token: String) {
		self.authentication_token = Some(token);
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

	/// Sends the request, and returns the parsed expected response without verifying it's signature
	pub async fn send_unverified<T>(
		&self,
		method: Method,
		endpoint: &str,
		body: Option<String>,
	) -> Response<T>
	where
		T: DeserializeOwned,
	{
		let unverified_response = self.send_request(method, endpoint, body).await;

		let response_code = unverified_response.status();
		let response_body = unverified_response
			.bytes()
			.await
			.expect("Failed to retrieve body from API response");

		let api_response: Result<ApiResponseBody<T>, _> = serde_json::from_slice(&response_body);

		let body = match api_response {
			Ok(body) => body,
			Err(parse_error) => {
				println!("Encountered parsing error: {parse_error}");
				println!("Dumping file to: data_dump.json");
				Self::dump_json_to_file(&response_body, "data_dump.json")
					.expect("Failed to dump JSON to file");
				panic!("Failed");
			}
		};

		return Response {
			body,
			code: response_code,
		};
	}

	// Simply sends the request, returns the http response
	async fn send_request(
		&self,
		method: Method,
		endpoint: &str,
		body: Option<String>,
	) -> reqwest::Response {
		let url = format!("{}/{}", self.base_url, endpoint);
		let mut request = match method {
			Method::POST => self.http_client.post(url),
			Method::GET => self.http_client.get(url),
			Method::PUT => self.http_client.put(url),
			_ => todo!("HTTP method not yet implemented"),
		}
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

		let request = request.build().expect("Failed to build request");

		// And send it
		let response = self
			.http_client
			.execute(request)
			.await
			.expect("Failed to send API request");

		return response;
	}
}

impl Messenger<NotVerified> {
	pub fn new(base_url: String, app_name: String, private_sign_key: PKey<Private>) -> Self {
		Self {
			base_url,
			app_name,
			http_client: reqwest::Client::new(),
			private_sign_key,
			verification_data: NotVerified,
			authentication_token: None,
		}
	}

	pub fn make_verified(self, bunq_public_key: PKey<Public>) -> Messenger<Verified> {
		Messenger {
			base_url: self.base_url,
			app_name: self.app_name,
			http_client: self.http_client,
			private_sign_key: self.private_sign_key,
			verification_data: Verified { bunq_public_key },
			authentication_token: self.authentication_token,
		}
	}
}

impl Messenger<Verified> {
	/// Verifies the signature of given body
	fn verify_body_signature(&self, signature: &str, body: &[u8]) -> bool {
		let decoded_signature = general_purpose::STANDARD
			.decode(signature)
			.expect("Failed to decode Bunq's signature");

		let mut verifier = Verifier::new(
			MessageDigest::sha256(),
			&self.verification_data.bunq_public_key,
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

	/// Sends the request, and returns the verified parsed expected response
	pub async fn send<T>(&self, method: Method, endpoint: &str, body: Option<String>) -> Response<T>
	where
		T: DeserializeOwned,
	{
		let unverified_response = loop {
			let response = self
				.send_request(method.clone(), endpoint, body.clone())
				.await;

			// If we encountered rate limit (status code 429):
			if response.status().as_u16() == 429 {
				println!(
					"RATE LIMIT ERROR! Method: {method}, endpoint: {endpoint}, body: {body:?}"
				);
				println!(
					"RATE LIMIT RESPONSE: {}",
					response
						.text()
						.await
						.expect("Failed to get response of body")
				);
				println!("Sleeping for a bit...");
				sleep(Duration::from_secs(3));
				println!("Resending");
			} else {
				break response;
			}
		};

		let body_signature = unverified_response
			.headers()
			.get("X-Bunq-Server-Signature")
			.expect("No Server signature available. Cannot validate response")
			.to_str()
			.expect("Failed to convert Bunq's response signature to a string")
			.to_string();

		let response_code = unverified_response.status();
		let response_body = unverified_response
			.bytes()
			.await
			.expect("Failed to retrieve body from API response");

		if !self.verify_body_signature(&body_signature, &response_body) {
			panic!("Received invalid signature of response from Bunq");
		}

		let api_response: Result<ApiResponseBody<T>, _> = serde_json::from_slice(&response_body);

		let body = match api_response {
			Ok(body) => body,
			Err(parse_error) => {
				println!("Encountered parsing error: {parse_error}");
				println!("Dumping file to: data_dump.json");
				Self::dump_json_to_file(&response_body, "data_dump.json")
					.expect("Failed to dump JSON to file");
				panic!("Failed");
			}
		};

		return Response {
			body,
			code: response_code,
		};
	}
}
