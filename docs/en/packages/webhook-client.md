# Webhook Client (Extension)

## Support status {#support-status}

The optional `openrust-webhook-client` package supplies named configuration, typed implementation factories, explicit callback routing, `WebhookController`, a manual `WebhookProcessor` and scoped database receipt storage. Applications provide the job dispatcher. The database receiver supports opt-in deduplication by verified event ID. Optional recovery scanning and private Storage disk references are available. A saved receipt and successful dispatch do not prove business completion. No public callback route is installed automatically.

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

pub fn register_routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust::support::facades::Route;
    use openrust_webhook_client::{WebhookRouteOptions, WebhookRoutes};
    Route::webhooks("/callbacks/payments", WebhookRouteOptions::new().name("payments"))
}

pub fn register_dispatcher(
    container: &openrust::container::Container,
    dispatcher: std::sync::Arc<dyn openrust_webhook_client::WebhookJobDispatcher>,
) -> openrust_webhook_client::Result<()> {
    openrust_webhook_client::WebhookRuntime::register_job_dispatcher(
        container, "payments", move |_, _| Ok(dispatcher.clone()),
    )
}

pub async fn process_named(
    app: std::sync::Arc<Application>,
    config: std::sync::Arc<openrust_webhook_client::WebhookConfig>,
    request: &WebhookRequest,
) -> openrust_webhook_client::Result<openrust::http::Response> {
    use openrust_webhook_client::{WebhookProcessor, WebhookRuntime};
    let owner = app.clone();
    app.call(openrust::container::Callback::new(&[], move |_| async move {
        let runtime = WebhookRuntime::from_application(owner, config.clone()).await;
        Ok(match runtime {
            Ok(runtime) => WebhookProcessor::new(request, &config, &runtime).process().await,
            Err(error) => Err(error),
        })
    }), Default::default()).await?
}
```

`StandardSignatureValidator` uses the official `standardwebhooks 1.3.0` library with `webhook-id`, `webhook-timestamp` and `webhook-signature`. Supply a valid base64 or `whsec_` secret. Its five-minute timestamp window and multiple signatures in one header are supported; duplicate protocol headers are rejected. The library requires UTF-8 payloads. To suppress repeated deliveries of the same verified event, enable database event deduplication.

### Register callback routes and implementations {#routes}

Import `WebhookRoutes` to call `Route::webhooks(uri, options)` or `Route::webhooks_with(uri, options, configure)`. `WebhookRouteOptions::default()` uses the `default` configuration and `post`. Its `name` and `methods` builders select a different profile or method list. Registration accepts the exact lowercase values `get`, `query`, `post`, `put`, `patch`, `delete`; `HEAD` follows native GET routing, but `head` is not a registration option. Uppercase options and unsupported methods fail registration. Native OPTIONS negotiation and 405/Allow behavior remain in effect.

Place routes in the application’s existing API or web route registration. The underlying `Router` also has a `webhooks` extension when `WebhookRouter` is imported; call it within application route registration. Groups retain prefixes, names and middleware. `webhooks_with` allows native constraints, defaults and route middleware customization. The package does not disable CSRF. If a supplier uses a web route, explicitly exclude only that callback path through the application’s request-forgery configuration; unrelated web routes remain protected.

Configuration is selected from the router’s actual matched route name, not a custom Request route resolver or an account supplied in the URL/body. By default the name is `webhook-client-{name}`. `add_unique_token_to_route_name=true` appends a random eight-character suffix; this is naming, not authentication. Matching the fixed source, unique-token mode uses the first dot-separated name before that suffix; use names without dots in that mode.

Register custom implementations in the application Provider using `WebhookRuntime::register_signature_validator`, `register_webhook_profile`, `register_webhook_response`, `register_webhook_store` or `register_job_dispatcher`. Each factory receives the immutable `WebhookConfig` and the application’s `Container` and returns an `Arc` of the appropriate trait. Await `WebhookRuntime::from_application`; construction and processing use that application’s native context and restore the caller’s context afterward. The HTTP controller keeps request-dependent factories and processing inside one request lease; request handles must not escape that lease. Names are case-sensitive ASCII identifiers of at most 128 bytes, using letters, digits, `-`, `_`, `.`, `:`. A missing or wrongly typed binding fails; another application/account is never used as a fallback.

The corresponding profile fields are `signature_validator`, `webhook_profile`, `webhook_response`, `webhook_model`, `process_webhook_job`. Built-ins are validator `default`/`standard`, profile `process-everything`, response `default`, store `database`. The job dispatcher must be registered explicitly. `process_webhook_job` currently selects that dispatcher; native queued processing is available through `QueuedWebhookJob`. A database profile needs its trusted stable `scope` and optional named `connection` (`null` uses the application default). The published template leaves `scope` null and the job name empty so an unconfigured receiver fails instead of acknowledging a delivery. Select `signature_validator`, `webhook_profile`, `webhook_model` and `process_webhook_job` explicitly; missing required fields are rejected. Only `webhook_response` may be omitted or null to select the default.

For example, a profile named `payments` may use `scope="billing/site-a"`, `signing_secret={"env":"PAYMENT_WEBHOOK_SECRET"}`, the built-in default validator/profile/response and database store, and `process_webhook_job="payments"`. Put those fields in the published profile configuration and register the `payments` dispatcher in your Provider. For manual consumers, `WebhookConfig` also provides `with_signature_validator`, `with_webhook_profile`, `with_webhook_response`, `with_webhook_model`, `with_process_webhook_job`, `with_scope` and `with_connection` builders.

Validators, profiles and responses are constructed before processing. Stores are resolved only after validation and profile acceptance; a valid ignored event does not open storage. The job factory is checked for registration up front but constructs the dispatcher after saving and before clearing the old exception. Missing configuration and invalid signatures are ordinary errors: the default controller returns 500, matching the fixed package source, unless your [exception handling](../the-basics/errors.md) maps them differently. Body-limit failures remain 413. Earlier examples that call a validator directly can choose their own response policy.

The controller uses native `Request::body_bytes_with_limit`: raw bytes are retained, including when middleware has already parsed JSON/form input, and cached bytes must still satisfy the endpoint limit. This method cannot increase the native 8 MiB raw-reader ceiling. An earlier middleware read may already have consumed data up to its own limit; the smaller endpoint limit does not retroactively reduce that memory use.

## Custom implementations and snapshots {#customization}

Implement `SignatureValidator`, `WebhookProfile`, `RespondsToWebhook`, `WebhookStore` or `WebhookJobDispatcher` in another crate. `ProcessEverythingWebhookProfile` selects every request; `DefaultRespondsTo` produces `{"message":"ok"}`. `WebhookRuntime::new` accepts explicit dependencies. `WebhookRuntime::from_application` resolves the selected names through the current application’s typed factories; it does not evaluate class-name strings or use a global registry.

`WebhookConfigRepository::add_config` replaces a named configuration. Owned `Arc<WebhookConfig>` values retain their original revision and key snapshot. Unknown names return `Ok(None)`, and resolution errors remain errors. The provider binds the repository to the current HTTP, Artisan or Worker lifecycle; runtime additions do not leak into the next lifecycle. `WebhookConfig::new` accepts a runtime-injected secret, keeps it private and redacts Debug; do not serialize that secret into jobs or logs. `with_verification_secrets` adds at most seven previous keys for a total of eight in one profile.

Configuration caching keeps credential references, so `config:show webhook-client` does not expose resolved keys from package defaults. Each process reads its own immutable environment snapshot when resolving a profile; restart long-running processes for credential changes. Retaining historical credentials across process restarts and queued work is not yet implemented. The `revision` label is not a substitute for a durable credential revision store.

### Store and dispatch a receipt {#receipts}

The database store saves parsed input after signature verification and profile acceptance. Use `call.payload(&store).await?` for JSON, form fields and query parameters. Body fields take precedence over query fields. Reads of pre-upgrade records return `None` for parsed input. Use `call.get_attachments(&store).await?` for native, keyed upload groups reconstructed from the saved original bytes. No server upload path is persisted. `store_attachments=false` disables the attachment view. The original multipart body still contains file bytes.

The provider registers the `webhook_calls` migration. Run `./artisan migrate` before using `DatabaseWebhookStore`. Supply a stable application/tenant scope of at most 128 bytes, shared by every node for that same scope. Reads and exception updates are scoped; a digest key prevents database collation from mixing differently cased scopes. This is storage isolation, not caller authorization: derive the scope from trusted application context.

`WebhookProcessor::process` validates, applies the profile, stores the receipt, clears the previous exception, dispatches the job, then creates the response. Invalid signatures dispatch the typed `InvalidWebhookSignatureEvent` before returning an error; that event gives trusted application listeners access to the request and configuration and has no automatic serializable diagnostic properties. Validator errors remain errors. Ignored valid requests only receive the configured response. Failed storage or dispatch does not return a success response. Dispatch errors leave a bounded, generic exception record, without arbitrary third-party error text or stack traces.

`models::WebhookCall` delegates `store_webhook`, `find`, `save_exception` and `clear_exception` to the store. It retains the exact raw body, request target supplied by `with_uri`, method, revision and selected headers, including duplicate and opaque header values. A stored record is not a verification token. `VerifiedWebhook` is created only by the processor after validation and profile acceptance. The database representation uses base64 for raw bytes; it is not encryption. Apply database access and retention controls.

No headers are stored by default. `with_store_headers(&["X-Source"])` selects names case-insensitively; `with_store_headers(&["*"])` explicitly stores all headers, including potentially sensitive values. The config-file equivalent is the string `"*"`, not an array containing `"*"`. Exception JSON is limited to 8192 bytes, request targets to 8192 bytes and stored methods to 32 bytes. Applications must redact manually supplied exception details.

#### Keep multipart attachments on a private Storage disk {#attachment-disk}

Set `attachment_disk` to a configured private disk name and run `./artisan migrate`. A manually constructed profile can use `with_attachment_disk("local")`. With attachment storage enabled, an accepted multipart request containing files is written as one private object. The receipt stores the disk, length and SHA-256 digest. The object name derives from the trusted scope and receipt ID. Workers verify length and digest when reading the original body and recreating native uploads. All workers must be able to read the same disk under the saved name.

Established inline receipts and requests with `store_attachments: false` continue using database storage. On a confirmed database rollback, the receiver attempts to delete the staged object. A crash before commit or an uncertain storage failure can leave a staging record for cleanup. Receipt pruning removes an expired receipt and records its detached object for cleanup in the same database transaction.

Schedule `./artisan webhook-client:prune --scope=payments --attachment-staging` to delete detached objects and staging objects that have remained for at least one hour. Use `--pretend` to count candidates. A failed deletion leaves its staging record for retry after the lease expires and makes the command exit with an error. Keep the saved disk name mapped to the same private storage on receivers, workers and cleanup processes. Attachment cleanup also runs when receipt retention is disabled.

Stop Webhook writers before rolling back the attachment migration. The rollback refuses to remove live references or staging records.

### Process receipts in a Worker {#processing-jobs}

Implement `jobs::ProcessWebhookJob` to process a saved receipt. In an application Provider shared by HTTP and Workers, call `QueuedWebhookJob::register` with the native `QueueManager`, a stable name, a store for the trusted scope and your handler. Register a `QueuedWebhookJob` dispatcher under the name selected by `process_webhook_job`. Use distinct registrations for distinct storage scopes. The Worker checks the receipt ID, scope, profile and revision against the stored record before calling your handler.

Set connection and queue on the dispatcher. The native `JobOptions` controls attempts, backoff and timeout. Assign business policy in your handler. The queued data contains the receipt identifiers. Workers open the saved record without using the receiver’s verification secret. Failed handler attempts save a generic receipt exception, and a subsequent attempt clears the previous diagnostic. Choose a Worker-backed connection. The dispatcher rejects built-in sync, null, deferred and background connections and requires the queue to return an enqueue result.

### Commit the receipt with its database queue task {#atomic-database-queue}

Set `atomic_database_queue: true` on a profile, or use `with_atomic_database_queue(true)` for a runtime configuration. Use `DatabaseWebhookStore` and `QueuedWebhookJob` with the same application database connection. The receipt and native queue task commit together before the HTTP acknowledgment. Queue insertion failure rolls both back. Other drivers and distinct connection instances are rejected, even when they refer to the same database. The default remains off. This option closes the pre-enqueue process exit window for that deployment. Use verified event deduplication when the signing protocol supplies an authenticated event ID.

### Recover after committing the receipt {#dispatch-recovery}

Set `recover_dispatch: true` on the profile or use `with_dispatch_recovery(true)` in Rust. Run `./artisan migrate`. This mode commits the verified receipt and a dispatch intent together, then returns the configured HTTP acknowledgment. Leave `atomic_database_queue` off for this profile.

Register a leased handler with `QueuedWebhookJob::register_with_database_recovery` on the receiver, recovery process and Workers. Use the same handler name, receipt database and scope. Set an explicit durable connection and queue on the dispatcher with `on_connection` and `on_queue`. Set a positive job timeout at least two seconds shorter than the Worker lease. Built-in inline and failover connections are rejected. A durable direct connection such as Redis can run separately from the receipt database.

Schedule the bounded command for each receipt scope. Keep the stored queue connection name mapped to the same broker. Use `--database` to select a named receipt connection.

```sh
./artisan webhook-client:recover --scope=billing/site-a --limit=100 --pretend
./artisan webhook-client:recover --scope=billing/site-a --limit=100
```

Failed dispatches keep their intent and return a failing exit status. A subsequent run may claim the intent after 60 seconds. If the queue accepts a task and the recovery process exits before clearing the intent, the next run may enqueue it again. Workers share the same receipt claim and completion state. Make business side effects idempotent to handle interruptions during processing.

#### Recover an overdue receipt after enqueue {#processing-recovery}

For a profile with `recover_dispatch: true`, set `"recover_processing": {"after_seconds": 300, "max_requeues": 2}` or call `with_processing_recovery(300, 2)?`. Run `./artisan migrate`. The delay accepts 60–86,400 seconds and the budget accepts 1–10 requeues. Both are captured at receipt time. Choose a delay that allows for normal queue backlog, retries and backoff.

The same scheduled command requeues an overdue unfinished receipt using its saved queue route. It skips active processing leases and future processing times. Each requeue claim spends one unit of budget, including a claim interrupted before queue confirmation. Budget exhaustion stops automatic requeues and leaves any already queued task eligible to run. Once the receipt is complete or stopped, the next recovery run removes the dispatch tracking row. `--pretend` only previews eligible dispatches. Processing can still repeat after a crash during a side effect, so handlers need business idempotency.

### Claim a receipt across Workers {#processing-lease}

Run `./artisan migrate` and register the handler with `QueuedWebhookJob::register_with_database_lease(&queue, name, store, handler, lease_seconds)`. The store must be an `Arc<DatabaseWebhookStore>`. Deploy the same registration to every Worker serving that queue during a coordinated restart. For example, use a 20-second job timeout, a 30-second lease and a database queue `retry_after` of 60 seconds. The lease accepts 3–86,400 seconds and requires a positive job timeout at least two seconds shorter than the lease. Allow additional time for database work and configure the queue’s reservation or visibility timeout to exceed the lease. A competing job is released through the native queue and can use an attempt from its configured budget.

An active lease permits one Worker to enter the handler. A completed receipt is acknowledged on redelivery without another handler call. A handler can explicitly release its job for another attempt. After process exit, a reserved task can be reclaimed when both the queue reservation and the receipt lease expire. Make external effects idempotent using the receipt ID because a process can exit after an effect has occurred and before marking the receipt complete.

The database records each lease attempt by receipt and generation. A completed, released, deferred, failed or stopped attempt records its outcome with the corresponding receipt change. If another Worker reclaims an expired lease after a crash, the previous attempt is marked expired. Attempt records contain timestamps and status and are retained with the receipt.

### Deduplicate a verified event {#verified-event-deduplication}

For a profile with `atomic_database_queue: true` or `recover_dispatch: true`, enable `deduplicate_verified_events: true` and run `./artisan migrate`. At runtime, use `with_verified_event_deduplication(true)` together with the corresponding dispatch option. The `standard` validator supplies the signed `webhook-id` and requires it to contain no periods in this mode. A custom validator can supply a stable ID through `SignatureValidator::verified_event_id` after verifying that the signature covers that ID. The default body-only HMAC validator supplies no event ID. The receiver rejects the deduplication mode if its validator cannot supply a valid ID. Configure a distinct scope or profile for each source account.

The database receives the event, stores its deduplication key and enqueues its job in the same transaction. The same ID and request content return the configured acknowledgment without a second receipt or job. Reusing an ID within the same scope and profile with a changed method, raw path/query, body or Content-Type returns HTTP 409. Signing timestamps, signatures and credential revisions may change between retries. A key is retained with its receipt, so retain receipts at least as long as the provider can redeliver. The mode is off by default.

### Deduplicate by request content when no stable ID is available {#body-deduplication}

Set `deduplicate_body_seconds` to 1–3,600 on a profile with `atomic_database_queue: true` or `recover_dispatch: true`, or use `with_body_deduplication(seconds)` with the corresponding dispatch option. Choose either this window or verified event ID deduplication for each profile. The receiver applies the window after signature verification and profile acceptance.

The window starts with the first accepted request, using the database clock. The fingerprint covers the HTTP method, raw path and query, original body and Content-Type. Identical requests within the window share one receipt and task. The next request after expiry can create a new receipt while earlier receipts remain available. Enable this mode only when identical legitimate requests in that window should be treated as retries.

### Defer processing until a record can be matched {#deferred-processing}

For jobs that can receive a notification before the matching business record exists, register the Worker with `QueuedWebhookJob::register_with_database_deferred_processing`. Use a native database queue on the same database connection as the receipt store. Set `DeferredProcessingPolicy { max_deferrals, max_age_seconds }` and keep the same lease, timeout and queue reservation settings described above. The policy accepts 1–1,000 deferrals and a maximum age of 2 seconds to 30 days, measured from the first deferral.

Override `ProcessWebhookJob::handle_outcome` to return `WebhookProcessingOutcome::Deferred { delay_seconds }` when the record is still missing. Use a positive delay that fits within the remaining budget. Return `Completed` after business processing, `RetryableFailure` to apply native retry settings, or `PermanentFailure` to stop. A deferral records the next processing time and enqueues a new job in one transaction. The new job has its own queue attempt budget. The receipt remains pending until a Worker completes it or the deferral budget expires. Do not call `JobContext::release` when returning `Deferred`.

When the application associates the business record, it can call `QueuedWebhookJob::wake_deferred(&store, queue, receipt_id)` to make a pending receipt eligible immediately. Pass the store for the trusted scope and the application’s `Arc<QueueManager>`. The method uses the destination and handler saved with the deferral. It returns `true` when it schedules an early wake-up, or `false` when the receipt is already due, settled, or outside that scope.

With `recover_dispatch: true` and a separate queue backend, use `QueuedWebhookJob::register_with_database_recovery_and_deferred_processing`. The receipt transaction records the deferral and its next dispatch intent. The scheduled `webhook-client:recover` command dispatches it when due. An early wake-up updates that intent even when the broker is offline. The command retains enough state to stop expired deferrals even if the broker loses an acknowledged wake-up task and lost-task requeues are disabled. Reaching a deferral time or waking it early creates a new queue task with its own attempts and leaves the receipt’s deferral and optional lost-task requeue budgets in place.

### Inspect and replay exhausted receipts {#replay-exhausted}

Run `./artisan migrate` and upgrade all Webhook workers before enabling replay. `webhook-client:exhausted` lists receipt identifiers and metadata in a trusted scope, with a limit of 1–500. It selects acknowledged durable dispatches whose automatic requeue budgets and waiting windows have elapsed, and whose receipts have no active lease, scheduled deferral or terminal state.

Register your application’s `ReplayPolicy` in the Provider used by the command, dispatcher and workers. The policy authenticates the operator from trusted application context, validates the receipt’s scope and current profile or credential revision, and grants limits of 1–10 replay generations and a 60–86,400 second lifetime. `--actor` and `--reason` are audited inputs to that policy.

```rust
use openrust_webhook_client::replay::{self, ReplayPolicy};

