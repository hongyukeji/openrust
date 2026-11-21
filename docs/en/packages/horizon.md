# Horizon

## Installation {#installation}

Enable the root application's `horizon` feature. `openrust-horizon` is an optional Redis queue supervisor with a read-only dashboard. Its `HorizonServiceProvider` is discovered automatically; applications using the package independently register that provider explicitly. Loading the provider starts no workers and opens no Redis connection.

Add `horizon` to the application’s `[features].default` list in `Cargo.toml`, preserving existing features. Use the source launcher:

```sh
./artisan vendor:publish --tag=horizon-config
./artisan horizon
```

Published configuration takes effect after recompiling the application. Repeating publication preserves existing files; `--force` replaces them. See [configuration](../getting-started/configuration.md) for configuration caching.

## Configuration {#configuration}

Use existing standalone Redis connections from `database.redis` and Redis queue connections from `queue.connections`. `horizon.use` selects the metadata connection (default `default`); `horizon.prefix` defaults to `horizon:` and must isolate applications and environments. Redis Cluster is unsupported. Core [queues](../digging-deeper/queues.md) still support their own transports.

`horizon.defaults` maps supervisor names to options. `horizon.environments` selects the first pattern matching `app.env`, including `*`. An unmatched environment starts no supervisors; `maxProcesses: 0` disables a supervisor. Environment options merge recursively with defaults, replacing array entries by index. The default template uses a budget of 10 processes in `production` and 3 in `local`.

Supervisor options include `connection`, `queue` (a string or string array), `balance`, `autoScalingStrategy`, `minProcesses`, `maxProcesses`, `balanceMaxShift`, `balanceCooldown`, `tries`, `timeout`, `sleep`, `memory`, `maxJobs`, `maxTime`, `workersName`, `backoff`, `force`, `rest`, `nice` and `directory`. `backoff` accepts an integer, comma-separated string or integer array. `tries: 0` allows unlimited attempts, `sleep: 0` skips waiting, and `timeout: 0` disables the job timeout. A nonzero `nice` requires the Unix `nice` command and explicitly fails on other platforms. Configuration accepts snake_case keys and the `processes` alias. The Rust options struct uses `maxTries`; configuration also accepts `tries`.

`balance: "auto"` computes floating targets from the full `maxProcesses` budget using estimated time to clear (`time`), pending count (`size`), or proportional `log1p` counts (`log`). Pools are adjusted in ascending target order, rounding up and applying `minProcesses`, available capacity and `balanceMaxShift`. Unknown runtimes are zero; when total runtime is zero, nonempty queues target the maximum budget. `simple` divides the budget evenly; `false` or `"off"` uses one priority pool, scaling by pending count between the minimum and maximum. `balanceCooldown` controls the interval. Calculations include terminating processes; each master manages its own budget.

The worker `timeout` must be shorter than the queue's `retry_after`. Defaults are 60 and 90 seconds respectively. Workers inherit the application's executable, base path, environment, handlers and configuration. `maxJobs` and `maxTime` recycle workers; unexpected exits are replaced. Run the master under an operating-system process manager. SIGTERM/SIGINT and termination requests drain running work, allowing the configured timeout plus five seconds before forceful termination.

## Dashboard authorization {#authorization}

Register authorization and routes in the application's route setup. Access is denied unless the callback authorizes each request, including the stats endpoint. Use the application's existing authenticated session and authorization rules.

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

`Horizon::routes()` registers `GET /horizon` and `GET /horizon/api/stats`; `routesAt("/operations/horizon")` chooses another static path. Existing route groups add their own prefix and middleware. The dashboard refreshes every five seconds, uses a restrictive content security policy and disables response caching. It displays queue backlog, supervisors, recent jobs and aggregate metrics. Control actions use Artisan.

## Commands {#commands}

```sh
./artisan horizon:status
./artisan horizon:status --json
./artisan horizon:pause
./artisan horizon:continue
./artisan horizon:terminate
./artisan horizon:failed
./artisan horizon:retry JOB_UUID
./artisan horizon:forget JOB_UUID
```

`horizon:status` exits with code 1 when no live master is registered. Pause stops new reservations by Horizon workers; a running job can finish. Continue resumes reservations. Terminate requests graceful exit of masters active before that request; subsequently started masters use the new generation. Control commands apply to the shared metadata prefix.

Retry and forget delegate to the native failed-job provider after checking that the job belongs to a configured Horizon connection and queue. Configure durable failed-job storage as described in [queues](../digging-deeper/queues.md). The recent failure list contains sanitized metadata and may outlive a forgotten native failure until retention expires. No payload or exception text is displayed.

## Metrics and limits {#metrics}

`Repository::recent(1..=100)`, `metrics(connection, queue)`, `masters()`, `pause()`, `r#continue()` and `terminate()` expose the same storage operations. `SupervisorOptions::fromConfig` validates the configuration; `AutoScaler::numberOfWorkersPerQueue` computes floating targets and `scalePool` computes each pool adjustment. `MasterSupervisor` owns the child process lifecycle.

Job events from configured Redis queues record IDs, normalized job names, connection, queue, attempt count, status, milliseconds and Redis server time. Payloads, exception messages and credentials are excluded. Metrics aggregate the latest 60 minute buckets, subject to `retention` (60–86400 seconds; default 3600). Recent indexing retains at most 1000 job identities; reads return at most 100. A job's record shows its latest attempt. Repeated finalization is ignored until a new processing event; manual retries with reset attempt counts start a new observation. This is monitoring, not a durable execution ledger.

Heartbeats expire after 15 seconds. Metadata errors suspend new Horizon reservations and stop the master; recovery relies on the process manager. HTTP authorization belongs to the application. This package does not include Laravel Horizon's tags, notification delivery, long-wait alerts, snapshot commands or writable web controls.

Average duration uses completed jobs only; failed, released and retrying attempts do not affect it.
