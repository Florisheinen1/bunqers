use openssl::{
	error::ErrorStack,
	pkey::{PKey, Private, Public},
	rsa::Rsa,
};
use reqwest::Method;
use rust_decimal::Decimal;

use crate::{
	messenger::{Messenger, Response, Verified},
	types::Session as BunqSession,
	types::*,
};

pub enum DeviceInstallationError {
	KeyCreationError(ErrorStack),
	KeySerialization(ErrorStack),
	BunqRequestError,
	BunqResponseError(Vec<ApiErrorDescription>),
	KeyDeserializationError(ErrorStack),
}

pub enum SessionCreationError {
	BunqRequestError,
	BunqResponseError(Vec<ApiErrorDescription>),
}

pub struct BunqDeviceInstallation {
	pub private_key: PKey<Private>,
	pub installation_token: String,
	pub bunq_public_key: PKey<Public>,
}

/// Contains device installation specific data
/// Required to create a session with Bunq
impl BunqDeviceInstallation {
	/// Installs the current device
	/// Sends newly created private key to Bunq and retrieves Bunq's public key and installation token
	pub async fn install_device() -> Result<Self, DeviceInstallationError> {
		let new_key = Rsa::generate(2048)
			.map_err(|error| DeviceInstallationError::KeyCreationError(error))?;
		let private_key = PKey::from_rsa(new_key)
			.map_err(|error| DeviceInstallationError::KeyCreationError(error))?;

		Self::install_device_with_key(private_key).await
	}

	/// Installs the current device
	/// Sends private key to Bunq and retrieves Bunq's public key and installation token
	pub async fn install_device_with_key(
		private_key: PKey<Private>,
	) -> Result<Self, DeviceInstallationError> {
		let messenger = Messenger::new(
			format!("https://api.bunq.com/v1/"),
			format!("bunqers-sdk-test"),
			private_key.clone(),
		);

		let body = CreateInstallation {
			client_public_key: String::from_utf8_lossy(
				&private_key
					.public_key_to_pem()
					.map_err(|error| DeviceInstallationError::KeySerialization(error))?,
			)
			.to_string(),
		};

		let body =
			serde_json::to_string(&body).map_err(|_| DeviceInstallationError::BunqRequestError)?;

		let response: Response<Installation> = messenger
			.send_unverified(Method::POST, "installation", Some(body))
			.await;

		let result = response
			.body
			.into_result()
			.map_err(|error| DeviceInstallationError::BunqResponseError(error))?;

		// Parse Bunq's public key
		let bunq_public_key = Rsa::public_key_from_pem(result.bunq_public_key.as_bytes())
			.map_err(|error| DeviceInstallationError::KeyDeserializationError(error))?;
		let bunq_public_key = PKey::from_rsa(bunq_public_key)
			.map_err(|error| DeviceInstallationError::KeyDeserializationError(error))?;

		Ok(Self {
			private_key,
			installation_token: result.token.token,
			bunq_public_key: bunq_public_key,
		})
	}
}

pub struct NoSession;
pub struct Session {
	owner_id: u32,
}

pub struct Client<T> {
	messenger: Messenger<Verified>,
	bunq_api_key: String,
	installation_token: String,
	context: T,
}

impl Client<NoSession> {
	/// Creates a new Client object without active session
	pub fn new(installation: BunqDeviceInstallation, bunq_api_key: String) -> Self {
		let messenger = Messenger::new(
			format!("https://api.bunq.com/v1/"),
			format!("bunqers-sdk-test"),
			installation.private_key,
		)
		.make_verified(installation.bunq_public_key);

		Self {
			messenger,
			bunq_api_key: bunq_api_key,
			installation_token: installation.installation_token,
			context: NoSession,
		}
	}

