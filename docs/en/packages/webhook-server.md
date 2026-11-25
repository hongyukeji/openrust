# Webhook Server (Extension)

## Support status {#support-status}

The optional `openrust-webhook-server` package provides request preparation, target validation, named signers, backoff strategies and durable delivery attempts. The package also supports versioned endpoints.

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

pub fn register_application_replay_policy(
    container: &openrust::container::Container,
    policy: std::sync::Arc<dyn openrust_webhook_server::ReplayPolicy>,
) -> Result<()> {
    openrust_webhook_server::register_replay_policy(container, policy)
}

pub async fn prepare_endpoint_invoice(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::PreparedWebhook> {
    let endpoint = store.endpoints().find("billing").await?
        .ok_or(openrust_webhook_server::Error::EndpointRevoked)?;
    invoice_call()?.use_endpoint(&endpoint)
        .prepare(&openrust_webhook_server::TargetPolicy::default())
}

pub fn configure_webhook_limits(container: &openrust::container::Container) -> Result<()> {
    use openrust_webhook_server::{DeliveryLimit, DeliveryLimits, register_delivery_limits};

    register_delivery_limits(container, DeliveryLimits {
        sends_per_scope: Some(DeliveryLimit::new(100, 60)?),
        sends_per_endpoint: Some(DeliveryLimit::new(20, 60)?),
        replays_per_scope: Some(DeliveryLimit::new(5, 60)?),
    })
}

pub async fn publish_invoice(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::PublishedMessage> {
    use openrust_webhook_server::{EventType, PublishOptions, TargetPolicy};

    store.publish(
        "invoice-paid-inv-123",
        &EventType::new("invoice.paid", 1)?,
        serde_json::json!({"invoice_id": "inv-123"}),
        &PublishOptions::default(),
        &TargetPolicy::default(),
    ).await
}

pub fn grant_invoice_tls(
    policy: openrust_webhook_server::TargetPolicy,
    ca: &[u8],
    client_chain: &[u8],
    client_key: &[u8],
) -> Result<openrust_webhook_server::DeliveryPermission> {
    openrust_webhook_server::DeliveryPermission::new(policy)
        .with_tls_ca(ca)?
        .with_mutual_tls(client_chain, client_key)
}


pub fn grant_invoice_proxy(
    permission: openrust_webhook_server::DeliveryPermission,
    address: std::net::SocketAddr,
) -> Result<openrust_webhook_server::DeliveryPermission> {
    let proxy = openrust::http::client::PinnedSocksProxy::new(address)
        .map_err(|_| openrust_webhook_server::Error::InvalidConfig("proxy"))?;
    permission.with_proxy("corp-egress", proxy)
}


pub fn grant_invoice_connect_proxy(
    permission: openrust_webhook_server::DeliveryPermission,
    address: std::net::SocketAddr,
) -> Result<openrust_webhook_server::DeliveryPermission> {
    let proxy = openrust::http::client::PinnedHttpProxy::new(address)
        .map_err(|_| openrust_webhook_server::Error::InvalidConfig("proxy"))?;
    permission.with_proxy("corp-egress", proxy)
}

pub fn grant_invoice_https_proxy(
    permission: openrust_webhook_server::DeliveryPermission,
    address: std::net::SocketAddr,
    server_name: &str,
    ca_pem: &[u8],
) -> Result<openrust_webhook_server::DeliveryPermission> {
    use openrust::http::client::{PinnedHttpProxy, TlsCertificate};
    let roots = TlsCertificate::from_pem_bundle(ca_pem)
        .map_err(|_| openrust_webhook_server::Error::InvalidConfig("proxy"))?;
    let proxy = PinnedHttpProxy::https(address, server_name, roots)
        .map_err(|_| openrust_webhook_server::Error::InvalidConfig("proxy"))?;
    permission.with_proxy("corp-egress", proxy)
}

pub async fn inspect_invoice_attempts(
    store: &openrust_webhook_server::DeliveryStore,
    id: &str,
    after: u32,
) -> Result<openrust_webhook_server::DeliveryAttemptsPage> {
    store.inspect_attempts(id, after, 25).await
}


pub async fn preview_invoice_pruning(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::DeliveryPruneReport> {
    store.prune(openrust_webhook_server::DeliveryRetention::Payloads, 30, 100, true).await
}

pub async fn preview_shared_body_pruning(
    store: &openrust_webhook_server::DeliveryStore,
    after: Option<&str>,
) -> Result<openrust_webhook_server::PublicationPruneReport> {
    store.prune_publication_bodies(30, 100, after, true).await
}

pub async fn hold_invoice_delivery(
    store: &openrust_webhook_server::DeliveryStore,
    id: &str,
) -> Result<bool> {
    store.set_retention_hold(openrust_webhook_server::RetentionRecord::Delivery, id, true).await
}

pub async fn preview_expiring_invoice_keys(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::IdempotencyPruneReport> {
    store.prune_idempotency_keys(openrust_webhook_server::IdempotencyKind::Delivery, 30, 100, true).await
}

pub async fn preview_invoice_history_pruning(
    store: &openrust_webhook_server::DeliveryStore,
    after: Option<&str>,
) -> Result<openrust_webhook_server::HistoryPruneReport> {
    store.prune_history(openrust_webhook_server::IdempotencyKind::Delivery, 90, 100, after, true).await
}

pub async fn preview_invoice_replay_audit_pruning(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::ReplayAuditPruneReport> {
    store.prune_replay_audit(90, 100, true).await
}

```


### Prepare an outgoing request {#prepare}

Use `WebhookCall::create()` in a booted application to load the package configuration, or `WebhookCall::new()` for the built-in defaults. Set the destination, payload and credential, then call `prepare(&TargetPolicy::default())`. A prepared request keeps its stable ID, serialized payload and scheduling settings. `headers_at` signs the stored bytes using the attempt timestamp. Use a named secret reference when the application resolves credentials at execution time.

POST and other body methods use the prepared bytes. GET sends an RFC 3986 query and uses the prepared JSON for the Spatie signature. The raw body option preserves arbitrary bytes. The snapshot is bounded to 16 MiB of body data and 64 KiB of headers. Register external implementations in the application container with `register_signer` and `register_backoff_strategy`.

The default target policy requires HTTPS and validates every resolved socket address. Use a policy built from trusted endpoint configuration for an internal route with a fixed origin, path and addresses. Applications can use `PreparedWebhook::bind_target` to validate an address list for one attempt. The HTTP client provides a pinned transport that preserves Host and SNI and disables redirects and uses a direct connection by default. HTTPS certificate verification remains the default.

### Configure TLS {#tls}

For a private CA or mutual TLS, let the application's `DeliveryPolicy` authorize the scope and exact destination first. Return a `DeliveryPermission` with `with_tls_ca(&pem_bundle)` to replace the system roots for that peer, or `with_mutual_tls(&certificate_chain, &private_key)` to present a client identity. The Worker loads PEM material from trusted application configuration. Supply the decrypted private key at runtime. The queue, delivery snapshot and events carry no PEM material. The connection preserves the destination hostname for both SNI and certificate verification.

For an isolated test, `do_not_verify_ssl()` disables server certificate and hostname verification only when the application's policy also returns `allow_unverified_tls()`. An attempt to skip verification is denied without policy permission or when custom CA or client credentials are attached to that call. Restore verification with `verify_ssl(true)`.

### Use a named outbound proxy {#proxy}

Set `webhook-server.proxy` to an approved name such as `corp-egress`, or select it per delivery with `use_proxy(Some("corp-egress".into()))`. Use `use_proxy(None)` to send directly. The application policy grants the matching name with `DeliveryPermission::with_proxy` after authorizing the delivery scope and destination. Override `DeliveryPolicy::authorize_transport` to inspect the requested name. Only the name is stored with the delivery. Build the proxy from a trusted numeric socket address and supply optional credentials at runtime.

Use `PinnedSocksProxy` for SOCKS5 with local address resolution. For CONNECT, use `PinnedHttpProxy::https` with the proxy’s numeric address, certificate name, and trusted CA certificates. The Worker verifies the proxy and the destination independently. Destination mTLS credentials go only to the destination. The proxy receives the verified destination IP and port, while destination TLS and HTTP use the original host. Use `PinnedHttpProxy::new` for plain HTTP CONNECT on a trusted network because proxy authentication is unencrypted on that link. Missing authorization or proxy errors prevent delivery from falling back to a direct connection. SOCKS5 remote DNS and client certificates for the proxy connection are currently unsupported.

### Submit and send a delivery {#delivery}

Run `./artisan migrate` after enabling the package. Create a `DeliveryStore` with the application’s database connection and delivery scope. Use `submit_in_transaction` to record an outgoing delivery in the same database transaction as a business change. Use `submit` when the delivery has its own transaction. The same key and identical snapshot return the existing delivery ID. Changing the snapshot for that key returns a conflict. Assign a stable message ID before preparing repeated submissions. Treat returned delivery IDs and published message IDs as opaque values. The same scoped key keeps its original result, including records registered before the idempotency mapping migration. Apply migrations and stop older writers before deploying the new write path.

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

### Replay a delivery {#replay}

Register an application `ReplayPolicy` before using replay. Validate the actual operator identity and its permission to act on the requested scope and endpoint. Use `ReplayPhase::Request` to apply operator admission limits and `ReplayPhase::Attempt` to recheck authorization during execution.

Replay creates an audited delivery with its own attempts and deadline. It reuses the original message ID, body and destination, and the policy can authorize a different named signing credential. The scheduler sends it through the same Worker pipeline. The default source is a terminal failure. A previously successful source also requires `--include-succeeded` and explicit policy permission. The command reports both delivery IDs and the new generation.

```sh
./artisan webhook-server:replay <delivery-id> --scope=billing --actor=operator-42 --reason="confirmed receiver recovery"
```

### Manage endpoint versions {#endpoints}

Use `DeliveryStore::endpoints()` from an authorized application operation. Create an endpoint with a name, URL and optional credential reference. Bind a call with `.use_endpoint(&endpoint)`. `replace`, `pause`, `resume` and `delete` take an expected revision so stale administrative changes are rejected.

Pause retains pending deliveries and their original deadlines. Resume allows eligible deliveries to proceed. Replacing the URL or credential creates a new revision for new deliveries. Old revisions and deleted endpoints are cancelled on execution. A request already sent may still complete at the receiver, and the attempt records the observed response. Replay also checks the current endpoint approval. Existing URL calls use the application's delivery policy.

### Share delivery limits {#limits}

Register `DeliveryLimits` in the application on each node. A scope limit includes direct URL calls, queued deliveries, retries and replay sends. A named endpoint also uses its own bucket, shared across its configuration revisions. Replay registration has a separate optional scope limit. Each window starts with its first reservation and uses the database clock.

When a send reaches a limit, the delivery waits until the bucket resets with its original attempt budget and deadline. A limited replay registration returns an error with the next eligible Unix timestamp. Keep the limit configuration consistent on all nodes sharing the delivery database.

### Inspect deliveries and attempts {#inspect}

Use `./artisan webhook-server:inspect <id> --scope=<scope>` for a delivery ID. Add `--message` for the internal message ID returned by `publish`. Add `--attempts --limit=25` for an attempt page, then pass the returned `next_after` to `--after` if another page exists. All timestamps are Unix seconds. `--database` selects a named database connection.

Results include state, timing, HTTP status and bounded attempt outcomes. Published message summaries include the event type, schema version and original delivery IDs. The package does not mount a diagnostics HTTP route. Authorize the scope in the application before calling the read-only `DeliveryStore` methods. Give Artisan access to trusted operators because the command accepts a database and scope. Diagnostics contain no request URL, body, headers, signing material or remote response body.

### Prune outgoing delivery data {#prune}

Run `./artisan webhook-server:prune --scope=<scope> --days=30 --pretend` to preview candidates. Remove `--pretend` to clear the stored request snapshot while retaining the delivery ID, state and idempotency digest. The cutoff is measured from the delivery deadline. Add `--attempts` to prune finished attempt rows separately using their completion time. Each invocation handles up to 500 rows, with 100 by default. `--database` selects a named connection.

Apply migrations and upgrade all processes that can register replays before enabling pruning. Only terminal deliveries qualify. A pruned original can no longer be used to register a replay. Replays already registered can still send their own stored request. Diagnostics expose `payload_pruned_at` in Unix seconds after pruning.

Use `--messages` to prune shared publication bodies after the publication retention period. A message qualifies once every original and replay delivery has had its request snapshot pruned. Supply the returned `next_after` to `--after` to scan the next page. Start again without a cursor to recheck previously blocked messages. The message ID, original delivery IDs and idempotency digest remain available. `webhook-server:inspect --message` shows `body_pruned_at` after cleanup.

### Hold outgoing records {#retention-hold}

Run `./artisan webhook-server:hold <id> --scope=<scope>` to keep a delivery snapshot and its attempts. Add `--message` to hold the shared body by publication ID. Use `--release` to remove the hold. A publication hold applies only to the shared body. A delivery hold also keeps a shared body while that delivery's snapshot still refers to it. Releasing a hold leaves the ordinary age requirements in effect.

`retention_hold_at` appears in diagnostics when set. A hold protects the data that still exists. Apply the migrations and upgrade all pruning processes before placing holds. Authorize the scope in the application before calling `DeliveryStore::set_retention_hold`, and give the Artisan command only to trusted operators.

### Expire idempotency keys {#idempotency-expiry}

Use `./artisan webhook-server:prune --scope=<scope> --days=30 --idempotency=delivery --pretend` to preview direct delivery key expiry. Use `--idempotency=publication` for publication keys. The period starts when the request snapshot or shared body was pruned. A hold prevents expiry. Remove `--pretend` to release the selected keys. Run migrations and upgrade all writers and pruners before enabling expiry.

Reusing a released key creates a new record with a new ID. Old queue tasks still address the old delivery. Existing delivery and message history keeps its original IDs and exposes `idempotency_expired_at` in diagnostics. For direct submissions, choose a new application message ID if the receiver should treat the next delivery as a new event.

### Prune historical metadata {#history-retention}

Use `./artisan webhook-server:prune --scope=<scope> --days=90 --history=delivery --pretend` to preview historical delivery deletion. Select `--history=publication` for publication receipts. The period starts when the idempotency key expired. Keep using `next_after` with `--after` to scan later pages. Restart without a cursor on a subsequent pass to revisit blocked records.

Delivery metadata is eligible when attempts have been cleaned, no replay audit refers to it and no shared publication body still depends on it. Publication receipt deletion requires prior body cleanup and key expiry. Holds block both operations. Removing historical metadata makes that ID unavailable in diagnostics.

### Prune replay audit records {#replay-retention}

Use `./artisan webhook-server:prune --scope=<scope> --days=90 --replay-audit --pretend` to preview audit cleanup, then remove `--pretend` when ready. Both request snapshots must have been pruned, both idempotency keys must have passed the requested retention period, and neither delivery may have a hold. A run handles at most 500 audit records.

This removes the replay relationship and its operator metadata. The original and replay delivery histories remain until their own history cleanup runs. An original whose payload has been pruned remains ineligible for new replays.

### Publish to subscriptions {#subscriptions}

Subscribe a managed endpoint to an exact event name and schema version using `DeliveryStore::subscribe`. Use `unsubscribe` to remove it for future publications. Publish supplies a JSON payload and produces an envelope with `type`, `schema_version` and `data`. The original publication stores the subscriber set, message body and delivery records in one transaction. `publish_in_transaction` can include the publication in a business transaction on the same database.

Publications accept up to 32 matching endpoints and a 1 MiB envelope. A repeat with the same scoped key and input returns the original receipt, including an empty delivery set. A changed input for the same key returns a conflict. Each delivery and replay uses the original shared body and message ID. The scheduler can recover publication delivery after commit.

## Custom signers and retry timing {#customization}

Implement `Signer` in an external crate. `SigningContext` supplies immutable body bytes, URL, runtime credential, stable message ID and per-attempt timestamp; this context is the Rust adaptation of the upstream signature arguments. Its secret is borrowed, and the context is not serializable or Debug-printable. The `headers` method lets protocols provide their additional authenticated headers. Do not add secrets to request metadata, logs or ordinary queue payloads.

`BackoffStrategy::wait_in_seconds_after_attempt` returns seconds. The default `ExponentialBackoffStrategy` follows the fixed baseline: attempts 1–4 wait 10, 100, 1000 and 10000 seconds; attempts above 4 wait 100000 seconds. Zero produces 1 second for the low-level contract; a first real attempt is numbered 1.


## Scope {#scope}

Use the existing [HTTP client](../digging-deeper/http-client.md) and [queues](../digging-deeper/queues.md) for networking and jobs. Pending deliveries may need another queue wake-up or a bounded scan after their next attempt time.

### Single host and replicas {#deployment}

Use one writable PostgreSQL or MySQL database for delivery state and limits, and point every Worker at the same database or Redis queue. Keep credential references and authorized revisions available on every Worker. A recovery schedule using `on_one_server()` needs a shared database or Redis cache. SQLite is a single-host option. The database and Redis combinations were exercised with independent application processes on one host. Validate your infrastructure before deploying across hosts.
