//! Smallest possible OpenRust application. Run with `cargo run -p openrust --example hello -- help`.

use openrust::prelude::*;

struct Hello;

impl Application for Hello {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        app.routes(|r| r.get("/", hello));
    }
}

async fn hello() -> &'static str {
    "Hello from OpenRust"
}

fn main() -> std::process::ExitCode {
    openrust::run::<Hello>()
}
