# Logging

## Configuration

Configure the default channel with `LOG_CHANNEL`, the ordered members of the default stack with `LOG_STACK`, and the minimum severity with `LOG_LEVEL`. The default `stack` contains `single`, which appends to `storage/logs/openrust.log`. Relative paths are resolved from the application root. A channel's optional `name` changes the label on each record. The application environment is the default label.

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

Set `driver: "syslog"` to send records to a local Unix datagram socket. The channel uses `/var/run/syslog` on macOS and `/dev/log` on Linux by default. Set `socket` to use another receiver. Relative socket paths resolve from the application root. If delivery fails, the same channel can retry on the next record and the failed write follows the emergency log path.

The `facility` defaults to `user`. Use a name such as `local4` or its numeric syslog constant. The wire priority combines the facility and record severity. The tag comes from the application name and process ID. With no formatter specified, the payload uses the framework line format. Set `formatter: "default"` to omit the payload timestamp, or choose a registered formatter. Platforms without Unix datagrams use the emergency log.

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
