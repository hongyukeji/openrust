use std::{
    ffi::OsString,
    sync::{Arc, Weak},
    time::Duration,
};

use indexmap::IndexMap;
use serde_json::Value;

use crate::{
    container::{Container, ServiceKey},
    contracts::{
        config::Repository,
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    foundation::Application,
    http::HttpFuture,
    log::context::{Context, DehydratedContext},
    process::Factory,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Facade, current_container, isolated_scope},
    },
};

use super::{DeferredCallback, Error, ProcessDriver, Registry, Result, Task, deferred};

#[derive(Clone, Copy)]
enum Kind {
    Process,
    Sync,
}

/// Chooses the application's concurrency driver.
pub struct Manager {
    config: Arc<dyn Repository>,
    registry: Arc<Registry>,
    process: ProcessDriver,
    owner: Weak<Container>,
}

impl Manager {
    pub fn driver(&self, name: Option<&str>) -> Result<Driver> {
        let configured = self.config.get("concurrency.default");
        let name = name
            .or_else(|| configured.as_ref().and_then(Value::as_str))
            .unwrap_or("process");
        let kind = match name {
            "process" => Kind::Process,
            "sync" => Kind::Sync,
            _ => return Err(Error::UnknownDriver(name.to_owned())),
        };
        Ok(Driver {
            kind,
            registry: self.registry.clone(),
            process: self.process.clone(),
            owner: self.owner.clone(),
        })
    }
}

/// The selected process or sequential execution driver.
#[derive(Clone)]
pub struct Driver {
    kind: Kind,
    registry: Arc<Registry>,
    process: ProcessDriver,
    owner: Weak<Container>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Concurrency facade")]
impl Driver {
    pub async fn run(
        &self,
        tasks: impl IntoIterator<Item = Task>,
        timeout: Option<Duration>,
    ) -> Result<Vec<Value>> {
        Ok(self
            .runNamed(numbered(tasks), timeout)
            .await?
            .into_values()
            .collect())
    }

    pub async fn runNamed(
        &self,
        tasks: IndexMap<String, Task>,
        timeout: Option<Duration>,
    ) -> Result<IndexMap<String, Value>> {
        let context = match self.kind {
            Kind::Process => self.capture_context()?,
            Kind::Sync => DehydratedContext::default(),
        };
        self.execute(tasks, timeout, context).await
    }

    pub fn defer(&self, tasks: impl IntoIterator<Item = Task>) -> Result<DeferredCallback> {
        let driver = self.clone();
        let tasks = numbered(tasks);
        let context = self.capture_context()?;
        deferred::enqueue(Box::pin(async move {
            if let Kind::Sync = driver.kind
                && let Some(owner) = driver.owner.upgrade()
            {
                return isolated_scope(owner, async move {
                    let repository = Context::repository().map_err(context_error)?;
                    Context::hooks()
                        .map_err(context_error)?
                        .hydrate(&context, &repository);
                    driver.execute(tasks, None, context).await.map(|_| ())
                })
                .await;
            }
            driver.execute(tasks, None, context).await.map(|_| ())
        }))
    }

    fn capture_context(&self) -> Result<DehydratedContext> {
        if let Some((owner, active)) = self.owner.upgrade().zip(current_container())
            && Arc::ptr_eq(&owner, &active)
        {
            let context = Context::repository().map_err(context_error)?;
            Ok(Context::hooks().map_err(context_error)?.dehydrate(&context))
        } else {
            Ok(DehydratedContext::default())
        }
    }

    async fn execute(
        &self,
        tasks: IndexMap<String, Task>,
        timeout: Option<Duration>,
        context: DehydratedContext,
    ) -> Result<IndexMap<String, Value>> {
        match self.kind {
            Kind::Process => self.process.run_with_context(tasks, timeout, context).await,
            Kind::Sync => {
                let mut results = IndexMap::with_capacity(tasks.len());
                for (key, task) in tasks {
                    results.insert(key, self.registry.invoke(task).await?);
                }
                Ok(results)
            }
        }
    }
}

fn numbered(tasks: impl IntoIterator<Item = Task>) -> IndexMap<String, Task> {
    tasks
        .into_iter()
        .enumerate()
        .map(|(index, task)| (index.to_string(), task))
        .collect()
}

fn context_error(error: crate::log::context::ContextError) -> Error {
    Error::Io(std::io::Error::other(error))
}

/// Resolves the current application's concurrency manager.
pub struct Concurrency;

impl Facade for Concurrency {
    type Service = Manager;

    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Manager>()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Concurrency facade")]
impl Concurrency {
    pub fn driver(name: Option<&str>) -> Result<Driver> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .driver(name)
    }

    pub async fn run(
        tasks: impl IntoIterator<Item = Task>,
        timeout: Option<Duration>,
    ) -> Result<Vec<Value>> {
        Self::driver(None)?.run(tasks, timeout).await
    }

    pub async fn runNamed(
        tasks: IndexMap<String, Task>,
        timeout: Option<Duration>,
    ) -> Result<IndexMap<String, Value>> {
        Self::driver(None)?.runNamed(tasks, timeout).await
    }

    pub fn defer(tasks: impl IntoIterator<Item = Task>) -> Result<DeferredCallback> {
        Self::driver(None)?.defer(tasks)
    }
}

pub(crate) struct ConcurrencyServiceProvider;

impl ServiceProvider for ConcurrencyServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container_handle();
            let base_path = app.base_path().to_owned();
            let owner = Arc::downgrade(&container);
            container.singleton::<Registry, _>(|_| Ok(Arc::new(Registry::default())))?;
            container.singleton::<Manager, _>(move |container| {
                let executable = std::env::current_exe().map_err(|source| {
                    BindingResolutionError::FactoryFailed {
                        service: "concurrency",
                        source: Box::new(source),
                    }
                })?;
                let command = [
                    executable.into_os_string(),
                    OsString::from("artisan"),
                    OsString::from(super::protocol::COMMAND),
                ];
                let process = ProcessDriver::new(
                    container.make::<Factory>()?.as_ref().clone(),
                    command,
                    base_path.clone(),
                );
                Ok(Arc::new(Manager {
                    config: container.get::<dyn Repository>("config")?,
                    registry: container.make::<Registry>()?,
                    process,
                    owner: owner.clone(),
                }))
            })?;
            Ok(())
        })
    }
}
