# Horizon

## Installation {#installation}

Enable the root application's `horizon` feature. `openrust-horizon` is an optional Redis queue supervisor with an authorized operations dashboard. Its `HorizonServiceProvider` is discovered automatically; applications using the package independently register that provider explicitly. Loading the provider starts no workers and opens no Redis connection.

Add `horizon` to the application’s `[features].default` list in `Cargo.toml`, preserving existing features. Use the source launcher:

```sh
./artisan vendor:publish --tag=horizon-config
./artisan horizon
```

Published configuration takes effect after recompiling the application. Repeating publication preserves existing files; `--force` replaces them. See [configuration](../getting-started/configuration.md) for configuration caching.

## Configuration {#configuration}

Use existing standalone Redis connections from `database.redis` and Redis queue connections from `queue.connections`. `horizon.use` selects the metadata connection (default `default`); `horizon.prefix` reads `HORIZON_PREFIX`, defaulting to the underscore slug of `APP_NAME` followed by `_horizon:`. Use separate prefixes for environments sharing Redis. Redis Cluster is unsupported. Core [queues](../digging-deeper/queues.md) still support their own transports.

For an existing deployment using the former `horizon:` metadata prefix, set `HORIZON_PREFIX=horizon:` while keeping its workers on that prefix. Change prefixes only after draining the old master; a new prefix has separate control state and history.

`horizon.defaults` maps supervisor names to options. `horizon.environments` selects the first pattern matching `app.env`, including `*`. An unmatched environment starts no supervisors; `max_processes: 0` disables a supervisor. Environment options merge recursively with defaults, replacing array entries by index. The default template uses a budget of 10 processes in `production` and 3 in `local`.

Supervisor options include `connection`, `queue` (a string or string array), `balance`, `auto_scaling_strategy`, `min_processes`, `max_processes`, `balance_max_shift`, `balance_cooldown`, `tries`, `timeout`, `sleep`, `memory`, `max_jobs`, `max_time`, `workers_name`, `backoff`, `force`, `rest`, `nice` and `directory`. `backoff` accepts an integer, comma-separated string or integer array. `tries: 0` allows unlimited attempts, `sleep: 0` skips waiting, and `timeout: 0` disables the job timeout. A nonzero `nice` uses Unix process priority interfaces; workers inherit the supervisor priority. Other platforms explicitly reject this option. Configuration accepts snake_case keys and the `processes` alias. The Rust options struct uses `max_tries`; configuration also accepts `tries`.

`balance: "auto"` computes floating targets from the full `max_processes` budget using estimated time to clear (`time`), pending count (`size`), or proportional `log1p` counts (`log`). Pools are adjusted in ascending target order, rounding up and applying `min_processes`, available capacity and `balance_max_shift`. Unknown runtimes are zero; when total runtime is zero, nonempty queues target the maximum budget. `simple` divides the budget evenly; `false` or `"off"` uses one priority pool, scaling by pending count between the minimum and maximum. `balance_cooldown` controls the interval. Calculations include terminating processes; each master manages its own budget.

The worker `timeout` must be shorter than the queue's `retry_after`. Defaults are 60 and 90 seconds respectively. Workers inherit the application's executable, base path, environment, handlers and configuration. `max_jobs` and `max_time` recycle workers; unexpected exits are replaced. Run the master under an operating-system process manager. SIGTERM/SIGINT and termination requests drain running work, allowing the configured timeout plus five seconds before forceful termination.

The process supervisor targets Unix deployments. Signal-based supervisor control and orphan-process inspection require Unix process tools. On other platforms, use the supported metadata commands and test deployment behavior before running workers.

## Dashboard authorization {#authorization}

The package Provider registers dashboard routes automatically. Register authorization in an application Provider or route setup. Access is denied unless the callback authorizes each request, including the stats endpoint. Use the application's existing authenticated session and authorization rules.

```rust
use openrust::http::{HttpError, HttpFuture, Request};
use openrust_horizon::Horizon;

fn view_horizon(request: &mut Request) -> HttpFuture<'_, Result<bool, HttpError>> {
    // Replace with the application's authenticated operator policy.
    let _ = request;
    Box::pin(async { Ok(false) })
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Horizon::auth(view_horizon).expect("Horizon provider registered");
    Horizon::routes()
}
```

`HORIZON_PATH` controls `horizon.path` (default `horizon`); `HORIZON_DOMAIN` controls the optional `horizon.domain`. `horizon.middleware` defaults to `["web"]`. Set `horizon.routes` to `false` to disable automatic registration. `Horizon::routes()` uses these settings; an explicit `routes_at("/operations/horizon")` takes precedence over automatic registration. Routes register once per application. Existing route groups apply their own prefix and middleware. The dashboard refreshes every five seconds, uses a restrictive content security policy and disables response caching. It provides queue workload, masters and supervisors, pending/completed/silenced/failed jobs, job payloads and exceptions, monitored tags, metric snapshots and batches. Failed jobs and batch failures can be retried through the configured native failed-job provider.

