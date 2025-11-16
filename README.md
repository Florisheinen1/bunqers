# bunqers

Convenience Client for using Bunq API in Rust.

## Goals

- Explicit API datatypes and endpoints
- Reusable session data
- Verifying response signatures

### Non-goals

- Rate limiting

## Usage

```rust
let bunq_api_key = "your-api-key".into();
let api_base_url = "https://api.bunq.com/v1".into();

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

// Use the client object for fetching data from the API
let name = client.get_user().await.into_result()?.user_person.display_name;
println!("Hello, {name}!");
```

## Endpoints covered

The following endpoints have datatype definitions and corresponding method:
| Endpoint | Implemented |
|:----------------------------------------|:-----------:|
| /installation | ✅ |
| /device-server | ✅ |
| /session-server | ✅ |
| /user | ✅ |
| /user/{}/monetary-account-bank | ✅ |
| /user/{}/monetary-account/{}/bunqme-tab | ✅ |

More will be added on demand

## License

Licensed under MIT

## Contribution

Contributions are welcome. Feel free to create an Issue or open a PR!
