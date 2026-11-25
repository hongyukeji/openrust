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

Long-wait alerts include the deployment name in the email subject, SMS body and Slack details. Pass an incoming webhook URL or a channel ID to `Horizon::route_slack_notifications_to`. A channel ID uses the Slack Bot API and requires `services.slack.notifications.bot_user_oauth_token`.

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

`horizon:status` exits with code 0 when running, 1 when paused and 2 when inactive. Pause stops new reservations; running jobs can finish. Continue resumes reservations. Terminate drains current masters; subsequently started masters use the new generation. Supervisor commands target the named supervisor. Control commands apply to the shared metadata prefix.

Horizon job IDs identify individual Redis deliveries and are returned by queue pushes and the dashboard. Use these IDs for `horizon:retry` and `horizon:forget`. A retry gets a new ID and preserves the original failed monitoring record with its retry result. Native `queue:retry` and failed-job storage use the payload UUID, which Horizon resolves internally; batch retries also resolve those UUIDs to the current failed delivery.

Configure durable failed-job storage as described in [queues](../digging-deeper/queues.md). Retry uses its native claim and redispatch behavior. `horizon:forget` removes the monitoring failure and native failure; `--all` processes all Horizon failure pages. `horizon:clear` clears the selected queue and pending monitoring entries. `horizon:purge` handles orphaned processes. Run `horizon:snapshot` periodically through the [scheduler](../digging-deeper/scheduling.md) to retain metric history; `horizon:clear-metrics` clears metrics while preserving job records.

`horizon:listen` is a development command that polls `horizon.watch` files every 500 ms, including when `--poll` is omitted. It restarts Horizon after a change. In a source application with `Cargo.toml`, it runs through Cargo so Rust source and configuration changes are recompiled. Watched symbolic links are skipped.

By default it watches the application, bootstrap, configuration, database, routes and view directories, Rust files under `public/` and `resources/`, and the root files `main.rs`, `build.rs`, `Cargo.toml`, `Cargo.lock` and `.env`.

## Job tags {#tags}

Register `Horizon::tag(fn(&Payload) -> Vec<String>)` in the application provider after Horizon is registered. Callbacks run for each observed payload; empty tags are omitted and duplicate tags retain their first occurrence. Rust jobs explicitly supply their tags through these callbacks. Payloads are not reflected to discover model fields automatically.

`Tags::for_model(&model)` builds a tag from the Rust model type and its integer, unsigned integer or string primary key; a missing or unsupported key returns an error. Add this returned string to the callback's tags when the job carries that model identity. Tags registered by one application do not affect other applications. The dashboard's monitoring controls select which tags retain monitored jobs.

## Metrics and limits {#metrics}

`Repository::recent(1..=100)`, `metrics(connection, queue)`, `masters()`, `pause()`, `r#continue()` and `terminate()` expose the same storage operations. `SupervisorOptions::from_config` validates the configuration; `AutoScaler::number_of_workers_per_queue` computes floating targets and `scale_pool` computes each pool adjustment. `MasterSupervisor` owns the child process lifecycle.

Redis queue events record IDs, job names, connection, queue, attempt count, status, runtime and Redis server time. Authorized job detail endpoints also expose serialized payloads, tags and exception text. `Repository::recent(1..=100)` remains a metadata projection. Its recent index retains at most 1000 identities and uses `retention` (60–86400 seconds, default 3600) for the rolling 60 minute attempt metrics.

`horizon.trim` controls job-list retention in minutes: `recent`, `pending` and `completed` default to 60; `recent_failed`, `failed` and `monitored` default to 10080. Job lists return 50 entries per page. `horizon.metrics.trim_snapshots.job` and `.queue` default to 24 snapshots per metric. Completed jobs alone contribute to average duration and throughput; failed, released and retrying attempts do not. Duplicate finalization is ignored until another processing event; a manual retry with reset attempts begins a new observation.

`Repository::jobs()`, `tags()`, `metrics_repository()`, `master_supervisors()`, `supervisor_repository()`, `processes()` and `workload()` expose the dedicated repositories. Monitored tags are persisted per metadata prefix. Snapshot reads reset the current metric counters atomically with each stored snapshot. Master heartbeats expire after 15 seconds and supervisor heartbeats after 30 seconds. Metadata errors suspend new reservations and stop the master; an operating-system process manager handles recovery. Monitoring records expire and are not a durable execution ledger.

The workload endpoint (`/horizon/api/workload` by default) returns `split_queues: null` for a single queue. For a group, `split_queues` is keyed by queue name. It is encoded as a JSON array when the unique keys are `0`, `1`, and so on in that order. Duplicate names contribute once to the displayed backlog and split rows, and each occurrence contributes to the group wait estimate.
