# openrust

Facade crate of the [OpenRust](https://github.com/hongyukeji/openrust) framework. Applications depend on this crate only; it re-exports the kernel (`openrust-core`), the HTTP layer (`openrust-http`) and, behind Cargo features, every other capability.

```rust,no_run
use openrust::prelude::*;

struct App;

impl Application for App {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        app.routes(|r| r.get("/", hello));
    }
}

async fn hello() -> &'static str {
    "Hello from OpenRust"
}

fn main() -> std::process::ExitCode {
    openrust::run::<App>()
}
```

`openrust::run` loads configuration, installs tracing, boots providers and dispatches the first command-line argument as a command (`serve` when absent), so the application binary doubles as its own CLI: `my-app serve`, `my-app config:show`, `my-app help`.
