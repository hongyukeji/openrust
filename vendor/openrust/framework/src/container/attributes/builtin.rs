//! Built-in attributes backed by the current application's container.

use std::{any::type_name, marker::PhantomData, sync::Arc};

use serde::de::DeserializeOwned;

use crate::contracts::{
    config::Repository,
    container::{BindingResolutionError, Container as _, ContextualAttribute},
};

use super::{AttributeParameter, AttributeValue, Container, ParameterValue, Result, ServiceKey};
use crate::container::{CallFuture, ParameterOverrides, TaggedServices};

/// Reads a typed value from the application's config repository.
pub struct Config<T> {
    key: Arc<str>,
    default: ParameterValue,
    output: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned + Send + Sync + 'static> Config<T> {
    pub fn new(key: impl Into<Arc<str>>) -> Self {
        Self {
            key: key.into(),
            default: ParameterValue::null(),
            output: PhantomData,
        }
    }

    #[allow(non_snake_case, reason = "Compiled Laravel attribute options")]
    pub fn withDefault(mut self, value: T) -> Self {
        self.default = ParameterValue::new(value);
        self
    }

    fn read(&self, config: &dyn Repository) -> Result<AttributeValue> {
        let value = match config.get(&self.key) {
            None => self.default.clone(),
            Some(serde_json::Value::Null) => ParameterValue::null(),
            Some(value) => serde_json::from_value::<T>(value)
                .map(ParameterValue::new)
                .map_err(|_| BindingResolutionError::ConfigurationValueTypeMismatch {
                    key: self.key.clone(),
                    expected: type_name::<T>(),
                })?,
        };
        Ok(value.into())
    }
}

impl<T: DeserializeOwned + Send + Sync + 'static> ContextualAttribute for Config<T> {
    fn resolve(&self, container: &Container, _: AttributeParameter) -> Result<AttributeValue> {
        self.read(container.get::<dyn Repository>("config")?.as_ref())
    }

    fn resolveAsync<'a>(
        self: Arc<Self>,
        container: &'a Container,
        _: AttributeParameter,
    ) -> CallFuture<'a, AttributeValue> {
        Box::pin(async move {
            self.read(
                container
                    .getAsync::<dyn Repository>("config")
                    .await?
                    .as_ref(),
            )
        })
    }
}

/// Selects an implementation with its own constructor parameters.
pub struct Give<T: ?Sized> {
    key: ServiceKey,
    parameters: ParameterOverrides,
    output: PhantomData<fn() -> Arc<T>>,
}
impl<T: ?Sized + Send + Sync + 'static> Give<T> {
    pub fn new(key: impl Into<ServiceKey>) -> Self {
        Self {
            key: key.into(),
            parameters: ParameterOverrides::new(),
            output: PhantomData,
        }
    }

    #[allow(non_snake_case, reason = "Compiled Laravel attribute options")]
    pub fn withParameters(mut self, parameters: ParameterOverrides) -> Self {
        self.parameters = parameters;
        self
    }
}

impl<T: ?Sized + Send + Sync + 'static> ContextualAttribute for Give<T> {
    fn resolve(&self, container: &Container, _: AttributeParameter) -> Result<AttributeValue> {
        container
            .makeWithNamed::<T>(self.key.clone(), self.parameters.clone())
            .map(|value| ParameterValue::new(value).into())
    }

    fn resolveAsync<'a>(
        self: Arc<Self>,
        container: &'a Container,
        _: AttributeParameter,
    ) -> CallFuture<'a, AttributeValue> {
        Box::pin(async move {
            container
                .makeWithAsyncNamed::<T>(self.key.clone(), self.parameters.clone())
                .await
                .map(|value| ParameterValue::new(value).into())
        })
    }
}

/// Supplies an owned description of the tagged services, resolved when iterated.
pub struct Tag<T: ?Sized> {
    tag: Arc<str>,
    output: PhantomData<fn() -> Arc<T>>,
}
impl<T: ?Sized + Send + Sync + 'static> Tag<T> {
    pub fn new(tag: impl Into<Arc<str>>) -> Self {
        Self {
            tag: tag.into(),
            output: PhantomData,
        }
    }
}

impl<T: ?Sized + Send + Sync + 'static> ContextualAttribute for Tag<T> {
    fn resolve(&self, container: &Container, _: AttributeParameter) -> Result<AttributeValue> {
        Ok(ParameterValue::new(TaggedServices::<T>::new(container, &self.tag)).into())
    }
}

/// Exposes the current consumer as a typed service key.
pub struct CurrentResolving;
impl ContextualAttribute for CurrentResolving {
    fn resolve(&self, container: &Container, _: AttributeParameter) -> Result<AttributeValue> {
        Ok(container
            .currentlyResolving()
            .map_or_else(ParameterValue::null, ParameterValue::new)
            .into())
    }
}
