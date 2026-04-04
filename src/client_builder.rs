//! Typestate builder for constructing a [`Client`].
//!
//! The builder enforces the correct Bunq setup order at compile time:
//!
//! ```text
//! () ──install_device()──► Installed ──register_device()──► Registered
//!                                                               │
//!                                              create_session() │
//!                                                               ▼
//!                                                       SessionContext
//!                                                               │
//!                                                      build() │
//!                                                               ▼
//!                                                           Client
//! ```
//!
//! Each state transition calls one Bunq API endpoint. It is impossible to call
//! `create_session` without first going through `install_device` and
//! `register_device` — the compiler will reject it.
//!
//! For subsequent runs where device registration is already done, use
//! [`ClientBuilder::from_registration`] to start directly at the `Registered`
//! state, or [`ClientBuilder::from_unchecked_session`] to attempt reusing a
//! cached session token.

use openssl::{
	error::ErrorStack,
	pkey::{PKey, Private, Public},
	rsa::Rsa,
};
use reqwest::Method;

use crate::{
	client::{Client, SessionContext},
	messenger::{ApiErrorResponse, ApiResponse, MessageError, Messenger},
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

/// Builder state: device is registered and a session token exists, but the
/// token has not yet been validated.
///
/// Use [`ClientBuilder::from_unchecked_session`] to enter this state when
/// restoring a session from e.g. disk, then call
/// [`ClientBuilder::check_session`] to validate it.
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

/// Builder state: device is registered and ready to create a session.
///
/// Obtained after [`ClientBuilder::register_device`] succeeds, or constructed
/// directly via [`ClientBuilder::from_registration`] when restoring a
/// persisted [`crate::InstallationContext`].
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

/// Builder state: the `/installation` endpoint has been called and Bunq's
/// public key is available, but no device has been registered yet.
#[derive(Clone, Debug)]
pub struct Installed {
	pub installation_token: String,
	pub bunq_public_key: PKey<Public>,
}

/// Typestate builder for constructing a [`Client`].
///
/// The type parameter `T` represents the current builder state. See the
/// [module-level documentation](self) for the full state diagram.
pub struct ClientBuilder<T> {
	pub private_key: PKey<Private>,
	pub api_base_url: String,
	pub app_name: String,
	messenger: Messenger,
	pub context: T,
}

/// An error returned when a builder state transition fails.
#[derive(Debug)]
pub struct BuildError<T> {
	/// The reason the transition failed.
	pub reason: BuildErrorReason,
	/// The builder context before the failure, returned so the caller can
	/// recover or inspect the state.
	pub context: T,
}

/// Reasons a [`ClientBuilder`] state transition can fail.
#[derive(Debug)]
pub enum BuildErrorReason {
	/// OpenSSL failed to generate or wrap an RSA key pair.
	KeyCreationError(ErrorStack),
	/// OpenSSL failed to serialise a key to PEM.
	KeySerialization(ErrorStack),
	/// OpenSSL failed to parse a PEM-encoded key received from Bunq.
	KeyDeserializationError(ErrorStack),
	/// The HTTP request could not be built or sent.
	BunqRequestError,
	/// The response from Bunq could not be parsed.
	BunqInvalidResponse(MessageError),
	/// Bunq returned an API-level error response.
	BunqResponseApiError(ApiErrorResponse),
}

impl ClientBuilder<()> {
	/// Creates a builder using the provided RSA private key.
	///
	/// Use this when you already have a key from a previous run and want to
	/// avoid generating a new one.
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

	/// Creates a builder with a freshly generated 2048-bit RSA key pair.
	///
	/// Returns an error if OpenSSL fails to generate the key.
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

	/// Calls the Bunq `/installation` endpoint to exchange public keys.
	///
	/// Sends the client's public key to Bunq and receives Bunq's public key in
	/// return. Bunq's key is stored and used to verify response signatures from
	/// this point onward.
	///
	/// On success, advances the builder to the [`Installed`] state.
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

		// Use send_unverified because we do not yet have Bunq's public key.
		let response: ApiResponse<Installation> = self
			.messenger
			.send_unverified(Method::POST, "installation", Some(body_text))
			.await
			.map_err(|error| BuildError {
				reason: BuildErrorReason::BunqInvalidResponse(error),
				context: self.context.clone(),
			})?;

		let result = response.into_result().map_err(|error| BuildError {
			reason: BuildErrorReason::BunqResponseApiError(error),
			context: self.context.clone(),
		})?;

		// Parse Bunq's public key from the response.
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

		// From now on, sign requests with the installation token and verify
		// responses with Bunq's public key.
		let installation_token = result.token.token;
		let mut messenger = self.messenger;
		messenger.set_authentication_token(Some(installation_token.clone()));
		messenger.set_bunq_public_sign_key(Some(bunq_public_key.clone()));

		Ok(ClientBuilder {
			api_base_url: self.api_base_url,
			app_name: self.app_name,
			private_key: self.private_key,
			messenger,
			context: Installed {
				installation_token,
				bunq_public_key,
			},
		})
	}
}

