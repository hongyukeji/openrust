# Concurrency

## Running concurrent tasks {#running-concurrent-tasks}

Use `Concurrency` from `openrust::support::facades` inside an application. Register typed handlers in an application provider and send tasks with `Task::new(name, input)`. The default `process` driver starts the application executable through a hidden Artisan command for each task. Register the same provider in the entry point that serves HTTP and Artisan.

`run` returns results in submission order. `runNamed` takes an `IndexMap` and keeps its keys and order. The process driver accepts an optional `Duration` timeout for each child. Task errors are returned after the children are collected. Dropping an in-flight process run cancels its children. The `sync` driver runs tasks sequentially in the current process. Set `concurrency.default` to select the default driver.

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

Inside an HTTP handler, call `Concurrency::defer(tasks)?` to schedule work after the response. Responses below 400 execute the task list. Use `always()` on the returned handle to include error responses. Deferred tasks carry the request Context and failures are reported through the application exception handler.
