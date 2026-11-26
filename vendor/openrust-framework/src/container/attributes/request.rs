use super::{
    Arc, AttributeParameter, AttributeValue, Container, ContextualAttribute, ParameterValue, Result,
};
use crate::http::request_context;

/// Injects a named server-side attribute of the current HTTP request.
pub struct RequestAttribute {
    name: Arc<str>,
}

impl RequestAttribute {
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self { name: name.into() }
    }
}

impl ContextualAttribute for RequestAttribute {
    fn resolve(&self, container: &Container, _: AttributeParameter) -> Result<AttributeValue> {
        Ok(request_context::attribute(container, &self.name)
            .unwrap_or_else(ParameterValue::null)
            .into())
    }
}

/// Injects a matched route value, using the bound model once substitution succeeds.
#[derive(Default)]
pub struct RouteParameter {
    name: Option<Arc<str>>,
}

impl RouteParameter {
    /// Selects a route parameter by name. `default()` uses the injection parameter name.
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self {
            name: Some(name.into()),
        }
    }
}

impl ContextualAttribute for RouteParameter {
    fn resolve(
        &self,
        container: &Container,
        parameter: AttributeParameter,
    ) -> Result<AttributeValue> {
        Ok(request_context::route_parameter(
            container,
            self.name.as_deref().unwrap_or(parameter.name),
        )
        .unwrap_or_else(ParameterValue::null)
        .into())
    }
}