impl ClientBuilder<Installed> {
	/// Constructs a builder from a previously obtained [`Installed`] context.
	///
	/// Use this to skip the `install_device` step when restoring from a
	/// persisted [`crate::InstallationContext`] without a device ID.
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

	/// Calls the Bunq `/device-server` endpoint to register this device.
	///
	/// Links the provided API key to the current IP address. The returned
	/// device ID is needed to create sessions later.
	///
	/// On success, advances the builder to the [`Registered`] state.
	pub async fn register_device(
		self,
		bunq_api_key: String,
		device_description: &str,
	) -> Result<ClientBuilder<Registered>, BuildError<Installed>> {
		let body = CreateDeviceServer {
			bunq_api_key: &bunq_api_key,
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
			.map_err(|error| BuildError {
				reason: BuildErrorReason::BunqInvalidResponse(error),
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
	/// Constructs a builder from a persisted [`Registered`] context.
	///
	/// Use this to skip both `install_device` and `register_device` when
	/// restoring from a persisted [`crate::InstallationContext`].
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

	/// Calls the Bunq `/session-server` endpoint to create a session.
	///
	/// The session token returned by Bunq is stored and used as the
	/// `X-Bunq-Client-Authentication` header for all subsequent requests.
	///
	/// On success, advances the builder to the [`SessionContext`] state, from
	/// which [`ClientBuilder::build`] produces a ready-to-use [`Client`].
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
			.map_err(|error| BuildError {
				reason: BuildErrorReason::BunqInvalidResponse(error),
				context: self.context.clone(),
			})?;
		let result = response.into_result().map_err(|error| BuildError {
			reason: BuildErrorReason::BunqResponseApiError(error),
			context: self.context.clone(),
		})?;

		let session_token = result.token.token;
		let owner_id = result.user_person.id;

		let mut messenger = self.messenger;
		messenger.set_authentication_token(Some(session_token.clone()));

		Ok(ClientBuilder {
			api_base_url: self.api_base_url,
			app_name: self.app_name,
			private_key: self.private_key,
			messenger,
			context: SessionContext {
				owner_id,
				session_token,
				registered_device_id: self.context.registered_device_id,
				bunq_api_key: self.context.bunq_api_key,
				installation_token: self.context.installation_token,
				bunq_public_key: self.context.bunq_public_key,
			},
		})
	}
}

impl ClientBuilder<UncheckedSession> {
	/// Constructs a builder from a session token that has not yet been
	/// validated.
	///
	/// Call [`check_session`](ClientBuilder::check_session) afterwards to
	/// verify the token is still accepted by the API.
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

	/// Validates the session by calling `GET /user`.
	///
	/// Returns `Ok` if the session is still accepted by Bunq, or `Err` if the
	/// token has expired or is otherwise invalid.
	pub async fn check_session(
		self,
	) -> Result<ClientBuilder<SessionContext>, BuildError<UncheckedSession>> {
		let response: Result<ApiResponse<Single<User>>, _> =
			self.messenger.send(Method::GET, "user", None).await;

		match response {
			Ok(response) => match response.into_result() {
				Ok(user) => Ok(ClientBuilder {
					api_base_url: self.api_base_url,
					app_name: self.app_name,
					private_key: self.private_key,
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
				Err(error) => Err(BuildError {
					reason: BuildErrorReason::BunqResponseApiError(error),
					context: self.context,
				}),
			},
			Err(_) => Err(BuildError {
				reason: BuildErrorReason::BunqRequestError,
				context: self.context,
			}),
		}
	}
}

impl ClientBuilder<SessionContext> {
	/// Consumes the builder and returns a ready-to-use [`Client`].
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
