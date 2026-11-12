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

### Queue workers and deployment {#queue-workers-and-deployment}

Run the restart command during deployment. A worker finishes its current job and exits successfully. A process supervisor can then start the updated worker. The command and workers need to share the application’s cache store. Use a database, Redis, or shared file cache. An array cache is local to each process.

```sh
./artisan queue:restart
```

### Worker status and exit reasons {#worker-status-and-exit-reasons}

A worker writes job progress and pause or resume notices while it runs. Use `--json` to emit one JSON record per line. Normal output is suppressed by `--quiet` or `--silent`.

```sh
./artisan queue:work --json --stop-when-empty
```

A graceful stop writes a `stopped` record with the reason, exit code, and number of processed jobs. The supported reasons are `empty`, `empty_for`, `max_jobs`, `max_time`, `interrupted`, and `restart_signal`. A one-shot worker outputs job records and skips the stop summary. Applications can listen for `WorkerStopping` to observe the graceful stop.

Timestamps use `queue.output_timezone` if configured, then `app.timezone` or UTC. Memory usage is currently reported as `null`.

#### Worker lifecycle events {#worker-lifecycle-events}

A daemon dispatches `WorkerStarting` when it starts, `Looping` before each polling iteration, and `WorkerIdle` when a poll finds no available job. It dispatches `WorkerStopping` when it exits gracefully. These events carry the connection, queue selection, and worker options.

A synchronous `Looping` listener can return `Some(ListenerResponse::Value(Box::new(false)))` to skip the current poll. The worker waits before trying again and continues to honor its stopping limits. A skipped poll does not dispatch `WorkerIdle`. The dispatcher uses the first listener response, so use `None` to allow later listeners to decide.

With an empty queue, this daemon dispatches the startup, loop, idle, and stopping events. `--once` skips these daemon events.

```sh
./artisan queue:work --json --stop-when-empty
```

#### Job reservation events {#job-reservation-events}

Listen for `JobPopping` to observe a worker poll. It includes the connection and requested queues. `JobPopped` fires after a successful reservation and before the worker runs the job. The event carries a read-only snapshot with the driver job ID, queue, attempt number, and payload.

A `JobPopping` listener error stops the command before it reserves a job. A `JobPopped` listener error is reported through the application exception handler. The worker leaves the reservation intact and the driver can retry the job after `retry_after` expires.

With an empty queue, this command dispatches `JobPopping` and exits. `JobPopped` requires an available job.

```sh
./artisan queue:work --json --once
```

#### Job attempt events {#job-attempt-events}

Listen for `JobProcessing` before the job runs and `JobProcessed` after it returns successfully. If the job explicitly releases itself, `JobReleased` follows `JobProcessed`. A failed attempt emits `JobExceptionOccurred`, followed by `JobReleasedAfterException` if the worker schedules a retry. A permanent failure emits `JobFailed` after recording the failure and removing the reservation.

`JobAttempted` follows the other events for the attempt. It exposes `successful()`, an optional exception message, and the elapsed milliseconds if execution began. Events carry a read-only job snapshot. A `JobProcessing` listener error follows the normal retry policy. The worker reports errors from the other listeners through the application's exception handler and preserves the queue operation's result.

`JobTimedOut` includes the configured timeout. OpenRust cancels the async job future and applies the worker retry policy, including `fail_on_timeout`.

With a job waiting on the default queue, run one attempt and print its status.

```sh
./artisan queue:work --json --once --tries=2
```

#### Resting between jobs and stopping after inactivity {#worker-rest-and-idle-timeout}

Use `--rest` to wait between job attempts. Use `--stop-when-empty-for` to stop after the queue is empty and the worker has gone the specified number of seconds without processing a job. The idle timer starts when the worker starts and resets when it finishes each job.

```sh
./artisan queue:work --json --rest=1 --stop-when-empty-for=2
```

This worker reports `empty_for` when the idle limit is met. A value of zero disables the idle limit. `--stop-when-empty` still exits as soon as no job is available. With an idle limit, a Redis blocking reservation waits at most for the remaining idle window.

