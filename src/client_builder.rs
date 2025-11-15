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
}

impl From<UncheckedSession> for Registered {
	fn from(context: UncheckedSession) -> Self {
		Self {
			installation_token: context.installation_token,
			registered_device_id: context.registered_device_id,
			bunq_api_key: context.bunq_api_key,
			bunq_public_key: context.bunq_public_key,
		}
	}
}

/// Fully ready to create a session
#[derive(Clone, Debug)]
pub struct Registered {
	pub registered_device_id: u32,
	pub bunq_api_key: String,
	pub installation_token: String,
	pub bunq_public_key: PKey<Public>,
}

impl From<Registered> for Installed {
	fn from(context: Registered) -> Self {
		Self {
			installation_token: context.installation_token,
			bunq_public_key: context.bunq_public_key,
		}
	}
}

#[derive(Clone, Debug)]
pub struct Installed {
	pub installation_token: String,
	pub bunq_public_key: PKey<Public>,
}

pub struct ClientBuilder<T> {
	pub private_key: PKey<Private>,
	pub api_base_url: String,
	pub app_name: String,
	messenger: Messenger,
	pub context: T,
}

#[derive(Debug)]
pub struct BuildError<T> {
	pub reason: BuildErrorReason,
	pub context: T,
}

#[derive(Debug)]
pub enum BuildErrorReason {
	KeyCreationError(ErrorStack),
	KeySerialization(ErrorStack),
	KeyDeserializationError(ErrorStack),
	BunqRequestError,
	BunqResponseError, // TODO: Create better response error
}

impl ClientBuilder<()> {
	/// Creates a new Client builder with given private key
	pub fn new_with_key(
		api_base_url: String,
		app_name: String,
		private_key: PKey<Private>,
	) -> Self {
		Self {
			api_base_url: api_base_url.clone(),
			app_name: app_name.clone(),
			private_key: private_key.clone(),
			context: (),
			messenger: Messenger::new(api_base_url, app_name, private_key, None, None),
		}
	}

	/// Creates a new Client builder with a newly generated private key
	pub fn new_without_key(api_base_url: String, app_name: String) -> Result<Self, BuildError<()>> {
		let new_key_pair = Rsa::generate(2048).map_err(|error| BuildError {
			reason: BuildErrorReason::KeyCreationError(error),
			context: (),
		})?;
		let private_key = PKey::from_rsa(new_key_pair).map_err(|error| BuildError {
			reason: BuildErrorReason::KeyCreationError(error),
			context: (),
		})?;

		Ok(Self::new_with_key(api_base_url, app_name, private_key))
	}

	/// Installs this computer
	/// By sending our public key and fetching the API's public key
	pub async fn install_device(self) -> Result<ClientBuilder<Installed>, BuildError<()>> {
		let body = CreateInstallation {
			client_public_key: String::from_utf8_lossy(
				&self
					.private_key
					.public_key_to_pem()
					.map_err(|error| BuildError {
						reason: BuildErrorReason::KeySerialization(error),
						context: self.context.clone(),
					})?,
			)
			.to_string(),
		};

		let body_text = serde_json::to_string(&body).map_err(|_| BuildError {
			reason: BuildErrorReason::BunqRequestError,
			context: self.context.clone(),
		})?;

		let response: ApiResponse<Installation> = self
			.messenger
			.send_unverified(Method::POST, "installation", Some(body_text))
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
			api_base_url: self.api_base_url,
			app_name: self.app_name,
			private_key: self.private_key,
			messenger: self.messenger,
			context: Installed {
				installation_token: result.token.token,
				bunq_public_key,
			},
		})
	}
}

impl ClientBuilder<Installed> {
	/// Creates an installed Client builder from given context
	pub fn from_installation(
		context: Installed,
		api_base_url: String,
		app_name: String,
		private_key: PKey<Private>,
	) -> Self {
		Self {
			api_base_url: api_base_url.clone(),
			app_name: app_name.clone(),
			private_key: private_key.clone(),
			messenger: Messenger::new(
				api_base_url,
				app_name,
				private_key,
				Some(context.bunq_public_key.clone()),
				Some(context.installation_token.clone()),
			),
			context,
		}
	}

