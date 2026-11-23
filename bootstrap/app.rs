use std::error::Error;

use openrust::foundation::{Application, configuration::Routing};

use crate::{app::console::commands, config, database, routes};

pub(crate) fn create_application() -> Result<Application, Box<dyn Error + Send + Sync>> {
    let base_path = Application::inferBasePath(
        (!cfg!(feature = "standalone")).then_some(env!("CARGO_MANIFEST_DIR")),
    )?;
    Application::configure(base_path)
        .withConfig(config::load)
        .withPackages(super::packages::manifest())
        .withProviders(super::providers::providers())
        .withMigrations(database::migrations::entries())
        .withSeeders(database::seeders::entries())
        .withCommands(commands::register)
        .withRouting(
            Routing::default()
                .web(routes::web::register)
                .commands(routes::console::register)
                .health("/up"),
        )
        .withMiddleware(|_middleware| {
            //
        })
        .withExceptions(|exceptions| {
            exceptions
                .shouldRenderJsonWhen(|request, _| request.isApiPath() || request.expectsJson());
        })
        .create()
        .map_err(Into::into)
}
