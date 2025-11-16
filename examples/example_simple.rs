//! This document is a minimal example
//! It shows:
//! - Setting up a session
//! - Fetchign user data

use std::{env, time::Duration};

use bunqers::client_builder::ClientBuilder;

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
	let mut args = env::args().skip(1);
	let bunq_api_key = args.next().expect("No API key passed as parameter");
	println!("Entered API key: {bunq_api_key}");

	let api_base_url = format!("https://api.bunq.com/v1");

	let client = ClientBuilder::new_without_key(api_base_url, "example-app-name".into())
		.expect("Failed to create private key")
		.install_device()
		.await
		.expect("Failed to install device")
		.register_device(bunq_api_key, "my-test-device")
		.await
		.expect("Failed to register device")
		.create_session()
		.await
		.expect("Failed to create session")
		.build();

	// Cooldown just to be sure
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

	Ok(())
}
