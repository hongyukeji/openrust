# Logging

The Slack channel’s default username is the configured application name followed by ` Log`, such as `ExampleStore Log`. Set `LOG_SLACK_USERNAME` in `.env` to override it.

## Configuration

Configure the default channel with `LOG_CHANNEL`, the ordered members of the default stack with `LOG_STACK`, and the minimum severity with `LOG_LEVEL`. The default `stack` contains `single`, which appends to `storage/logs/app.log`. Relative paths are resolved from the application root. A channel's optional `name` changes the label on each record. The application environment is the default label.

Supported drivers are `single`, `daily`, `monthly`, `stderr`, `syslog`, `errorlog`, `slack`, `papertrail`, `null`, `stack`, `monolog` and registered `custom` factories. A file is opened when a record meets the severity threshold. The `single` driver accepts `locking`, `bubble`, and an integer `permission` such as Rust's `0o600`. Permission applies on Unix. Records use UTC timestamps.

## Logging Deprecation Warnings {#logging-deprecation-warnings}

`LOG_DEPRECATIONS_CHANNEL` selects the channel for supported runtime compatibility warnings; the default `null` discards them. `LOG_DEPRECATIONS_TRACE=true` adds their location and backtrace. Configure these through `logging.deprecations`. A named `logging.channels.deprecations` channel always takes precedence over the selected channel. In `APP_ENV=testing`, warnings are skipped unless `LOG_DEPRECATIONS_WHILE_TESTING=true`. Logging failure does not turn a deprecation into a failed response.

These are runtime compatibility warnings, such as lossy numeric conversion during template evaluation. Rust compiler diagnostics remain build output; this is not a global interceptor for warnings from every dependency. Use the configured deprecations channel explicitly for application runtime warnings.

## Building Log Stacks

Set `channels` to a list or comma-separated string. Members run in order. A handler with `bubble: false` stops the stack after accepting a record. The `null` handler discards an accepted record and stops the stack. With `ignore_exceptions: true`, the group attempts every handler and suppresses write failures.

Use `Log::stack(&["single", "stderr"], None)` for a stack scoped to the returned logger. Named channels are resolved lazily and cached per application. After changing a named channel's configuration, call `forget_channel(Some(name))` to resolve it again.

## Writing Log Messages

Use `Log::info("Order {id} created", &context)` in an application lifecycle. `Log::channel(Some("stderr"))?` selects a named channel. Context values are structured JSON, and `replace_placeholders` enables interpolation. The Rust logger writes synchronously, as does the current channel contract.

An invalid channel falls back to `logging.channels.emergency.path`. A write failure reports a diagnostic and sends the record to the emergency logger. If the emergency file also fails, the emergency logger writes to standard error.

```rust
use openrust::{config::Repository, log::{LogContext, LogManager}};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let logger = LogManager::new(Arc::new(Repository::new(Default::default())), root);
    let context = LogContext::from_iter([("id".into(), 42.into())]);
    logger.info("Order {id} created", &context)?;
    Ok(())
}
```

## Rotating File Channels

Use `daily` or `monthly` for files such as `app-2026-11-11.log` and `app-2026-11.log`. The first accepted record of a UTC period opens its file. Set `max_files` to the number of files to keep, including the active file, or set it to zero for unlimited retention. The application defaults retain 14 daily files and three monthly files. Retention only considers regular files with the same base name, extension and period format.

## On-Demand and Custom Channels

`Log::build(&settings)?` creates an on-demand logger. Use `Log::stack_channels` with `StackChannel::Named` and `StackChannel::Logger` to combine configured channels and on-demand loggers. The resulting stack has its own label.

For `driver: "custom"`, set `via` to the name of a factory registered with `LogManager::register_factory`. Use `LogManager::extend` to register a named driver. Factories are compiled Rust closures that receive the manager and channel configuration and return a logger implementing the logging contract. They can run concurrently on the first lookup, so keep initialization safe to repeat. Successful named lookups share the published logger. Factory failures go to the emergency logger and can be retried.

