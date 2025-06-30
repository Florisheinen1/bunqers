use std::{fs::File, io::Write};

use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use openssl::{pkey::{PKey, Private}, sign::Signer};
use base64::{engine::general_purpose, Engine};

use crate::types::ApiResponseBody;

#[derive(Debug)]
pub struct Response<T> {
	pub body: ApiResponseBody<T>,
	pub code: StatusCode
}

pub struct Messenger {
	base_url: String,
	app_name: String,
	http_client: reqwest::Client,

	authentication_header: Option<String>,
	private_sign_key: PKey<Private>,
}

impl Messenger {
	pub fn new(base_url: String, app_name: String, private_sign_key: PKey<Private>) -> Self {
		Self {
			base_url,
			app_name,
			http_client: reqwest::Client::new(),

			authentication_header: None,
			private_sign_key
		}
	}

	/// Updates the auth header of this Messenger
	pub fn set_auth_header(&mut self, new_auth_header: Option<String>) {
		self.authentication_header = new_auth_header;
	}

	/// Signs the provided request body with the private key
	fn sign_body(&self, body: &str) -> String {
		let mut signer = Signer::new(
			openssl::hash::MessageDigest::sha256(), 
			&self.private_sign_key)
		.expect("Failed to create Signer with key");

		signer.update(body.as_bytes()).expect("Failed to add body to signer");
		
		let signature = signer.sign_to_vec().expect("Failed to sign body");
		
		let signature_base64 = general_purpose::STANDARD.encode(signature);

		return signature_base64;
	}

	/// Dumps given error to the file
	fn dump_json_to_file(contents: &str, path: &str) -> std::io::Result<()> {
		let mut file = File::create(path)?;
		file.write_all(contents.as_bytes())?;
		Ok(())
	}

	pub async fn send<T>(&self, method: Method, endpoint: &str, body: Option<String>) -> Response<T> 
	where T: DeserializeOwned {
		
		let url = format!("{}/{}", self.base_url, endpoint);
		let mut request = match method {
			Method::POST => self.http_client.post(url),
			Method::GET => self.http_client.get(url),
			Method::PUT => self.http_client.put(url),
			_ => todo!("HTTP method not yet implemented")
		}.header("User-Agent", self.app_name.clone())
		.header("Cache-Control", "no-cache");

		// Attach body and corresponding signature
		if let Some(body) = body {
			let body_signature = self.sign_body(&body);

			request = request
				.header("X-Bunq-Client-Signature", body_signature)
				.body(body);
		}

		// Add authentication header
		if let Some(authentication_token) = &self.authentication_header {
			request = request.header("X-Bunq-Client-Authentication", authentication_token)
		}

		let request = request.build().expect("Failed to build request");

		// And send it
		let response = self.http_client.execute(request).await.expect("Failed to send API request");

		let response_code = response.status();
		let response_body = response.text().await.expect("Failed to retrieve body from API response");

		let api_response: Result<ApiResponseBody<T>, _> = serde_json::from_str(&response_body);

		let body = match api_response {
			Ok(body) => body,
			Err(parse_error) => {
				println!("Encountered parsing error: {parse_error}");
				println!("Dumping file to: data_dump.json");
				Self::dump_json_to_file(&response_body, "data_dump.json").expect("Failed to dump JSON to file");
				panic!("Failed");
			},
		};

		return Response {
			body,
			code: response_code,
		}
	}
}