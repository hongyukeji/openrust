use std::error::Error;

use openrust::foundation::{Application, configuration::Routing};

use crate::{app::console::commands, config, routes};

#[allow(non_snake_case, reason = "Preserves application assembly names")]
pub(crate) fn createApplication() -> Result<Application, Box<dyn Error + Send + Sync>> {
    let basePath = Application::inferBasePath(
        (!cfg!(feature = "standalone")).then_some(env!("CARGO_MANIFEST_DIR")),
    )?;
    Application::configure(basePath)
        .withConfig(config::load)
        .withProviders(super::providers::providers())
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
                .shouldRenderJsonWhen(|request, _| request.is("api/*") || request.expectsJson());
        })
        .create()
        .map_err(Into::into)
}
