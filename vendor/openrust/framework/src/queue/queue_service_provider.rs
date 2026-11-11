use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::{config::Repository, container::Container as _},
    foundation::Application,
    support::{ProviderResult, ServiceProvider},
};

use super::{
    QueueFuture, QueueManager,
    failed::{DatabaseUuidFailedJobProvider, FailedJobProviderInterface, NullFailedJobProvider},
};

/// Registers the queue manager in the application container.
pub struct QueueServiceProvider;

impl ServiceProvider for QueueServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> QueueFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container_handle();
            let owner = Arc::downgrade(&container);
            container.singletonNamed("queue", move |container| {
                let config = container.get::<dyn Repository>("config")?;
                Ok(Arc::new(QueueManager::new(config, owner.clone())))
            })?;
            container.alias("queue", ServiceKey::typed::<QueueManager>())?;
            container.singletonNamed::<dyn FailedJobProviderInterface, _>(
                "queue.failer",
                |container| {
                    let config = container.get::<dyn Repository>("config")?;
                    let driver = config.get("queue.failed.driver");
                    let driver = driver
                        .as_ref()
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("null");
                    if driver == "null" {
                        return Ok(
                            Arc::new(NullFailedJobProvider) as Arc<dyn FailedJobProviderInterface>
                        );
                    }
                    if driver != "database-uuids" {
                        return Err(failer_error(super::QueueError::UnsupportedDriver(
                            driver.into(),
                        )));
                    }
                    let manager = container.make::<crate::database::DatabaseManager>()?;
                    let name = config.get("queue.failed.database");
                    let connection = manager
                        .connection(name.as_ref().and_then(serde_json::Value::as_str))
                        .map_err(failer_error)?;
                    let table = config
                        .get("queue.failed.table")
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .unwrap_or_else(|| "failed_jobs".into());
                    Ok(Arc::new(
                        DatabaseUuidFailedJobProvider::new(connection, table)
                            .map_err(failer_error)?,
                    ))
                },
            )?;
            Ok(())
        })
    }
}

fn failer_error(
    source: impl std::error::Error + Send + Sync + 'static,
) -> crate::contracts::container::BindingResolutionError {
    crate::contracts::container::BindingResolutionError::FactoryFailed {
        service: "queue.failer",
        source: Box::new(source),
    }
}
