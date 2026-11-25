# Webhook Client (Extension)

## Support status {#support-status}

The optional `openrust-webhook-client` package supplies named configuration, signature validators, a manual `WebhookProcessor` and scoped database receipt storage. Applications provide the job dispatcher. Automatic callback routing, parsed payloads/attachments, deduplication and recovery remain unavailable. A saved receipt and successful dispatch do not prove business completion. No public callback route is installed automatically.

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

pub async fn receive(
    request: &WebhookRequest,
    config: &openrust_webhook_client::WebhookConfig,
    connection: openrust::database::Connection,
    scope: &str,
    job: std::sync::Arc<dyn openrust_webhook_client::WebhookJobDispatcher>,
    events: std::sync::Arc<dyn openrust::contracts::events::Dispatcher>,
) -> openrust_webhook_client::Result<openrust::http::Response> {
    use openrust_webhook_client::{DatabaseWebhookStore, WebhookProcessor, WebhookRuntime};
    let store = std::sync::Arc::new(DatabaseWebhookStore::new(connection, scope)?);
    let runtime = WebhookRuntime::new(store, job, events);
    WebhookProcessor::new(request, config, &runtime).process().await
}
```

`StandardSignatureValidator` uses the official `standardwebhooks 1.3.0` library with `webhook-id`, `webhook-timestamp` and `webhook-signature`. Supply a valid base64 or `whsec_` secret. Its five-minute timestamp window and multiple signatures in one header are supported; duplicate protocol headers are rejected. The library requires UTF-8 payloads. Validation does not replace persistent event deduplication.

## Custom implementations and snapshots {#customization}

Implement `SignatureValidator`, `WebhookProfile`, `RespondsToWebhook`, `WebhookStore` or `WebhookJobDispatcher` in another crate. `ProcessEverythingWebhookProfile` selects every request; `DefaultRespondsTo` produces `{"message":"ok"}`. Build `WebhookRuntime` with the application’s store, job dispatcher and event dispatcher. Select custom validators, profiles and responses through its builder methods; configuration class-name fields do not yet resolve these implementations automatically.

`WebhookConfigRepository::add_config` replaces a named configuration. Owned `Arc<WebhookConfig>` values retain their original revision and key snapshot. Unknown names return `Ok(None)`, and resolution errors remain errors. The provider binds the repository to the current HTTP, Artisan or Worker lifecycle; runtime additions do not leak into the next lifecycle. `WebhookConfig::new` accepts a runtime-injected secret, keeps it private and redacts Debug; do not serialize that secret into jobs or logs. `with_verification_secrets` adds at most seven previous keys for a total of eight in one profile.

Configuration caching keeps credential references, so `config:show webhook-client` does not expose resolved keys from package defaults. Each process reads its own immutable environment snapshot when resolving a profile; restart long-running processes for credential changes. Retaining historical credentials across process restarts and queued work is not yet implemented. The `revision` label is not a substitute for a durable credential revision store.

### Store and dispatch a receipt {#receipts}

The provider registers the `webhook_calls` migration. Run `./artisan migrate` before using `DatabaseWebhookStore`. Supply a stable application/tenant scope of at most 128 bytes, shared by every node for that same scope. Reads and exception updates are scoped; a digest key prevents database collation from mixing differently cased scopes. This is storage isolation, not caller authorization: derive the scope from trusted application context.

`WebhookProcessor::process` validates, applies the profile, stores the receipt, clears the previous exception, dispatches the job, then creates the response. Invalid signatures dispatch the typed `InvalidWebhookSignatureEvent` before returning an error; that event gives trusted application listeners access to the request and configuration and has no automatic serializable diagnostic properties. Validator errors remain errors. Ignored valid requests only receive the configured response. Failed storage or dispatch does not return a success response. Dispatch errors leave a bounded, generic exception record, without arbitrary third-party error text or stack traces.

`models::WebhookCall` delegates `store_webhook`, `find`, `save_exception` and `clear_exception` to the store. It retains the exact raw body, request target supplied by `with_uri`, method, revision and selected headers, including duplicate and opaque header values. A stored record is not a verification token. `VerifiedWebhook` is created only by the processor after validation and profile acceptance. The database representation uses base64 for raw bytes; it is not encryption. Apply database access and retention controls.

No headers are stored by default. `with_store_headers(&["X-Source"])` selects names case-insensitively; `with_store_headers(&["*"])` explicitly stores all headers, including potentially sensitive values. The config-file equivalent is the string `"*"`, not an array containing `"*"`. Exception JSON is limited to 8192 bytes, request targets to 8192 bytes and stored methods to 32 bytes. Applications must redact manually supplied exception details.

The provided `WebhookJobDispatcher` implementation must enqueue only a receipt identifier and trusted scope, then load that receipt in the worker. It must not serialize signing keys or the full model. Queued processing should use the accepted record instead of revalidating an expired signature. This API does not yet close the commit/dispatch crash gap, deduplicate requests, claim processing leases or supply an automatically registered processing job. Use it only with those limitations understood.

## Scope {#scope}

Use the existing [HTTP client](../digging-deeper/http-client.md) and [queues](../digging-deeper/queues.md) for application networking and jobs. Persistent recovery, parsed form/attachment handling, the public route/controller and a complete delivery state machine remain outside the current implementation.
