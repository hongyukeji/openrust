# Webhook Client (Extension)

## Support status {#support-status}

The optional `openrust-webhook-client` package supplies named configuration, signature validators, profile and response contracts. Durable receipt storage, business jobs and recovery are not yet available. Signature validation alone does not acknowledge durable receipt or prove business processing. No public callback route is installed automatically.

## Enable and configure {#configuration}

Add `webhook-client` to the root application’s `[features].default` list, preserving existing features; package discovery registers `WebhookClientServiceProvider`. Other applications may depend on the crate and register that provider directly. Registration and boot do not query a database or contact a supplier.

The default profile is `default`, using the `Signature` header and a credential reference to `WEBHOOK_CLIENT_SECRET`. Supply that secret in the process environment or protected application `.env`. The package configuration stores the environment variable name, not the resolved secret. To customize, publish the template, add `mod webhook_client;` and `("webhook-client".to_owned(), webhook_client::load(env)?),` in `config/mod.rs`, then rebuild. Publication preserves existing customization unless `--force` is explicit.

```sh
./artisan vendor:publish --tag=webhook-client-config
```

## Available entry points {#available-entry-points}

### Validate untouched bytes {#signatures}

Apply the configured body limit while reading the HTTP stream, before creating `WebhookRequest`. Preserve the original body bytes and header values. The default validator uses HMAC-SHA256 and requires the exact lowercase hexadecimal signature. It does not sign the URL or timestamp and supplies no replay window. Duplicate signature headers are rejected.

The default body limit is 1 MiB; `with_max_body_bytes` accepts up to 16 MiB. A snapshot permits at most 128 header values and 64 KiB of combined header names and values. Configuration sources permit at most 128 profiles; names and revision labels are limited to 128 bytes and individual secrets to 8192 bytes. The HTTP transport may enforce lower limits.

Custom validators that sign the request target should attach the original `Request::uri()` with `WebhookRequest::with_uri`; query encoding is preserved.

```rust
use openrust::{foundation::Application, http::{HeaderMap, Method}};
use openrust_webhook_client::{
    DefaultSignatureValidator, Error, SignatureValidator,
    WebhookConfigRepository, WebhookRequest,
};

pub async fn verify_signature(
    app: &Application,
    headers: HeaderMap,
    original_body: Vec<u8>,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let configs = app.make::<WebhookConfigRepository>().await?;
    let config = configs.get_config("default")?
        .ok_or(Error::InvalidConfig("name"))?;
    let request = WebhookRequest::new(Method::POST, headers, original_body)?;
    Ok(DefaultSignatureValidator.is_valid(&request, &config)?)
}
```

`StandardSignatureValidator` uses the official `standardwebhooks 1.3.0` library with `webhook-id`, `webhook-timestamp` and `webhook-signature`. Supply a valid base64 or `whsec_` secret. Its five-minute timestamp window and multiple signatures in one header are supported; duplicate protocol headers are rejected. The library requires UTF-8 payloads. Validation does not replace persistent event deduplication.

## Custom implementations and snapshots {#customization}

Implement `SignatureValidator`, `WebhookProfile` or `RespondsToWebhook` in another crate. `ProcessEverythingWebhookProfile` selects every request, while `DefaultRespondsTo` produces `{"message":"ok"}`. Only call the response contract after your own required durable acceptance steps. The package has no complete Processor or business Worker yet.

`WebhookConfigRepository::add_config` replaces a named configuration. Owned `Arc<WebhookConfig>` values retain their original revision and key snapshot. Unknown names return `Ok(None)`, and resolution errors remain errors. The provider binds the repository to the current HTTP, Artisan or Worker lifecycle; runtime additions do not leak into the next lifecycle. `WebhookConfig::new` accepts a runtime-injected secret, keeps it private and redacts Debug; do not serialize that secret into jobs or logs. `with_verification_secrets` adds at most seven previous keys for a total of eight in one profile.

Configuration caching keeps credential references, so `config:show webhook-client` does not expose resolved keys from package defaults. Each process reads its own immutable environment snapshot when resolving a profile; restart long-running processes for credential changes. Retaining historical credentials across process restarts and queued work is not yet implemented. The `revision` label is not a substitute for a durable credential revision store.

## Scope {#scope}

Use the existing [HTTP client](../digging-deeper/http-client.md) and [queues](../digging-deeper/queues.md) for application networking and jobs. These package primitives do not provide durable processing or a delivery state machine.
