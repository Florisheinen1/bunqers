use openssl::pkey::{PKey, Private, Public};
use reqwest::Method;
use rust_decimal::Decimal;

use crate::{
	client_builder::{ClientBuilder, Registered},
	messenger::{ApiResponse, Messenger, Verified},
	types::*,
};

#[derive(Clone)]
pub struct SessionContext {
	pub owner_id: u32,
	pub session_token: String,
	pub registered_device_id: u32,
	pub bunq_api_key: String,
	pub installation_token: String,
	pub bunq_public_key: PKey<Public>,
	pub private_key: PKey<Private>,
}

pub struct Client {
	pub messenger: Messenger<Verified>,
	pub context: SessionContext,
}

impl Client {
	// // Removes the session token from the client
	// // Turns this client back into a client builder
	// pub fn invalidate_session(self) -> ClientBuilder<Registered> {
	// 	todo!()
	// }

	// /// Takes this instance and either returns itself or the No Session version
	// async fn check_session(self) -> Result<Self, Client<NoSession>> {
	// 	let response = self.get_user().await.into_result();
	// 	if response.is_err() {
	// 		// Session is not available anymore
	// 		// TODO: Check for actual session expiration error
	// 		Err(Client {
	// 			messenger: self.messenger,
	// 			bunq_api_key: self.bunq_api_key,
	// 			installation_token: self.installation_token,
	// 			context: NoSession,
	// 		})
	// 	} else {
	// 		// Otherwise, our session is still in-tact
	// 		Ok(self)
	// 	}
	// }

	// /// Checks if current session is still valid by making GET call to '/user'
	// pub async fn ensure_session(self) -> Result<Self, SessionCreationError> {
	// 	match self.check_session().await {
	// 		Ok(session) => Ok(session),
	// 		Err(no_session) => {
	// 			// Try to create a new session
	// 			no_session.create_session().await
	// 		}
	// 	}
	// }

	/// Checks if current session is still valid
	/// If not, it will try to create a new one
	pub async fn ensure_session(self) -> Result<Self, Registered> {
		// Reuse the ClientBuilder logic to verify the session
		let unchecked_session = ClientBuilder::from_unchecked_session(self.context.into());

		match unchecked_session.check_session().await {
			Ok(checked_session) => {
				return Ok(checked_session.build());
			}
			Err(error) => {
				// The session token is invalid. Remove it and create a new one
				let new_session_builder = ClientBuilder::from_registration(error.context.into());
				match new_session_builder.create_session().await {
					Ok(checked_session) => {
						return Ok(checked_session.build());
					}
					Err(error) => Err(error.context),
				}
			}
		}
	}

	//@===@===@===@===@// ENDPOINTS //@===@===@===@===@//

	/// Fetches the user data of this session (GET)
	pub async fn get_user(&self) -> ApiResponse<Single<User>> {
		self.messenger
			.send(Method::GET, "user", None)
			.await
			.expect("Failed to send request to Bunq")
	}

	/// Fetches a list of monetary accounts (GET)
	pub async fn get_monetary_accounts(&self) -> ApiResponse<Multiple<MonetaryAccountBankWrapper>> {
		let endpoint = format!("user/{}/monetary-account-bank", self.context.owner_id);
		self.messenger
			.send(Method::GET, &endpoint, None)
			.await
			.expect("Failed to send request to Bunq")
	}

	/// Fetches a list of monetary accounts (GET)
	pub async fn get_monetary_account(
		&self,
		bank_account_id: u32,
	) -> ApiResponse<Single<MonetaryAccountBankWrapper>> {
		let endpoint = format!(
			"user/{}/monetary-account-bank/{}",
			self.context.owner_id, bank_account_id
		);
		self.messenger
			.send(Method::GET, &endpoint, None)
			.await
			.expect("Failed to send request to Bunq")
	}

	/// Fetches the payment request with given id (GET)
	pub async fn get_payment_request(
		&self,
		monetary_account_id: u32,
		payment_request_id: u32,
	) -> ApiResponse<Single<BunqMeTabWrapper>> {
		let endpoint = format!(
			"user/{}/monetary-account/{monetary_account_id}/bunqme-tab/{payment_request_id}",
			self.context.owner_id
		);
		self.messenger
			.send(Method::GET, &endpoint, None)
			.await
			.expect("Failed to send request to Bunq")
	}

	/// Creates a new payment request (POST)
	pub async fn create_payment_request(
		&self,
		monetary_account_id: u32,
		amount: Decimal,
		description: String,
		redirect_url: String,
	) -> ApiResponse<Single<CreateBunqMeTabResponseWrapper>> {
		let endpoint = format!(
			"user/{}/monetary-account/{monetary_account_id}/bunqme-tab",
			self.context.owner_id
		);

		let body = CreateBunqMeTabWrapper {
			bunqme_tab_entry: CreateBunqMeTab {
				amount_inquired: Amount {
					value: amount,
					currency: format!("EUR"),
				},
				description,
				redirect_url,
			},
		};

		let body = serde_json::to_string(&body)
			.expect("Failed to serialize body of create payment request");

		self.messenger
			.send(Method::POST, &endpoint, Some(body))
			.await
			.expect("Failed to send request to Bunq")
	}

