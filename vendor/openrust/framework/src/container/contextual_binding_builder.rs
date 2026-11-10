use super::ServiceKey;
use crate::contracts::container::{BindingResolutionError, Container};
use std::{marker::PhantomData, sync::Arc};

/// Chooses the consumers for a contextual binding.
pub struct ContextualBindingBuilder<'a, C: Container> {
    container: &'a C,
    consumers: Vec<ServiceKey>,
}

/// A typed dependency of the selected consumers.
pub struct ContextualBinding<'a, C: Container, T: ?Sized> {
    builder: ContextualBindingBuilder<'a, C>,
    need: ServiceKey,
    service: PhantomData<fn() -> T>,
}

impl<'a, C: Container> ContextualBindingBuilder<'a, C> {
    pub(crate) fn new(container: &'a C, consumers: Vec<ServiceKey>) -> Self {
        Self {
            container,
            consumers,
        }
    }

    pub fn needs<T: ?Sized + Send + Sync + 'static>(self) -> ContextualBinding<'a, C, T> {
        self.needs_key(ServiceKey::typed::<T>())
    }

    #[allow(non_snake_case, reason = "Named overload for Laravel's needs")]
    pub fn needsNamed<T: ?Sized + Send + Sync + 'static>(
        self,
        name: &str,
    ) -> ContextualBinding<'a, C, T> {
        self.needs_key(ServiceKey::named(name))
    }

    fn needs_key<T: ?Sized>(self, need: ServiceKey) -> ContextualBinding<'a, C, T> {
        ContextualBinding {
            builder: self,
            need,
            service: PhantomData,
        }
    }
}

impl<'a, C: Container, T: ?Sized + Send + Sync + 'static> ContextualBinding<'a, C, T> {
    pub fn give<F>(
        self,
        factory: F,
    ) -> Result<ContextualBindingBuilder<'a, C>, BindingResolutionError>
    where
        F: Fn(&C) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        let factory = Arc::new(factory);
        for consumer in &self.builder.consumers {
            let factory = factory.clone();
            self.builder.container.addContextualBinding(
                consumer.clone(),
                self.need.clone(),
                move |container| factory(container),
            )?;
        }
        Ok(self.builder)
    }

    /// Resolves an explicitly registered implementation using the same output contract.
    #[allow(non_snake_case, reason = "Rust form of Laravel's service-name give")]
    pub fn giveService(
        self,
        service: impl Into<ServiceKey>,
    ) -> Result<ContextualBindingBuilder<'a, C>, BindingResolutionError> {
        let service = service.into();
        self.give(move |container| container.get::<T>(service.clone()))
    }
}

impl<'a, C: Container, T: serde::de::DeserializeOwned + Send + Sync + 'static>
    ContextualBinding<'a, C, T>
{
    /// Reads the current configuration repository at resolution time.
    #[allow(non_snake_case, reason = "Matches Laravel's contextual builder API")]
    pub fn giveConfig(
        self,
        key: impl Into<String>,
        default: Option<serde_json::Value>,
    ) -> Result<ContextualBindingBuilder<'a, C>, BindingResolutionError> {
        let key = key.into();
        self.give(move |container| {
            let config = container.get::<dyn crate::contracts::config::Repository>("config")?;
            let value = config
                .get(&key)
                .unwrap_or_else(|| default.clone().unwrap_or(serde_json::Value::Null));
            serde_json::from_value(value)
                .map(Arc::new)
                .map_err(|source| BindingResolutionError::FactoryFailed {
                    service: std::any::type_name::<T>(),
                    source: Box::new(source),
                })
        })
    }
}

impl<'a, C: Container, T: ?Sized + Send + Sync + 'static> ContextualBinding<'a, C, Vec<Arc<T>>> {
    #[allow(non_snake_case, reason = "Matches Laravel's contextual builder API")]
    pub fn giveTagged(
        self,
        tag: impl Into<String>,
    ) -> Result<ContextualBindingBuilder<'a, C>, BindingResolutionError> {
        let tag = tag.into();
        self.give(move |container| {
            container
                .tagged::<T>(&tag)
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .map(Arc::new)
        })
    }
}

impl<'a, C: Container, T: ?Sized + Send + Sync + 'static> ContextualBinding<'a, C, T> {
    #[allow(non_snake_case, reason = "Async contextual factory adapter")]
    pub fn giveAsync<F>(
        self,
        factory: F,
    ) -> Result<ContextualBindingBuilder<'a, C>, BindingResolutionError>
    where
        F: for<'b> Fn(&'b C) -> super::FactoryFuture<'b, T> + Send + Sync + 'static,
    {
        let factory = Arc::new(factory);
        for consumer in &self.builder.consumers {
            let factory = factory.clone();
            self.builder.container.addContextualBindingAsync(
                consumer.clone(),
                self.need.clone(),
                move |container| factory(container),
            )?;
        }
        Ok(self.builder)
    }
    #[allow(non_snake_case, reason = "Async contextual service adapter")]
    pub fn giveServiceAsync(
        self,
        service: impl Into<ServiceKey>,
    ) -> Result<ContextualBindingBuilder<'a, C>, BindingResolutionError> {
        let service = service.into();
        self.giveAsync(move |container| {
            let service = service.clone();
            Box::pin(async move { container.getAsync(service).await })
        })
    }
}
impl<'a, C: Container, T: serde::de::DeserializeOwned + Send + Sync + 'static>
    ContextualBinding<'a, C, T>
{
    #[allow(non_snake_case, reason = "Async contextual config adapter")]
    pub fn giveConfigAsync(
        self,
        key: impl Into<String>,
        default: Option<serde_json::Value>,
    ) -> Result<ContextualBindingBuilder<'a, C>, BindingResolutionError> {
        let key = key.into();
        self.giveAsync(move |container| {
            let key = key.clone();
            let default = default.clone();
            Box::pin(async move {
                let config = container
                    .getAsync::<dyn crate::contracts::config::Repository>("config")
                    .await?;
                let value = config
                    .get(&key)
                    .unwrap_or_else(|| default.unwrap_or(serde_json::Value::Null));
                serde_json::from_value(value)
                    .map(Arc::new)
                    .map_err(|source| BindingResolutionError::FactoryFailed {
                        service: std::any::type_name::<T>(),
                        source: Box::new(source),
                    })
            })
        })
    }
}
impl<'a, C: Container, T: ?Sized + Send + Sync + 'static> ContextualBinding<'a, C, Vec<Arc<T>>> {
    #[allow(non_snake_case, reason = "Async contextual tagged adapter")]
    pub fn giveTaggedAsync(
        self,
        tag: impl Into<String>,
    ) -> Result<ContextualBindingBuilder<'a, C>, BindingResolutionError> {
        let tag = tag.into();
        self.giveAsync(move |container| {
            let tag = tag.clone();
            Box::pin(async move {
                container
                    .tagged::<T>(&tag)
                    .collectAsync()
                    .await
                    .map(Arc::new)
            })
        })
    }
}