### Reacting to worker signals {#reacting-to-worker-signals}

On Unix, send `SIGUSR2` to pause new polls in a single daemon and `SIGCONT` to resume them. The worker finishes a job it has already reserved. These signals emit `WorkerPausing` and `WorkerResuming` and leave shared queue pause settings untouched. Target the compiled worker process by PID.

```sh
kill -s USR2 "$WORKER_PID"
kill -s CONT "$WORKER_PID"
kill -s TERM "$WORKER_PID"
```

On Unix, `SIGTERM`, `SIGQUIT`, and `SIGINT` request a graceful stop. On Windows, Ctrl+C requests the same stop. The daemon emits `WorkerInterrupted` before `WorkerStopping` and lets its current job finish. The signal number, connection, queues, and worker options are available to listeners. A blocking driver reservation that has already begun can return before a pause takes effect.

### Pausing and resuming queue workers {#pausing-and-resuming-queue-workers}

Pause a queue by its connection and name. A bare queue name uses the default connection. A worker finishes its current job and skips the paused queue while continuing to serve any other eligible queues.

```sh
./artisan queue:pause database:default
./artisan queue:continue database:default
```

Pause or resume every queue with `--all`. Resuming all queues preserves individual pauses. `queue:continue` is an alias for `queue:resume`.

```sh
./artisan queue:pause --all
./artisan queue:resume --all
```

Workers check the shared cache between jobs and while idle. The `--sleep` option controls the idle polling interval. To disable restart and pause polling for an application, call `Queue::withoutInterruptionPolling()?` in a service provider’s `boot` method. Redis workers with `block_for` check again after the blocking pop returns. Use a finite block duration if idle workers need to pick up control signals promptly.

#### Pausing a queue from application code {#pausing-queues-in-code}

Call the Queue facade inside an application scope. Pass a duration with `Ttl::Seconds` or an absolute deadline with `Ttl::Until`. A worker can consume the queue again after the deadline or after an explicit `Queue::resume`.

```rust
use openrust::{cache::Ttl, queue::QueueResult, support::facades::Queue};

async fn pause_default_queue() -> QueueResult<()> {
    let connection = Queue::connection(None)?;
    let name = connection.get_connection_name();
    Queue::pauseFor(name, "default", Ttl::Seconds(3)).await?;
    assert!(Queue::isPaused(name, "default").await?);
    assert_eq!(Queue::getPausedQueues(name, &["low", "default"]).await?, ["default"]);
    Ok(())
}
```

`QueuePaused`, `QueueResumed`, `QueuesPaused`, and `QueuesResumed` describe changes made through the application or Artisan. `WorkerQueuePaused` and `WorkerQueueResumed` describe the state a running worker observes. For example, a timed pause emits `WorkerQueueResumed` when a worker next checks after expiry.

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

### Chain connection and queue {#chain-connection-queue}

Set chain defaults with `PendingChain::onConnection` and `onQueue`. Use the corresponding methods on a job payload to override its own destination. The next job still uses the chain defaults unless it has an override. A payload delay starts when its predecessor succeeds. On the first job, its own delay takes precedence over the chain builder’s delay.

```rust
use openrust::queue::{Delay, Payload};

fn route_job(job: Payload, connection: &str, queue: &str, delay: Delay) -> Payload {
    job.onConnection(connection).onQueue(queue).delay(delay)
}
```

Workers move a successor to another queue on the same connection atomically. Between durable connections, the source worker records a forwarding job in its own queue. Keep a worker running on that queue so it can retry publication after an outage. Run `./artisan migrate` on each database destination to create `job_chain_deliveries`.

A successor assigned to `sync` runs in the source worker process when it handles the forwarding job. Sync ignores the job’s delay. If the handler fails, the chain stops and invokes its failure callbacks. A process crash before acknowledgment can replay the synchronous handler.

After the source acknowledges forwarding, Redis expires its receipt after 24 hours. For SQL, run the command below against the destination queue connection. It removes confirmed receipts older than `--hours`. Pending receipts and pre-upgrade receipts without confirmation stay available for replay protection.

