use std::{error::Error, sync::Arc};

use openrust::foundation::{Application, configuration::Routing};

use crate::{app::console::commands, config, database, routes};

pub(crate) fn create_application() -> Result<Application, Box<dyn Error + Send + Sync>> {
    #[allow(non_snake_case, reason = "Preserves Laravel's basePath spelling")]
    let basePath = Application::inferBasePath(
        (!cfg!(feature = "standalone")).then_some(env!("CARGO_MANIFEST_DIR")),
    )?;
    Application::configure(basePath)
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
        .withMiddleware(|middleware| {
            middleware.append(Arc::new(crate::app::http::middleware::SetLocale));
        })
        .withExceptions(|exceptions| {
            exceptions
                .shouldRenderJsonWhen(|request, _| request.isApiPath() || request.expectsJson());
        })
        .create()
        .map_err(Into::into)
}