	/// Registers this computer with the API
	/// By linking the given API key to the current IP address
	pub async fn register_device(
		self,
		bunq_api_key: String,
		device_description: String,
	) -> Result<ClientBuilder<Registered>, BuildError<Installed>> {
		let body = CreateDeviceServer {
			bunq_api_key: bunq_api_key.clone(),
			description: device_description,
			permitted_ips: Vec::new(),
		};

		let body = serde_json::to_string(&body).map_err(|_| BuildError {
			reason: BuildErrorReason::BunqRequestError,
			context: self.context.clone(),
		})?;

		let response: ApiResponse<Single<DeviceServerSmall>> = self
			.messenger
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
			api_base_url: self.api_base_url,
			app_name: self.app_name,
			private_key: self.private_key,
			messenger: self.messenger,
			context: Registered {
				registered_device_id,
				bunq_api_key,
				installation_token: self.context.installation_token,
				bunq_public_key: self.context.bunq_public_key,
			},
		})
	}
}

impl ClientBuilder<Registered> {
	/// Creates a new Client builder from a registered context
	pub fn from_registration(
		context: Registered,
		api_base_url: String,
		app_name: String,
		private_key: PKey<Private>,
	) -> Self {
		Self {
			api_base_url: api_base_url.clone(),
			app_name: app_name.clone(),
			private_key: private_key.clone(),
			messenger: Messenger::new(
				api_base_url,
				app_name,
				private_key,
				Some(context.bunq_public_key.clone()),
				Some(context.installation_token.clone()),
			),
			context,
		}
	}

	/// Creates a session
	pub async fn create_session(
		self,
	) -> Result<ClientBuilder<SessionContext>, BuildError<Registered>> {
		let body = CreateSession {
			bunq_api_key: self.context.bunq_api_key.clone(),
		};
		let body = serde_json::to_string(&body).map_err(|_| BuildError {
			reason: BuildErrorReason::BunqRequestError,
			context: self.context.clone(),
		})?;

		let response: ApiResponse<BunqSession> = self
			.messenger
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
			api_base_url: self.api_base_url,
			app_name: self.app_name,
			private_key: self.private_key.clone(),
			messenger: self.messenger,
			context: SessionContext {
				owner_id: owner_id,
				session_token: session_token,
				registered_device_id: self.context.registered_device_id,
				bunq_api_key: self.context.bunq_api_key,
				installation_token: self.context.installation_token,
				bunq_public_key: self.context.bunq_public_key,
			},
		})
	}
}

impl ClientBuilder<UncheckedSession> {
	/// Creates a Client builder with unchecked session
	pub fn from_unchecked_session(
		context: UncheckedSession,
		api_base_url: String,
		app_name: String,
		private_key: PKey<Private>,
	) -> Self {
		Self {
			api_base_url: api_base_url.clone(),
			app_name: app_name.clone(),
			private_key: private_key.clone(),
			messenger: Messenger::new(
				api_base_url,
				app_name,
				private_key,
				Some(context.bunq_public_key.clone()),
				Some(context.session_token.clone()),
			),
			context,
		}
	}

	/// Checks if the session is working
	/// By requesting user data
	pub async fn check_session(
		self,
	) -> Result<ClientBuilder<SessionContext>, BuildError<UncheckedSession>> {
		// TODO: Avoid repetition?
		let response: ApiResponse<Single<User>> = self
			.messenger
			.send(Method::GET, "user", None)
			.await
			.expect("Failed to send request to Bunq");

		match response.into_result() {
			Ok(user) => Ok(ClientBuilder {
				api_base_url: self.api_base_url,
				app_name: self.app_name,
				private_key: self.private_key.clone(),
				messenger: self.messenger,
				context: SessionContext {
					owner_id: user.user_person.id,
					session_token: self.context.session_token,
					registered_device_id: self.context.registered_device_id,
					bunq_api_key: self.context.bunq_api_key,
					installation_token: self.context.installation_token,
					bunq_public_key: self.context.bunq_public_key,
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
		Client {
			api_base_url: self.api_base_url,
			app_name: self.app_name,
			private_key: self.private_key,
			messenger: self.messenger,
			context: self.context,
		}
	}
}
