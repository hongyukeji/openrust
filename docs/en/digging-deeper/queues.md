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

Batch dispatch currently supports a database queue using the same connection as the batch repository. Batch creation and all enqueues commit in one transaction. If you configure a different batch table, also provision its companion table with the `_jobs` suffix. Redis dispatch, callbacks, and pruning are pending.

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
