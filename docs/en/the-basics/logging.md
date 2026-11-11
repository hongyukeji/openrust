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
