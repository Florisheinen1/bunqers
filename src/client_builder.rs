use openssl::{
	error::ErrorStack,
	pkey::{PKey, Private, Public},
	rsa::Rsa,
};
use reqwest::Method;

use crate::{
	client::{Client, SessionContext},
	messenger::{ApiResponse, Messenger},
	types::{
		CreateDeviceServer, CreateInstallation, CreateSession, DeviceServerSmall, Installation,
		Session as BunqSession, Single, User,
	},
};

impl From<SessionContext> for UncheckedSession {
	fn from(context: SessionContext) -> Self {
		Self {
			session_token: context.session_token,
			registered_device_id: context.registered_device_id,
			bunq_api_key: context.bunq_api_key,
			installation_token: context.installation_token,
			bunq_public_key: context.bunq_public_key,
			private_key: context.private_key,
		}
	}
}

/// Has a session, but we are unsure if it is still valid
pub struct UncheckedSession {
	pub session_token: String,
	pub registered_device_id: u32,
	pub bunq_api_key: String,
	pub installation_token: String,
	pub bunq_public_key: PKey<Public>,
	pub private_key: PKey<Private>,
}

impl From<UncheckedSession> for Registered {
	fn from(context: UncheckedSession) -> Self {
		Self {
			installation_token: context.installation_token,
			registered_device_id: context.registered_device_id,
			bunq_api_key: context.bunq_api_key,
			bunq_public_key: context.bunq_public_key,
			private_key: context.private_key,
		}
	}
}

/// Fully ready to create a session
#[derive(Clone)]
pub struct Registered {
	pub registered_device_id: u32,
	pub bunq_api_key: String,
	pub installation_token: String,
	pub bunq_public_key: PKey<Public>,
	pub private_key: PKey<Private>,
}

impl From<Registered> for Installed {
	fn from(context: Registered) -> Self {
		Self {
			installation_token: context.installation_token,
			bunq_public_key: context.bunq_public_key,
			private_key: context.private_key,
		}
	}
}

#[derive(Clone)]
pub struct Installed {
	pub installation_token: String,
	pub bunq_public_key: PKey<Public>,
	pub private_key: PKey<Private>,
}

impl From<Installed> for Initialized {
	fn from(context: Installed) -> Self {
		Self {
			private_key: context.private_key,
		}
	}
}

#[derive(Clone)]
pub struct Initialized {
	pub private_key: PKey<Private>,
}
#[derive(Clone)]
pub struct Uninitialized;

pub struct ClientBuilder<T> {
	pub context: T,
}

pub struct BuildError<T> {
	pub reason: BuildErrorReason,
	pub context: T,
}

pub enum BuildErrorReason {
	KeyCreationError(ErrorStack),
	KeySerialization(ErrorStack),
	KeyDeserializationError(ErrorStack),
	BunqRequestError,
	BunqResponseError, // TODO: Create better response error
}

impl ClientBuilder<Uninitialized> {
	/// Creates a new Client builder
	pub fn new() -> Self {
		Self {
			context: Uninitialized,
		}
	}

	/// Adds the given private key to the ClientBuilder
	/// Used for signing messages to the API
	pub fn with_private_key(self, private_key: PKey<Private>) -> ClientBuilder<Initialized> {
		ClientBuilder {
			context: Initialized { private_key },
		}
	}

	/// Creates a new private key for signing messages for the API
	pub fn create_private_key(
		self,
	) -> Result<ClientBuilder<Initialized>, BuildError<Uninitialized>> {
		let new_key = Rsa::generate(2048).map_err(|error| BuildError {
			reason: BuildErrorReason::KeyCreationError(error),
			context: self.context.clone(),
		})?;
		let private_key = PKey::from_rsa(new_key).map_err(|error| BuildError {
			reason: BuildErrorReason::KeyCreationError(error),
			context: self.context.clone(),
		})?;

		Ok(self.with_private_key(private_key))
	}
}

