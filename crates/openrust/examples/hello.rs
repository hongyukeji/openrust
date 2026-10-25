//! Smallest possible OpenRust application. Run with `cargo run -p openrust --example hello -- help`.

use openrust::http::HttpPlugin;
use openrust::prelude::*;

struct Hello;

impl Application for Hello {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        app.plugin(HttpPlugin).routes(|r| r.get("/", hello));
    }
}

async fn hello() -> Json<&'static str> {
    Json("Hello from OpenRust")
}

fn main() -> std::process::ExitCode {
    openrust::run::<Hello>()
}
