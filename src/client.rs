use openssl::pkey::{PKey, Private};
use reqwest::Method;

use crate::{messenger::{Messenger, Response}, types::*};

#[derive(Debug, Clone)]
pub struct NoSessionContext {
	pub private_key: PKey<Private>,
	pub install_token: Option<String>,
	pub session_token: Option<String>,
	pub owner_id: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct SessionContext {
	private_key: PKey<Private>,
	pub install_token: String,
	pub session_token: String,
	pub owner_id: u32,
}

pub struct Client<T> {
	bunq_api_key: String,
	messenger: Messenger,

	state: T,
}

impl Client<NoSessionContext> {
	pub fn new(bunq_api_key: String, private_key: PKey<Private>, session_token: Option<String>) -> Self {
		let messenger = Messenger::new(format!("https://api.bunq.com/v1/"), format!("bunqers-sdk-test"), private_key.clone());
		
		Client {
			messenger,
			bunq_api_key: bunq_api_key,
			state: NoSessionContext {
				private_key,
				install_token: None,
				session_token,
				owner_id: None
			},
		}
	}

	pub fn from_context(bunq_api_key: String, context: NoSessionContext) -> Self {
		let messenger = Messenger::new(
			format!("https://api.bunq.com/v1/"), 
			format!("bunqers-sdk-test"), context.private_key.clone());
		Self {
			bunq_api_key,
			messenger,
			state: context,
		}
	}

	/// Converts this client to one with a session
	pub async fn get_session(mut self) -> Client<SessionContext> {
		self.ensure_installation_token().await;
		self.ensure_device_registered().await;
		self.ensure_session().await;

		let session_client = Client::from_no_session(self).await;
		session_client
	}

	/// Sends a request providing Bunq with our public key to get an installation token
	async fn send_create_installation_token(&self) -> Response<Installation> {
		let body = CreateInstallation {
			client_public_key: String::from_utf8_lossy(
				&self.state.private_key.public_key_to_pem()
				.expect("Failed to serialize public key")
			).to_string(),
		};
		let body = serde_json::to_string(&body).expect("Failed to serialize installation body");

		self.messenger.send(Method::POST, "installation", Some(body)).await
	}

	/// Provides Bunq with our public key. Step 1 in creating a session
	async fn ensure_installation_token(&mut self) {
		if self.state.install_token.is_some() {
			println!("Installation token already present.");
			// TODO: Check if we can verify it's correctness?
		} else {
			print!("No installation token found. Creating new one...");

			let response = self.send_create_installation_token().await;
			
			let body = response.body.into_result().expect("Failed to get body from installation request");

			println!("Created: {:?}", body.token.token);
			self.state.install_token = Some(body.token.token.clone());
		}
		self.messenger.set_auth_header(self.state.install_token.clone());
	}
	
	/// Sends request to bind current IP address (device) to API key
	async fn send_create_device_server(&self) -> Response<Single<DeviceServerSmall>> {
		let body = CreateDeviceServer {
			bunq_api_key: self.bunq_api_key.clone(),
			description: format!("Test laptop 2"),
			permitted_ips: Vec::new(),
		};

		let body = serde_json::to_string(&body).expect("Failed to serialize");

		self.messenger.send(Method::POST, "device-server", Some(body)).await
	}

	async fn get_registered_devices(&self) -> Response<Multiple<DeviceServerWrapper>> {
		self.messenger.send(Method::GET, "device-server", None).await
	}

	/// Makes sure this device with IP and API key are registered. Step 2 of getting a session
	async fn ensure_device_registered(&self) {
		let registered_devices = self.get_registered_devices().await;
		
		let body = registered_devices.body.into_result().expect("Failed to get body of get devices request");

		if body.data.is_empty() {
			// We need to register this device
			println!("No registered device found. Registering this new device.");

			let _created_device = self.send_create_device_server().await;
			
		} else {
			println!("Registered device found.");
		}
	}

	/// Sends a request to Bunq to create a new session
	async fn send_create_new_session(&self) -> Response<Session> {
		let body = CreateSession {
			bunq_api_key: self.bunq_api_key.clone(),
		};
		
		let body = serde_json::to_string(&body).expect("Failed to serialize");
	
		self.messenger.send(Method::POST, "session-server", Some(body)).await
	}

