//! Application entry point.

mod app;
mod routes;
{{database_module}}

fn main() -> std::process::ExitCode {
    openrust::run::<app::{{type_name}}>()
}