Set `HORIZON_NAME` to show a deployment name in the dashboard title and heading.

Register synchronous listeners for `JobPending::NAME`, `JobPushed::NAME`, and `JobReserved::NAME` from `openrust_horizon` to observe Redis queue operations. `JobPending` runs before Redis writes the job, `JobPushed` runs after a successful write, and `JobReserved` runs after Redis reserves the job. The events include the connection, queue, delivery ID, and native payload. For `JobReserved`, the `job` snapshot also records the attempt number. Use the payload UUID to correlate separate deliveries of the same job.

When a job chain advances on the same Redis connection, `JobPending` describes the next job before Redis atomically enqueues it and removes the previous reservation. After Redis confirms both changes, `JobPushed` for the next job precedes `JobDeleted` for the previous job. An expired reservation can prevent the transition after `JobPending`, so use `JobPushed` to confirm the new delivery.

`JobReleased::NAME` runs after Redis returns the reservation to the delayed queue. Its `delay` is a Rust `Duration`, and job details expose it as `release_delay_ms`. `JobDeleted::NAME` runs after Redis acknowledges deletion, including when a job advances its chain. It includes the job snapshot, failure flag, and elapsed time since reservation.

`JobsMigrated::NAME` runs after each Redis promotion batch and before the worker reserves a job. Each poll checks delayed jobs first, then expired reservations. Listeners receive the connection, queue, and job snapshots, including an empty list when a batch moves no jobs.

`JobFailed::NAME` identifies a failed Redis delivery. It includes the connection, reservation snapshot, and exception text. For worker failures, Horizon publishes the event after confirmation and the failed job callback, when failed-job details and tags are available to listeners. A direct `Job::fail` also publishes after confirmation.

For native Redis dispatches deferred until a database transaction commits, a delayed job gets its Horizon metadata when the dispatch is requested. An immediate job gets its metadata during the commit callback. Both publish `JobPending` and `JobPushed` during the commit callback around the Redis write. A rollback leaves the queue and dashboard without a new delivery.

For a failover dispatch inside a transaction, the first connection that accepts the job handles its own transaction settings. A failure before a connection accepts the job can select the next connection. Once a connection has registered a commit callback, an error from that callback is reported to the transaction caller.

Use `JobPayload::from_payload` to read the Horizon metadata on a Redis job. Horizon evaluates tags and silencing rules before the write and saves the result with the delivery. The metadata includes the time of delivery as decimal seconds and the `job_type()` reported by the queue entry point. The serialized type is `job`, `broadcast`, `event`, `mail`, or `notification`. A Horizon retry keeps the saved tags, clears the silenced flag, records the original delivery ID in `retry_of()`, and reports the new delivery as `job`. When the original delivery included a retry deadline and a delivery time, Horizon applies the same retry window to the new delivery. Older payloads may lack Horizon metadata or report the default type `job`.

Call `JobPayload::with_tags(payload, tags)` before dispatch to preset tags. Horizon appends the tags returned by application callbacks, preserving their order and duplicates across the two sources. Use `JobPayload::new` to read or edit raw job JSON. The `offset_set` and `offset_unset` methods update `decoded()`. The `set` method merges fields and rewrites the full `value()`. The `JobPayload::from_payload` method returns the typed `JobPayloadMetadata` snapshot from native Redis deliveries.

Use `value_mut()` and `decoded_mut()` to edit the raw text and decoded data independently. `JobPayload::new` accepts JSON objects and arrays. `set` renumbers integer keys in merge order and encodes consecutive integer keys in order as a JSON array.

List the underlying application types in `horizon.silenced` to silence a broadcast event, listener, mailable, or notification. Use their fully qualified Rust type names. Ordinary jobs also accept their registered queue names. To opt in from application code, implement `openrust_horizon::Silenced` for the type and call `Horizon::silence::<YourType>()` during application boot. The registration is scoped to the application.

Supervisor queue names follow the native Redis transport's 255-byte limit. Spaces, Unicode, and colons are retained. Supervisor queue lists use commas as separators. Names ending in `:delayed`, `:reserved`, or `:notify` are reserved by the transport.

Long-wait alerts include the deployment name in the email subject, SMS body and Slack details. Pass an incoming webhook URL or a channel ID to `Horizon::route_slack_notifications_to`. A channel ID uses the Slack Bot API and requires `services.slack.notifications.bot_user_oauth_token`.