```sh
./artisan queue:prune-chain-deliveries database --hours=24
```

### Adding jobs to the chain {#adding-jobs-to-the-chain}

Inside a job handler, use `JobContext::prependToChain` to run a job immediately after the current job. Use `appendToChain` to place it after the existing chain. Edits take effect when the handler succeeds. A failed or released attempt discards its edits.

```rust
use openrust::queue::{JobContext, Payload, QueueResult};

fn edit_chain(context: &mut JobContext<'_>, before: Payload, after: Payload) -> QueueResult<()> {
    context.prependToChain(before)?;
    context.appendToChain(after)
}
```

Each call accepts a job payload or a pending batch. An inserted batch follows the [batches within chains](#batches-within-chains) rules. In a chain already running inside a batch, insert job payloads to extend that batch. The new jobs share the batch ID, and the worker records their count and successor with the current job’s completion. Run `./artisan migrate` before using runtime edits in batched chains.

### Conditional chain dispatch {#conditional-chain-dispatch}

Use `dispatchIf` to dispatch a chain when the condition is true, or `dispatchUnless` when it is false. Both accept a boolean or a closure returning a boolean. The closure runs once when the dispatch future is awaited.

```rust
use openrust::{
    bus::PendingChain,
    queue::{DispatchResult, QueueResult},
};

async fn dispatch_chain_if(
    chain: PendingChain,
    enabled: bool,
) -> QueueResult<Option<DispatchResult>> {
    chain.dispatchIf(enabled).await
}
```

The methods return `None` when the chain is skipped and `Some(DispatchResult)` when dispatched. A skipped chain never resolves its queue connection or runs a job.

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

### Chains within batches {#chains-and-batches}

Use `addChain` on a pending batch to add an ordered sequence of jobs. It represents a nested array in Laravel’s `Bus::batch`. All members share the batch ID, connection, and queue. The batch counts every member, and initially enqueues only the first job of each chain. Multiple workers can process different chains in parallel.

```rust
use openrust::{bus::PendingBatch, queue::Payload, support::facades::Bus};

fn batched_chains(left: [Payload; 2], right: [Payload; 2]) -> PendingBatch {
    Bus::batch([]).addChain(left).addChain(right)
}
```

Call `dispatch().await` on the builder. To append a sequence after dispatch, call `Batch::addChain(...).await`. A permanent failure stops the affected chain. Its unrun members remain pending, even when the batch allows failures. Retrying the failed job can resume that chain.

### Batches within chains {#batches-within-chains}

Use `appendBatch` or `prependBatch` to add a pending batch to a chain. The worker dispatches the batch when it reaches that step. The batch inherits the chain connection and queue unless it configures its own. Both stages support Redis and database queues on the batch repository connection.

```rust
use openrust::{
    bus::{PendingBatch, PendingChain},
    queue::{Payload, QueueResult},
    support::facades::Bus,
};

fn release_chain(first: Payload, batch: PendingBatch, last: Payload) -> QueueResult<PendingChain> {
    Ok(Bus::chain([first]).appendBatch(batch)?.append(last))
}
```

The outer chain resumes after the batch reaches its final callback phase. If the batch allows failures, it can still reach that phase and continue. If the batch is cancelled, the chain stops. When a failure cancels the batch, the outer chain’s `catch` callbacks receive the failed batch job and its error.

Run `./artisan migrate` to create the continuation table. A failed publication leaves the continuation pending in SQL. Run `./artisan queue:publish-batches` to recover it.

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

OpenRust commits the batch and the Redis publication intent in SQL before contacting Redis. If publication fails, `dispatch()` still returns the accepted batch. Run `queue:publish-batches` periodically to retry pending intents. Redis writes the jobs and a publication marker together. The marker stays in Redis until SQL acknowledges the publication, then OpenRust attempts to set a 24-hour expiry. Configure Redis persistence and eviction for queue data and markers. If you customize the batch table, create its companion tables with the `_jobs`, `_outbox`, `_additions`, and `_continuations` suffixes.

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
