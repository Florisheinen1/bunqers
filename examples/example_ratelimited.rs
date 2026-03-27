//! Example using the rate-limited client.

use std::{env, sync::Arc, time::Duration};

use bunqers::client::Client;
use bunqers::client_rate_limited::ClientRateLimited;
use ritlers::async_rt::RateLimiter;

#[tokio::main]
async fn main() {
	let mut args = env::args().skip(1);
	let bunq_api_key = args.next().expect("No API key passed as parameter");

	let api_base_url = "https://api.bunq.com/v1".into();
	let app_name = "example-ratelimited".into();

	// Install the device once and persist the resulting InstallationContext.
	// On subsequent runs, load it from disk and skip this step.
	let installation =
		bunqers::install_device(bunq_api_key, api_base_url, app_name, "my-device".into()).await;
	let client: Client = bunqers::create_client(installation, None).await;

	// Wrap the client in a rate-limited shell.
	// Bunq allows 3 GET and 1 POST per second by default.
	let client_rl = Arc::new(ClientRateLimited {
		client,
		ratelimiter_get: RateLimiter::new(3, Duration::from_secs(1)).unwrap(),
		ratelimiter_post: RateLimiter::new(1, Duration::from_secs(1)).unwrap(),
		ratelimiter_put: RateLimiter::new(1, Duration::from_secs(1)).unwrap(),
		max_retries: 0,
	});

	// Schedule a rate-limited user fetch.
	// on_response is called when the request succeeds (not a 429).
	// On a 429, the task is automatically re-queued as a priority task by ritlers.
	client_rl
		.get_user_ratelimited(|response| async move {
			let user = response
				.unwrap()
				.into_result()
				.expect("API returned an error");
			println!("Hello, {}!", user.user_person.display_name);
		})
		.await;

	// Fetch monetary accounts — callbacks follow the same pattern.
	client_rl
		.get_monetary_accounts_ratelimited(|response| async move {
			let accounts = response
				.unwrap()
				.into_result()
				.expect("API returned an error");
			for account in &accounts.data {
				println!(
					"Account {}: {} {}",
					account.id, account.balance.value, account.balance.currency
				);
			}
		})
		.await;

	// Create a payment request — uses ratelimiter_post.
	client_rl
		.create_payment_request_ratelimited(
			12345,
			"10.00".parse().unwrap(),
			"Example payment".into(),
			"https://example.com/redirect".into(),
			|response| async move {
				let created = response.into_result().expect("API returned an error");
				println!("Created payment request with id: {}", created.id.id);
			},
		)
		.await;
}
