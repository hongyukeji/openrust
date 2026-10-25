//! Smallest possible OpenRust application. Run with `cargo run -p openrust --example hello -- help`.

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