impl ClientBuilder<Initialized> {
	/// Creates an initialized client builder with given context
	pub fn from_initialization(context: Initialized) -> Self {
		Self { context }
	}

	/// Installs this computer
	/// By sending our public key and fetching the API's public key
	pub async fn install_device(self) -> Result<ClientBuilder<Installed>, BuildError<Initialized>> {
		let messenger = Messenger::new(
			format!("https://api.bunq.com/v1/"),
			format!("bunqers-sdk-test"),
			self.context.private_key.clone(),
		);

		let body = CreateInstallation {
			client_public_key: String::from_utf8_lossy(
				&self
					.context
					.private_key
					.public_key_to_pem()
					.map_err(|error| BuildError {
						reason: BuildErrorReason::KeySerialization(error),
						context: self.context.clone(),
					})?,
			)
			.to_string(),
		};

		let body = serde_json::to_string(&body).map_err(|_| BuildError {
			reason: BuildErrorReason::BunqRequestError,
			context: self.context.clone(),
		})?;

		let response: ApiResponse<Installation> = messenger
			.send_unverified(Method::POST, "installation", Some(body))
			.await
			.map_err(|_| BuildError {
				reason: BuildErrorReason::BunqResponseError,
				context: self.context.clone(),
			})?;

		let result = response.into_result().map_err(|_| BuildError {
			reason: BuildErrorReason::BunqResponseError,
			context: self.context.clone(),
		})?;

		// Parse Bunq's public key
		let bunq_public_key =
			Rsa::public_key_from_pem(result.bunq_public_key.as_bytes()).map_err(|error| {
				BuildError {
					reason: BuildErrorReason::KeyDeserializationError(error),
					context: self.context.clone(),
				}
			})?;
		let bunq_public_key = PKey::from_rsa(bunq_public_key).map_err(|error| BuildError {
			reason: BuildErrorReason::KeyDeserializationError(error),
			context: self.context.clone(),
		})?;

		Ok(ClientBuilder {
			context: Installed {
				installation_token: result.token.token,
				bunq_public_key,
				private_key: self.context.private_key,
			},
		})
	}
}

impl ClientBuilder<Installed> {
	/// Creates an installed Client builder from given context
	pub fn from_installation(context: Installed) -> Self {
		Self { context }
	}

	/// Registers this computer with the API
	/// By linking the given API key to the current IP address
	pub async fn register_device(
		self,
		bunq_api_key: String,
		device_description: String,
	) -> Result<ClientBuilder<Registered>, BuildError<Installed>> {
		let mut messenger = Messenger::new(
			format!("https://api.bunq.com/v1/"),
			format!("bunqers-sdk-test"),
			self.context.private_key.clone(),
		)
		.make_verified(self.context.bunq_public_key.clone());
		messenger.set_authentication_token(self.context.installation_token.clone());

		let body = CreateDeviceServer {
			bunq_api_key: bunq_api_key.clone(),
			description: device_description,
			permitted_ips: Vec::new(),
		};

		let body = serde_json::to_string(&body).map_err(|_| BuildError {
			reason: BuildErrorReason::BunqRequestError,
			context: self.context.clone(),
		})?;

		let response: ApiResponse<Single<DeviceServerSmall>> = messenger
			.send(Method::POST, "device-server", Some(body))
			.await
			.map_err(|_| BuildError {
				reason: BuildErrorReason::BunqResponseError,
				context: self.context.clone(),
			})?;
		let result = response.into_result().map_err(|error| {
			println!(
				"Status: {:?}, descriptions: {:?}",
				error.status_code, error.reasons
			);
			panic!("Failed");
		})?;
		let registered_device_id = result.id;

		Ok(ClientBuilder {
			context: Registered {
				registered_device_id,
				bunq_api_key,
				installation_token: self.context.installation_token,
				bunq_public_key: self.context.bunq_public_key,
				private_key: self.context.private_key,
			},
		})
	}
}

