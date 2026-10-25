//! Application entry point.

mod app;
mod routes;

fn main() -> std::process::ExitCode {
    openrust::run::<app::{{type_name}}>()
}
