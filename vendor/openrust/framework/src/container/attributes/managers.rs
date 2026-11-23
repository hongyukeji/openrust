use crate::{
    cache::CacheManager, contracts::container::Container as _, database::DatabaseManager,
    filesystem::FilesystemManager, log::LogManager,
};

use super::{
    Arc, AttributeParameter, AttributeValue, BindingResolutionError, CallFuture, Container,
    ContextualAttribute, ParameterValue, Result,
};

/// Selects the default or named cache repository.
#[derive(Default)]
pub struct Cache {
    store: Option<Arc<str>>,
    memo: bool,
}

impl Cache {
    pub fn new(store: impl Into<Arc<str>>) -> Self {
        Self {
            store: Some(store.into()),
            memo: false,
        }
    }

    #[must_use]
    pub fn memoized(mut self) -> Self {
        self.memo = true;
        self
    }

    fn repository(&self, manager: &CacheManager) -> Result<AttributeValue> {
        let name = self.store.as_deref();
        let repository = if self.memo {
            manager.memo(name)
        } else {
            manager.store(name)
        }
        .map_err(|source| failed("cache", source))?;
        Ok(ParameterValue::service(repository).into())
    }
}

impl ContextualAttribute for Cache {
    fn resolve(&self, container: &Container, _: AttributeParameter) -> Result<AttributeValue> {
        self.repository(container.get::<CacheManager>("cache")?.as_ref())
    }

    fn resolve_async<'a>(
        self: Arc<Self>,
        container: &'a Container,
        _: AttributeParameter,
    ) -> CallFuture<'a, AttributeValue> {
        Box::pin(async move {
            self.repository(container.get_async::<CacheManager>("cache").await?.as_ref())
        })
    }
}

/// Selects the default or named database connection.
#[derive(Default)]
pub struct Database(Option<Arc<str>>);

impl Database {
    pub fn new(connection: impl Into<Arc<str>>) -> Self {
        Self(Some(connection.into()))
    }

    fn connection(&self, manager: &DatabaseManager) -> Result<AttributeValue> {
        let connection = manager
            .connection(selected_name(&self.0))
            .map_err(|source| failed("db", source))?;
        Ok(ParameterValue::service(Arc::new(connection)).into())
    }
}

impl ContextualAttribute for Database {
    fn resolve(&self, container: &Container, _: AttributeParameter) -> Result<AttributeValue> {
        self.connection(container.get::<DatabaseManager>("db")?.as_ref())
    }

    fn resolve_async<'a>(
        self: Arc<Self>,
        container: &'a Container,
        _: AttributeParameter,
    ) -> CallFuture<'a, AttributeValue> {
        Box::pin(async move {
            self.connection(container.get_async::<DatabaseManager>("db").await?.as_ref())
        })
    }
}

/// Has the same connection selection as Database and its own attribute identity.
#[derive(Default)]
pub struct Db(Database);

impl Db {
    pub fn new(connection: impl Into<Arc<str>>) -> Self {
        Self(Database::new(connection))
    }
}

impl ContextualAttribute for Db {
    fn resolve(
        &self,
        container: &Container,
        parameter: AttributeParameter,
    ) -> Result<AttributeValue> {
        self.0.resolve(container, parameter)
    }

    fn resolve_async<'a>(
        self: Arc<Self>,
        container: &'a Container,
        _: AttributeParameter,
    ) -> CallFuture<'a, AttributeValue> {
        Box::pin(async move {
            self.0
                .connection(container.get_async::<DatabaseManager>("db").await?.as_ref())
        })
    }
}

/// Selects the default or named filesystem disk.
#[derive(Default)]
pub struct Storage(Option<Arc<str>>);

impl Storage {
    pub fn new(disk: impl Into<Arc<str>>) -> Self {
        Self(Some(disk.into()))
    }

    fn disk(&self, manager: &FilesystemManager) -> Result<AttributeValue> {
        let disk = manager
            .disk(selected_name(&self.0))
            .map_err(|source| failed("filesystem", source))?;
        Ok(ParameterValue::service(disk).into())
    }
}

impl ContextualAttribute for Storage {
    fn resolve(&self, container: &Container, _: AttributeParameter) -> Result<AttributeValue> {
        self.disk(container.get::<FilesystemManager>("filesystem")?.as_ref())
    }

    fn resolve_async<'a>(
        self: Arc<Self>,
        container: &'a Container,
        _: AttributeParameter,
    ) -> CallFuture<'a, AttributeValue> {
        Box::pin(async move {
            self.disk(
                container
                    .get_async::<FilesystemManager>("filesystem")
                    .await?
                    .as_ref(),
            )
        })
    }
}

/// Selects the default or named logger and optionally gives the injected logger its own name.
#[derive(Default)]
pub struct Log {
    channel: Option<Arc<str>>,
    name: Option<Arc<str>>,
}

impl Log {
    pub fn new(channel: impl Into<Arc<str>>) -> Self {
        Self {
            channel: Some(channel.into()),
            name: None,
        }
    }

    #[must_use]
    pub fn named(mut self, name: impl Into<Arc<str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    fn logger(&self, manager: &LogManager) -> Result<AttributeValue> {
        let logger = manager
            .channel(self.channel.as_deref())
            .map_err(|source| failed("log", source))?;
        let logger = match &self.name {
            Some(name) => Arc::new(logger.with_name(name.as_ref())),
            None => logger,
        };
        Ok(ParameterValue::service(logger).into())
    }
}

impl ContextualAttribute for Log {
    fn resolve(&self, container: &Container, _: AttributeParameter) -> Result<AttributeValue> {
        self.logger(container.get::<LogManager>("log")?.as_ref())
    }

    fn resolve_async<'a>(
        self: Arc<Self>,
        container: &'a Container,
        _: AttributeParameter,
    ) -> CallFuture<'a, AttributeValue> {
        Box::pin(
            async move { self.logger(container.get_async::<LogManager>("log").await?.as_ref()) },
        )
    }
}

fn selected_name(name: &Option<Arc<str>>) -> Option<&str> {
    name.as_deref().filter(|name| !matches!(*name, "" | "0"))
}

fn failed(
    service: &'static str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> BindingResolutionError {
    BindingResolutionError::FactoryFailed {
        service,
        source: Box::new(source),
    }
}
