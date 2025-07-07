use openssl::{pkey::{PKey, Private}, rsa::Rsa};
use reqwest::Method;

use crate::{messenger::{Messenger, Response}, types::*};

#[derive(Debug, Clone, Default)]
pub struct NoSessionContext {
	pub installation_token: Option<String>,
	pub bunq_public_key: Option<String>,
	pub registered_device_id: Option<u32>,
	pub session_token: Option<String>,
	pub owner_id: Option<u32>, // TODO: Hide this
}

#[derive(Debug, Clone)]
pub struct SessionContext {
	pub installation_token: String,
	pub bunq_public_key: String,
	pub registered_device_id: u32,
	pub session_token: String,
	owner_id: u32,
}

pub struct Client<T> {
	private_key: PKey<Private>,
	api_key: String,
	messenger: Messenger,

	context: T,
}

struct InstallationResult {
	token: String,
	public_key: String,
}

impl Client<NoSessionContext> {
	/// Creates a new Client with no session related state
	pub fn new(api_key: String, private_key: PKey<Private>) -> Self {
		Self::from_context(api_key, private_key, NoSessionContext::default())
	}

	/// Creates a no-session Client from given context
	pub fn from_context(api_key: String, private_key: PKey<Private>, context: NoSessionContext) -> Self {
		let messenger = Messenger::new(
			format!("https://api.bunq.com/v1/"), 
			format!("bunqers-sdk-test"),
			private_key.clone());

		Self {
			private_key,
			api_key,
			messenger,
			context,
		}
	}

	/// Converts this client to one with a session
	pub async fn get_session(mut self) -> Client<SessionContext> {
		let installation = self.get_or_create_installation_token().await;

		// Update messenger with authentication details
		self.messenger.set_auth_header(Some(installation.token.clone()));

		// Parse Bunq's public key
		let key = Rsa::public_key_from_pem(installation.public_key.as_bytes())
			.expect("Failed to create public key from string");
		let key = PKey::from_rsa(key)
			.expect("Failed to create general public key from RSA");

		self.messenger.set_bunq_public_key(key);

		let registered_device_id = self.get_or_create_registered_device().await;

		let session_token = self.get_or_create_session_token().await;

		self.messenger.set_auth_header(Some(session_token.clone()));

		let owner_id = self.get_or_fetch_owner_id().await;

		let client = Client {
			private_key: self.private_key,
			api_key: self.api_key,
			messenger: self.messenger,
			context: SessionContext {
				installation_token: installation.token,
				bunq_public_key: installation.public_key,
				registered_device_id,
				session_token,
				owner_id,
			},
		};

		return client;
	}

	// ===================== Installation token ===================== //

	/// Sends a request providing Bunq with our public key to get an installation token
	async fn create_installation_token(&self) -> Response<Installation> {
		let body = CreateInstallation {
			client_public_key: String::from_utf8_lossy(
				&self.private_key.public_key_to_pem()
				.expect("Failed to serialize public key")
			).to_string(),
		};
		let body = serde_json::to_string(&body).expect("Failed to serialize installation body");

		self.messenger.send_unverified(Method::POST, "installation", Some(body)).await
	}

	/// Provides Bunq with our public key and returns the Installation token
	/// Step 1 in creating a session
	async fn get_or_create_installation_token(&self) -> InstallationResult {
		// Check if we already have installed
		if let Some(token) = &self.context.installation_token {
			if let Some(key) = &self.context.bunq_public_key {
				println!("Reusing installation token: {token}");
				return InstallationResult {
					token: token.to_string(),
					public_key: key.to_string(),
				}
			}
			println!("Installation token present, but not Bunq's public key for verification!");
		}

		// Create a new token
		print!("Creating new installation token... ");
		let response = self.create_installation_token()
			.await
			.body.into_result().expect("Failed to create Installation token");

		println!("Created: {}", response.token.token);

		return InstallationResult {
			token: response.token.token,
			public_key: response.bunq_public_key,
		};
	}

