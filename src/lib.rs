//! A Rust client library for the [Bunq API](https://doc.bunq.com/).
//!
//! # Setup
//!
//! Bunq requires every application to register a device before it can use the
//! API. Registration generates an RSA key pair and calls three endpoints:
//! `/installation`, `/device-server`, and `/session-server`. The result is an
//! [`InstallationContext`] that can be serialised and stored so that future
//! runs can skip straight to creating a session.
//!
//! ## First run
//!
//! ```rust,no_run
//! use bunqers::InstallationContext;
//!
//! # #[tokio::main]
//! # async fn main() {
//! let installation: InstallationContext = bunqers::install_device(
//!     "your-api-key".into(),
//!     "https://api.bunq.com/v1".into(),
//!     "my-app".into(),
//!     "my-device".into(),
//! ).await;
//!
//! // Serialise and save `installation` to disk (e.g. as JSON).
//! # }
//! ```
//!
//! ## Subsequent runs
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() {
//! # let installation: bunqers::InstallationContext = todo!();
//! // Load `installation` from disk, then:
//! let client = bunqers::create_client(installation, None).await;
//!
//! let user = client.get_user().await.into_result().unwrap();
//! println!("Hello, {}!", user.user_person.display_name);
//! # }
//! ```
//!
//! Pass a cached session token as the second argument to [`create_client`] to
//! reuse an existing session. The token is validated; a new session is created
//! automatically if it has expired.
//!
//! # Feature flags
//!
//! | Feature | Description |
//! |---------|-------------|
//! | `ratelimited` | Enables [`client_rate_limited::ClientRateLimited`], a wrapper that queues requests through [`ritlers`](https://crates.io/crates/ritlers) and auto-retries on 429 responses |

use openssl::pkey::PKey;
use serde::{Deserialize, Serialize};

use crate::{
	client::Client,
	client_builder::{ClientBuilder, Registered, UncheckedSession},
};

pub mod client;
pub mod client_builder;
pub mod deserialization;
pub mod messenger;
pub mod types;

#[cfg(feature = "ratelimited")]
pub mod client_rate_limited;

/// All credentials needed to authenticate with the Bunq API.
///
/// Obtaining this struct requires calling three Bunq endpoints and generating
/// an RSA key pair (see [`install_device`]). Serialise it to disk so that
/// subsequent runs can skip device registration and go straight to
/// [`create_client`].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InstallationContext {
	/// Short-lived token issued by the `/installation` endpoint.
	/// Used as `X-Bunq-Client-Authentication` during device registration.
	pub installation_token: String,
	/// Bunq's RSA public key in PEM format, used to verify response signatures.
	pub bunq_public_key: String,
	/// The numeric device ID assigned by the `/device-server` endpoint.
	pub registered_device_id: u32,
	/// The Bunq API key used to register the device.
	pub bunq_api_key: String,
	/// The client's RSA private key in PKCS#8 PEM format, used to sign requests.
	pub client_private_key: String,
	/// The client's RSA public key in PEM format.
	pub client_public_key: String,
	/// Base URL of the Bunq API (e.g. `https://api.bunq.com/v1`).
	pub api_base_url: String,
	/// Application name sent as the `User-Agent` header.
	pub app_name: String,
}

/// Registers the current device with the Bunq API.
///
/// This performs the full three-step registration flow:
/// 1. Generates a 2048-bit RSA key pair.
/// 2. Calls `/installation` to exchange public keys with Bunq.
/// 3. Calls `/device-server` to link the API key to this IP address.
///
/// The returned [`InstallationContext`] should be serialised and stored on
/// disk. On subsequent runs, pass it directly to [`create_client`] — there is
/// no need to call this function again unless the device registration is
/// revoked.
///
/// # Panics
///
/// Panics if any step of the registration flow fails (key generation, network
/// error, or an API error response from Bunq).
pub async fn install_device(
	bunq_api_key: String,
	api_base_url: String,
	app_name: String,
	device_description: String,
) -> InstallationContext {
	println!("Installing device...");
	let builder = ClientBuilder::new_without_key(api_base_url.clone(), app_name.clone())
		.expect("Failed to create public and private key pair")
		.install_device()
		.await
		.expect("Failed to install device")
		.register_device(bunq_api_key, &device_description)
		.await
		.expect("Failed to register device");

	let bunq_public_key = String::from_utf8(
		builder
			.context
			.bunq_public_key
			.public_key_to_pem()
			.expect("Failed to serialize Bunq's public key"),
	)
	.expect("Bunq's public key contained non-UTF-8 characters");
	let client_private_key = String::from_utf8(
		builder
			.private_key
			.private_key_to_pem_pkcs8()
			.expect("Failed to serialize client's private key"),
	)
	.expect("Client's private key contained non-UTF-8 characters");
	let client_public_key = String::from_utf8(
		builder
			.private_key
			.public_key_to_pem()
			.expect("Failed to serialize client's public key"),
	)
	.expect("Client's public key contained non-UTF-8 characters");

	InstallationContext {
		installation_token: builder.context.installation_token,
		bunq_public_key,
		registered_device_id: builder.context.registered_device_id,
		bunq_api_key: builder.context.bunq_api_key,
		client_private_key,
		client_public_key,
		api_base_url,
		app_name,
	}
}

/// Creates a [`Client`] from a previously obtained [`InstallationContext`].
///
/// If `session_token` is `Some`, that token is validated by making a test
/// request to the API. On success the existing session is reused, avoiding an
/// unnecessary `/session-server` round-trip. If the token is invalid or
/// expired, a new session is created transparently.
///
/// If `session_token` is `None`, a fresh session is always created.
///
/// # Panics
///
/// Panics if session creation fails (e.g. if the device registration has been
/// revoked).
pub async fn create_client(
	installation_context: InstallationContext,
	session_token: Option<String>,
) -> Client {
	let bunq_public_key =
		PKey::public_key_from_pem(installation_context.bunq_public_key.as_bytes())
			.expect("Failed to parse Bunq's public key");

	let client_private_key =
		PKey::private_key_from_pem(installation_context.client_private_key.as_bytes())
			.expect("Failed to parse Client's private key");

	if let Some(session_token) = session_token {
		// Attempt to reuse the provided session token.
		let unchecked_session = UncheckedSession {
			session_token: session_token,
			registered_device_id: installation_context.registered_device_id,
			bunq_api_key: installation_context.bunq_api_key.clone(),
			installation_token: installation_context.installation_token.clone(),
			bunq_public_key: bunq_public_key.clone(),
		};
		let checked_session = ClientBuilder::from_unchecked_session(
			unchecked_session,
			installation_context.api_base_url.clone(),
			installation_context.app_name.clone(),
			client_private_key.clone(),
		)
		.check_session()
		.await;

		if let Ok(checked_session) = checked_session {
			return checked_session.build();
		} else {
			println!("Provided session was invalid.");
		}
	};
	println!("Creating new session...");

	let registration_data = Registered {
		registered_device_id: installation_context.registered_device_id,
		bunq_api_key: installation_context.bunq_api_key,
		installation_token: installation_context.installation_token,
		bunq_public_key,
	};

	ClientBuilder::from_registration(
		registration_data,
		installation_context.api_base_url,
		installation_context.app_name,
		client_private_key,
	)
	.create_session()
	.await
	.expect("Failed to create session. Is the installation invalidated?")
	.build()
}
