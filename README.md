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
// 1. Create session context from last time, or None's if this is the first time
let context = NoSessionContext {
	installation_token: Some("installation token"),
	bunq_public_key: Some("bunq's public key"),
	registered_device_id: Some("this device's ID"),
	session_token: Some("session token"),
	owner_id: None,
};

// 2. Build the client
let no_session_client  = Client::from_context(
	"bunq_api_key",
	"private_key",
	context
);

// 3. Convert to session-client by reusing or creating session
let session_client = no_session_client.get_session().await;

// Don't forget to save the new session context for next time!
let new_context = session_client.get_session_context();

// 4. And use the endpoints!
let response: Response<Single<User>> = session_client.get_user().await;
```

## Endpoints covered
The following endpoints have datatype definitions and corresponding method:
| Endpoint                                | Implemented |
|:----------------------------------------|:-----------:|
| /installation                           | ✅          |
| /device-server                          | ✅          |
| /session-server                         | ✅          |
| /user                                   | ✅          |
| /user/{}/monetary-account-bank          | ✅          |
| /user/{}/monetary-account/{}/bunqme-tab | ✅          |

More will be added on demand

## License
Licensed under MIT

## Contribution
Contributions are welcome. Feel free to create an Issue or open a PR!