	// ===================== Device registration ===================== //
	
	/// Sends request to bind current IP address (device) to API key
	/// If this fails, you most likely need a new API key
	async fn register_new_device(&self, device_description: String) -> Response<Single<DeviceServerSmall>> {
		let body = CreateDeviceServer {
			bunq_api_key: self.api_key.clone(),
			description: device_description,
			permitted_ips: Vec::new(),
		};

		let body = serde_json::to_string(&body).expect("Failed to serialize register device body");

		self.messenger.send(Method::POST, "device-server", Some(body)).await
	}

	/// Returns a list of all registered devices linked to this installation key
	async fn get_registered_devices(&self) -> Response<Multiple<DeviceServerWrapper>> {
		self.messenger.send(Method::GET, "device-server", None).await
	}

	/// Registers this device (IP address) to the installation key, retuning the device ID
	/// Step 2 in creating a session
	async fn get_or_create_registered_device(&self) -> u32 {
		let current_device_id = self.context.registered_device_id;
		
		// If already registered, verify it!
		if let Some(current_device_id) = current_device_id {
			print!("Verifying known registered device ID... ");
			let response = self.get_registered_devices().await;

			let registered_devices = match response.body.into_result() {
				Ok(devices) => devices,
				Err(error) => panic!(
					"Failed to get list of registered devices. Code: {}, Error: {:?}",
					response.code, error
				)
			};

			let matching_device = registered_devices.data.iter().find(|device| {
					device.id == current_device_id &&
					device.status == DeviceServerStatus::Active
				}
			);

			if let Some(matching_device) = matching_device {
				println!("Verified: {}", matching_device.id);
				return matching_device.id
			}

			println!("Invalid ID: {current_device_id}\nIn list:\n{:?}", registered_devices);
		}

		// Otherwise, we'll need to register this new device!
		print!("Registering new device... ");
		let response = self.register_new_device(format!("Couch Laptop 2")).await;

		let registered_device = match response.body.into_result() {
			Ok(device) => device,
			Err(error) => panic!(
				"Failed to register new device. Code: {}, Error: {:?}\n\n=> You probably need a new API key!",
				response.code, error
			),
		};

		println!("Registered: {}", registered_device.id);
		return registered_device.id;
	}

	// ===================== Session token ===================== //

	/// Sends a request to Bunq to create a new session
	async fn create_new_session(&self) -> Response<Session> {
		let body = CreateSession {
			bunq_api_key: self.api_key.clone(),
		};
		
		let body = serde_json::to_string(&body).expect("Failed to serialize create session body");
	
		self.messenger.send(Method::POST, "session-server", Some(body)).await
	}

	/// Reuses or creates a new session token
	/// Step 3 in creating a session
	async fn get_or_create_session_token(&self) -> String {

		// If we already have a session token, return it
		if let Some(session_token) = &self.context.session_token {
			println!("Reusing session token: {session_token}");
			return session_token.clone();
		}

		// Now, we need to create a new session token!
		print!("Creating new session token... ");
		let response = self.create_new_session().await;

		let session = match response.body.into_result() {
			Ok(session) => session,
			Err(error) => panic!(
				"Failed to create new session. Code: {}, Error: {:?}",
				response.code, error
			),
			// TODO: Handle error nicely
		};

		// TODO: Also store ID of owner

		println!("Created: {}", session.token.token);
		return session.token.token;
	}

	// ===================== Owner ID ===================== //

	/// Fetches the owner ID of this Bunq account
	async fn fetch_owner_id(&self) -> Response<Single<User>> {
		self.messenger.send(Method::GET, "user", None).await
	}

