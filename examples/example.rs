use std::{env, time::Duration};

use bunqers::{
	client::{Client, SessionContext},
	client_builder::{
		ClientBuilder, Initialized, Installed, Registered, UncheckedSession, Uninitialized,
	},
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
impl From<SessionContext> for ContextStorage {
	fn from(context: SessionContext) -> Self {
		Self {
			private_key: Some(serialize_private_key(context.private_key)),
			installation_token: Some(context.installation_token),
			bunq_public_key: Some(serialize_public_key(context.bunq_public_key)),
			bunq_api_key: Some(context.bunq_api_key),
			registered_device_id: Some(context.registered_device_id),
			session_token: Some(context.session_token),
			owner_id: Some(context.owner_id),
		}
	}
}
impl From<Registered> for ContextStorage {
	fn from(context: Registered) -> Self {
		Self {
			private_key: Some(serialize_private_key(context.private_key)),
			installation_token: Some(context.installation_token),
			bunq_public_key: Some(serialize_public_key(context.bunq_public_key)),
			registered_device_id: Some(context.registered_device_id),
			..Default::default()
		}
	}
}
impl From<Installed> for ContextStorage {
	fn from(context: Installed) -> Self {
		Self {
			private_key: Some(serialize_private_key(context.private_key)),
			installation_token: Some(context.installation_token),
			bunq_public_key: Some(serialize_public_key(context.bunq_public_key)),
			..Default::default()
		}
	}
}
impl From<Initialized> for ContextStorage {
	fn from(context: Initialized) -> Self {
		return Self {
			private_key: Some(serialize_private_key(context.private_key)),
			..Default::default()
		};
	}
}

enum ContextType {
	WithSession(UncheckedSession),
	WithRegistration(Registered),
	WithInstallation(Installed),
	WithInitialization(Initialized),
	Uninitialized,
}

impl From<ContextStorage> for ContextType {
	fn from(storage: ContextStorage) -> Self {
		if let Some(session_token) = storage.session_token {
			return Self::WithSession(UncheckedSession {
				session_token,
				registered_device_id: storage
					.registered_device_id
					.expect("Cannot use session token without registering device first"),
				bunq_api_key: storage
					.bunq_api_key
					.expect("Cannot use session without having Bunq's API key"),
				installation_token: storage
					.installation_token
					.expect("Cannot use session token without installing device first"),
				bunq_public_key: parse_public_key(storage.bunq_public_key.expect(
					"Cannot use session token without having Bunq's public key (retrieved from installing)",
				)),
				private_key: parse_private_key(
					storage
						.private_key
						.expect("Cannot use session token without having a private key"),
				),
			});
		};
		if let Some(registered_device_id) = storage.registered_device_id {
			return Self::WithRegistration(Registered {
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
				private_key: parse_private_key(
					storage
						.private_key
						.expect("Cannot use device registration without having a private key"),
				),
			});
		}
		if let Some(installation_token) = storage.installation_token {
			return Self::WithInstallation(Installed {
				installation_token,
				bunq_public_key: parse_public_key(storage.bunq_public_key.expect(
					"Cannot use device installation without having Bunq's public key (retrieved from installing)",
				)),
				private_key: parse_private_key(
					storage
						.private_key
						.expect("Cannot use device installation without having a private key"),
				),
			});
		}
		if let Some(private_key) = storage.private_key {
			return Self::WithInitialization(Initialized {
				private_key: parse_private_key(private_key),
			});
		}

		Self::Uninitialized
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

async fn try_from_session(context: UncheckedSession) -> Result<Client, ()> {
	println!("Testing existing session...");
	match ClientBuilder::from_unchecked_session(context)
		.check_session()
		.await
	{
		Ok(builder) => Ok(builder.build()),
		Err(error) => {
			// If creating session failed, see if we can register this device again
			println!("Session is invalid!");
			std::thread::sleep(Duration::from_secs(3));
			Ok(try_from_registration(error.context.into()).await?)
		}
	}
}
async fn try_from_registration(context: Registered) -> Result<Client, ()> {
	println!("Trying to create new session...");
	match ClientBuilder::from_registration(context)
		.create_session()
		.await
	{
		Ok(builder) => Ok(builder.build()),
		Err(error) => {
			println!("Failed to create new session!");
			std::thread::sleep(Duration::from_secs(3));
			Ok(
				try_from_installation(error.context.bunq_api_key.clone(), error.context.into())
					.await?,
			)
		}
	}
}
async fn try_from_installation(bunq_api_key: String, context: Installed) -> Result<Client, ()> {
	println!("Trying to register device...");
	match ClientBuilder::from_installation(context)
		.register_device(bunq_api_key.clone(), format!("my-test-device"))
		.await
	{
		Ok(builder) => Ok(builder.create_session().await.map_err(|_| ())?.build()),
		Err(error) => {
			println!("Failed to register device!");
			std::thread::sleep(Duration::from_secs(3));
			Ok(try_from_initialization(bunq_api_key, error.context.into()).await?)
		}
	}
}
async fn try_from_initialization(bunq_api_key: String, context: Initialized) -> Result<Client, ()> {
	println!("Trying to install device...");
	match ClientBuilder::from_initialization(context)
		.install_device()
		.await
	{
		Ok(builder) => Ok(builder
			.register_device(bunq_api_key.clone(), format!("my-test-device"))
			.await
			.map_err(|_| ())?
			.create_session()
			.await
			.map_err(|_| ())?
			.build()),
		Err(_error) => {
			println!("Failed to install device!");
			std::thread::sleep(Duration::from_secs(3));
			Ok(try_from_uninitialized(bunq_api_key, Uninitialized).await?)
		}
	}
}
async fn try_from_uninitialized(
	bunq_api_key: String,
	_context: Uninitialized,
) -> Result<Client, ()> {
	println!("Trying to create private key...");
	match ClientBuilder::new().create_private_key() {
		Ok(builder) => Ok(builder
			.install_device()
			.await
			.map_err(|_| ())?
			.register_device(bunq_api_key.clone(), format!("my-test-device"))
			.await
			.map_err(|_| ())?
			.create_session()
			.await
			.map_err(|_| ())?
			.build()),
		Err(_error) => {
			// Well, if creating a private key fails, we really messed up somewhere
			println!("Failed to create private key!");
			Err(())
		}
	}
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
	let mut args = env::args().skip(1);
	let bunq_api_key = args.next().expect("No API key passed");
	println!("Entered API key: {bunq_api_key}");

	let storage = ContextStorage::load().await.unwrap_or_default();

	// TODO: Also store intermediate context states
	let context: ContextType = storage.into();
	let client = match context {
		ContextType::WithSession(unchecked_session) => try_from_session(unchecked_session).await,
		ContextType::WithRegistration(registered) => try_from_registration(registered).await,
		ContextType::WithInstallation(installed) => {
			try_from_installation(bunq_api_key, installed).await
		}
		ContextType::WithInitialization(initialized) => {
			try_from_initialization(bunq_api_key, initialized).await
		}
		ContextType::Uninitialized => try_from_uninitialized(bunq_api_key, Uninitialized).await,
	}
	.expect("Failed to create session and client");

	print!("Saving new session context... ");
	let new_storage = ContextStorage::from(client.context);
	new_storage.store().await;
	println!("Saved!");

	Ok(())
}
