//! This document shows an extensive example
//! It shows:
//! - Storing/loading context data
//! - Recovering from old session data at any point
//! - Fetching userdata

use std::{env, time::Duration};

use bunqers::{
	client::{Client, SessionContext},
	client_builder::{ClientBuilder, Installed, Registered, UncheckedSession},
};
use openssl::{
	pkey::{PKey, Private, Public},
	rsa::Rsa,
};
use serde::{Deserialize, Serialize};
use tokio::fs;

const CONTEXT_FILENAME: &'static str = "context.json";

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct ContextStorage {
	private_key: Option<String>,
	installation_token: Option<String>,
	bunq_public_key: Option<String>,
	bunq_api_key: Option<String>,
	registered_device_id: Option<u32>,
	session_token: Option<String>,
	owner_id: Option<u32>,
}
impl ContextStorage {
	fn from_session(context: SessionContext, private_key: PKey<Private>) -> Self {
		Self {
			private_key: Some(serialize_private_key(private_key)),
			installation_token: Some(context.installation_token),
			bunq_public_key: Some(serialize_public_key(context.bunq_public_key)),
			bunq_api_key: Some(context.bunq_api_key),
			registered_device_id: Some(context.registered_device_id),
			session_token: Some(context.session_token),
			owner_id: Some(context.owner_id),
		}
	}

	fn from_registration(context: Registered, private_key: PKey<Private>) -> Self {
		Self {
			private_key: Some(serialize_private_key(private_key)),
			installation_token: Some(context.installation_token),
			bunq_public_key: Some(serialize_public_key(context.bunq_public_key)),
			registered_device_id: Some(context.registered_device_id),
			..Default::default()
		}
	}
	fn from_installation(context: Installed, private_key: PKey<Private>) -> Self {
		Self {
			private_key: Some(serialize_private_key(private_key)),
			installation_token: Some(context.installation_token),
			bunq_public_key: Some(serialize_public_key(context.bunq_public_key)),
			..Default::default()
		}
	}
}

enum ContextType {
	WithSession(UncheckedSession),
	WithRegistration(Registered),
	WithInstallation(Installed),
	Uninitialized,
}

impl ContextType {
	fn from_storage(storage: ContextStorage) -> Option<(Self, PKey<Private>)> {
		if let Some(session_token) = storage.session_token {
			// Looks like a session was used before
			let registered_device_id = storage
				.registered_device_id
				.expect("Cannot use session token without registering device first");
			let bunq_api_key = storage
				.bunq_api_key
				.expect("Cannot use session without having Bunq's API key");
			let installation_token = storage
				.installation_token
				.expect("Cannot use session token without installing device first");
			let bunq_public_key = parse_public_key(storage.bunq_public_key.expect(
				"Cannot use session token without having Bunq's public key (retrieved from installing)",
			));
			let private_key = parse_private_key(
				storage
					.private_key
					.expect("Cannot use session token without having a private key"),
			);

			return Some((
				Self::WithSession(UncheckedSession {
					session_token,
					registered_device_id,
					bunq_api_key,
					installation_token,
					bunq_public_key,
				}),
				private_key,
			));
		};
		if let Some(registered_device_id) = storage.registered_device_id {
			return Some((
				Self::WithRegistration(Registered {
					registered_device_id,
					bunq_api_key: storage
						.bunq_api_key
						.expect("Cannot use device registration without having Bunq's API key"),
					installation_token: storage
						.installation_token
						.expect("Cannot use device registration without installing device first"),
					bunq_public_key: parse_public_key(storage.bunq_public_key.expect(
						"Cannot use device registration without having Bunq's public key (retrieved from installing)",
					)),
				}),
				parse_private_key(
					storage
						.private_key
						.expect("Cannot use device registration without having a private key"),
				),
			));
		}
		if let Some(installation_token) = storage.installation_token {
			return Some((
				Self::WithInstallation(Installed {
					installation_token,
					bunq_public_key: parse_public_key(storage.bunq_public_key.expect(
						"Cannot use device installation without having Bunq's public key (retrieved from installing)",
					)),
				}),
				parse_private_key(
					storage
						.private_key
						.expect("Cannot use device installation without having a private key"),
				),
			));
		}
		if let Some(private_key) = storage.private_key {
			return Some((Self::Uninitialized, parse_private_key(private_key)));
		}

		None
	}
}