	/// Gets known or fetches unknown ID of the owner of this Bunq account
	/// Used to verify if session is successful
	async fn get_or_fetch_owner_id(&self) -> u32 {

		if let Some(owner_id) = self.context.owner_id {
			println!("Fetched owner already during session creation: {owner_id}");
			return owner_id;
		}

		// Fetch owner id from Bunq's servers
		print!("Fetching owner id... ");
		let response  = self.fetch_owner_id().await;
		
		let user = match response.body.into_result() {
			Ok(user) => user,
			Err(error) => panic!(
				"Failed to fetch owner of account. Code: {}, Error: {:?}",
				response.code, error
			),
			// TODO: Handle error better
		};

		println!("Fetched: {}, a.k.a. {}", user.user_person.id, user.user_person.display_name);
		return user.user_person.id;
	}
}

impl Client<SessionContext> {
	/// Returns the session context of this Client
	pub fn get_session_context(&self) -> SessionContext {
		self.context.clone()
	}

	// =========== Endpoints =========== //
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
	pub async fn get_monetary_account(&self, bank_account_id: u32) -> Response<Single<MonetaryAccountBankWrapper>> {
		let endpoint = format!("user/{}/monetary-account-bank/{}", self.context.owner_id, bank_account_id);
		self.messenger.send(Method::GET, &endpoint, None).await
	}

	// /// Fetches a list of payment requests
	// pub async fn get_payment_requests(&self, monetary_account_id: u32) -> Response<Multiple<BunqMeTabWrapper>> {
	// 	let endpoint = format!("user/{}/monetary-account/{monetary_account_id}/bunqme-tab", self.state.owner_id);
	// 	self.messenger.send(Method::GET, &endpoint, None).await
	// }

	/// Fetches the payment request with given id (GET)
	pub async fn get_payment_request(&self, monetary_account_id: u32, payment_request_id: u32) -> Response<Single<BunqMeTabWrapper>> {
		let endpoint = format!("user/{}/monetary-account/{monetary_account_id}/bunqme-tab/{payment_request_id}", self.context.owner_id);
		self.messenger.send(Method::GET, &endpoint, None).await
	}

	/// Creates a new payment request (POST)
	pub async fn create_payment_request(&self, monetary_account_id: u32, amount: f32, description: String, redirect_url: String) -> Response<Single<CreateBunqMeTabResponseWrapper>> {
		let endpoint = format!("user/{}/monetary-account/{monetary_account_id}/bunqme-tab", self.context.owner_id);
		
		let body = CreateBunqMeTabWrapper{
			bunqme_tab_entry: CreateBunqMeTab {
				amount_inquired: Amount { value: amount, currency: format!("EUR") },
				description,
				redirect_url,
			}
		};

		let body = serde_json::to_string(&body).expect("Failed to serialize body of create payment request");
		
		self.messenger.send(Method::POST, &endpoint, Some(body)).await
	}

	// /// Returns the bunq data of given payment request
	// pub async fn get_payment_request(&self, monetary_account_id: u32, payment_request_id: u32) -> Result<BunqMeTabWrapper, Error> {
	// 	let endpoint = format!("user/{}/monetary-account/{monetary_account_id}/bunqme-tab/{payment_request_id}", self.state.owner_id);
	// 	let response = self.do_request(Method::GET, &endpoint, None).await?;
	// 	let payment_request = response.response.into_iter().next().expect("No payment request data in response");
	// 	Ok(payment_request)
	// }

	// /// Creates a payment request
	// pub async fn create_payment_request(&self, monetary_account_id: u32, request: &CreateBunqMeTab) -> Result<BunqMeTabCreate, Error> {
	// 	let body = serde_json::to_string(request).expect("Failed to serialize payment request creation");

	// 	let endpoint = format!("user/{}/monetary-account/{monetary_account_id}/bunqme-tab", self.state.owner_id);

	// 	let response = self.do_request(Method::POST, &endpoint, Some(body)).await?;
	// 	let created_payment_request = response.response.into_iter().next().expect("No payment request data available in response");
	// 	Ok(created_payment_request)
	// }

	// pub async fn get_notification_filters(&self) -> Response<Multiple<NotificationFilter>> {
	// 	let endpoint = format!("user/{}/notification-filter-url", self.context.owner_id);
	// 	self.messenger.send(Method::GET, &endpoint, None).await
	// }
}

