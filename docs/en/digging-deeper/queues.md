# Queues

## Generating job classes {#generating-job-classes}

Artisan creates job modules under `app/jobs`. A queued job carries serializable data and registers a handler for the worker. A `--sync` job executes directly when its `dispatch` future is awaited. Use `--test` for a matching feature test and `--force` to overwrite existing files.

```sh
./artisan make:job SendReport --test
./artisan make:job LocalReport --sync
```

Declare the generated modules in the application. Call `register_jobs` from a service provider's `boot` method. The example runs the local report before dispatching the queued report.

```rust
use crate::app::jobs::{local_report::LocalReport, send_report::SendReport};
use openrust::queue::{DispatchResult, QueueResult};

fn register_jobs() -> QueueResult<()> {
    SendReport::register()
}

async fn dispatch_reports() -> QueueResult<DispatchResult> {
    LocalReport.dispatch().await?;
    SendReport.dispatch().await
}
```

Generate a batchable job with `--batched`. If you also pass `--sync`, Artisan selects the batchable queue template.

```sh
./artisan make:job BatchReport --batched --test
```

Declare the module and call `BatchReport::register()` in your provider’s `boot` method. The generated handler skips cancelled batches and receives the optional batch ID in `handle`.

```rust
use crate::app::jobs::batch_report::BatchReport;
use openrust::{bus::Batch, queue::QueueResult, support::facades::Bus};

async fn dispatch_batch() -> QueueResult<Batch> {
    Bus::batch([BatchReport.payload()?, BatchReport.payload()?])
        .name("monthly reports")
        .onQueue("reports")
        .dispatch()
        .await
}
```

## Creating and dispatching jobs

Register a stable job name with `QueueManager::register` or `Queue::register` during application boot. A `JobHandler` reads the JSON data from `Payload` and receives the connection, queue and attempt in `JobContext`. Use `Queue::dispatch` to send the payload to a configured connection.

```rust
use openrust::{
    queue::{DispatchOptions, DispatchResult, Payload, QueueResult},
    support::facades::Queue,
};

async fn dispatch_email(user_id: u64, connection: &str) -> QueueResult<DispatchResult> {
    let payload = Payload::new("emails.send", serde_json::json!({ "user_id": user_id }))?;
    let options = DispatchOptions {
        connection: Some(connection.into()),
        ..Default::default()
    };
    Queue::dispatch(payload, options, None).await
}
```

## Connections and reservations

The skeleton uses `QUEUE_CONNECTION=database` by default. Run `./artisan migrate` to create `jobs`. `sync` executes immediately and returns the handler's result. `database` and `redis` enqueue jobs. A connection can override its default queue and `retry_after`. Use `Delay::For` for a duration or `Delay::Until` for an absolute time.

A stored connection exposes `pop`, `delete` and `release` through the queue and job contracts. An expired reservation can be claimed again. Its previous handle cannot confirm or release the new reservation. Redis `block_for` is measured in seconds. `null` returns immediately, and `0` waits for a job.

## Dispatching with database transactions

Pass the active transaction as the last argument of `Queue::dispatch`. A connection with `after_commit` enabled dispatches only after the outer commit. Set `DispatchOptions.after_commit` to override the connection setting. Rolling back discards deferred dispatch. With immediate dispatch, a database queue on the same connection writes within the transaction, and a sync handler can access it via `JobContext::transaction`.

## Running the queue worker

Use `./artisan queue:work database --stop-when-empty` to process pending jobs and exit. `--once` attempts one job. Use `--tries`, `--backoff` and `--timeout` to configure the worker. A job can override these values in `JobOptions`. On SIGTERM, the worker stops taking new jobs and finishes the current attempt.

## Job chaining {#job-chaining}

Pass registered job payloads to `Bus::chain`. Call `dispatch().await` on the builder to dispatch the first job. On the database and Redis drivers, the worker acknowledges each successful job and enqueues the next one in a single storage operation. A retry or release pauses the chain at the current job. A permanent failure stops it. The sync driver runs the chain inline.

```rust
use openrust::{
    bus::PendingChain,
    queue::Payload,
    support::facades::Bus,
};

fn report_chain(prepare: Payload, generate: Payload, publish: Payload) -> PendingChain {
    Bus::chain([prepare, generate, publish]).onQueue("reports")
}
```