	/// Closes the given payment request
	pub async fn close_payment_request(
		&self,
		monetary_account_id: u32,
		payment_request_id: u32,
	) -> ApiResponse<Single<CreateBunqMeTabResponseWrapper>> {
		let endpoint = format!(
			"user/{}/monetary-account/{monetary_account_id}/bunqme-tab/{payment_request_id}",
			self.context.owner_id
		);
		let body = AlterBunqMeTabRequest {
			status: Some(BunqMeTabStatus::Cancelled),
		};
		let body =
			serde_json::to_string(&body).expect("Failed to serialize AlterBunqMeTab request body");
		self.messenger
			.send(Method::PUT, &endpoint, Some(body))
			.await
			.expect("Failed to send request to Bunq")
	}
}

// #[derive(Debug)]
// pub enum DeviceInstallationError {
// 	KeyCreationError(ErrorStack),
// 	KeySerialization(ErrorStack),
// 	BunqRequestError,
// 	BunqResponseError(Vec<ApiErrorDescription>),
// 	KeyDeserializationError(ErrorStack),
// }
// #[derive(Debug)]
// pub enum SessionCreationError {
// 	BunqRequestError,
// 	BunqResponseError(Vec<ApiErrorDescription>),
// }

// // ================================ //

// pub struct ContextWithSession {
// 	pub session_token: String,
// 	pub registered_device_id: u32,
// 	pub installation_token: String,
// 	pub bunq_public_key: PKey<Public>,
// 	pub private_key: PKey<Private>,
// }
// pub struct ContextWithRegistration {
// 	pub registered_device_id: u32,
// 	pub installation_token: String,
// 	pub bunq_public_key: PKey<Public>,
// 	pub private_key: PKey<Private>,
// }
// pub struct ContextWithInstallation {
// 	pub installation_token: String,
// 	pub bunq_public_key: PKey<Public>,
// 	pub private_key: PKey<Private>,
// }
// pub struct ContextWithInitialization {
// 	pub private_key: PKey<Private>,
// }

// // ================================ //

// /// Contains device installation specific data
// /// Required to create a session with Bunq
// pub struct BunqDeviceInstallation {
// 	pub private_key: PKey<Private>,
// 	pub installation_token: String,
// 	pub bunq_public_key: PKey<Public>,
// }

// impl BunqDeviceInstallation {
// 	/// Installs the current device
// 	/// Sends newly created private key to Bunq and retrieves Bunq's public key and installation token
// 	pub async fn install_device() -> Result<Self, DeviceInstallationError> {
// 		let new_key = Rsa::generate(2048)
// 			.map_err(|error| DeviceInstallationError::KeyCreationError(error))?;
// 		let private_key = PKey::from_rsa(new_key)
// 			.map_err(|error| DeviceInstallationError::KeyCreationError(error))?;

// 		Self::install_device_with_key(private_key).await
// 	}

// 	/// Installs the current device
// 	/// Sends private key to Bunq and retrieves Bunq's public key and installation token
// 	pub async fn install_device_with_key(
// 		private_key: PKey<Private>,
// 	) -> Result<Self, DeviceInstallationError> {
// 		let messenger = Messenger::new(
// 			format!("https://api.bunq.com/v1/"),
// 			format!("bunqers-sdk-test"),
// 			private_key.clone(),
// 		);

// 		let body = CreateInstallation {
// 			client_public_key: String::from_utf8_lossy(
// 				&private_key
// 					.public_key_to_pem()
// 					.map_err(|error| DeviceInstallationError::KeySerialization(error))?,
// 			)
// 			.to_string(),
// 		};

// 		let body =
// 			serde_json::to_string(&body).map_err(|_| DeviceInstallationError::BunqRequestError)?;

// 		let response: ApiResponse<Installation> = messenger
// 			.send_unverified(Method::POST, "installation", Some(body))
// 			.await
// 			.expect("Failed to send message to Bunq");

// 		let result = response.into_result().map_err(|error| {
// 			println!(
// 				"Received error from Bunq. Status: {:?}, descriptions: {:?}",
// 				error.status_code, error.reasons
// 			);
// 			panic!("Failed to send request");
// 			// TODO: Handle elegant resends
// 		})?;