impl ContextStorage {
	async fn store(&self) {
		let json = serde_json::to_string(self).expect("Failed to serialize installation");
		fs::write(CONTEXT_FILENAME, json)
			.await
			.expect("Failed to store context file");
	}
	async fn load() -> Result<Self, ()> {
		let bytes = fs::read(CONTEXT_FILENAME).await.map_err(|_| ())?;
		serde_json::from_slice(&bytes).map_err(|_| ())
	}
}

fn parse_public_key(text: String) -> PKey<Public> {
	PKey::from_rsa(Rsa::public_key_from_pem(text.as_bytes()).expect("Failed to parse public key"))
		.expect("Failed to parse public key")
}
fn parse_private_key(text: String) -> PKey<Private> {
	PKey::from_rsa(Rsa::private_key_from_pem(text.as_bytes()).expect("Failed to parse private key"))
		.expect("Failed to parse private key")
}
fn serialize_public_key(key: PKey<Public>) -> String {
	String::from_utf8_lossy(
		&key.public_key_to_pem()
			.expect("Failed to serialize public key"),
	)
	.to_string()
}
fn serialize_private_key(key: PKey<Private>) -> String {
	String::from_utf8_lossy(
		&key.private_key_to_pem_pkcs8()
			.expect("Failed to serialize private key"),
	)
	.to_string()
}

/// Tries using the given session to build a Client.
/// If it fails, it will retry registering, installing and creating a new private key
async fn try_reuse_session(
	context: UncheckedSession,
	api_base_url: String,
	app_name: String,
	device_description: &str,
	private_key: PKey<Private>,
) -> Client {
	print!("Checking session... ");
	match ClientBuilder::from_unchecked_session(
		context,
		api_base_url.clone(),
		app_name.clone(),
		private_key.clone(),
	)
	.check_session()
	.await
	{
		Ok(builder) => {
			println!("Session is valid!");
			builder.build()
		}
		Err(error) => {
			// If the session is not valid, try creating a new session
			println!("Session is invalid!");
			std::thread::sleep(Duration::from_secs(3));
			return try_use_registration(
				error.context.into(),
				api_base_url,
				app_name,
				device_description,
				private_key,
			)
			.await;
		}
	}
}

/// Tries creating a new session with a registration
async fn try_use_registration(
	context: Registered,
	api_base_url: String,
	app_name: String,
	device_description: &str,
	private_key: PKey<Private>,
) -> Client {
	print!("Creating new session... ");
	match ClientBuilder::from_registration(
		context,
		api_base_url.clone(),
		app_name.clone(),
		private_key.clone(),
	)
	.create_session()
	.await
	{
		Ok(builder) => {
			ContextStorage::from_session(builder.context.clone(), builder.private_key.clone())
				.store()
				.await;
			println!("Created session!");

			builder.build()
		}
		Err(error) => {
			// If creating a session failed, try registering this device again
			println!("Failed to create session!");
			std::thread::sleep(Duration::from_secs(3));
			return try_use_installation(
				error.context.clone().into(),
				error.context.bunq_api_key,
				api_base_url,
				app_name,
				device_description,
				private_key,
			)
			.await;
		}
	}
}

/// Tries registering this device with given installation
async fn try_use_installation(
	context: Installed,
	bunq_api_key: String,
	api_base_url: String,
	app_name: String,
	device_description: &str,
	private_key: PKey<Private>,
) -> Client {
	print!("Registering device... ");
	match ClientBuilder::from_installation(
		context,
		api_base_url.clone(),
		app_name.clone(),
		private_key.clone(),
	)
	.register_device(bunq_api_key.clone(), device_description)
	.await
	{
		Ok(builder) => {
			ContextStorage::from_registration(builder.context.clone(), builder.private_key.clone())
				.store()
				.await;
			println!("Registered device!");

			println!("-> Creating new session...");
			let builder = builder
				.create_session()
				.await
				.expect("Failed to create session!");
			ContextStorage::from_session(builder.context.clone(), builder.private_key.clone())
				.store()
				.await;

			builder.build()
		}
		Err(_error) => {
			// Failed to register device, try to install device again
			// with existing private key
			println!("Failed to register device!");
			std::thread::sleep(Duration::from_secs(3));
			return try_install_with_existing_key(
				api_base_url,
				app_name,
				device_description,
				bunq_api_key,
				private_key,
			)
			.await;
		}
	}
}