	/// Takes this instance and returns a client with a session
	/// Otherwise, device needs to be re-installed
	pub async fn create_session(mut self) -> Result<Client<Session>, SessionCreationError> {
		// Set messenger's authentication header to our installation token

		let body = CreateSession {
			bunq_api_key: self.bunq_api_key.clone(),
		};
		let body =
			serde_json::to_string(&body).map_err(|_| SessionCreationError::BunqRequestError)?;

		let response: Response<BunqSession> = self
			.messenger
			.send(Method::POST, "session-server", Some(body))
			.await;
		let result = response
			.body
			.into_result()
			.map_err(|error| SessionCreationError::BunqResponseError(error))?;

		let session_token = result.token.token;
		let owner_id = result.user_person.id;

		// Update messenger
		self.messenger
			.set_authentication_token(session_token.clone());

		return Ok(Client {
			messenger: self.messenger,
			bunq_api_key: self.bunq_api_key,
			installation_token: self.installation_token,
			context: Session { owner_id },
		});
	}

	pub async fn use_existing_session(
		mut self,
		session_token: String,
	) -> Result<Client<Session>, ()> {
		// Update messenger's authentication header first
		self.messenger.set_authentication_token(session_token);

		let test_session: Client<Session> = Client {
			messenger: self.messenger,
			bunq_api_key: self.bunq_api_key,
			installation_token: self.installation_token,
			context: Session {
				// Temporary wrong value, will be updated by 'check_session'
				// TODO: See more elegant solution
				owner_id: 0,
			},
		};
		test_session.check_session().await.map_err(|_| ())
	}
}

impl Client<Session> {
	/// Takes this instance and either returns itself or the No Session version
	async fn check_session(self) -> Result<Self, Client<NoSession>> {
		let response = self.get_user().await.body.into_result();
		if response.is_err() {
			// Session is not available anymore
			// TODO: Check for actual session expiration error
			Err(Client {
				messenger: self.messenger,
				bunq_api_key: self.bunq_api_key,
				installation_token: self.installation_token,
				context: NoSession,
			})
		} else {
			// Otherwise, our session is still in-tact
			Ok(self)
		}
	}

	/// Checks if current session is still valid by making GET call to '/user'
	pub async fn ensure_session(self) -> Result<Self, SessionCreationError> {
		match self.check_session().await {
			Ok(session) => Ok(session),
			Err(no_session) => {
				// Try to create a new session
				no_session.create_session().await
			}
		}
	}

	//@===@===@===@===@// ENDPOINTS //@===@===@===@===@//

	/// Fetches the user data of this session (GET)
	pub async fn get_user(&self) -> Response<Single<User>> {
		self.messenger.send(Method::GET, "user", None).await
	}

	/// Fetches a list of monetary accounts (GET)
	pub async fn get_monetary_accounts(&self) -> Response<Multiple<MonetaryAccountBankWrapper>> {
		let endpoint = format!("user/{}/monetary-account-bank", self.context.owner_id);
		self.messenger.send(Method::GET, &endpoint, None).await
	}

	/// Fetches a list of monetary accounts (GET)
	pub async fn get_monetary_account(
		&self,
		bank_account_id: u32,
	) -> Response<Single<MonetaryAccountBankWrapper>> {
		let endpoint = format!(
			"user/{}/monetary-account-bank/{}",
			self.context.owner_id, bank_account_id
		);
		self.messenger.send(Method::GET, &endpoint, None).await
	}

	/// Fetches the payment request with given id (GET)
	pub async fn get_payment_request(
		&self,
		monetary_account_id: u32,
		payment_request_id: u32,
	) -> Response<Single<BunqMeTabWrapper>> {
		let endpoint = format!(
			"user/{}/monetary-account/{monetary_account_id}/bunqme-tab/{payment_request_id}",
			self.context.owner_id
		);
		self.messenger.send(Method::GET, &endpoint, None).await
	}

	/// Creates a new payment request (POST)
	pub async fn create_payment_request(
		&self,
		monetary_account_id: u32,
		amount: Decimal,
		description: String,
		redirect_url: String,
	) -> Response<Single<CreateBunqMeTabResponseWrapper>> {
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
	}

	/// Closes the given payment request
	pub async fn close_payment_request(
		&self,
		monetary_account_id: u32,
		payment_request_id: u32,
	) -> Response<Single<CreateBunqMeTabResponseWrapper>> {
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
	}
}
