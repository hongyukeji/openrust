use std::error::Error;

use openrust::foundation::{Application, configuration::Routing};

use crate::{app::console::commands, config, database, routes};

pub(crate) fn create_application() -> Result<Application, Box<dyn Error + Send + Sync>> {
    let base_path = Application::infer_base_path(
        (!cfg!(feature = "standalone")).then_some(env!("CARGO_MANIFEST_DIR")),
    )?;
    Application::configure(base_path)
        .with_config(config::load)
        .with_packages(super::packages::manifest())
        .with_providers(super::providers::providers())
        .with_migrations(database::migrations::entries())
        .with_seeders(database::seeders::entries())
        .with_commands(commands::register)
        .with_routing(
            Routing::default()
                .web(routes::web::register)
                .api(routes::api::register)
                .commands(routes::console::register)
                .health("/up"),
        )
        .with_middleware(|_middleware| {
            //
        })
        .with_exceptions(|exceptions| {
            exceptions.should_render_json_when(|request, _| {
                request.is_api_path() || request.expects_json()
            });
        })
        .create()
        .map_err(Into::into)
}
