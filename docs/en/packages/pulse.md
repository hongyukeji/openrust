# Pulse

Pulse collects application performance metrics for slow requests, SQL, jobs and outgoing HTTP, exceptions, queues, cache interactions and user activity.

## Installation {#installation}

Enable the application's `pulse` feature in `Cargo.toml`. When using `./artisan`, add `"pulse"` to `[features].default`, keeping existing features. Package discovery loads `PulseServiceProvider`; the default application needs no additional Provider registration.

```sh
./artisan migrate
./artisan vendor:publish --tag=pulse-config
```

PostgreSQL, MySQL, MariaDB and SQLite are supported. The Provider registers migrations for three metric tables without querying them at startup. Configuration and templates belong to the package. Published Rust configuration must be connected to the application's compiled loader and rebuilt. Existing files are preserved by default; cached configuration bypasses package environment reads.

## Configuration {#configuration}

`PULSE_ENABLED` controls collection. Metrics go directly to the database by default. `PULSE_DB_CONNECTION` selects the metric connection; `PULSE_STORAGE_KEEP` defaults to `7 days`. `PULSE_INGEST_BUFFER` defaults to 5000; exceeding it attempts ingestion. HTTP, Artisan and Worker lifecycle boundaries also ingest buffered data. Forced process termination or cancellation does not guarantee ingestion.

To separate collection from persistence, set `PULSE_INGEST_DRIVER=redis`, choose a native connection with `PULSE_REDIS_CONNECTION`, and run the consumer command. `PULSE_INGEST_KEEP` accepts a retention duration; compiled configuration may also specify an entry count. Storage failures retain stream entries for retry. Multiple consumers and failures deleting entries after a successful database write do not provide strict exactly-once processing.

`PULSE_CACHE_DRIVER` selects the native cache store for server throttling, check locks and restart signals. Processes must share their cache connection and prefix; file caching works across processes sharing the same filesystem.

## Dashboard and Authorization {#dashboard}

Visit `/pulse`. Customize the entry point with `PULSE_PATH` and `PULSE_DOMAIN`. Local environments allow access; other environments require the native `viewPulse` Gate. Configure the application's authentication and session middleware as appropriate. The JSON endpoint at `/api/metrics` beneath the dashboard path shares this authorization and accepts `period=1h`, `6h`, `24h` or `7d`.

```rust
let gate = app.make::<openrust::auth::access::GateRegistry>().await?;
gate.define_guest("viewPulse", |user, _| Box::pin(async move {
    Ok(if user.is_some() {
        openrust::auth::access::Response::allow()
    } else {
        openrust::auth::access::Response::deny("Forbidden")
    })
}));
```

The dashboard uses native HTML, JavaScript and JSON. Publish its template with `./artisan vendor:publish --tag=pulse-dashboard`. Keep `<!-- PULSE_STYLES -->` and `<!-- PULSE_SCRIPTS -->` in a custom template to inject package assets. `Pulse::withCss` appends trusted application CSS paths or HTML, removing duplicates; `css` and `js` return asset HTML. Rust expresses the upstream overloaded CSS setter as `withCss`.

## Recorders {#recorders}

Configure `pulse.recorders` using the type names `CacheInteractions`, `Exceptions`, `Queues`, `SlowJobs`, `SlowOutgoingRequests`, `SlowQueries`, `SlowRequests`, `UserJobs`, `UserRequests` and `Servers`. Set a recorder to `false` or `enabled=false` to disable registration at startup. `sample_rate` ranges from 0 to 1. Slow recorder `threshold` values use milliseconds; `ignore` accepts PCRE patterns with PHP-style delimiters.

Use ordered `[[pattern, value], ...]` arrays for `groups` and pattern-specific `threshold` values. For example, `[["/^orders:\\d+$/", "orders:*"]]` preserves first-match precedence; object forms follow JSON object key order. Job sampling uses UUIDs consistently across their lifecycle. Request keys use matched routes instead of individual dynamic IDs. Outgoing HTTP URLs omit usernames and passwords.

