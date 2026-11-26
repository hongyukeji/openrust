# Telescope

## Installation and configuration {#configuration}

Telescope records application diagnostics in its own database tables and exposes an authorized diagnostic dashboard. Add `telescope` to the application's `Cargo.toml` `[features].default`, retaining existing features, then run:

```sh
./artisan telescope:install
# Rebuild the application to load the published configuration and provider.
./artisan migrate
```

The installer publishes `config/telescope.rs` and `app/providers/telescope_service_provider.rs`, registers both modules and the provider, and preserves existing files on repeated installation. Package discovery registers the package services and migrations. Booting the provider does not query Telescope tables.

`TELESCOPE_ENABLED` defaults to true; `TELESCOPE_PATH` defaults to `telescope`; `TELESCOPE_DOMAIN` optionally restricts the route domain. `TELESCOPE_DRIVER` defaults to `database`. Storage uses `storage.database.connection` and `storage.database.chunk` (default 1000). The package migration is registered on the default connection; when selecting another storage connection, register `TelescopeServiceProvider::migrations()` on that connection before migrating. Configuration caching uses the saved snapshot. Restart persistent processes after configuration changes.

Each `watchers.<Name>Watcher.enabled` switch has a `TELESCOPE_<NAME>_WATCHER` environment variable; compound names use underscores, such as `TELESCOPE_CLIENT_REQUEST_WATCHER`. Watcher-specific options are published in the configuration file. `only_paths`, `ignore_paths` and `ignore_commands` limit recording. Paths are matched without the initial slash. Pause state and monitored tags use the configured application cache.

`init` and its internal `init:resources` command are excluded from recording so initialization can prepare the database and package resources without accessing diagnostic storage first.

## Authorization and filtering {#authorization}

Visit `/telescope`, or the configured path. The installed application provider permits local access by default and rejects other environments until its authorization callback is customized. Use the application's authenticated identity and permissions in that callback. Keep the `web` and Telescope `Authorize` middleware on every dashboard and API route; mutations use the normal request forgery protection, accepting same-origin fetch metadata or a valid session CSRF token.

The installed provider records all local diagnostics. In other environments its default filter retains reportable exceptions, failed requests and jobs, scheduled tasks, and entries with monitored tags. Customize `Telescope::filter`, `filter_batch` and `tag` in the application provider. Callback configuration is shared across execution scopes; pending entries remain isolated per HTTP request, Artisan command and queue attempt. `without_recording(future).await` suppresses nested recording and restores the previous state after cancellation as well as normal completion.

Request passwords and authorization headers are masked by default. The installed provider adds CSRF and cookie masking outside local mode. `hide_request_parameters`, `hide_request_headers` and `hide_response_parameters` accept additional names; nested parameter paths use dot notation. Review recorded event payloads, mail and logs before enabling collection of sensitive application data.

## Watchers and recording {#watchers}

Watchers cover requests, queries, exceptions, logs, jobs, batches, cache, HTTP client requests, commands, scheduled tasks, mail, notifications, models, events, gates, views, Redis commands and dumps. Native lifecycle events supply the actual diagnostic data. Pending entries flush through the framework's deferred lifecycle callbacks. A hard process kill cannot guarantee a flush.

Request records show the client IP resolved by the framework’s trusted proxy configuration and any loaded session data. Apply `hide_request_parameters` to sensitive session keys as well as request inputs. Request duration is recorded in whole milliseconds. PHP allocator peak memory has no native request-level reading.

To exclude requests, set `ignore_http_methods` to exact method names, which are matched case-insensitively, or set `ignore_status_codes` to response codes. The `size_limit` option compares the whole number of 1,000-character units in buffered text or JSON. A limit of `1` keeps up to 1,999 characters.

For rendered Blade responses, the request record includes the resolved template path and its explicit bindings, including values set by composers. View objects contribute their read-only projection. To mask a sensitive binding, use a response parameter path such as `data.token`.

