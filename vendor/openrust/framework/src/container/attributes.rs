//! Compiled contextual attributes and their resolution callbacks.

use std::sync::Arc;

use crate::contracts::container::{BindingResolutionError, ContextualAttribute};

use super::{Container, ParameterValue, ServiceKey};

type Result<T> = std::result::Result<T, BindingResolutionError>;
type Instantiate = dyn Fn() -> Result<ParameterValue> + Send + Sync;
type Resolve =
    dyn Fn(&ParameterValue, &Container, AttributeParameter) -> Result<AttributeValue> + Send + Sync;
pub(super) type After = dyn Fn(&AttributeEvent<'_>, &Container) -> Result<()> + Send + Sync;

/// Metadata of the parameter being injected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttributeParameter {
    pub name: &'static str,
    pub variadic: bool,
}

/// A single argument (including null), or a list of constructor variadic arguments.
#[derive(Clone, Debug)]
pub enum AttributeValue {
    One(ParameterValue),
    Many(Vec<ParameterValue>),
}

impl From<ParameterValue> for AttributeValue {
    fn from(value: ParameterValue) -> Self {
        Self::One(value)
    }
}

/// An attribute declared on a compiled class or parameter.
#[derive(Clone)]
pub struct ClassAttribute {
    key: ServiceKey,
    instantiate: Arc<Instantiate>,
    resolve: Option<Arc<Resolve>>,
    after: Option<Arc<After>>,
}

impl ClassAttribute {
    pub fn marker<A: Send + Sync + 'static>(
        factory: impl Fn() -> Result<A> + Send + Sync + 'static,
    ) -> Self {
        Self {
            key: ServiceKey::typed::<A>(),
            instantiate: Arc::new(move || factory().map(ParameterValue::new)),
            resolve: None,
            after: None,
        }
    }

    pub fn contextual<A: ContextualAttribute>(
        factory: impl Fn() -> Result<A> + Send + Sync + 'static,
    ) -> Self {
        Self {
            resolve: Some(Arc::new(|attribute, container, parameter| {
                attribute.downcast::<A>()?.resolve(container, parameter)
            })),
            after: Some(Arc::new(|event, container| {
                event
                    .attribute
                    .downcast::<A>()?
                    .after(event.value, container)
            })),
            ..Self::marker(factory)
        }
    }

    pub(super) fn is_contextual(&self) -> bool {
        self.resolve.is_some()
    }
}

/// Values observed by an attribute callback. Class callbacks have no parameter metadata.
pub struct AttributeEvent<'a> {
    pub attribute: &'a ParameterValue,
    pub value: &'a AttributeValue,
    pub parameter: Option<AttributeParameter>,
}

#[derive(Default)]
pub(super) struct Attributes {
    resolvers: std::collections::HashMap<ServiceKey, Arc<Resolve>>,
    after: Vec<(ServiceKey, Vec<Arc<After>>)>,
}

#[allow(non_snake_case, reason = "Matches Laravel's contextual attribute API")]
impl Container {
    /// Overrides the resolver for a contextual attribute in this container.
    pub fn whenHasAttribute<A: ContextualAttribute>(
        &self,
        handler: impl Fn(&A, &Self, AttributeParameter) -> Result<AttributeValue>
        + Send
        + Sync
        + 'static,
    ) {
        let handler: Arc<Resolve> = Arc::new(move |value, container, parameter| {
            handler(value.downcast::<A>()?.as_ref(), container, parameter)
        });
        let retired = self
            .registry
            .write()
            .expect("container lock poisoned")
            .attributes
            .resolvers
            .insert(ServiceKey::typed::<A>(), handler);
        drop(retired);
    }

    pub fn resolveFromAttribute(
        &self,
        attribute: &ClassAttribute,
        parameter: AttributeParameter,
    ) -> Result<AttributeValue> {
        super::scope::check_open(self)?;
        let handler = self
            .registry
            .read()
            .expect("container lock poisoned")
            .attributes
            .resolvers
            .get(&attribute.key)
            .cloned();
        let instance = (attribute.instantiate)()?;
        let resolve = handler
            .as_ref()
            .or(attribute.resolve.as_ref())
            .ok_or_else(|| BindingResolutionError::AttributeHandlerMissing {
                attribute: attribute.key.clone(),
            })?;
        resolve(&instance, self, parameter)
    }

    /// Registers a callback by attribute type or by the declared service type of the result.
    pub fn afterResolvingAttribute<A: ?Sized + 'static>(
        &self,
        callback: impl Fn(&AttributeEvent<'_>, &Self) -> Result<()> + Send + Sync + 'static,
    ) {
        let key = ServiceKey::typed::<A>();
        let mut registry = self.registry.write().expect("container lock poisoned");
        let callbacks = &mut registry.attributes.after;
        let callback: Arc<After> = Arc::new(callback);
        if let Some((_, callbacks)) = callbacks
            .iter_mut()
            .find(|(registered, _)| *registered == key)
        {
            callbacks.push(callback);
        } else {
            callbacks.push((key, vec![callback]));
        }
    }

    pub(super) fn fire_attribute_callbacks(
        &self,
        attributes: &[ClassAttribute],
        value: &AttributeValue,
        parameter: Option<AttributeParameter>,
        service: Option<&ServiceKey>,
    ) -> Result<()> {
        for attribute in attributes {
            let invoke = |callback: &After| {
                let instance = (attribute.instantiate)()?;
                callback(
                    &AttributeEvent {
                        attribute: &instance,
                        value,
                        parameter,
                    },
                    self,
                )
            };
            if let Some(callback) = &attribute.after {
                invoke(callback.as_ref())?;
            }
            let callbacks: Vec<_> = self
                .registry
                .read()
                .expect("container lock poisoned")
                .attributes
                .after
                .iter()
                .filter(|(key, _)| key == &attribute.key || Some(key) == service)
                .flat_map(|(_, callbacks)| callbacks.iter().cloned())
                .collect();
            for callback in callbacks {
                invoke(callback.as_ref())?;
            }
        }
        Ok(())
    }
}
