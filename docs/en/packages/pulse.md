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

Visit `/pulse`. Customize the entry point with `PULSE_PATH` and `PULSE_DOMAIN`. Local environments allow access; other environments require the native `viewPulse` Gate. Configure the application's authentication and session middleware as appropriate. Share a dashboard link with `period=6_hours`, `24_hours` or `7_days`. A link without a period shows the last hour. Browser back and forward restore the selected period. The authorized JSON endpoint beneath the dashboard path at `/api/metrics` accepts these names and `1_hour`, plus the short forms `1h`, `6h`, `24h` and `7d`.

Cards load individually when they first enter the viewport. The dashboard refreshes opened cards together every five seconds. In background tabs, each card skips 95% of its scheduled polls independently. Offline browsers pause automatic updates until the connection returns. In the JSON endpoint, use `cards=servers,queues` to request selected cards.

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

The dashboard uses native HTML, JavaScript and JSON. Publish its template with `./artisan vendor:publish --tag=pulse-dashboard`. Keep `<!-- PULSE_STYLES -->` and `<!-- PULSE_SCRIPTS -->` in a custom template to inject package assets. `Pulse::with_css` appends trusted application CSS paths or HTML, removing duplicates; `css` and `js` return asset HTML. Rust expresses the upstream overloaded CSS setter as `with_css`.

Slow activity cards default to the slowest operation and can sort by count. Exceptions default to count and can sort by latest occurrence. Application usage shows the top ten users for requests, slow requests or jobs. Dashboard selections are kept in the page URL. The metrics endpoint accepts `slow-requests=count`, `slow-queries=count`, `slow-jobs=count`, `slow-outgoing-requests=count`, `exceptions=latest` and `usage=requests|slow_requests|jobs`.

Slow activity, exception, usage, cache and queue cards cache query results for five seconds in `pulse.cache`. Each period and applicable sort or usage type selects a separate result. Cache totals and per-key rows use separate entries. Server heartbeats and graphs share a cache entry for each period and server configuration. Hover over a card heading to see its query duration and execution time. The cache card shows separate timings for totals and key details.

The Queues card shows the peak count for each queue in the selected period. At sample rates between zero and one, it marks the estimated peak with `~` and shows the raw count in a tooltip. Queue names include their connection when the card shows more than one connection. A zero sample rate displays existing raw counts. Hover over a time bucket, tap it, or focus the chart and use the arrow keys to read its counts. Automatic refresh retains the selected time while it remains visible.

Cache shows hits, misses and hit rate across the selected period, including keys outside the 100-row table. When sampling is enabled, displayed counts are estimated and marked with `~`. The JSON endpoint returns raw totals and the sample rate in `cache_totals`.

Slow operations, exceptions, and application usage show estimated counts when their recorder sample rate is between zero and one. The tooltip shows the sample rate and raw count. The JSON endpoint exposes the current rates in `sample_rates` and keeps the stored counts in the card data.

The Servers card shows average CPU and memory history for the selected period alongside the latest snapshot. A server is marked as recently reported if it reported less than 30 seconds ago. Configure `pulse.dashboard.servers.ignore_after` with seconds or a relative duration such as `15 seconds` or `1 minute and 15 seconds` to hide older snapshots. The default `null` keeps them visible. The default sort is by name in ascending order. Set `sort_direction` to `asc` or `desc`. The `sort_by` field can be `name`, `cpu_current`, `memory_current`, `memory_total`, `updated_at`, `recently_reported`, `cpu`, `memory` or `storage`. Nested paths such as `storage.0.used` select a specific reading. Numeric host names sort by numeric value.

## Recorders {#recorders}

Configure `pulse.recorders` using the type names `CacheInteractions`, `Exceptions`, `Queues`, `SlowJobs`, `SlowOutgoingRequests`, `SlowQueries`, `SlowRequests`, `UserJobs`, `UserRequests` and `Servers`. Set a recorder to `false` or `enabled=false` to disable registration at startup. `sample_rate` ranges from 0 to 1. Slow recorder `threshold` values use milliseconds; `ignore` accepts PCRE patterns with PHP-style delimiters.

`Queues`, `UserJobs`, and `SlowJobs` match `ignore` patterns against the job display name. A command dispatched with `Artisan::queue`, for example, matches by command name. `SlowJobs` also uses the display name for threshold rules and aggregation. It measures each attempt from `JobProcessing` to the first success, failure, or release after an exception. It skips the `sync` connection.

For named routes ending in `livewire.update`, `SlowRequests` and `UserRequests` use `memo.path` from the decoded `components[0].snapshot` JSON when it is available. Slow-request entries keep the handling route in the `via` field. Pulse uses the request snapshot already available to the kernel and leaves opaque bodies unread.

`SlowOutgoingRequests` measures native HTTP client attempts, including failed attempts and retries. URL credentials are removed before ignore and grouping rules run. Custom transports can pass an absolute URL and millisecond duration to `SlowOutgoingRequests::record_outgoing`, which uses the same rules.

Slow request timing includes response termination middleware and deferred work, including on error responses. User attribution is captured within the request scope before termination.

Use ordered `[[pattern, value], ...]` arrays for `groups` and pattern-specific `threshold` values. For example, `[["/^orders:\\d+$/", "orders:*"]]` preserves first-match precedence; object forms follow JSON object key order. Job sampling uses UUIDs consistently across their lifecycle. Request keys use matched routes instead of individual dynamic IDs. Outgoing HTTP URLs omit usernames and passwords. Unicode character classes are enabled by `u`. The `r` modifier requires PCRE2 10.43 or later.