Request diagnostics also record buffered plain text and metadata for parsed multipart uploads. Upload entries contain the client filename and size in decimal KB.

Each of the 18 public types under `openrust_telescope::watchers` implements `Watcher`. Construct a watcher with `new(options)` and call `register(app).await?` from an application Provider; disable its default configuration entry first to avoid registering it twice. The package Provider uses these same registration paths. `QueryWatcher` options include `slow`; `EventWatcher` supports `ignore` patterns. Framework events are ignored by default; set `telescope.ignore_framework_events.store(false, std::sync::atomic::Ordering::Release)` on the resolved service to include them. This setting is shared only within that application.

For application events, implement `Event::properties()` to expose a JSON diagnostic snapshot; Rust does not reflect arbitrary struct fields. Event listener names come from actual dispatcher registrations. Model hydration counts use the native Eloquent lifecycle. Mail records preserve the original mailable and queued flag. Mail previews use a browser sandbox, and downloads contain the captured MIME message.

Scheduled task details record a task's explicit time zone, then the configured `app.schedule_timezone` or the application time zone when no schedule setting is present. An explicit null schedule setting leaves the default diagnostic value empty. Tasks run by `schedule:run` or `schedule:finish` appear under scheduled tasks. A manual `schedule:test` is recorded by CommandWatcher.

Use `openrust::support::dump(&value).await?` for an observed Debug dump. Collection requires an active dashboard dump watcher or `TELESCOPE_DUMP_WATCHER_ALWAYS=true`. This helper does not intercept `dbg!`, `println!` or synchronous collection dump methods. Request input recording uses already-decoded data and never consumes an unread body stream. Redis observation covers successful commands executed through `Connection::query_command`; pipelines and dedicated blocking transports are outside this observation path.

The Dumps page checks for new entries every two seconds while it is open. Select a shared cache store to collect dumps from other processes.

Other lists check for new records while open. Select **Load new entries** when a new record is available, or turn on **Auto load entries** to refresh the list automatically. The browser remembers this setting.

Most lists show one row per family hash on each page. Filter by family hash to see each matching entry. The Jobs list keeps every row and refreshes pending statuses in place.

Source locations and stack frames are absent when the runtime provides no reliable location. Gate entries use the closest eligible call site captured by a Rust authorization entry point. Nested checks in the same application can fall back to the enclosing authorization call site. `ignore_packages` excludes package sources, including Cargo registry and Git checkout paths under a custom `CARGO_HOME`. `ignore_paths` matches the file path that the compiler records. If a release remapping removes the package directory structure, use `ignore_paths` with a distinct remapped dependency prefix. When no captured caller passes the filters, the decision is retained with an empty file and line. Query entries use the nearest eligible captured database caller. Count, page and eager-load queries issued by a single pagination call can all point to the same application line. Eloquent writes, including quiet and transactional writes, use the application call site when it is available. Direct relation lookups, batch reads and cursors can also record their application call sites. Model and collection relationship loads, including the first lazy access and relationship counts, can record the application call site. Association creates, quiet saves, batch writes, and read-then-write operations can attribute their SQL to the application call site. Many-to-many pivot operations can attribute their reads and writes to the same application line. Schema operations can also report their application call sites. Queries inside a foreign-key constraint callback retain the call site inside the callback. A query with captured application callers is excluded when the filters reject all of them. Queries whose entry point supplies no application caller can still appear with an empty file and line. View composer names are not available from opaque Rust callbacks. Buffered JSON/text responses are captured with response parameter masking and the configured character limit. Opaque bodies and streams are not read; they are marked `Not Captured`. Route handler/controller names and the resolved route middleware types come from the router. The HTTP client records multipart fields supplied to `as_multipart()` and the filename, size in decimal KB, and headers of attachments added with `attach_body()` or `attach_file()`. `attach()` records native `Part::text` and UTF-8 `Part::bytes` as field values. Other bytes and stream parts contribute their filename, known size and configured headers. The transport reads attachment streams, and unknown lengths appear as `null`. When a field name repeats, the last part supplies its recorded value at the first part's position. Field names that run from `0` in order appear as a JSON array. The dashboard provides lists, filters, details, monitored tags and recording controls; it does not reproduce the upstream Vue interface or avatars.