/// Tries to create a session with a complete new client builder while
/// reusing existing private key
async fn try_install_with_existing_key(
	api_base_url: String,
	app_name: String,
	device_description: &str,
	bunq_api_key: String,
	private_key: PKey<Private>,
) -> Client {
	print!("Installing device... ");
	match ClientBuilder::new_with_key(api_base_url.clone(), app_name.clone(), private_key.clone())
		.install_device()
		.await
	{
		Ok(builder) => {
			ContextStorage::from_installation(builder.context.clone(), builder.private_key.clone())
				.store()
				.await;
			println!("Installed device!");

			println!("-> Registering device...");
			let builder = builder
				.register_device(bunq_api_key.clone(), device_description)
				.await
				.expect("Failed to register device!");
			ContextStorage::from_registration(builder.context.clone(), builder.private_key.clone())
				.store()
				.await;

			println!("-> Creating session...");
			let builder = builder
				.create_session()
				.await
				.expect("Failed to create session!");
			ContextStorage::from_session(builder.context.clone(), builder.private_key.clone())
				.store()
				.await;

			builder.build()
		}
		Err(_error) => {
			println!("Failed to install device!");
			std::thread::sleep(Duration::from_secs(3));
			return try_install_with_new_key(
				api_base_url,
				app_name,
				device_description,
				bunq_api_key,
			)
			.await;
		}
	}
}

/// Tries to create a session with a complete new client builder
async fn try_install_with_new_key(
	api_base_url: String,
	app_name: String,
	device_description: &str,
	bunq_api_key: String,
) -> Client {
	println!("Starting from scratch!");
	println!("-> Creating private key...");
	let builder = ClientBuilder::new_without_key(api_base_url, app_name)
		.expect("Failed to create private key");

	println!("-> Installing device...");
	let builder = builder
		.install_device()
		.await
		.expect("Failed to install device!");
	ContextStorage::from_installation(builder.context.clone(), builder.private_key.clone())
		.store()
		.await;

	println!("-> Registering device...");
	let builder = builder
		.register_device(bunq_api_key.clone(), device_description)
		.await
		.expect("Failed to register device!");
	ContextStorage::from_registration(builder.context.clone(), builder.private_key.clone())
		.store()
		.await;

	println!("-> Creating session...");
	let builder = builder
		.create_session()
		.await
		.expect("Failed to create session!");
	ContextStorage::from_session(builder.context.clone(), builder.private_key.clone())
		.store()
		.await;

	builder.build()
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
	let mut args = env::args().skip(1);
	let bunq_api_key = args.next().expect("No API key passed as parameter");
	println!("Entered API key: {bunq_api_key}");

	let storage = ContextStorage::load().await.unwrap_or_default();

	let app_name = "example-app-name".into();
	let api_base_url = "https://api.bunq.com/v1".into();
	let device_description = "my-test-device";

	let context = ContextType::from_storage(storage);

	let client = match context {
		Some((context, private_key)) => match context {
			ContextType::WithSession(unchecked_session) => {
				try_reuse_session(
					unchecked_session,
					api_base_url,
					app_name,
					device_description,
					private_key,
				)
				.await
			}
			ContextType::WithRegistration(registered) => {
				try_use_registration(
					registered,
					api_base_url,
					app_name,
					device_description,
					private_key,
				)
				.await
			}
			ContextType::WithInstallation(installed) => {
				try_use_installation(
					installed,
					bunq_api_key,
					api_base_url,
					app_name,
					device_description,
					private_key,
				)
				.await
			}
			ContextType::Uninitialized => {
				try_install_with_existing_key(
					api_base_url,
					app_name,
					device_description,
					bunq_api_key,
					private_key,
				)
				.await
			}
		},
		None => {
			// Nothing was found, create from scratch
			try_install_with_new_key(api_base_url, app_name, device_description, bunq_api_key).await
		}
	};

	println!("Succesfully created Client with valid session");

	// Fetch user
	std::thread::sleep(Duration::from_secs(3));
	println!(
		"Hello, {}!",
		client
			.get_user()
			.await
			.into_result()
			.expect("Failed to fetch userdata")
			.user_person
			.display_name
	);

	let new_storage = ContextStorage::from_session(client.context, client.private_key);
	new_storage.store().await;
	println!("You can view updated context data in: {}", CONTEXT_FILENAME);

	Ok(())
}
