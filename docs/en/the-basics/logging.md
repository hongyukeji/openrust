# Logging

The Slack channel’s default username is the configured application name followed by ` Log`, such as `ExampleStore Log`. Set `LOG_SLACK_USERNAME` in `.env` to override it.

## Configuration

Configure the default channel with `LOG_CHANNEL`, the ordered members of the default stack with `LOG_STACK`, and the minimum severity with `LOG_LEVEL`. The default `stack` contains `single`, which appends to `storage/logs/app.log`. Relative paths are resolved from the application root. A channel's optional `name` changes the label on each record. The application environment is the default label.

The implemented drivers are `single` for files, `stderr` for standard error, `null` to discard records, and `stack` to combine channels. A file is opened when a record meets the severity threshold. The `single` driver accepts `locking`, `bubble`, and an integer `permission` such as Rust's `0o600`. Permission applies on Unix. Records use UTC timestamps.

## Building Log Stacks

Set `channels` to a list or comma-separated string. Members run in order. A handler with `bubble: false` stops the stack after accepting a record. The `null` handler discards an accepted record and stops the stack. With `ignore_exceptions: true`, the group attempts every handler and suppresses write failures.

Use `Log::stack(&["single", "stderr"], None)` for a stack scoped to the returned logger. Named channels are resolved lazily and cached per application. After changing a named channel's configuration, call `forgetChannel(Some(name))` to resolve it again.

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

Use `daily` or `monthly` for files such as `openrust-2026-11-11.log` and `openrust-2026-11.log`. The first accepted record of a UTC period opens its file. Set `max_files` to the number of files to keep, including the active file, or set it to zero for unlimited retention. The application defaults retain 14 daily files and three monthly files. Retention only considers regular files with the same base name, extension and period format.

## On-Demand and Custom Channels

`Log::build(&settings)?` creates an on-demand logger. Use `Log::stackChannels` with `StackChannel::Named` and `StackChannel::Logger` to combine configured channels and on-demand loggers. The resulting stack has its own label.

For `driver: "custom"`, set `via` to the name of a factory registered with `LogManager::registerFactory`. Use `LogManager::extend` to register a named driver. Factories are compiled Rust closures that receive the manager and channel configuration and return a logger implementing the logging contract. They can run concurrently on the first lookup, so keep initialization safe to repeat. Successful named lookups share the published logger. Factory failures go to the emergency logger and can be retried.

```rust
use openrust::{config::Repository, log::{LogContext, LogManager, StackChannel}};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let manager = LogManager::new(Arc::new(Repository::new(Default::default())), root);
    manager.registerFactory("audit", |manager, config| {
        let logger = manager.build(&json!({"driver":"monthly", "path":config["path"]}))?;
        Ok(logger)
    });
    let audit = manager.build(&json!({"driver":"custom", "via":"audit", "path":"audit.log"}))?;
    let stack = manager.stackChannels(
        &[StackChannel::Named("single"), StackChannel::Logger(&audit)],
        Some("orders"),
    )?;
    stack.info("Audit ready", &LogContext::new())?;
    Ok(())
}
```

## Customizing Channel Records {#customizing-channel-records}

Register names with `LogManager::registerFormatter`, `registerProcessor`, and `registerTap` in your service provider. Select a formatter with `formatter` and pass its settings in `formatter_with`. Use `formatter: "default"` for the built-in format. Formatters write the exact bytes returned, including any record separator, for file, rotating file, and standard error handlers. Custom logger factories control their own output.

Declare `processors` as names or objects with `processor` and `with`. Processors run in configuration order before formatting. They can edit the message, record context, and public scope metadata through `LogRecord`. A stack collects its members’ processors in member order. Each member retains its own formatter.

