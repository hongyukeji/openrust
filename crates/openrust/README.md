# openrust

Facade crate of the [OpenRust](https://github.com/hongyukeji/openrust) framework. Applications depend on this crate only; it re-exports the kernel (`openrust-core`), the HTTP layer (`openrust-http`) and, behind Cargo features, every other capability.

The facade also re-exports `tokio` and `tower::ServiceExt` so an application with only an `openrust` dependency can exercise its HTTP router in an integration test.

```rust,no_run
use openrust::prelude::*;
use openrust::http::HttpPlugin;

struct App;

impl Application for App {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        app.plugin(HttpPlugin).routes(|r| r.get("/", hello));
    }
}

async fn hello() -> &'static str {
    "Hello from OpenRust"
}

fn main() -> std::process::ExitCode {
    openrust::run::<App>()
}
```

`openrust::run` loads configuration, installs tracing, builds and finishes the application's plugins, and dispatches the first command-line argument as a command (`serve` when absent). Applications install `HttpPlugin` to provide `serve`, `/health` and HTTP routing. The application binary doubles as its own CLI: `my-app serve`, `my-app config show`, `my-app help`.

The facade re-exports the resource controller macro as `#[openrust::resource]`. Apply it to an inherent controller `impl` containing `list`, `get`, `create`, `update` and/or `delete` associated handlers to implement `Resource`.

The prelude exports OpenRust `Json<T>`, `Path<T>` and `Query<T>` extractors. They render parse failures as `ApiResponse<T>` JSON envelopes; `Json<T>` is also a response type. Axum's original types remain available through `openrust::http::axum`. For example, `async fn show() -> Json<&'static str> { Json("ok") }` returns JSON.