Use `onConnection` and `onQueue` to select the destination for the chain. `delay` applies to the first job. Use `prepend` and `append` on the builder to change the sequence before dispatch.

Register a failure callback by name in your provider’s `boot` method with `Bus::registerChainCallback`. Attach it with `catch("reports.failed")`. The worker resolves the callback in its application and passes the failed payload and queue error. Make job side effects idempotent because the worker can execute a job again after losing its reservation. Chain callbacks have no separate retry record.

## Job batching {#job-batching}

Run `./artisan migrate` to create the batch ledger and delivery history. Register the job handler during application boot, then dispatch the jobs together with `Bus::batch`.

```rust
use openrust::{
    bus::Batch,
    queue::{Payload, QueueResult},
    support::facades::Bus,
};

async fn dispatch_reports() -> QueueResult<Batch> {
    Bus::batch([
        Payload::new("reports.generate", "summary")?,
        Payload::new("reports.generate", "detail")?,
    ])
    .name("monthly reports")
    .onQueue("batch")
    .dispatch()
    .await
}
```

Run `./artisan queue:work database --queue=batch --stop-when-empty` to process the batch. Read its current state with `Bus::findBatch(&id)` and cancel it explicitly with `Bus::cancelBatch(&id)`. By default, a terminal job failure cancels the batch. Add `allowFailures()` to let the batch continue without cancellation, and attach `SkipIfBatchCancelled` to skip a job after cancellation. A failed job stays pending until it succeeds on retry.

A database queue and its batch repository must use the same SQL connection. Their batch and queue writes commit together. To publish a batch to Redis, select `onConnection("redis")` before `dispatch()`.

### Adding jobs to batches {#adding-jobs-to-batches}

Call `Batch::add` from a handler or another application entry point. It uses the batch’s queue and connection options and returns the updated batch if it still exists. Appending jobs increases the total and pending counts and clears the completion timestamp. A cancelled batch retains its cancellation state. Each appended job receives a new queue UUID.

```rust
use openrust::{
    bus::Batch,
    queue::{Payload, QueueResult},
    support::facades::Bus,
};

async fn append_report(id: &str, report: &str) -> QueueResult<Option<Batch>> {
    let Some(batch) = Bus::findBatch(id).await? else {
        return Ok(None);
    };
    batch.add([Payload::new("reports.generate", report)?]).await
}
```

For Redis, each addition has a separate SQL publication record. The initial publication is acknowledged before additions can be published. The recovery command also retries pending additions. If you customize the batch table, include the companion table with the `_additions` suffix.

### Publishing batches to Redis {#redis-batches}

OpenRust commits the batch and the Redis publication intent in SQL before contacting Redis. If publication fails, `dispatch()` still returns the accepted batch. Run `queue:publish-batches` periodically to retry pending intents. Redis writes the jobs and a publication marker together. The marker stays in Redis until SQL acknowledges the publication, then OpenRust attempts to set a 24-hour expiry. Configure Redis persistence and eviction for queue data and markers. If you customize the batch table, create its companion tables with the `_jobs` and `_outbox` suffixes.

```sh
./artisan queue:publish-batches --limit=1
```

### Batch callbacks {#batch-callbacks}

Register each callback by a stable name during provider boot in both the application and worker. The batch stores these names so a separate worker can resolve them. The callback receives a batch snapshot and, where applicable, the terminal failure message.

```rust
use openrust::{
    bus::Batch,
    log::Log,
    queue::{Payload, QueueError, QueueFuture, QueueResult},
    support::facades::Bus,
};

fn report_complete<'a>(batch: &'a Batch, _: Option<&'a str>) -> QueueFuture<'a, QueueResult<()>> {
    Box::pin(async move {
        let context = serde_json::Map::from_iter([("batch_id".into(), batch.id.clone().into())]);
        Log::info("Reports batch completed", &context)
            .map_err(|source| QueueError::Execution { source: Box::new(source) })
    })
}

fn register_batch_callbacks() -> QueueResult<()> {
    Bus::registerBatchCallback("reports.completed", report_complete)
}

async fn dispatch_with_callbacks() -> QueueResult<Batch> {
    Bus::batch([Payload::new("reports.generate", "documentation")?])
        .name("report export")
        .onQueue("reports")
        .then("reports.completed")
        .dispatch()
        .await
}
```

