# Concurrency

## Running concurrent tasks {#running-concurrent-tasks}

Use `Concurrency` from `openrust::support::facades` inside an application. Register typed handlers in an application provider and send tasks with `Task::new(name, input)`. The default `process` driver starts the application executable through a hidden Artisan command for each task. Register the same provider in the entry point that serves HTTP and Artisan.

`run` returns results in submission order. `runNamed` takes an `IndexMap` and keeps its keys and order. The process driver accepts an optional `Duration` timeout for each child. Task errors are returned after the children are collected. Dropping an in-flight process run cancels its children. The `sync` driver runs tasks sequentially in the current process. Set `CONCURRENCY_DRIVER` or the `default` value in `config/concurrency.rs` to select the default driver. Process tasks use the application root and the environment file selected by Artisan's `--env` option. OpenRust uses managed processes for parallel execution.

```rust
use openrust::{
    concurrency::{Manager, Registry, Task},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

struct Tasks;

impl ServiceProvider for Tasks {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().make::<Registry>()?.register("math:double", |value: u64| async move {
                Ok::<_, std::convert::Infallible>(value * 2)
            })?;
            Ok(())
        })
    }
}

let mut app = Application::configure(std::env::current_dir()?)
    .withProviders(vec![Box::new(Tasks)])
    .create()?;
app.boot().await?;

let manager = app.container().make::<Manager>()?;
let result = manager.driver(Some("sync"))?
    .run([Task::new("math:double", 21)?, Task::new("math:double", 6)?], None)
    .await?;
assert_eq!(result, vec![serde_json::json!(42), serde_json::json!(12)]);
```

## Deferring concurrent tasks {#deferring-concurrent-tasks}

Call `Concurrency::defer(tasks)?` to register tasks whose results you do not need. The application runs them at the end of the current lifecycle:

- **HTTP** runs deferred tasks after sending a response with a status below 400.
- **Artisan** runs them after a command returns exit status zero.
- **Queue workers** run them after `JobAttempted` when the attempt completes without failure or an unhandled exception. An explicit release without an exception counts as a successful attempt. Jobs on the `sync` and `deferred` queue connections follow the enclosing HTTP or Artisan lifecycle.

Call `always()` on the returned handle to include error responses, unsuccessful commands and failed job attempts that reach their completion boundary. A job process killed before returning its outcome loses tasks registered in that process. Once a hard-timeout job returns its outcome, the parent worker runs the deferred tasks outside the job's time limit.

The `process` driver starts one independent Artisan child per task at the lifecycle boundary. The initiating process can exit once the children are launched, even while tasks are running. Child input is sent through stdin. The parent reports launch failures, and each child reports its own task failure through the application's exception handler. If you need results or want to wait for completion, call `run`.

The `sync` driver runs the tasks in submission order in the current process before that lifecycle finishes.

Tasks receive the public and hidden Context visible when their deferred callback runs. An isolated job captures its final Context when handing its callbacks to the parent worker, which starts them after `JobAttempted`. Changes made after registration and before either boundary are included.
