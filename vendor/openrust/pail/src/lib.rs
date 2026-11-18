//! Pail session capture for OpenRust application logs.

mod file;
mod files;
mod handler;

pub use file::File;
pub use files::Files;
pub use handler::Handler;

use openrust::{
    contracts::{container::Container as _, events::Dispatcher as _, foundation::Application as _},
    events::{Delivery, Dispatcher},
    foundation::Application,
    http::HttpFuture,
    log::LogManager,
    queue::events::{JobAttempted, JobExceptionOccurred, JobProcessed, JobProcessing},
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;

/// Registers Pail capture against the application's logging lifecycle.
pub struct PailServiceProvider;

impl ServiceProvider for PailServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let files = Arc::new(Files::new(app.base_path().join("storage/pail")));
            app.container().instance(files.clone())?;
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
            let handler = app.make::<Handler>().await?;
            app.make::<LogManager>()
                .await?
                .listen(move |event| handler.log(event))?;
            let events = app.make::<Dispatcher>().await?;
            let observer = Arc::new(handler::QueueLifecycle);
            for name in [
                JobProcessing::NAME,
                JobExceptionOccurred::NAME,
                JobProcessed::NAME,
                JobAttempted::NAME,
            ] {
                events.listen(name, observer.clone(), Delivery::Sync)?;
            }
            Ok(())
        })
    }
}

pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(PailServiceProvider)]
}