A tap such as `"audit:orders,prod"` receives the logger and the two arguments. Taps run in list order before the channel is cached. Use `setFormatter` to replace the channel formatter or `pushProcessor` to prepend a processor.

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
    manager.registerFormatter("message", |_| {
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

Use `driver: "monolog"` with a named Rust handler. The built-in `stream` handler accepts paths, `stderr`, and `stdout`. To supply another handler, use `LogManager::registerHandler`. Its factory receives the merged settings and returns a callback that receives the structured record and formatted text. Settings are merged in order from the channel `level`, `with`, then `handler_with`. Later values take precedence.

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
    assert!(packet.ends_with("Import complete"));
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

Pass an error to `Log::logException(level, message, &error, &context)` to include `context.exception` and its `previous` chain. Register `exceptionContext::<E>` in `withExceptions` to add fields to each matching error type, including sources. The existing `context` callbacks enrich a directly logged root error. When the application reports an error, its root fields appear only in the record context, and source-specific fields remain with their source. Rust can name the directly supplied type and registered source types. A type-erased source can appear as `Error`. Rust's standard error contract does not supply the original file or stack trace.

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
    logger.logException(Level::Error, "Import failed", &error, &LogContext::new())?;
    Ok(())
}
```

## Contextual Information

The public fields captured by the [Context facade](../digging-deeper/context.md) appear after a log record's own fields as separate JSON metadata. Custom sinks can implement `logWithContext` to receive both maps. Hidden fields stay in the context repository.

## Listening for Log Messages

Register `Log::listen` in a service provider's `boot` method. Its callback runs synchronously after an accepted record is written, and returns `LogResult<()>`. `LogManager::listen` and `Logger::listen` use the same application-wide listeners across named channels, stacks, on-demand channels, and renamed loggers.

The `MessageLogged` payload contains the original message, level, and context. Placeholder interpolation and channel processors apply to the written record. `logException` also supplies the normalized `context["exception"]`. Records below every handler's threshold skip notification. Emergency fallback records and its diagnostics notify listeners too.

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
Log::logWithRequest(request, openrust::log::Level::Info, "Order placed", &LogContext::new()).await?;
```

The same method is available on `LogManager` and resolved `Logger` channels. The channel writes first; accepted records then notify observers in registration order. With an active Pail session, Pail awaits the selected guard and captures its ID and email before this call returns. Cached users are reused; login, logout and guard selection affect subsequent logs. No active session, a rejected log level or a suppressed deprecation does not trigger authentication. The built-in HTTP exception reporter uses the request-aware path as well.

This explicit await replaces PHP's ability to resolve a guard inside a synchronous log listener. Container callbacks and other synchronous code keep snapshot semantics. `LogManager::listenForRequest` registers a synchronous observer and its awaited request counterpart; ordinary `listen` callbacks still run synchronously. No listener registry lock is held while callbacks run. Logs produced during guard resolution capture the existing snapshot and do not start recursive authentication.

Authentication or observer errors return through the log result after the ordinary channel write. Cancelling the future may therefore leave a written channel record without a Pail capture. Cancellation releases the request borrow and resolution scope; a later call can retry. Requests and applications keep separate authentication state; programmatic Artisan calls within HTTP retain that HTTP origin, while standalone Artisan uses console origin.

### Reading and Filtering Captures

`openrust_pail::value_objects::MessageLogged::fromJson` reads a capture line. It exposes its message, level, date/time, exception class/file, trace and typed HTTP, console or queue origin. Parsing, date conversion and origin decoding return `Result` where they can fail. `context()` excludes the reserved `__pail`, `exception` and `userId` fields.

`Options::accepts` combines all supplied filters. Level comparison and substring searches fold ASCII case. The message filter searches only message text; the general filter searches the four-field JSON representation, including origin and context. That representation follows PHP's Unicode escaping, object order and float notation, so a general Unicode search may need its `\u` spelling. Authentication IDs compare as exact strings; `12` and `012` differ. `fromCommand(&Input)` preserves `auth` precedence over `user`, including an explicitly empty value, and PHP's timeout conversion. It reads the Rust command's separate input object.

```rust
use openrust_pail::{Options, value_objects::MessageLogged};

fn select(line: &str) -> Result<bool, serde_json::Error> {
    let message = MessageLogged::fromJson(line)?;
    let options = Options::new(3600, None, Some("ERROR".into()), None, Some("payment".into()));
    Ok(options.accepts(&message))
}
```

These parsing and filtering APIs also drive the live [Pail command](#tailing-log-messages-using-pail). Request authentication follows the explicit await and snapshot rules above.

### Provider Availability

Pail still registers Files and Handler when capture is disabled. During boot it skips listeners if the bound application environment is exactly `testing`, or `VAPOR_SSM_PATH` is a nonempty string other than `0`. `PAIL_TESTS=true` enables Pail's own acceptance tests and overrides both checks. This flag follows Laravel Env normalization, including false, empty, null and quoted values; the Vapor marker uses raw PHP string truthiness.

Flags come from the current application's immutable environment snapshot. Process variables take precedence over the selected environment file. With configuration cached, environment files are not loaded, so a file-only `PAIL_TESTS` does not apply. Handler is resolved on the first accepted log or queue lifecycle event. Its construction error propagates then, after the original accepted log has been written.

Pail resolves `Application::storagePath("pail")` through the shared path service. Configure `useStoragePath` or `APP_STORAGE_PATH` before the first capture. See [Application Storage Path](../getting-started/configuration.md#application-storage-path).

Default file log channels and the emergency fallback follow the application storage path. Configure the path before loading configuration or resolving channels. Loaded channel paths retain their configured values; see [Application Storage Path](../getting-started/configuration.md#application-storage-path).

## Pail CLI Printer {#pail-cli-printer}

The optional Pail package exposes `openrust_pail::contracts::Printer` and `printers::CliPrinter`. In an Artisan command, construct `CliPrinter::new(output, basePath)` and call `print(&messageLogged)?`. It reads the current output's verbose and decoration flags. Normal output shows the time and abbreviated class/file/message; `-v` shows the date and full text, and `-vv` or `-vvv` adds captured trace frames.

HTTP method/path and authentication, queue/job/command, and public context appear in the footer. Numeric/null context entries, PHP-style value exports, literal HTML text and multiline messages follow the pinned Pail rendering rules. The frame is limited to 150 terminal columns; source messages and long footers can overflow as in upstream. Rust `::` class namespaces and native Windows path separators are adapted for abbreviation and relative source display.

Rust JSON unsigned integers retain their original precision; values beyond PHP’s signed integer range can therefore display differently from PHP’s floating-point approximation. Rust renders the frame directly and uses `Output::writeRaw` for already-rendered text. Real Artisan output streams immediately; captured output stays buffered. `--ansi`, `--no-ansi`, `--quiet` and `--silent` apply, and write failures propagate. Formatting errors return before the frame is written. Output verbosity is scoped to the invocation and restored after nested calls. `PAIL_TESTS` controls capture testing; displayed dates, files and trace frames retain their recorded values rather than PHP's test fixture replacements.

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

Use `withContext` and `withoutContext` for channel context. `shareContext` merges context into resolved and future channels; per-message fields override channel fields. `sharedContext` reads the defaults for future channels. `flushSharedContext` clears those defaults without deleting fields already merged into channels; use `withoutContext` to remove channel fields. Boot-time defaults are inherited by each request or queue scope. Dynamic context is isolated by scope, even when channel handles are reused.


## Inspecting resolved channels

`LogManager::getChannels()` and `Log::getChannels()` return a snapshot mapping resolved channel names to their `Arc<Logger>` handles. Reading the map does not create unused channels. Removing a channel with `forgetChannel` affects subsequent resolutions; previously returned snapshots and logger handles remain usable.

## Accessing the Native Logger

`Logger::getLogger()` returns the channel's actual `Arc<NativeLogger>` handle. `LogManager::getLogger()` and `Log::getLogger()` resolve the default channel and return the same native instance through `Result`. Reading an already resolved handle does not create other channels, and retained handles remain usable after `forgetChannel`.

```rust
use openrust::{log::LogContext, support::facades::Log};

let channel = Log::channel(None)?;
let native = channel.getLogger();
native.info("Worker started", &LogContext::new())?;
let default_native = Log::getLogger()?;
```

Native writes use the configured severity filters, processors, formatters and output handlers. They receive only the supplied context and bypass application channel context, Context metadata, MessageLogged observers and Pail capture. Native write or processor failures return an error directly. Use the channel or Log facade's level methods for application logging with context, events and emergency recovery. The native handle implements the existing Rust log contract; configure shared channel processors and formatters through the usual registration and tap interfaces.
