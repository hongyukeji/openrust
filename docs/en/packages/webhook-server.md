# Webhook Server (Extension)

## Support status {#support-status}

The optional `openrust-webhook-server` package supplies signer and backoff contracts and package configuration. It does not yet provide a complete send Builder, durable deliveries, retries or endpoint management. Signing headers is not a network send.

## Enable and configure {#configuration}

Add `webhook-server` to the root application’s `[features].default` list, preserving existing features. Package discovery registers `WebhookServerServiceProvider`; other applications may register it directly. Defaults are merged without database or network access. Publish the template with the command below, add `mod webhook_server;` and `("webhook-server".to_owned(), webhook_server::load(env)?),` to `config/mod.rs`, then rebuild. Existing customization is preserved unless `--force` is explicit.

```sh
./artisan vendor:publish --tag=webhook-server-config
```

## Available entry points {#available-entry-points}

### Sign original bytes {#signatures}

`DefaultSigner` uses HMAC-SHA256 with a lowercase hexadecimal signature in `Signature`. Its default algorithm covers only the body; URL, message ID and timestamp changes do not change that signature. `DefaultSigner::new` selects a different header. `StandardWebhookSigner` uses the official `standardwebhooks 1.3.0` library and returns all three protocol headers; it requires a valid base64 secret, optionally prefixed `whsec_`, and UTF-8 body bytes.

`StandardWebhookSigner::headers` requires a non-empty ASCII message ID of at most 256 bytes, with no control characters or leading/trailing whitespace, so the produced headers survive HTTP parsing unchanged.

```rust
use openrust::http::HeaderMap;
use openrust_webhook_server::{
    DefaultSigner, Result, Signer, SigningContext, StandardWebhookSigner,
};

pub fn spatie_signature(body: &[u8], secret: &str) -> Result<String> {
    DefaultSigner::default().calculate_signature(&SigningContext {
        webhook_url: "https://example.com/webhook",
        body, secret, message_id: "message-1", timestamp: 0,
    })
}

pub fn standard_headers(
    body: &[u8], secret: &str, message_id: &str, timestamp: i64,
) -> Result<HeaderMap> {
    StandardWebhookSigner.headers(&SigningContext {
        webhook_url: "https://example.com/webhook",
        body, secret, message_id, timestamp,
    })
}
```

## Custom signers and retry timing {#customization}

Implement `Signer` in an external crate. `SigningContext` supplies immutable body bytes, URL, runtime credential, stable message ID and per-attempt timestamp; this context is the Rust adaptation of the upstream signature arguments. Its secret is borrowed, and the context is not serializable or Debug-printable. The `headers` method lets protocols provide their additional authenticated headers. Do not add secrets to request metadata, logs or ordinary queue payloads.

`BackoffStrategy::wait_in_seconds_after_attempt` returns seconds. The default `ExponentialBackoffStrategy` follows the fixed baseline: attempts 1–4 wait 10, 100, 1000 and 10000 seconds; attempts above 4 wait 100000 seconds. Zero produces 1 second for the low-level contract; a first real attempt is numbered 1.

These helpers do not supply SSRF protection, durable claims, deduplication or an HTTP retry loop. Use them only inside an application flow that already provides those guarantees; the package does not yet expose a production delivery service.

## Scope {#scope}

Use the existing [HTTP client](../digging-deeper/http-client.md) and [queues](../digging-deeper/queues.md) for application networking and jobs. These package primitives do not provide durable processing or a delivery state machine.