Pass an ordered list of descriptors to `as_multipart().post()` when a request needs repeated names or per-part headers. Each descriptor requires `name` and a non-null string, number or boolean in `contents`. It may also supply `filename` and `headers`. A multipart object can include descriptors alongside ordinary fields. Any parts registered with `attach()` follow the supplied data. For supplied fields, the client writes floating-point values with the upstream default of 14 significant digits and keeps the original numbers in diagnostics. Nonempty fields get a part-level `Content-Length` unless one is already supplied.

When request middleware changes multipart to another media type, successful diagnostics use the original part descriptors and mask configured part headers. An empty descriptor list allows the rewritten JSON or form body to be decoded. Connection failures use the media type and buffered body of the request that was sent.

Import `openrust::database::DatabaseCursorExt` and call `cursor.try_next_with_source().await?` to associate SQL with the line that reads the next item. Standard stream consumers use the cursor creation site when available.

Outbound diagnostics show JSON or form fields, or the query parameters supplied in a GET URL. Forms preserve the original scalar types in diagnostics. When a name repeats, diagnostics show its last value. Builder query options still affect the URL sent over HTTP. Raw bodies are only decoded for supported JSON or form content types. In decoded raw JSON, empty objects and objects whose keys run from `0` in order appear as arrays. Structured JSON passed to the client keeps its nested object shapes.

After a redirect, response diagnostics show the final URL and preserve the original GET, JSON, or form input. Each redirect resets `Host` from the target URL. Diagnostics include the recorded host and port. Connection failures use the request carried by the exception, so diagnostics only decode a supported buffered body.

## Queries and commands {#commands}

```sh
./artisan telescope:list request --limit=20
./artisan telescope:list exception --tag=production --json
./artisan telescope:show latest:exception --full
./artisan telescope:show latest --type=query,log --json
./artisan telescope:pause
./artisan telescope:resume
./artisan telescope:prune --hours=48 --keep-exceptions
./artisan telescope:clear
```

`list` accepts `--tag`, `--batch`, `--family` and the `--before` sequence cursor. `show` accepts a UUID, an unambiguous UUID prefix, `latest`, or `latest:<type>`. Its optional `--type` filters related batch entries. The batch summary shows total query time, slow queries, duplicate groups and cache hit rate. Request details include available controller, middleware and user information. Exception and failed job details show the recorded message and stack trace. The detail view shows fields specific to each entry type. Query SQL and log messages appear in full. Payload blocks and batch cells use display-width limits, which `--full` removes. JSON output always preserves captured content. The list shows the age of each entry. Details show both the age and the recorded UTC timestamp. With ANSI output enabled, the list and details use terminal colors for status, severity, and query flags.

Use a decimal sequence number for `--before` and the displayed UUID for `--batch`. The storage database evaluates both filters using its own comparison rules.

The list ignores empty filters and a standalone `0`. A comma-separated tag list can include `0`. The cursor text `00` selects sequences below zero. A digit-only `--limit` above the signed 64-bit maximum is capped at that maximum.

`prune` uses the application clock with a default retention of 24 hours. `--hours=0.5` sets the cutoff 30 minutes before the current time. A negative number moves the cutoff into the future. `--keep-exceptions` preserves exception entries. `clear` deletes all Telescope entries, tags and monitoring rows. Use the application scheduler for retention. `telescope:publish` publishes configuration; `--force` overwrites that file. Standalone code can construct a `Telescope` with an `EntriesRepository` implementation and explicitly call `store().await`; application providers may replace that contract before Telescope is resolved. Providers can bind `ClearableRepository` for `telescope:clear` and `PrunableRepository` for `telescope:prune` independently. Both commands use the configured `EntriesRepository` by default.
