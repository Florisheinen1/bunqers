# bunqers

A Rust client library for the [Bunq API](https://doc.bunq.com/).

## Features

- Typed request and response bodies for all covered endpoints
- RSA request signing and response signature verification
- Typestate builder that enforces the correct setup order at compile time
- Serialisable `InstallationContext` so device registration survives process restarts
- Optional rate-limited client wrapper (`ratelimited` feature)

## Quick start

Add the dependency:

```toml
[dependencies]
bunqers = "0.1"
```

If you also want the rate-limited client:

```toml
[dependencies]
bunqers = { version = "0.1", features = ["ratelimited"] }
```

### First run — install and register the device

Device registration generates an RSA key pair and calls three Bunq endpoints.
Persist the returned `InstallationContext` (e.g. as JSON) so you can skip this
step on subsequent runs.

```rust
use bunqers::InstallationContext;

let installation: InstallationContext = bunqers::install_device(
    "your-api-key".into(),
    "https://api.bunq.com/v1".into(),
    "my-app".into(),
    "my-device".into(),
).await;

// Serialise and save `installation` to disk here.
```

### Subsequent runs — create a client from saved context

```rust
// Load `installation` from disk here.
let client = bunqers::create_client(installation, None).await;

let user = client.get_user().await.into_result()?;
println!("Hello, {}!", user.user_person.display_name);
```

Pass a cached session token as the second argument to `create_client` to avoid
creating a new session on every startup. The session is validated automatically
and a new one is created if it has expired.

### Rate-limited client

Bunq enforces rate limits per device. The optional `ratelimited` feature
provides `ClientRateLimited`, which queues requests and automatically retries
on a 429 response. Use `create_rate_limited_client` to get one without having
to configure rate limiters yourself:

```rust
use std::sync::Arc;

// Load `installation` from disk here.
let client_rl = Arc::new(bunqers::create_rate_limited_client(installation, None).await);

client_rl.get_user_ratelimited(|result| async move {
    let response = result.expect("rate limit exhausted");
    let user = response.into_result().expect("API error");
    println!("Hello, {}!", user.user_person.display_name);
}).await;
```

The callback receives `Ok(response)` on success or `Err(RateLimitExhausted)`
if all retries are used up. On a 429 the task is automatically re-queued as a
priority task — no extra callback or retry flag needed.

## Covered endpoints

| Endpoint | Implemented |
|:----------------------------------------|:-----------:|
| /installation | ✅ |
| /device-server | ✅ |
| /session-server | ✅ |
| /user | ✅ |
| /user/{id}/monetary-account-bank | ✅ |
| /user/{id}/monetary-account/{id}/bunqme-tab | ✅ |

More endpoints will be added on demand.

## System requirements

`bunqers` links against OpenSSL for RSA key generation and SHA-256 signing.
Make sure the OpenSSL development headers are installed on your system (e.g.
`libssl-dev` on Debian/Ubuntu or `openssl` via Homebrew on macOS).

## License

Licensed under MIT.

## Contributions

Contributions are welcome — feel free to open an issue or a pull request!