```rust
use openrust::{config::Repository, log::{LogContext, LogManager, StackChannel}};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let manager = LogManager::new(Arc::new(Repository::new(Default::default())), root);
    manager.register_factory("audit", |manager, config| {
        let logger = manager.build(&json!({"driver":"monthly", "path":config["path"]}))?;
        Ok(logger)
    });
    let audit = manager.build(&json!({"driver":"custom", "via":"audit", "path":"audit.log"}))?;
    let stack = manager.stack_channels(
        &[StackChannel::Named("single"), StackChannel::Logger(&audit)],
        Some("orders"),
    )?;
    stack.info("Audit ready", &LogContext::new())?;
    Ok(())
}
```

## Customizing Channel Records {#customizing-channel-records}

Register names with `LogManager::register_formatter`, `register_processor`, and `register_tap` in your service provider. Select a formatter with `formatter` and pass its settings in `formatter_with`. Use `formatter: "default"` to retain the handler’s own formatter; omitting it selects the channel’s default formatter. Formatters write the exact bytes returned, including any record separator, for file, rotating file, and standard error handlers. Custom logger factories control their own output.

Declare `processors` as names or objects with `processor` and `with`. Processors run in configuration order before formatting. They can edit the message, record context, and public scope metadata through `LogRecord`. A stack collects its members’ processors in member order. Each member retains its own formatter.

A tap such as `"audit:orders,prod"` receives the logger and the two arguments. Taps run in list order before the channel is cached. Use `set_formatter` to replace the channel formatter or `push_processor` to prepend a processor.

```rust
use openrust::{
    config::Repository,
    log::{LogContext, LogManager},
};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = LogManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    manager.register_formatter("message", |_| {
        Ok(Arc::new(|record| Ok(format!("{}\n", record.message()))))
    });
    let channel = manager.build(&json!({
        "driver": "single",
        "path": "storage/logs/audit.log",
        "formatter": "message",
    }))?;
    channel.info("Order recorded", &LogContext::new())?;
    Ok(())
}
```

## Handler Channels {#handler-channels}

Use `driver: "monolog"` with a named Rust handler. The built-in `stream` handler accepts paths, `stderr`, and `stdout`. To supply another handler, use `LogManager::register_handler`. Its factory receives the merged settings and returns a callback that receives the structured record and formatted text. Settings are merged in order from the channel `level`, `with`, then `handler_with`. Later values take precedence.

Set `action_level` to accumulate records until a record reaches that severity, then flush the accumulated batch. The wrapped handler applies its own minimum `level` to every record. After the first flush, subsequent records go straight to the handler. Set `stop_buffering: false` to start a new batch after each flush. Buffer state belongs to the resolved channel; an untriggered batch is discarded when the channel is dropped.

```rust
use openrust::{config::Repository, log::{LogContext, LogManager}};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = LogManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    let audit = manager.build(&json!({
        "driver": "monolog",
        "handler": "stream",
        "with": {"stream": "storage/logs/audit.log"},
        "handler_with": {"level": "info"},
        "action_level": "error",
        "stop_buffering": false,
    }))?;
    audit.info("Import started", &LogContext::new())?;
    audit.error("Import failed", &LogContext::new())?;
    Ok(())
}
```

## Syslog {#syslog-channel}

Set `driver: "syslog"` to send records to the local system log. On Unix, the channel sends datagrams to the system socket. The channel uses `/var/run/syslog` on macOS and `/dev/log` on Linux by default. Set `socket` to use another receiver. Relative socket paths resolve from the application root. If delivery fails, the same channel can retry on the next record and the failed write follows the emergency log path.

The `facility` defaults to `user`. Use a name such as `local4` or its numeric syslog constant. The wire priority combines the facility and record severity. The tag comes from the application name and process ID. With no formatter specified, the payload uses the framework line format. Set `formatter: "default"` to omit the payload timestamp, or choose a registered formatter.