Use `Horizon::route_slack_webhook_notifications_to(url, channel)` to include a channel override in the incoming Webhook payload. Slack app incoming Webhooks always post to the channel selected during installation. Use a Bot API channel ID to choose the destination on each request.

To customize these alerts, bind `dyn LongWaitNotificationFactory` in an application service provider. The factory receives each `LongWaitDetected` event and returns a `LongWaitDetectedNotification`. Its `signature()` controls the five-minute deduplication window.

Register a synchronous listener for `SupervisorLooped::NAME` to observe a Supervisor after it persists its heartbeat. `SupervisorLooped` exposes the Supervisor name and paused state. Horizon registers `MonitorWaitTimes` for this event.

Horizon emits `WorkerProcessRestarting` immediately before replacing an exited worker. The event identifies the supervisor, queue, previous PID when available, and exit code.

Each worker slot starts with a one-second cooldown for nonzero exits and process creation errors. When retrying an exited worker after its cooldown expires, Horizon emits `UnableToLaunchProcess` after `WorkerProcessRestarting` and begins a one-minute cooldown. A clean exit can be replaced during a cooldown. If the operating system could not create the process, the slot stays under the same supervisor and `UnableToLaunchProcess` carries no PID or exit code.

Horizon emits `MasterSupervisorDeployed` after preparing a matching environment's supervision plan. The event reports the master name, environment, and the supervisors scheduled to start.

Applications can dispatch `MasterSupervisorReviving` with a master name or `SupervisorProcessRestarting` with a shared `SupervisorProcess`.

`MasterSupervisorLooped::NAME` likewise fires after the Master heartbeat is persisted. `MasterSupervisorLooped` exposes the Master name and paused state. Horizon registers `ExpireSupervisors` to prune expired entries from both process indexes.

A process that exceeds its memory limit dispatches `MasterSupervisorOutOfMemory` or `SupervisorOutOfMemory` before shutting down with exit code `12`. The event includes the process name, measured resident memory and limit in MiB.

Horizon registers `MonitorMasterSupervisorMemory` and `MonitorSupervisorMemory` on the corresponding loop events. A shared per-cycle check keeps memory protection active if another synchronous listener stops event dispatch early.

The Master also prunes expired job indexes. Recent, pending, completed, silenced, and recently failed entries are checked every minute. The failed and monitored indexes are checked at intervals of their configured retention in minutes divided by 12, rounded down to a minimum of one minute.

Pause, continue, terminate, monitoring changes and retries require the authorization callback on every request. The browser sends `X-Horizon-Request: 1` for mutations; direct API clients must supply the same header. Cross-site browser mutations are rejected. Apply your application's authentication and middleware to the entire route group. Access to this dashboard permits inspection of job data and operational controls.

## Commands {#commands}

```sh
./artisan horizon:status
./artisan horizon:status --json
./artisan horizon:pause
./artisan horizon:continue
./artisan horizon:terminate
./artisan horizon:failed
./artisan horizon:retry JOB_ID
./artisan horizon:forget JOB_ID
./artisan horizon:forget --all
./artisan horizon:list
./artisan horizon:supervisors
./artisan horizon:supervisor-status supervisor-1
./artisan horizon:pause-supervisor supervisor-1
./artisan horizon:continue-supervisor supervisor-1
./artisan horizon:snapshot
./artisan horizon:clear-metrics
./artisan horizon:clear redis --queue=default
./artisan horizon:purge
```

`horizon:status` exits with code 0 when running, 1 when any active master is paused, and 2 when no master is active. Run `horizon:pause`, `horizon:continue`, and `horizon:terminate` on each host you want to control. Pause stops new reservations while running jobs finish. Continue resumes reservations. Terminate shuts down the local masters and broadcasts the native queue restart notification through the application cache. With `fast_termination` enabled, pass `--wait` to wait for the workers to drain. Named supervisor commands also select targets on the current host. Restart masters and supervisors left from an earlier version so they publish their host in Redis.

If an earlier version left a shared pause in Redis, use **Continue** in the authorized dashboard to clear it for that metadata prefix.

On Windows MSVC, `horizon:terminate` requests a graceful shutdown of the local process tree. The application builds with 16 MiB of executable stack reserve. Use `/STACK:16777216` when linking your own Windows MSVC application entry point for Horizon.

Master names use a slug of the hostname followed by a four-character process token. To set a fixed basename, call `openrust_horizon::MasterSupervisor::determine_name_using(|| "orders.primary".into())?` in the application provider's `boot` method. Use the same basename for Horizon and its Artisan control commands. A custom basename can be empty or contain Unicode text, spaces and punctuation. Control characters are rejected. If a full supervisor name contains spaces, quote it when passing it to Artisan from a shell.

