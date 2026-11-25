# Webhook Server (Extension)

## Support status {#support-status}

The optional `openrust-webhook-server` package provides request preparation, target validation, named signers and backoff strategies. Durable sending, retries and endpoint management remain in development.

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

pub fn prepare_invoice() -> Result<openrust_webhook_server::PreparedWebhook> {
    use openrust_webhook_server::{TargetPolicy, WebhookCall};

    WebhookCall::new()?
        .url("https://billing.example.com/callback")
        .payload(serde_json::json!({"invoice_id": "inv-123"}))
        .use_secret_reference("billing-v1")
        .prepare(&TargetPolicy::default())
}
```


### Prepare an outgoing request {#prepare}

Use `WebhookCall::create()` in a booted application to load the package configuration, or `WebhookCall::new()` for the built-in defaults. Set the destination, payload and credential, then call `prepare(&TargetPolicy::default())`. A prepared request keeps its stable ID, serialized payload and scheduling settings. `headers_at` signs the stored bytes using the attempt timestamp. Use a named secret reference when the application resolves credentials at execution time.

POST and other body methods use the prepared bytes. GET sends an RFC 3986 query and uses the prepared JSON for the Spatie signature. The raw body option preserves arbitrary bytes. The snapshot is bounded to 16 MiB of body data and 64 KiB of headers. Register external implementations in the application container with `register_signer` and `register_backoff_strategy`.

The default target policy requires HTTPS and validates every resolved socket address. Use a policy built from trusted endpoint configuration for an internal route with a fixed origin, path and addresses. Applications can use `PreparedWebhook::bind_target` to validate an address list for one attempt. The HTTP client provides a pinned transport that preserves Host and SNI and disables redirects and proxy routing. HTTPS certificate verification remains the default. Durable claims and attempt records remain prerequisites for package dispatch.

## Custom signers and retry timing {#customization}

Implement `Signer` in an external crate. `SigningContext` supplies immutable body bytes, URL, runtime credential, stable message ID and per-attempt timestamp; this context is the Rust adaptation of the upstream signature arguments. Its secret is borrowed, and the context is not serializable or Debug-printable. The `headers` method lets protocols provide their additional authenticated headers. Do not add secrets to request metadata, logs or ordinary queue payloads.

`BackoffStrategy::wait_in_seconds_after_attempt` returns seconds. The default `ExponentialBackoffStrategy` follows the fixed baseline: attempts 1–4 wait 10, 100, 1000 and 10000 seconds; attempts above 4 wait 100000 seconds. Zero produces 1 second for the low-level contract; a first real attempt is numbered 1.


## Scope {#scope}

Use the existing [HTTP client](../digging-deeper/http-client.md) and [queues](../digging-deeper/queues.md) for application networking and jobs. These package primitives do not provide durable processing or a delivery state machine.
