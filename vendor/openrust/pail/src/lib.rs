//! Pail session capture for OpenRust application logs.

pub mod console;
pub mod contracts;
pub mod printers;

mod file;
mod files;
mod handler;
mod options;
mod process_factory;
pub mod value_objects;

pub use file::File;
pub use files::Files;
pub use handler::Handler;
pub use options::Options;
pub use process_factory::{ProcessFactory, ProcessOutcome};

use openrust::{
    config::Environment,
    contracts::{
        container::{BindingResolutionError, Container as _},
        events::Dispatcher as _,
        foundation::Application as _,
    },
    events::{Delivery, Dispatcher},
    foundation::Application,
    http::HttpFuture,
    log::{LogError, LogManager},
    queue::events::{JobAttempted, JobExceptionOccurred, JobProcessed, JobProcessing},
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;

/// Registers Pail capture against the application's logging lifecycle.
pub struct PailServiceProvider;

impl ServiceProvider for PailServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().singleton::<Files, _>(|container| {
                Ok(Arc::new(Files::new(
                    container
                        .make::<openrust::foundation::Paths>()?
                        .storagePath("pail"),
                )))
            })?;
            let container = openrust::container::Container::getInstance();
            let owner = Arc::downgrade(&container);
            app.container().singleton::<Handler, _>(move |container| {
                Ok(Arc::new(Handler::new(
                    container.make::<Files>()?,
                    owner.clone(),
                )))
            })?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let environment = app.make::<Environment>().await?;
            if !self.runningPailTests(&environment)?
                && (app.runningUnitTests()? || truthy(environment.get_utf8("VAPOR_SSM_PATH")?))
            {
                return Ok(());
            }
            let container = openrust::container::Container::getInstance();
            let owner = Arc::downgrade(&container);
            let logging_owner = owner.clone();
            let request_owner = owner.clone();
            app.make::<LogManager>().await?.listenForRequest(
                move |event| {
                    let container = logging_owner
                        .upgrade()
                        .ok_or(LogError::Unavailable(BindingResolutionError::ScopeClosed))?;
                    container.make::<Handler>()?.log(event)
                },
                Arc::new(move |event, request| {
                    let owner = request_owner.clone();
                    Box::pin(async move {
                        let container = owner
                            .upgrade()
                            .ok_or(LogError::Unavailable(BindingResolutionError::ScopeClosed))?;
                        container
                            .make::<Handler>()?
                            .logWithRequest(event, request)
                            .await
                    })
                }),
            )?;
            let events = app.make::<Dispatcher>().await?;
            let observer = Arc::new(handler::QueueLifecycle(owner));
            for name in [
                JobProcessing::NAME,
                JobExceptionOccurred::NAME,
                JobProcessed::NAME,
                JobAttempted::NAME,
            ] {
                events.listen(name, observer.clone(), Delivery::Sync)?;
            }
            self.commands(app, console::commands::register);
            Ok(())
        })
    }
}

impl PailServiceProvider {
    #[allow(non_snake_case, reason = "Matches Laravel Pail's provider helper")]
    // Env::get normalizes reserved values before PHP casts PAIL_TESTS to bool.
    fn runningPailTests(
        &self,
        environment: &Environment,
    ) -> Result<bool, openrust::config::ConfigError> {
        let Some(value) = environment.get_utf8("PAIL_TESTS")? else {
            return Ok(false);
        };
        Ok(match value.to_ascii_lowercase().as_str() {
            "true" | "(true)" => true,
            "false" | "(false)" | "empty" | "(empty)" | "null" | "(null)" => false,
            _ => truthy(Some(
                ['\'', '"']
                    .into_iter()
                    .find_map(|quote| {
                        value
                            .strip_prefix(quote)
                            .and_then(|inner| inner.strip_suffix(quote))
                            .filter(|inner| !inner.contains('\n'))
                    })
                    .unwrap_or(value),
            )),
        })
    }
}

fn truthy(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(PailServiceProvider)]
}