Cache grouping in byte mode can produce a key that contains invalid UTF-8. Pulse retains the original bytes through Redis and SQLite. The Cache card renders replacement characters only for display. PostgreSQL and strict MySQL reject invalid keys during storage. MySQL in non-strict mode applies its own text conversion. Older Redis ingest workers leave binary-key payloads queued, so complete the worker upgrade before enabling rules that can produce them.

When a grouping pattern uses `(*UTF)`, a zero-width match followed by a byte offset inside a UTF-8 character returns a PCRE2 offset error. Add the `u` modifier to advance by whole Unicode characters.

Use `Pulse::graph_bytes` to read graph series keyed by their original bytes. This keeps distinct series separate even if their labels look identical. The textual `Pulse::graph` API requires UTF-8 keys.

Use `Pulse::set_bytes` to record a value with an original byte key and `Pulse::values_bytes` to filter by those bytes. SQLite preserves keys with invalid UTF-8 and returns them as database `Value::Bytes`. PostgreSQL and MySQL apply their column encoding and SQL mode. Update all `pulse:work` consumers before producers begin sending byte-keyed values through Redis. Custom filters and storage implementations that match every `Item` variant must handle `Item::ByteValue`.

Slow query `location` uses call sites captured by the database event, including in release builds. An unavailable call site is recorded as an empty string, while disabling `location` records a null value. `Exceptions` collects exceptions from the framework handler, including reported job failures, and from explicit Pulse reports. Call `Pulse::report_exception(error).await?` to send a Rust error directly to Pulse. Use `SharedException::located(error)` at the point of failure to capture the source file and line. Listeners can subscribe to `NativeExceptionReported::NAME` to inspect the original error. Use `Pulse::report(class, location)` when you already have a class name and location. Logging an error alone does not report it to Pulse.

`Servers` samples each instance at most once every 15 seconds, retaining CPU, memory and disk snapshots. Configuration defaults to the operating system hostname and captures it when building the configuration cache. `PULSE_SERVER_NAME` overrides the hostname, and `PULSE_SERVER_DIRECTORIES` sets the colon-separated directories. Override detectors with `Servers::detect_cpu_using` and `detect_memory_using`.

## Custom Metrics {#custom-metrics}

Use the Facade inside a bootstrapped request, command or job. Metrics are buffered for the current lifecycle; settings and filters are shared within the application.

```rust
use openrust_pulse::facades::Pulse;

Pulse::record("orders", "completed", Some(120), None).await?
    .count().sum().max().avg();
Pulse::set("deployment", "version", "v1", None).await?;
Pulse::ingest().await?;
let totals = Pulse::aggregate_total(&["orders".into()], "sum", 3600).await?;
```

`record` returns an Entry supporting `count`, `min`, `max`, `sum` and `avg`; `only_buckets` writes only aggregates. Returned entries share aggregation flags with buffered items. Other fields follow Rust value ownership: modifying a copy does not update buffered data. `set` retains the last supplied value for each type/key pair. The four windows are one hour, six hours, 24 hours and seven days; graphs fill 60 points.

`filter` selects items before ingestion. `ignore(future)` pauses collection. Call `start_recording` inside the callback to resume temporarily. Completion or cancellation restores the previous recording state. `lazy` defers metric creation; `handle_exceptions_using` receives recorder and ingestion errors. `flush` discards the buffer; `ingest` submits it immediately. Query methods include `values`, `graph`, `aggregate`, `aggregate_types` and `aggregate_total`. The latter accepts a slice of types and returns per-type results, using `None` for empty aggregates.

Implement `recorders::Recorder` with `name`, `listens` and asynchronous `record`, then call `pulse.register(vec![recorder])` in an application Provider after package boot to listen for native events. Inspect registered instances using `recorders()`. Compiled types and explicit event names replace PHP class reflection. Use a published template for custom HTML cards.

## Users {#users}

Usage metrics read identities already resolved by native authentication. Run authentication middleware or call `Auth::user(request).await?` first. Use `remember_user` to explicitly associate an actor when dispatching jobs. The usage cards show names and secondary details and lazy-load avatars. The default authentication Provider supplies names, email addresses, and avatars. A missing profile shows its ID and a placeholder avatar. When a profile supplies an email without an avatar, Pulse uses the Gravatar hash. Customize fields with `Pulse::user`, or bind `ResolvesUsers` before package boot to control identifiers and loading. The Eloquent authentication Provider loads user profiles in a single primary-key query. Generic providers continue to load users by ID.

`report(class, location).await` dispatches a native `ExceptionReported` event. `rescue(future)` returns `None` on an error and invokes the configured handler; the default handler is silent. The deprecated `users` bulk callback receives an identifier slice and returns a map keyed by identifier; new code should use `user` or `ResolvesUsers`. Native application scopes select the container automatically, so there is no `set_container` operation.

Callbacks registered with `Pulse::users` can call `User::from_profile(name, email, extra, avatar)` to keep email separate from explicit secondary text and avatar.

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

`pulse:check` schedules the next check one second after the current check starts. The server recorder applies its sampling interval. When `VAPOR_SSM_PATH` enables Vapor, the command takes one snapshot with `SharedBeat.instance` set to `vapor`. Other invocations use a 16-character alphanumeric instance identifier for the life of the command. Use `--once` to take one snapshot outside Vapor. `pulse:work` consumes Redis streams and periodically trims old metrics. It waits one second after each completed iteration. Direct storage does not require this process. `pulse:restart` signals long-running check and work commands to exit so a process manager can restart them. `pulse:clear` and `pulse:purge` remove selected types or all metrics; production requires confirmation or `--force`. Use `./main artisan` after deployment.