	/// Checks if a current session exists. Ohterwise, creates a new one. Step 3 in creating a new session
	/// Returns the user id of the owner of this session
	async fn ensure_session(&mut self) {
		print!("Ensuring session... ");
		if let Some(session_token) = &self.state.session_token {
			println!("Already present. Will need to fetch owner data to verify.");

			self.messenger.set_auth_header(Some(session_token.clone()));
		} else {
			// Fix a new one
			print!("Creating new one...");
			
			// TODO: If error, we should make new API key
			let response = self.send_create_new_session().await;

			let body = response.body.into_result().expect("Failed to get body of create session request");

			let session_token = &body.token.token;

			let owner_id = body.user_person.id;
			let owner_name = &body.user_person.display_name;

			self.state.session_token = Some(session_token.to_string());
			self.state.owner_id = Some(owner_id);

			self.messenger.set_auth_header(Some(session_token.clone()));
			
			println!("Created session for {owner_name}: {session_token}");
		}
	}
}

impl Client<SessionContext> {
	async fn from_no_session(no_session: Client<NoSessionContext>) -> Self {
		let mut client = Client::<SessionContext>{
			messenger: no_session.messenger,
			bunq_api_key: no_session.bunq_api_key,
			state: SessionContext {
				private_key: no_session.state.private_key,
				session_token: no_session.state.session_token.expect("No session token present"),
				install_token: no_session.state.install_token.expect("No install token present"),
    			owner_id: 0, // We update this value immediately after creating
			},
		};

		// Update the owner id!

 		client.state.owner_id = match no_session.state.owner_id {
			Some(id) => id,
			None => {
				let person = &client.get_user().await.body.into_result().expect("Failed to get person").user_person;
				println!("Fetched person from existing session: {}", person.display_name);
				// Gotta fetch it first!
				person.id
			},
		};

		client
	}

	/// Returns the session context of this Client
	pub fn get_session_context(&self) -> SessionContext {
		self.state.clone()
	}

	// =========== Endpoints =========== //
	/// Fetches the user data of this session
	pub async fn get_user(&self) -> Response<Single<User>> {
		self.messenger.send(Method::GET, "user", None).await
	}

	/// Fetches a list of monetary accounts
	pub async fn get_monetary_accounts(&self) -> Response<Multiple<MonetaryAccountBankWrapper>> {
		let endpoint = format!("user/{}/monetary-account-bank", self.state.owner_id);
		self.messenger.send(Method::GET, &endpoint, None).await
	}

	/// Fetches a list of monetary accounts
	pub async fn get_monetary_account(&self, bank_account_id: u32) -> Response<Single<MonetaryAccountBankWrapper>> {
		let endpoint = format!("user/{}/monetary-account-bank/{}", self.state.owner_id, bank_account_id);
		self.messenger.send(Method::GET, &endpoint, None).await
	}

	// /// Fetches a list of payment requests
	// pub async fn get_payment_requests(&self, monetary_account_id: u32) -> Response<Multiple<BunqMeTabWrapper>> {
	// 	let endpoint = format!("user/{}/monetary-account/{monetary_account_id}/bunqme-tab", self.state.owner_id);
	// 	self.messenger.send(Method::GET, &endpoint, None).await
	// }

	/// Fetches the payment request with given id
	pub async fn get_payment_request(&self, monetary_account_id: u32, payment_request_id: u32) -> Response<Single<BunqMeTabWrapper>> {
		let endpoint = format!("user/{}/monetary-account/{monetary_account_id}/bunqme-tab/{payment_request_id}", self.state.owner_id);
		self.messenger.send(Method::GET, &endpoint, None).await
	}

	/// Creates a new payment request
	pub async fn create_payment_request(&self, monetary_account_id: u32, amount: f32, description: String, redirect_url: String) -> Response<Single<CreateBunqMeTabResponseWrapper>> {
		let endpoint = format!("user/{}/monetary-account/{monetary_account_id}/bunqme-tab", self.state.owner_id);
		
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
}