impl ClientBuilder<Registered> {
	/// Creates a new Client builder from a registered context
	pub fn from_registration(context: Registered) -> Self {
		Self { context }
	}

	/// Creates a session
	pub async fn create_session(
		self,
	) -> Result<ClientBuilder<SessionContext>, BuildError<Registered>> {
		let mut messenger = Messenger::new(
			format!("https://api.bunq.com/v1/"),
			format!("bunqers-sdk-test"),
			self.context.private_key.clone(),
		)
		.make_verified(self.context.bunq_public_key.clone());
		// TODO: Use builder pattern
		messenger.set_authentication_token(self.context.installation_token.clone());

		let body = CreateSession {
			bunq_api_key: self.context.bunq_api_key.clone(),
		};
		let body = serde_json::to_string(&body).map_err(|_| BuildError {
			reason: BuildErrorReason::BunqRequestError,
			context: self.context.clone(),
		})?;

		let response: ApiResponse<BunqSession> = messenger
			.send(Method::POST, "session-server", Some(body))
			.await
			.map_err(|_| BuildError {
				reason: BuildErrorReason::BunqResponseError,
				context: self.context.clone(),
			})?;
		let result = response.into_result().map_err(|_| BuildError {
			reason: BuildErrorReason::BunqResponseError,
			context: self.context.clone(),
		})?;

		let session_token = result.token.token;
		let owner_id = result.user_person.id;

		Ok(ClientBuilder {
			context: SessionContext {
				owner_id: owner_id,
				session_token: session_token,
				registered_device_id: self.context.registered_device_id,
				bunq_api_key: self.context.bunq_api_key,
				installation_token: self.context.installation_token,
				bunq_public_key: self.context.bunq_public_key,
				private_key: self.context.private_key,
			},
		})
	}
}

impl ClientBuilder<UncheckedSession> {
	/// Creates a Client builder with unchecked session
	pub fn from_unchecked_session(context: UncheckedSession) -> Self {
		Self { context }
	}

	/// Checks if the session is working
	/// By requesting user data
	pub async fn check_session(
		self,
	) -> Result<ClientBuilder<SessionContext>, BuildError<UncheckedSession>> {
		let mut messenger = Messenger::new(
			format!("https://api.bunq.com/v1/"),
			format!("bunqers-sdk-test"),
			self.context.private_key.clone(),
		)
		.make_verified(self.context.bunq_public_key.clone());
		// TODO: Use builder pattern
		messenger.set_authentication_token(self.context.session_token.clone());

		// TODO: Avoid repetition?
		let response: ApiResponse<Single<User>> = messenger
			.send(Method::GET, "user", None)
			.await
			.expect("Failed to send request to Bunq");

		match response.into_result() {
			Ok(user) => Ok(ClientBuilder {
				context: SessionContext {
					owner_id: user.user_person.id,
					session_token: self.context.session_token,
					registered_device_id: self.context.registered_device_id,
					bunq_api_key: self.context.bunq_api_key,
					installation_token: self.context.installation_token,
					bunq_public_key: self.context.bunq_public_key,
					private_key: self.context.private_key,
				},
			}),
			Err(error) => {
				// Session is likely expired.
				// TODO: Check for actual expiration error
				dbg!(error);
				todo!()
			}
		}
	}
}

impl ClientBuilder<SessionContext> {
	pub fn build(self) -> Client {
		// Set the messenger to use the session token
		let mut messenger = Messenger::new(
			format!("https://api.bunq.com/v1/"),
			format!("bunqers-sdk-test"),
			self.context.private_key.clone(),
		)
		.make_verified(self.context.bunq_public_key.clone());
		// TODO: Use builder pattern
		messenger.set_authentication_token(self.context.session_token.clone());

		Client {
			messenger,
			context: self.context,
		}
	}
}