Slow query `location` attempts to resolve application source locations; builds without debug symbols may return an empty location. `Exceptions` handles native typed exception logs, job failures and `Pulse::report`. Plain JSON log context is not treated as an exception. Rust errors do not necessarily contain a trace; provide an explicit location when needed.

`Servers` samples each instance at most once every 15 seconds, retaining CPU, memory and disk snapshots. `PULSE_SERVER_NAME` and `PULSE_SERVER_DIRECTORIES` customize the name and colon-separated directories. Override detectors with `Servers::detectCpuUsing` and `detectMemoryUsing`.

## Custom Metrics {#custom-metrics}

Use the Facade inside a bootstrapped request, command or job. Metrics are buffered for the current lifecycle; settings and filters are shared within the application.

```rust
use openrust_pulse::facades::Pulse;

Pulse::record("orders", "completed", Some(120), None).await?
    .count().sum().max().avg();
Pulse::set("deployment", "version", "v1", None).await?;
Pulse::ingest().await?;
let totals = Pulse::aggregateTotal(&["orders".into()], "sum", 3600).await?;
```

`record` returns an Entry supporting `count`, `min`, `max`, `sum` and `avg`; `onlyBuckets` writes only aggregates. Returned entries share aggregation flags with buffered items. Other fields follow Rust value ownership: modifying a copy does not update buffered data. `set` retains the last supplied value for each type/key pair. The four windows are one hour, six hours, 24 hours and seven days; graphs fill 60 points.

`filter` selects items before ingestion. `ignore(future)` temporarily suppresses collection and restores it on cancellation. `lazy` defers metric creation; `handleExceptionsUsing` receives recorder and ingestion errors. `flush` discards the buffer; `ingest` submits it immediately. Query methods include `values`, `graph`, `aggregate`, `aggregateTypes` and `aggregateTotal`. The latter accepts a slice of types and returns per-type results, using `None` for empty aggregates.

Implement `recorders::Recorder` with `name`, `listens` and asynchronous `record`, then call `pulse.register(vec![recorder])` in an application Provider after package boot to listen for native events. Inspect registered instances using `recorders()`. Compiled types and explicit event names replace PHP class reflection. Use a published template for custom HTML cards.

## Users {#users}

Usage metrics read identities already resolved by native authentication. Run authentication middleware or call `Auth::user(request).await?` first. Use `rememberUser` to explicitly associate an actor when dispatching jobs. The default dashboard loads names, email addresses and avatars through the authentication Provider, falling back to the ID. Customize fields with `Pulse::user`, or bind `ResolvesUsers` before package boot to control identifiers and loading. The generic default authentication Provider retrieves users individually; implement the contract for optimized bulk loading.

`report(class, location).await` dispatches a native `ExceptionReported` event. `rescue(future)` returns `None` on an error and invokes the configured handler; the default handler is silent. The deprecated `users` bulk callback receives an identifier slice and returns a map keyed by identifier; new code should use `user` or `ResolvesUsers`. Native application scopes select the container automatically, so there is no `setContainer` operation.

## Commands {#commands}

```sh
./artisan pulse:check
./artisan pulse:check --once
./artisan pulse:work
./artisan pulse:work --stop-when-empty
./artisan pulse:restart
./artisan pulse:clear --type=slow_request --type=slow_query --force
./artisan pulse:purge --force
```

`pulse:check` dispatches checks every second; the server recorder applies its sampling interval. Vapor environments or `--once` take one snapshot. `pulse:work` consumes Redis streams and periodically trims old metrics; direct storage does not require this process. `pulse:restart` signals long-running check and work commands to exit so a process manager can restart them. `pulse:clear` and `pulse:purge` remove selected types or all metrics; production requires confirmation or `--force`. Use `./main artisan` after deployment.
