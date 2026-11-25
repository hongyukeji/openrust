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

Each of the 18 public types under `openrust_telescope::watchers` implements `Watcher`. Construct a watcher with `new(options)` and call `register(app).await?` from an application Provider; disable its default configuration entry first to avoid registering it twice. The package Provider uses these same registration paths. `QueryWatcher` options include `slow`; `EventWatcher` supports `ignore` patterns. Framework events are ignored by default; set `telescope.ignore_framework_events.store(false, std::sync::atomic::Ordering::Release)` on the resolved service to include them. This setting is shared only within that application.

For application events, implement `Event::properties()` to expose a JSON diagnostic snapshot; Rust does not reflect arbitrary struct fields. Event listener names come from actual dispatcher registrations. Model hydration counts use the native Eloquent lifecycle. Mail records preserve the original mailable and queued flag. Mail previews use a browser sandbox, and downloads contain the captured MIME message.

Use `openrust::support::dump(&value).await?` for an observed Debug dump. Collection requires an active dashboard dump watcher or `TELESCOPE_DUMP_WATCHER_ALWAYS=true`. This helper does not intercept `dbg!`, `println!` or synchronous collection dump methods. Request input recording uses already-decoded data and never consumes an unread body stream. Redis observation covers successful commands executed through `Connection::query_command`; pipelines and dedicated blocking transports are outside this observation path.

Source locations and stack frames are absent when the runtime provides no reliable location. Gate entries use the caller location supplied by the Rust authorization entry point. `ignore_packages` excludes package sources and `ignore_paths` matches path substrings. A filtered or unavailable caller leaves the file and line empty while the decision is retained. Query entries use the nearest eligible captured database caller. Count, page and eager-load queries issued by a single pagination call can all point to the same application line. Eloquent writes, including quiet and transactional writes, use the application call site when it is available. Direct relation lookups, batch reads and cursors can also record their application call sites. Association creates, quiet saves, batch writes, and read-then-write operations can attribute their SQL to the application call site. Many-to-many pivot operations can attribute their reads and writes to the same application line. Schema operations can also report their application call sites. Queries inside a foreign-key constraint callback retain the call site inside the callback. A query with captured application callers is excluded when the filters reject all of them. Queries whose entry point supplies no application caller can still appear with an empty file and line. View composer names are not available from opaque Rust callbacks. Buffered JSON/text responses are captured with response parameter masking and the configured character limit. Opaque bodies and streams are not read; they are marked `Not Captured`. Route handler/controller names and the resolved route middleware types come from the router. The HTTP client records multipart fields supplied to `as_multipart()` and the filename, size in decimal KB, and headers of attachments added with `attach_body()`. Attachment streams are left to the transport and unknown lengths appear as `null`. Applications can still send native `Part` values through `attach()`; Telescope omits fields whose last part is opaque. The dashboard provides lists, filters, details, monitored tags and recording controls; it does not reproduce the upstream Vue interface or avatars.

Outbound diagnostics show JSON or form fields, or the query parameters supplied in a GET URL. Forms preserve the original scalar types in diagnostics. When a name repeats, diagnostics show its last value. Builder query options still affect the URL sent over HTTP. Raw bodies are only decoded for supported JSON or form content types.

After a redirect, response diagnostics show the final URL and preserve the original GET, JSON, or form input. Connection failures use the request carried by the exception, so diagnostics only decode a supported buffered body.

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

`list` accepts `--tag`, `--batch`, `--family` and the `--before` sequence cursor. `show` accepts a UUID, an unambiguous UUID prefix, `latest`, or `latest:<type>`. Its optional `--type` filters related batch entries. Human output truncates long values; `--full` preserves them. JSON output always preserves captured content. Console presentation uses plain tables and timestamps rather than the PHP console's styling and relative time descriptions.

`prune` requires a non-negative integer number of hours. `clear` deletes all Telescope entries, tags and monitoring rows. Use the application scheduler for retention. `telescope:publish` publishes configuration; `--force` overwrites that file. Standalone code can construct a `Telescope` with an `EntriesRepository` implementation and explicitly call `store().await`; application providers may replace that contract before Telescope is resolved.
