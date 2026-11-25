# Webhook Server (Extension)

## Support status {#support-status}

The optional `openrust-webhook-server` package provides request preparation, target validation, named signers, backoff strategies and durable delivery attempts. Replay and endpoint management are still in development.

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

fn invoice_call() -> Result<openrust_webhook_server::WebhookCall> {
    use openrust_webhook_server::WebhookCall;

    Ok(WebhookCall::new()?
        .url("https://billing.example.com/callback")
        .uuid("invoice-issued-inv-123")
        .payload(serde_json::json!({"invoice_id": "inv-123"}))
        .use_secret_reference("billing-v1")
        .on_connection(Some("database".into())))
}

pub fn prepare_invoice() -> Result<openrust_webhook_server::PreparedWebhook> {
    invoice_call()?.prepare(&openrust_webhook_server::TargetPolicy::default())
}

pub async fn submit_invoice(store: &openrust_webhook_server::DeliveryStore) -> Result<String> {
    store.submit("invoice-issued-inv-123", &prepare_invoice()?, 3600).await
}

pub async fn queue_invoice(
    service: &openrust_webhook_server::DeliveryService,
    queue: &openrust::queue::QueueManager,
) -> Result<openrust_webhook_server::QueuedDelivery> {
    invoice_call()?.dispatch(
        service, queue, &openrust_webhook_server::TargetPolicy::default(),
        "invoice-issued-inv-123", 3600,
    ).await
}

pub fn schedule_webhook_recovery(schedule: &mut openrust::console::scheduling::Schedule) {
    schedule
        .command("webhook-server:recover", ["--scope=billing".into()])
        .every_minute();
}

struct StopOnGone;

impl openrust_webhook_server::CallWebhookJob for StopOnGone {
    fn should_be_removed_from_queue(
        &self,
        attempt: &openrust_webhook_server::WebhookJobAttempt<'_>,
    ) -> bool {
        attempt.get_response().is_some_and(|response| response.status().as_u16() == 410)
    }
}