// 		// Parse Bunq's public key
// 		let bunq_public_key = Rsa::public_key_from_pem(result.bunq_public_key.as_bytes())
// 			.map_err(|error| DeviceInstallationError::KeyDeserializationError(error))?;
// 		let bunq_public_key = PKey::from_rsa(bunq_public_key)
// 			.map_err(|error| DeviceInstallationError::KeyDeserializationError(error))?;

// 		Ok(Self {
// 			private_key,
// 			installation_token: result.token.token,
// 			bunq_public_key: bunq_public_key,
// 		})
// 	}
// }

// pub struct BunqDeviceRegistration {
// 	pub installation: BunqDeviceInstallation,
// 	pub registered_device_id: u32,
// }

// impl BunqDeviceRegistration {
// 	/// Links this device + installation with given API key
// 	pub async fn register_device(
// 		installation: BunqDeviceInstallation,
// 		bunq_api_key: String,
// 		device_description: String,
// 	) -> Result<Self, ()> {
// 		let mut messenger = Messenger::new(
// 			format!("https://api.bunq.com/v1/"),
// 			format!("bunqers-sdk-test"),
// 			installation.private_key.clone(),
// 		)
// 		.make_verified(installation.bunq_public_key.clone());
// 		messenger.set_authentication_token(installation.installation_token.clone());

// 		let body = CreateDeviceServer {
// 			bunq_api_key: bunq_api_key,
// 			description: device_description,
// 			permitted_ips: Vec::new(),
// 		};

// 		let body = serde_json::to_string(&body).expect("Failed to serialize register device body");

// 		let response: ApiResponse<Single<DeviceServerSmall>> = messenger
// 			.send(Method::POST, "device-server", Some(body))
// 			.await
// 			.expect("Failed to send message to Bunq");
// 		let result = response.into_result().map_err(|error| {
// 			println!(
// 				"Status: {:?}, descriptions: {:?}",
// 				error.status_code, error.reasons
// 			);
// 			panic!("Failed");
// 		})?;
// 		let registered_device_id = result.id;

// 		Ok(Self {
// 			installation,
// 			registered_device_id,
// 		})
// 	}
// }

// - //
//
//
// impl Client<NoSession> {
// 	/// Creates a new Client object without active session
// 	pub fn new(registration: BunqDeviceRegistration, bunq_api_key: String) -> Self {
// 		let messenger = Messenger::new(
// 			format!("https://api.bunq.com/v1/"),
// 			format!("bunqers-sdk-test"),
// 			registration.installation.private_key,
// 		)
// 		.make_verified(registration.installation.bunq_public_key);

// 		Self {
// 			messenger,
// 			bunq_api_key: bunq_api_key,
// 			installation_token: registration.installation.installation_token,
// 			context: NoSession,
// 		}
// 	}

// 	/// Takes this instance and returns a client with a session
// 	/// Otherwise, device needs to be re-installed
// 	pub async fn create_session(mut self) -> Result<Client<Session>, SessionCreationError> {
// 		// Set messenger's authentication header to our installation token
// 		self.messenger
// 			.set_authentication_token(self.installation_token.clone());

// 		let body = CreateSession {
// 			bunq_api_key: self.bunq_api_key.clone(),
// 		};
// 		let body =
// 			serde_json::to_string(&body).map_err(|_| SessionCreationError::BunqRequestError)?;

// 		let response: ApiResponse<BunqSession> = self
// 			.messenger
// 			.send(Method::POST, "session-server", Some(body))
// 			.await
// 			.expect("Failed to send message to Bunq");
// 		let result = response.into_result().map_err(|error| {
// 			println!(
// 				"Status: {:?}, descriptions: {:?}",
// 				error.status_code, error.reasons
// 			);
// 			panic!("Failed");
// 		})?;

// 		let session_token = result.token.token;
// 		let owner_id = result.user_person.id;

// 		// Update messenger
// 		self.messenger
// 			.set_authentication_token(session_token.clone());

// 		return Ok(Client {
// 			messenger: self.messenger,
// 			bunq_api_key: self.bunq_api_key,
// 			installation_token: self.installation_token,
// 			context: Session {
// 				owner_id,
// 				session_token,
// 			},
// 		});
// 	}

// 	/// Creates a Client from given session token
// 	pub async fn use_existing_session(
// 		mut self,
// 		session_token: String,
// 	) -> Result<Client<Session>, ()> {
// 		// Update messenger's authentication header first
// 		self.messenger
// 			.set_authentication_token(session_token.clone());

// 		let test_session: Client<Session> = Client {
// 			messenger: self.messenger,
// 			bunq_api_key: self.bunq_api_key,
// 			installation_token: self.installation_token,
// 			context: Session {
// 				// Temporary wrong value, will be updated by 'check_session'
// 				// TODO: Find more elegant solution
// 				owner_id: 0,
// 				session_token,
// 			},
// 		};
// 		test_session.check_session().await.map_err(|_| ())
// 	}
// }
