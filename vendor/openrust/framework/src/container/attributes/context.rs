use std::{any::type_name, marker::PhantomData};

use serde::de::DeserializeOwned;

use crate::{
    container::CallFuture,
    contracts::container::{BindingResolutionError, Container as _},
    log::context::Repository,
};

use super::{
    Arc, AttributeParameter, AttributeValue, Container, ContextualAttribute, ParameterValue, Result,
};

/// Injects a public or hidden value from the current execution context.
pub struct Context<T> {
    key: Arc<str>,
    hidden: bool,
    default: Box<dyn Fn() -> Result<ParameterValue> + Send + Sync>,
    output: PhantomData<fn() -> T>,
}

#[allow(
    non_snake_case,
    reason = "Matches compiled Laravel contextual attribute options"
)]
impl<T: DeserializeOwned + Send + Sync + 'static> Context<T> {
    pub fn new(key: impl Into<Arc<str>>) -> Self {
        Self {
            key: key.into(),
            hidden: false,
            default: Box::new(|| Ok(ParameterValue::null())),
            output: PhantomData,
        }
    }

    #[must_use]
    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    #[must_use]
    pub fn withDefault(mut self, value: T) -> Self {
        let value = ParameterValue::new(value);
        self.default = Box::new(move || Ok(value.clone()));
        self
    }

    #[must_use]
    pub fn withDefaultUsing(
        mut self,
        factory: impl Fn() -> Result<T> + Send + Sync + 'static,
    ) -> Self {
        self.default = Box::new(move || factory().map(ParameterValue::new));
        self
    }

    fn read(&self, repository: &Repository) -> Result<AttributeValue> {
        let value = if self.hidden {
            repository.getHidden(&self.key)
        } else {
            repository.get(&self.key)
        };
        let resolved = match value {
            None => (self.default)()?,
            Some(value) => serde_json::from_value::<T>(value)
                .map(ParameterValue::new)
                .map_err(|_| BindingResolutionError::ParameterTypeMismatch {
                    expected: type_name::<T>(),
                    actual: Some("JSON context value"),
                })?,
        };
        Ok(resolved.into())
    }
}

impl<T: DeserializeOwned + Send + Sync + 'static> ContextualAttribute for Context<T> {
    fn resolve(&self, container: &Container, _: AttributeParameter) -> Result<AttributeValue> {
        self.read(container.make::<Repository>()?.as_ref())
    }

    #[allow(non_snake_case)]
    fn resolveAsync<'a>(
        self: Arc<Self>,
        container: &'a Container,
        _: AttributeParameter,
    ) -> CallFuture<'a, AttributeValue> {
        Box::pin(async move { self.read(container.makeAsync::<Repository>().await?.as_ref()) })
    }
}