Use `before` for the initial snapshot before enqueueing, `progress` for terminal progress, `then` when the pending count reaches zero, `catch` for the first terminal failure, and `finally` after all jobs reach a terminal outcome. `onFailure` allows failures and registers a callback for each terminal failure. A callback error is logged and the worker continues. Worker callbacks run after their SQL transition commits. A process crash between commit and invocation can lose that invocation.

### Batch events {#batch-events}

Register listeners for `BatchDispatched`, `BatchStarted`, `BatchFinished`, and `BatchCanceled` from `openrust::bus::events`. Each carries a batch snapshot. Cancellation caused by a job failure also carries the failure message. Call the registration function in your provider’s `boot` method.

```rust
use std::sync::Arc;
use openrust::{
    bus::events::BatchFinished,
    contracts::events::{Event as Payload, Listener},
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    log::Log,
    support::facades::Event,
};

struct ReportBatchFinished;

impl Listener for ReportBatchFinished {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Payload,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if let Some(event) = event.downcast_ref::<BatchFinished>() {
                let context = serde_json::Map::from_iter([
                    ("batch_id".into(), event.batch.id.clone().into()),
                ]);
                let _ = Log::info("Batch finished", &context);
            }
            Ok(None)
        })
    }
}

fn register_batch_events() -> EventResult<()> {
    Event::listen(BatchFinished::NAME, Arc::new(ReportBatchFinished), Delivery::Sync)
}
```

Events follow the committed state change. `BatchStarted` follows the first terminal job outcome, and `BatchFinished` fires when no jobs remain pending. Replaying the same delivery leaves these events unchanged. Different workers can run listeners concurrently.

### Retrying failed batch jobs {#batch-failures}

Pass a batch ID or several IDs to requeue the failures still available in failed-job storage. Jobs return to their original connection and queue. On the same SQL connection, requeueing and removal of the failed record commit together. The Worker clears a job’s pending failure ID after it succeeds. Jobs configured to skip cancelled batches still honour cancellation.

```sh
./artisan queue:retry-batch "$BATCH_ID"
```

With a shared cache store, add `--isolated` to skip a concurrent invocation with the same batch ID list. Use `--isolated=12` to return exit code 12 if the lock is held.

### Pruning batches {#pruning-batches}

Run the command periodically to delete batches that finished more than 24 hours ago. Pass `--hours` to change the finished retention period. To also prune unfinished or cancelled batches, supply `--unfinished` or `--cancelled` with their retention in hours. Both optional rules measure age from batch creation. The command reports the number deleted by each rule. Delivery records for deleted batches are removed with the batch.

```sh
./artisan queue:prune-batches
./artisan queue:prune-batches --hours=48 --unfinished=72 --cancelled=36
```

## Job middleware {#job-middleware}

Return middleware from `JobHandler::middleware`. Call `JobNext::run` to continue, or call `JobContext::release` to schedule another attempt. `WithoutOverlapping` uses the configured cache lock and can release an overlapping job with a delay. An unfinished lock expires at its configured lease time.

Generate a middleware under `app/jobs/middleware`. Its handler forwards the payload and context to the next step.

```sh
./artisan make:job-middleware GuardReport --test
```

Declare the middleware module and add this method to the generated job's `impl JobHandler` to apply it.

```rust
fn middleware(&self) -> Vec<std::sync::Arc<dyn openrust::queue::middleware::JobMiddleware>> {
    vec![std::sync::Arc::new(crate::app::jobs::middleware::guard_report::GuardReport)]
}
```

## Failed jobs

Run `./artisan migrate` to create `failed_jobs`. Use `./artisan queue:failed --json` to list failures, `./artisan queue:retry <uuid>` to requeue a failed job on its original connection and queue, or `queue:forget` and `queue:flush` to remove failure records. A timeout drops the pending Rust future. Job code should stop any work it starts outside that future.

## Queued listeners

Use `ListenerRegistry::listenQueued::<YourEvent>` with a stable listener ID and a serializable event. The worker resolves the registered listener from its application. The listener's `after_commit` option can defer enqueueing until the transaction commits.
