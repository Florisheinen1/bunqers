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

/// Serializable installation context
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InstallationContext {
	pub installation_token: String,
	pub bunq_public_key: String,
	pub registered_device_id: u32,
	pub bunq_api_key: String,
	pub client_private_key: String,
	pub client_public_key: String,
	pub api_base_url: String,
	pub app_name: String,
}

/// Installs the current device with the given API key.
/// Creates a new public and private key pair
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
			.expect("Failed to serialize client's private key"),
	)
	.expect("Client's private key contained non-UTF-8 characters");

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

/// Creates a client with the given installation context.
/// If a session context is provided, that session will be reused.
/// Ensures that the created client as a valid session
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
		// Try to reuse the given session
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

	let registered_client = ClientBuilder::from_registration(
		registration_data,
		installation_context.api_base_url,
		installation_context.app_name,
		client_private_key,
	);

	return registered_client
		.create_session()
		.await
		.expect("Failed to create session. Is the installation invalidated?")
		.build();
}