pub fn register_delivery_job(
    container: &openrust::container::Container,
    service: &openrust_webhook_server::DeliveryService,
    queue: &openrust::queue::QueueManager,
) -> Result<()> {
    openrust_webhook_server::register_call_webhook_job(
        container, "stop-on-gone", std::sync::Arc::new(StopOnGone),
    )?;
    service.register_worker(queue, "stop-on-gone")
}
```


### Prepare an outgoing request {#prepare}

Use `WebhookCall::create()` in a booted application to load the package configuration, or `WebhookCall::new()` for the built-in defaults. Set the destination, payload and credential, then call `prepare(&TargetPolicy::default())`. A prepared request keeps its stable ID, serialized payload and scheduling settings. `headers_at` signs the stored bytes using the attempt timestamp. Use a named secret reference when the application resolves credentials at execution time.

POST and other body methods use the prepared bytes. GET sends an RFC 3986 query and uses the prepared JSON for the Spatie signature. The raw body option preserves arbitrary bytes. The snapshot is bounded to 16 MiB of body data and 64 KiB of headers. Register external implementations in the application container with `register_signer` and `register_backoff_strategy`.

The default target policy requires HTTPS and validates every resolved socket address. Use a policy built from trusted endpoint configuration for an internal route with a fixed origin, path and addresses. Applications can use `PreparedWebhook::bind_target` to validate an address list for one attempt. The HTTP client provides a pinned transport that preserves Host and SNI and disables redirects and proxy routing. HTTPS certificate verification remains the default.

### Submit and send a delivery {#delivery}

Run `./artisan migrate` after enabling the package. Create a `DeliveryStore` with the application’s database connection and delivery scope. Use `submit_in_transaction` to record an outgoing delivery in the same database transaction as a business change. Use `submit` when the delivery has its own transaction. The same key and identical snapshot return the existing delivery ID. Changing the snapshot for that key returns a conflict. Assign a stable message ID before preparing repeated submissions.

Enable `register_delivery_policy` in the application. The policy evaluates the current delivery scope, URL and credential reference for each attempt. Resolve secrets at runtime. The saved snapshot excludes inline secrets and sensitive headers. Queue payloads contain only the delivery ID. Keep sensitive values out of the URL, body and metadata wherever possible.

For queued sending, register the prepared Job name with `DeliveryService::register_worker` in each Worker process. Use the same named database, scope and policy on all Workers, and configure a direct Worker queue connection such as `database`. Each command or Worker can claim only one attempt for a given delivery at a time. HTTP 2xx succeeds. Other statuses follow the configured retry rules. Timeouts and interrupted attempts can leave the remote result unknown. Use the stable message ID for receiver-side deduplication.

Use the Builder’s `dispatch` / `dispatch_if` / `dispatch_unless` methods to save and enqueue a delivery. The `dispatch_sync` variants save the delivery and attempt it in the current process. Pass a Rust boolean to the conditional variants. A skipped call returns `None` before preparing a request. A synchronous call with `throw_exception_on_failure(true)` returns a typed error with the delivery ID on terminal failure. Queued terminal failures use the native failed-job mechanism.

Register synchronous listeners for these events under `openrust_webhook_server::events`. `DispatchingWebhookCallEvent` runs after the delivery record commits and before the queue push. `WebhookCallSucceededEvent` and `WebhookCallFailedEvent` report committed attempts. A terminal failure also emits `FinalWebhookCallFailedEvent` after the attempt failure. The shared payload exposes message and delivery IDs, request method, URL path, body and metadata accessors, and the attempt’s status. Request header values and response contents stay out of the event, and the URL omits its query string. Dispatch-time listener errors leave a recoverable record. A result listener error leaves the recorded result intact.

```sh
./artisan webhook-server:send <delivery-id> --scope=billing
./artisan webhook-server:send <delivery-id> --scope=billing --queue
./artisan webhook-server:send --scope=billing --limit=100
```

### Schedule recovery {#recovery}

Register `webhook-server:recover` with the application's scheduler and run `./artisan schedule:run` on each minute tick. The command uses the delivery's Worker route and returns counts for candidates, enqueued jobs, expired or exhausted deliveries, skipped claims and dispatch failures. `--pretend` lists the count without taking a claim. Use `--database` when the delivery store selects a named connection.

Recovery reserves a 60-second wake-up window before pushing to the queue. A later scan can recover an unsent or lost job after that window. A Worker claim or a committed retry outcome clears the window, so the delivery's existing lease and retry schedule control the next attempt. Each scan selects at most `--limit` records and allows up to 30 seconds for dispatch. Multiple schedulers coordinate through the shared database. Keep the configured Worker route available on each node.

```sh
./artisan webhook-server:recover --scope=billing --limit=100
./artisan webhook-server:recover --scope=billing --pretend
```

### Customize a delivery job {#custom-job}

Implement `CallWebhookJob` and register the same name with `register_call_webhook_job` and `DeliveryService::register_worker` on the producer, Scheduler and Workers. Select it with `.use_job("stop-on-gone")`. The hook `should_be_removed_from_queue` can stop a retry eligible under the original delivery budget. `failed` observes committed terminal failures from an attempt after the result events. Keep both hooks short and synchronous.

The hook can inspect the original body, URL, tags, metadata and the bounded native response. Transport, credential resolution, attempts and persistence remain managed by the package. Saved custom selections require the named hook at runtime. A missing hook aborts the claim without consuming an attempt. Enable the registration on all consumers before submitting jobs that use it.

## Custom signers and retry timing {#customization}

Implement `Signer` in an external crate. `SigningContext` supplies immutable body bytes, URL, runtime credential, stable message ID and per-attempt timestamp; this context is the Rust adaptation of the upstream signature arguments. Its secret is borrowed, and the context is not serializable or Debug-printable. The `headers` method lets protocols provide their additional authenticated headers. Do not add secrets to request metadata, logs or ordinary queue payloads.

`BackoffStrategy::wait_in_seconds_after_attempt` returns seconds. The default `ExponentialBackoffStrategy` follows the fixed baseline: attempts 1–4 wait 10, 100, 1000 and 10000 seconds; attempts above 4 wait 100000 seconds. Zero produces 1 second for the low-level contract; a first real attempt is numbered 1.


## Scope {#scope}

Use the existing [HTTP client](../digging-deeper/http-client.md) and [queues](../digging-deeper/queues.md) for networking and jobs. Pending deliveries may need another queue wake-up or a bounded scan after their next attempt time.
