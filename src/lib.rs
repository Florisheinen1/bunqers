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
//! ## Rate-limited client (`ratelimited` feature)
//!
//! Enable the `ratelimited` feature to get [`create_rate_limited_client`],
//! which wraps the client with pre-configured rate limiters so you don't need
//! to depend on `ritlers` directly:
//!
//! ```rust,no_run
//! use std::sync::Arc;
//!
//! # #[tokio::main]
//! # async fn main() {
//! # let installation: bunqers::InstallationContext = todo!();
//! let client_rl = Arc::new(bunqers::create_rate_limited_client(installation, None).await);
//!
//! client_rl.get_user_ratelimited(|response| async move {
//!     let user = response.into_result().expect("API error");
//!     println!("Hello, {}!", user.user_person.display_name);
//! }).await;
//! # }
//! ```
//!
//! # Feature flags
//!
//! | Feature | Description |
//! |---------|-------------|
//! | `ratelimited` | Enables [`create_rate_limited_client`] and [`client_rate_limited::ClientRateLimited`], which queue requests through [`ritlers`](https://crates.io/crates/ritlers) and auto-retry on 429 responses |

use openssl::pkey::PKey;
use serde::{Deserialize, Serialize};

use crate::{
	client::Client,
	client_builder::{ClientBuilder, Registered, UncheckedSession},
};

#[cfg(feature = "ratelimited")]
use std::time::Duration;

#[cfg(feature = "ratelimited")]
use ritlers::async_rt::RateLimiter;

#[cfg(feature = "ratelimited")]
use crate::client_rate_limited::ClientRateLimited;

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

/// Creates a [`ClientRateLimited`] from a previously obtained [`InstallationContext`].
///
/// This is the recommended entry point when you want automatic rate limiting.
/// It behaves identically to [`create_client`] for session handling, then wraps
/// the resulting client with pre-configured rate limiters so callers do not need
/// to depend on `ritlers` directly.
///
/// The default limits match Bunq's documented per-device quotas:
/// - **GET**: 3 requests per 3 seconds
/// - **POST**: 5 requests per 3 seconds
///
/// 429 responses are retried automatically by the returned client — no extra
/// configuration is required.
///
/// If `session_token` is `Some`, that token is validated and reused if still
/// valid. If it has expired, or if `session_token` is `None`, a new session is
/// created.
///
/// # Panics
///
/// Panics if session creation fails or if the rate limiters cannot be
/// initialised (neither should happen under normal conditions).
///
/// # Example
///
/// ```rust,no_run
/// use std::sync::Arc;
///
/// # #[tokio::main]
/// # async fn main() {
/// # let installation: bunqers::InstallationContext = todo!();
/// let client_rl = Arc::new(bunqers::create_rate_limited_client(installation, None).await);
///
/// client_rl.get_user_ratelimited(|response| async move {
///     let user = response.into_result().expect("API error");
///     println!("Hello, {}!", user.user_person.display_name);
/// }).await;
/// # }
/// ```
#[cfg(feature = "ratelimited")]
pub async fn create_rate_limited_client(
	installation_context: InstallationContext,
	session_token: Option<String>,
	max_retries: u32,
) -> ClientRateLimited {
	let client = create_client(installation_context, session_token).await;
	ClientRateLimited {
		client,
		ratelimiter_get: RateLimiter::new(3, Duration::from_secs(3))
			.expect("Failed to create GET rate limiter"),
		ratelimiter_post: RateLimiter::new(5, Duration::from_secs(3))
			.expect("Failed to create POST rate limiter"),
		ratelimiter_put: RateLimiter::new(2, Duration::from_secs(3))
			.expect("Failed to create PUT rate limiter"),
		max_retries,
	}
}