pub fn register_replay_policy(
    container: &openrust::container::Container,
    policy: std::sync::Arc<dyn ReplayPolicy>,
) -> openrust_webhook_client::Result<()> {
    replay::register_policy(container, policy)
}
```

```sh
./artisan webhook-client:exhausted --scope=billing/site-a --limit=20
./artisan webhook-client:replay <receipt-id> --scope=billing/site-a --actor=operator-id --reason="Receipt reviewed"
./artisan webhook-client:recover --scope=billing/site-a
```

A successful request records one new generation and makes its intent available to `webhook-client:recover`. That command and the worker check the application policy again. Earlier queued generations cannot claim the receipt. Replay keeps the original receipt ID, verification history and payload. Audit entries follow the receipt’s retention, and business side effects still require idempotency.

### Prune old receipts {#retention}

The package’s `delete_after_days` defaults to 30. Set it to `null` to disable retention cleanup. The application chooses when to run the Artisan command, for example from its scheduler. Use the trusted receipt scope and optionally select a named database connection with `--database`.

```sh
./artisan webhook-client:prune --scope=billing/site-a --pretend
./artisan webhook-client:prune --scope=billing/site-a --limit=100
```

The database store prunes receipts whose creation and terminal timestamps are both outside the retention window. Pending receipts, active leases and receipts without a terminal marker are retained. Receipt deletion also removes the associated event keys and processing attempts. Each command processes up to 500 records. `WebhookCall::prunable` previews candidate IDs through the storage adapter.

### Lookup hints from unsigned callbacks {#unverified-hints}

For a supplier that supplies no signature, explicitly set `unverified_hint_receiver` to an implementation registered with `WebhookRuntime::register_unverified_hint_receiver`. The receiver gets an `UnverifiedHint`. The default verifier requires its signature header to be absent for this path. The Standard verifier requires all three protocol headers to be absent. Signed requests follow the normal verification and response path. Custom verifiers with other signature fields can override `SignatureValidator::has_signature`. The built-in receiver uses the profile’s configured `scope`.

Use `QueuedWebhookHint` with a database queue on the same connection as the hint store. Supply a `WebhookHintPolicy` that resolves only tasks already registered for the trusted scope, and a separate `ProcessWebhookHint` that checks the task again before looking it up. Configure a shared cache limiter across application nodes. The built-in adapter charges 1–600 requests per minute per scope and profile as configured. Accepted hints and unrecognized tasks return the same HTTP 202 response. Hint rows contain the local task reference and expire for processing after five minutes. Workers claim each hint once and get a 15-second task timeout.

The lookup handler validates the supplier response in its own domain. The original notice stays an unverified hint. The hint transport carries the receipt ID and revision. The trusted receipt and processing queue continue to handle verified callbacks.

### Retain verification evidence {#verification-audit}

Set `verification_audit: true` on a profile with a trusted `scope` and run `./artisan migrate`. The database connection follows the profile’s `connection`. The audit records the validator and policy names, profile revision, UTC minute, and a classification with a fixed reason. The revision is the configured label for the credential set, so update it when the set changes.

The audit keeps one observation per scope, profile, revision, validator, policy and reason in each minute. It records the trust decision before business processing. A valid signature, an unsigned hint, or a rejection produces a distinct classification. The audit stores only these metadata fields. When audit writes fail, the enabled receiver returns an error rather than continuing to business processing.

```sh
./artisan webhook-client:prune --scope=billing/site-a --verification-audit --pretend
./artisan webhook-client:prune --scope=billing/site-a --verification-audit
```

This cleanup uses the package’s `delete_after_days` setting and applies only to verification evidence.

## Scope {#scope}

Use the existing [HTTP client](../digging-deeper/http-client.md) and [queues](../digging-deeper/queues.md) for application networking and jobs. Recovery covers dispatch of committed receipts. Backends and deployments require their own operational validation.

### Single host and replicas {#deployment}

Point every replica at the same writable PostgreSQL or MySQL database. Use the database queue for atomic receipt and enqueue, or configure durable dispatch recovery when jobs go to Redis. Keep the queue names, application key, and authorized profile revisions consistent across HTTP nodes and Workers. Use private storage accessible to all Workers for persisted attachments. The local acceptance runs used a shared directory and independent node directories on one host. Validate your shared storage and deployment before using replicas across hosts.