```rust
use openrust::{config::Repository, log::{LogContext, LogManager}};
use serde_json::json;
use std::{os::unix::net::UnixDatagram, sync::Arc, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let receiver = UnixDatagram::bind("audit.sock")?;
    receiver.set_read_timeout(Some(Duration::from_secs(2)))?;
    let manager = LogManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    let audit = manager.build(&json!({
        "driver": "syslog",
        "socket": "audit.sock",
        "facility": "local4",
        "formatter": "default",
    }))?;
    audit.notice("Import complete", &LogContext::new())?;
    let mut bytes = [0; 1024];
    let received = receiver.recv(&mut bytes)?;
    let packet = std::str::from_utf8(&bytes[..received])?;
    assert!(packet.starts_with("<165>"));
    assert!(packet.ends_with("Import complete [] []"));
    Ok(())
}
```
On Windows the channel writes to the Application event log. The provider name comes from the application name. The event details contain the application tag with the process ID and the formatted message as separate insertion strings. The category is the syslog severity number. As in PHP's Windows adapter, `alert` maps to Error, `info` maps to Information, and other levels map to Warning. `facility` is validated, with no facility field in the Windows event. The Unix-only `socket` option must be omitted on Windows.

To inspect the raw message in PowerShell, replace `my-app` with the provider name derived from the application name.

```powershell
$filter = "*[System[Provider[@Name='my-app']]]"
Get-WinEvent -LogName Application -FilterXPath $filter -MaxEvents 1 |
    ForEach-Object { $_.Properties[1].Value }
```


## JSON Exception Logs {#json-exception-logs}

Select `formatter: "json"` to emit structured records. Use `formatter_with` to set `append_newline`, `ignore_empty_context_and_extra`, or `max_normalize_depth` (default 9).

Pass an error to `Log::log_exception(level, message, &error, &context)` to include `context.exception` and its `previous` chain. Register `exception_context::<E>` in `with_exceptions` to add fields to each matching error type, including sources. The existing `context` callbacks enrich a directly logged root error. When the application reports an error, its root fields appear only in the record context, and source-specific fields remain with their source. Rust can name the directly supplied type and registered source types. A type-erased source can appear as `Error`. Rust's standard error contract does not supply the original file or stack trace.

```rust
use openrust::{config::Repository, log::{Level, LogContext, LogManager}};
use serde_json::json;
use std::{io, sync::Arc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = LogManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    let logger = manager.build(&json!({
        "driver": "single",
        "path": "storage/logs/errors.jsonl",
        "formatter": "json",
    }))?;
    let error = io::Error::other("storage offline");
    logger.log_exception(Level::Error, "Import failed", &error, &LogContext::new())?;
    Ok(())
}
```

## Contextual Information

The public fields captured by the [Context facade](../digging-deeper/context.md) appear after a log record's own fields as separate JSON metadata. Custom sinks can implement `log_with_context` to receive both maps. Hidden fields stay in the context repository.

## Listening for Log Messages

Register `Log::listen` in a service provider's `boot` method. Its callback runs synchronously after an accepted record is written, and returns `LogResult<()>`. `LogManager::listen` and `Logger::listen` use the same application-wide listeners across named channels, stacks, on-demand channels, and renamed loggers.

The `MessageLogged` payload contains the original message, level, and context. Placeholder interpolation and channel processors apply to the written record. `log_exception` also supplies the normalized `context["exception"]`. Records below every handler's threshold skip notification. Emergency fallback records and its diagnostics notify listeners too.

Callbacks run in registration order. A callback error propagates after the write and stops later callbacks. A callback may log or register another listener. Registrations made during a notification take effect on the next notification. The synchronous log listener API executes inside the calling application scope. General asynchronous event dispatch remains an explicit operation.

```rust
use openrust::{log::LogError, support::facades::Log};

fn register_log_listener() -> Result<(), LogError> {
    Log::listen(|event| {
        eprintln!("{}", event.message);
        Ok(())
    })
}
```

## Pail Session Capture