Horizon job IDs identify individual Redis deliveries and are returned by queue pushes and the dashboard. Use these IDs for `horizon:retry` and `horizon:forget`. A retry gets a new ID and preserves the original failed monitoring record with its retry result. Native `queue:retry` and failed-job storage use the payload UUID, which Horizon resolves internally; batch retries also resolve those UUIDs to the current failed delivery.

Configure durable failed-job storage as described in [queues](../digging-deeper/queues.md). Retry uses its native claim and redispatch behavior. `horizon:forget` removes the monitoring failure and native failure; `--all` processes all Horizon failure pages. `horizon:clear` clears the selected queue and pending monitoring entries. `horizon:purge` handles orphaned processes. Run `horizon:snapshot` periodically through the [scheduler](../digging-deeper/scheduling.md) to retain metric history; `horizon:clear-metrics` clears metrics while preserving job records.

`horizon:listen` is a development command that polls `horizon.watch` files every 500 ms, including when `--poll` is omitted. It restarts Horizon after a change. In a source application with `Cargo.toml`, it runs through Cargo so Rust source and configuration changes are recompiled. Watched symbolic links are skipped.

By default it watches the application, bootstrap, configuration, database, routes and view directories, Rust files under `public/` and `resources/`, and the root files `main.rs`, `build.rs`, `Cargo.toml`, `Cargo.lock` and `.env`.

## Job tags {#tags}

After Horizon is registered, use `Horizon::tag_for::<T>(callback)` in the application provider to tag a job, mailable, notification, event, or listener type. Register a fallback with `Horizon::tag_models_for::<T>(callback)`. Its callback runs when the type supplies no explicit tags. Queued listeners and their events each follow this rule. Horizon keeps the first occurrence of each tag, with listener tags preceding event tags. Both callbacks can read the serialized event in the payload.

`Tags::for_model(&model)` builds a tag from the Rust model type and its scalar primary key. Missing, null, and false keys yield an empty suffix. True yields `1`. Structured values and invalid UTF-8 byte keys return an error. `Tags::for_models` keeps the first model with a given framework state and preserves equal tags from distinct models. Use `Tags::for_models_by` with a model comparator when custom fields or independently loaded relations affect equality. Rust applications declare the payload's model fields through these callbacks.

Use `Horizon::tag(callback)` to add tags to every observed payload in the application. This callback omits empty tags. Callbacks are isolated by application. The dashboard's monitoring controls select which tags retain monitored jobs.

## Metrics and limits {#metrics}

`Repository::recent(1..=100)`, `metrics(connection, queue)`, `masters()`, `pause()`, `r#continue()` and `terminate()` expose the same storage operations. `SupervisorOptions::from_config` validates the configuration; `AutoScaler::number_of_workers_per_queue` computes floating targets and `scale_pool` computes each pool adjustment. `MasterSupervisor` owns the child process lifecycle.

Redis queue events record IDs, job names, connection, queue, attempt count, status, runtime and Redis server time. Authorized job detail endpoints also expose serialized payloads, tags and exception text. `Repository::recent(1..=100)` remains a metadata projection. Its recent index retains at most 1000 identities and uses `retention` (60–86400 seconds, default 3600) for the rolling 60 minute attempt metrics.

`horizon.trim` controls job-list retention in minutes: `recent`, `pending` and `completed` default to 60; `recent_failed`, `failed` and `monitored` default to 10080. Job lists return 50 entries per page. `horizon.metrics.trim_snapshots.job` and `.queue` default to 24 snapshots per metric. Completed jobs alone contribute to average duration and throughput; failed, released and retrying attempts do not. Duplicate finalization is ignored until another processing event; a manual retry with reset attempts begins a new observation.

`Repository::jobs()`, `tags()`, `metrics_repository()`, `master_supervisors()`, `supervisor_repository()`, `processes()` and `workload()` expose the dedicated repositories. Monitored tags are persisted per metadata prefix. Snapshot reads reset the current metric counters atomically with each stored snapshot. Master heartbeats expire after 15 seconds and supervisor heartbeats after 30 seconds. Metadata errors suspend new reservations and stop the master; an operating-system process manager handles recovery. Monitoring records expire and are not a durable execution ledger.

The workload endpoint (`/horizon/api/workload` by default) returns `split_queues: null` for a single queue. For a group, `split_queues` is keyed by queue name. It is encoded as a JSON array when the unique keys are `0`, `1`, and so on in that order. Duplicate names contribute once to the displayed backlog and split rows, and each occurrence contributes to the group wait estimate.