Build with `cargo build --features pail` to discover the optional `openrust-pail` provider. It copies accepted messages from any log driver into active `storage/pail/*.pail` sessions. No session file means no capture file is written. The live command and filters are described [below](#tailing-log-messages-using-pail).

The package exposes `File`, `Files`, `Handler` and `PailServiceProvider`. `File::create()` starts a session without truncating an existing file; `destroy()` removes it. Captures use locked JSON lines across processes, preserve severity and structured exceptions, and expire a session after more than one hour without writes. A removed session is not recreated by logging. File errors propagate through the original logging result.

Records include HTTP method/path, command name or queue/job metadata under `context.__pail.origin`. Public Context values override message context; hidden values are excluded. Rust uses application-bound asynchronous scopes to isolate overlapping requests, nested commands and concurrent jobs in one process. Without log listeners, origin collection is skipped.

### Request Authentication at Log Time {#request-log-authentication}

Synchronous `Log::info` and other existing methods capture the current guard's already-resolved identity; unresolved users are null. In an asynchronous HTTP handler, use the request-aware entry point when the log should resolve authentication at that call:

```rust
Log::log_with_request(request, openrust::log::Level::Info, "Order placed", &LogContext::new()).await?;
```

The same method is available on `LogManager` and resolved `Logger` channels. The channel writes first; accepted records then notify observers in registration order. With an active Pail session, Pail awaits the selected guard and captures its ID and email before this call returns. Cached users are reused; login, logout and guard selection affect subsequent logs. No active session, a rejected log level or a suppressed deprecation does not trigger authentication. The built-in HTTP exception reporter uses the request-aware path as well.

This explicit await replaces PHP's ability to resolve a guard inside a synchronous log listener. Container callbacks and other synchronous code keep snapshot semantics. `LogManager::listen_for_request` registers a synchronous observer and its awaited request counterpart; ordinary `listen` callbacks still run synchronously. No listener registry lock is held while callbacks run. Logs produced during guard resolution capture the existing snapshot and do not start recursive authentication.

Authentication or observer errors return through the log result after the ordinary channel write. Cancelling the future may therefore leave a written channel record without a Pail capture. Cancellation releases the request borrow and resolution scope; a later call can retry. Requests and applications keep separate authentication state; programmatic Artisan calls within HTTP retain that HTTP origin, while standalone Artisan uses console origin.

Authentication callbacks may themselves log. A nested lookup for the same request and guard reads only the already-known identity, without restarting the pending lookup. Completion, errors and cancellation release that marker; the outer authentication error remains observable and a later request or retry can resolve normally.

### Reading and Filtering Captures

`openrust_pail::value_objects::MessageLogged::from_json` reads a capture line. It exposes its message, level, date/time, exception class/file, trace and typed HTTP, console or queue origin. Parsing, date conversion and origin decoding return `Result` where they can fail. `context()` excludes the reserved `__pail`, `exception` and `user_id` fields.

`Options::accepts` combines all supplied filters. Level comparison and substring searches fold ASCII case. The message filter searches only message text; the general filter searches the four-field JSON representation, including origin and context. That representation follows PHP's Unicode escaping, object order and float notation, so a general Unicode search may need its `\u` spelling. Authentication IDs compare as exact strings; `12` and `012` differ. `from_command(&Input)` preserves `auth` precedence over `user`, including an explicitly empty value, and PHP's timeout conversion. It reads the Rust command's separate input object.

```rust
use openrust_pail::{Options, value_objects::MessageLogged};

fn select(line: &str) -> Result<bool, serde_json::Error> {
    let message = MessageLogged::from_json(line)?;
    let options = Options::new(3600, None, Some("ERROR".into()), None, Some("payment".into()));
    Ok(options.accepts(&message))
}
```

These parsing and filtering APIs also drive the live [Pail command](#tailing-log-messages-using-pail). Request authentication follows the explicit await and snapshot rules above.

### Provider Availability

Pail still registers Files and Handler when capture is disabled. During boot it skips listeners if the bound application environment is exactly `testing`, or `VAPOR_SSM_PATH` is a nonempty string other than `0`. `PAIL_TESTS=true` enables Pail's own acceptance tests and overrides both checks. This flag follows the framework Env normalization, including false, empty, null and quoted values; the Vapor marker uses raw PHP string truthiness.

Flags come from the current application's immutable environment snapshot. Process variables take precedence over the selected environment file. With configuration cached, environment files are not loaded, so a file-only `PAIL_TESTS` does not apply. Handler is resolved on the first accepted log or queue lifecycle event. Its construction error propagates then, after the original accepted log has been written.

Pail resolves `Application::storage_path("pail")` through the shared path service. Configure `use_storage_path` or `APP_STORAGE_PATH` before the first capture. See [Application Storage Path](../getting-started/configuration.md#application-storage-path).

Default file log channels and the emergency fallback follow the application storage path. Configure the path before loading configuration or resolving channels. Loaded channel paths retain their configured values; see [Application Storage Path](../getting-started/configuration.md#application-storage-path).

## Pail CLI Printer {#pail-cli-printer}

The optional Pail package exposes `openrust_pail::contracts::Printer` and `printers::CliPrinter`. In an Artisan command, construct `CliPrinter::new(output, base_path)` and call `print(&message_logged)?`. It reads the current output's verbose and decoration flags. Normal output shows the time and abbreviated class/file/message; `-v` shows the date and full text, and `-vv` or `-vvv` adds captured trace frames.

HTTP method/path and authentication, queue/job/command, and public context appear in the footer. Numeric/null context entries, PHP-style value exports, literal HTML text and multiline messages follow the pinned Pail rendering rules. The frame is limited to 150 terminal columns; source messages and long footers can overflow as in upstream. Rust `::` class namespaces and native Windows path separators are adapted for abbreviation and relative source display.

Rust JSON unsigned integers retain their original precision; values beyond PHP’s signed integer range can therefore display differently from PHP’s floating-point approximation. Rust renders the frame directly and uses `Output::write_raw` for already-rendered text. Real Artisan output streams immediately; captured output stays buffered. `--ansi`, `--no-ansi`, `--quiet` and `--silent` apply, and write failures propagate. Formatting errors return before the frame is written. Output verbosity is scoped to the invocation and restored after nested calls. `PAIL_TESTS` controls capture testing; displayed dates, files and trace frames retain their recorded values rather than PHP's test fixture replacements.

The live [Pail command](#tailing-log-messages-using-pail) uses this printer. The loaded provider also adds the default `logs` process to [Artisan dev](../digging-deeper/artisan.md#the-dev-command).

## Tailing Log Messages Using Pail {#tailing-log-messages-using-pail}

Add `pail` to the application’s `[features].default` list in `Cargo.toml`, preserving existing features, then use the source launcher:

```sh
./artisan pail --timeout=1
./artisan pail --timeout=0 --level=error --message=payment -vv
```

`pail` follows application log events regardless of the channel's storage backend. Each invocation creates its own file under the current application storage path's `pail/` directory. Concurrent invocations receive their own copies. `--filter`, `--message`, `--level`, `--auth` and `--user` use the filters described above; `--auth` takes precedence over its `--user` alias. The execution timeout defaults to 3600 seconds; zero disables it. Timeout prints “Maximum execution time exceeded.” and exits successfully. Ctrl+C or SIGTERM stops following and removes the owned session. Invalid records and output errors fail the command and still clean up. `--quiet` and `--silent` suppress output.

Rust follows the file natively, including partial UTF-8 lines, truncation and replacement by name, without PHP, PCNTL or a `tail` subprocess. Initial existing content follows the last-ten-lines rule. An incomplete line is discarded at a detected truncation or replacement boundary to avoid combining different file generations. A completed JSON record is filtered and printed immediately. On Windows, use `.\artisan.bat pail --timeout=0`. Native console Ctrl+C stops the command and removes its capture session, as does a normal timeout.

The provider registers commands through the current application's ConsoleKernel, under the same testing/Vapor gates as capture. There is no PHP global SAPI flag: HTTP-only boot does not execute the registrar, while standalone and programmatic Artisan share it. Cancelling a captured command removes its session and restores the caller's output metadata. This does not add a heartbeat: as upstream, a session older than one hour without writes may be removed by the next producer even with `--timeout=0`. The `.gitignore` remains after cleanup.

### Development Process Registration

With `pail` enabled, package discovery loads the provider and `./artisan dev:list --json` includes `logs` with `pail --timeout=0`. Start it with `./artisan dev --inline`. The supervisor uses the same executable; it does not invoke Cargo or require PHP/PCNTL. A build without Pail, or an application that does not load its provider, has no default `logs` entry. Listing processes never creates a capture session.

Registration tests the provider identity, independently of capture gates. In `testing` or Vapor environments the loaded provider still yields a default `logs` entry, even though its `pail` command is disabled unless `PAIL_TESTS` overrides the gate. Use `DevCommands::except(&["logs"])?` to exclude it in such an application. On Windows, the supervisor sends console shutdown signals to the default `logs` process and reclaims its process tree through Windows jobs. A normal Ctrl+C shutdown removes the owned Pail session.

The loaded-provider check is repeated when resolving the registry, so an early query before Pail registers does not permanently hide `logs`.
## Channel Context

Use `with_context` and `without_context` for channel context. `share_context` merges context into resolved and future channels; per-message fields override channel fields. `shared_context` reads the defaults for future channels. `flush_shared_context` clears those defaults without deleting fields already merged into channels; use `without_context` to remove channel fields. Boot-time defaults are inherited by each request or queue scope. Dynamic context is isolated by scope, even when channel handles are reused.


## Inspecting resolved channels

`LogManager::get_channels()` and `Log::get_channels()` return a snapshot mapping resolved channel names to their `Arc<Logger>` handles. Reading the map does not create unused channels. Removing a channel with `forget_channel` affects subsequent resolutions; previously returned snapshots and logger handles remain usable.

## Accessing the Native Logger

`Logger::get_logger()` returns the channel's actual `Arc<NativeLogger>` handle. `LogManager::get_logger()` and `Log::get_logger()` resolve the default channel and return the same native instance through `Result`. Reading an already resolved handle does not create other channels, and retained handles remain usable after `forget_channel`.

```rust
use openrust::{log::LogContext, support::facades::Log};

let channel = Log::channel(None)?;
let native = channel.get_logger();
native.info("Worker started", &LogContext::new())?;
let default_native = Log::get_logger()?;
```

Native writes use the configured severity filters, processors, formatters and output handlers. They receive only the supplied context and bypass application channel context, Context metadata, MessageLogged observers and Pail capture. Native write or processor failures return an error directly. Use the channel or Log facade's level methods for application logging with context, events and emergency recovery. The native handle implements the existing Rust log contract; configure shared channel processors and formatters through the usual registration and tap interfaces.

## Process Error Log {#process-error-log}

Use `driver: "errorlog"` to send records to the process error stream. The supported `type` values are `0` (default) and `4`; both write to standard error in a native executable without a PHP SAPI. The destination appends one newline even if the formatter already ends with one. Level filtering, placeholders, processors and formatters follow the normal channel rules. This driver always bubbles to the next stack member, matching its constructor contract; a `bubble` option does not change that behavior. Unsupported types enter the existing emergency recovery path.

## Remote Log Channels {#remote-log-channels}

The `slack` driver sends JSON to the configured incoming Webhook. Set `LOG_SLACK_WEBHOOK_URL`; the application configuration uses `LOG_LEVEL` or `critical` as its threshold. `url`, `username`, `channel`, `emoji`, `attachment` (default `true`), `short` (default `false`), `context` (default `true`) and `exclude_fields` configure its payload. Attachments retain structured fields; `attachment: false` sends the formatted text. Placeholder replacement and processors run before delivery.

`exclude_fields` uses the external handler's dot-path behavior: a null value is retained, and a path with a repeated final segment stops at its first matching segment. Use a processor to apply a different redaction policy. Never put secrets in log context unless you intend to send them to the configured destination.

The native synchronous logger waits for the transport. A private channel thread owns the existing HTTP client and runtime, so logging also works inside an async request without nesting a runtime. `timeout` sets seconds per attempt (default `10`); transport connection, timeout and body failures are retried up to five attempts. The handler does not follow redirects. HTTP status codes alone are not transport errors, matching the external handler; receiving a response does not prove that a remote service accepted the message. Transport failures use the existing emergency recovery; the same channel can send later records after recovery.

The `papertrail` channel reads `PAPERTRAIL_URL` and `PAPERTRAIL_PORT` and sends remote Syslog datagrams. The existing `driver: "papertrail"` is retained as a native alias. For the generic handler form, use:

```rust
let settings = serde_json::json!({
    "driver": "monolog",
    "handler": "syslog_udp",
    "handler_with": {"host": "127.0.0.1", "port": "10000", "ident": "orders"},
    "replace_placeholders": true,
});
```

The handler accepts integer or numeric-string ports, `facility`, `ident` and `rfc`: `0` selects RFC 3164, `1` (default) RFC 5424, and `2` includes millisecond precision. The hostname comes from the OS; the default program identifier is the application name. Custom formatters can emit several lines, each sent in a separate datagram. The nominal datagram limit is 65,023 bytes including the header, subject to platform socket limits. On Unix, port `0` selects a local datagram socket; relative socket paths resolve from the application root. UDP provides no delivery acknowledgement. These drivers preserve normal stack filtering, formatting and emergency behavior.

## Built-in Formatters and Processors {#native-log-components}

`openrust::log::formatters::HtmlFormatter` and `openrust::log::processors::{MemoryUsageProcessor, PsrLogMessageProcessor}` are concrete public types. The channel configuration can select them directly:

```json
{"driver":"single","path":"storage/logs/formatted.html","formatter":"html","formatter_with":{"dateFormat":"Y-m-d H:i:s"},"processors":[{"processor":"memory_usage","with":{"realUsage":true,"useFormatting":false}},{"processor":"psr_log_message","with":{"removeUsedContextFields":true}}]}
```

`html` escapes message and field content and accepts the existing PHP-style `dateFormat`. `psr_log_message` interpolates JSON context and can remove used fields; opaque PHP objects and their date-format hooks are not part of JSON context. `memory_usage` records process resident memory in `extra.memory_usage`; it measures OS memory, not PHP allocator usage. `useFormatting` defaults to `true`; `false` retains numeric bytes. `realUsage: false` is rejected rather than returning an unrelated allocator metric. Compiled registrations with the same name override the built-in factory.

## Browser Console Output {#browser-log-output}

Configure a development channel:
```json
{"driver":"monolog","handler":"browser_console","level":"debug"}
```

`browser_console` appends a script to HTML or JavaScript responses. Severity, CSS markers `[[text]]{color: blue}`, channel labels, context and extra fields are retained. JSON, HEAD, bodyless statuses, encoded content, partial files and binary downloads are left intact. CLI commands do not print browser scripts. Records belong to one request and application. The handler is also available as `openrust::log::BrowserConsoleHandler`.

Stream bodies append console output before trailers or EOF. Raw body streams are polled after the request scope closes; capture an owned logger with `Log::channel(None)?` before creating the stream, and move that handle into it. This does not keep request-scoped Facades alive. A dynamically built browser channel must be created before returning the response, or declared in `logging.channels`, so the body wrapper can be installed.

## Container-Resolved Taps {#container-resolved-taps}

Call the following function from your service provider’s `register` method. Set the channel’s `tap` to `["customize:orders,prod"]`. The registered closure resolves the concrete customizer and its constructor dependency when that channel is first created, rather than eagerly constructing them during boot. Compiled registration replaces runtime class-name loading; dependencies still use the application container. `forget_channel` causes the channel and its tap to be resolved again.

```rust
use openrust::{
    container::{Container, ClassDescriptor, ConstructorParam},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    log::{Formatter, LogError, LogManager, LogResult, Logger, formatters::LineFormatter},
    support::ProviderResult,
};
use std::sync::Arc;

struct FormatterLabel(String);
struct CustomizeFormatter { label: Arc<FormatterLabel> }
impl CustomizeFormatter {
    fn invoke(&self, logger: &mut Logger, arguments: &[&str]) -> LogResult<()> {
        let pattern = format!("{}:{}:%message%\n", self.label.0, arguments.join(","));
        let formatter = LineFormatter::new(Some(&pattern), None);
        let formatter: Arc<Formatter> = Arc::new(move |record| formatter.format(record));
        for mut handler in logger.get_handlers() {
            handler.set_formatter(formatter.clone());
        }
        Ok(())
    }
}
fn register_formatter_tap(app: &Application) -> ProviderResult {
    let container = app.container();
    container.bind::<FormatterLabel, _>(|container| {
        let config = container.get::<dyn openrust::contracts::config::Repository>("config")?;
        let label = config.get("app.name").and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "App".into());
        Ok(Arc::new(FormatterLabel(label)))
    })?;
    container.register_class(ClassDescriptor::concrete::<CustomizeFormatter, _>(
        vec![ConstructorParam::class::<FormatterLabel>("label")],
        |arguments| Ok(Arc::new(CustomizeFormatter { label: arguments.service(0)? })),
    )?)?;
    let owner = Arc::downgrade(&Container::get_instance());
    container.make::<LogManager>()?.register_tap("customize", move |logger, arguments| {
        owner.upgrade().ok_or(LogError::InvalidConfiguration)?
            .make::<CustomizeFormatter>()?.invoke(logger, arguments)
    });
    Ok(())
}
```

`openrust::log::formatters::LineFormatter` and `formatter: "line"` accept the `%datetime%`, `%channel%`, `%level_name%`, `%level%`, `%message%`, `%context%`, `%extra%` and individual `%context.key%` / `%extra.key%` tokens. `formatter_with` accepts `format`, `dateFormat`, `allowInlineLineBreaks` and `ignoreEmptyContextAndExtra`; the last two default to `false`. The public setters and `format_batch` use the same implementation. Returned separators are exact. Native errors retain the existing structured snapshots; there is no fabricated PHP Throwable stack or runtime class reflection.

Inside a tap, `get_handlers()` borrows the actual chain through `&mut Logger`; each handle accepts `set_formatter`. Configuration uses copy on write, keeping other shared loggers unchanged and adding no formatter-configuration lock to normal writes. An emergency fallback has no configurable channel chain.

The Log facade also exposes `driver`, `get_default_driver`, `set_default_driver`, `forget_channel` and `extend`. `driver(None)` selects the current default, `forget_channel(None)` invalidates that channel, and an already-returned logger keeps its existing output target. These operations require an active application scope.

## Handlers With Their Own Formatter {#formattable-handlers}

For a custom text handler, implement `FormattableHandler` and register it with `LogManager::register_formattable_handler` in a provider. This uses the same handler registry and write pipeline as other channels.

```rust
use openrust::log::{FormattableHandler, Formatter, LogManager, LogRecord, LogResult};
use std::{io::Write as _, sync::Arc};

struct PrefixHandler { prefix: String }
impl FormattableHandler for PrefixHandler {
    fn get_default_formatter(&self) -> LogResult<Arc<Formatter>> {
        let prefix = self.prefix.clone();
        Ok(Arc::new(move |record| Ok(format!("{prefix}:{}\n", record.message()))))
    }
    fn write(&self, _record: &LogRecord<'_>, formatted: &str) -> LogResult<()> {
        std::io::stderr().lock().write_all(formatted.as_bytes())?;
        Ok(())
    }
}
fn register_own_formatter(manager: &LogManager) {
    manager.register_formattable_handler("own_text", |arguments| {
        let prefix = arguments.get("prefix").and_then(|v| v.as_str())
            .unwrap_or("APP").to_owned();
        Ok(Arc::new(PrefixHandler { prefix }))
    });
}
```

Select `handler: "own_text"` with `driver: "monolog"`, `handler_with: {"prefix":"APP"}` and `formatter: "default"`. Omitting `formatter` uses the framework format; choosing `line`, `html` or a registered formatter overrides the handler’s default. Ordinary `register_handler` callbacks remain supported and use the standard line formatter when `default` is selected.

The default getter runs only when the first accepted record needs it. A failed getter remains retryable on the same channel; a successful formatter is cached. Native calls return the original error chain and framework writes retain emergency recovery. No registration lock is held around user code. Concurrent first accesses may invoke the getter more than once, with one successful formatter retained, so keep getters idempotent. This capability produces text; it does not claim support for arbitrary PHP extension or structured APM formatter types.

The core does not include a New Relic APM agent. A generic handler callback is not a built-in integration with that supplier; its native SDK and structured APM adapter must be implemented separately.
