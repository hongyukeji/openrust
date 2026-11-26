use std::collections::HashMap;

use super::{MiddlewareEntry, RoutingError, route_pattern::constraint};

/// Shared prefix and middleware settings for a group of routes.
#[derive(Default)]
pub struct RouteGroup {
    pub(super) prefix: String,
    pub(super) name: String,
    pub(super) domain: Option<String>,
    pub(super) constraints: HashMap<String, String>,
    pub(super) middleware: Vec<MiddlewareEntry>,
    pub(super) excluded_middleware: Vec<String>,
}

impl RouteGroup {
    pub fn new() -> Self {
        Self::default()
    }

    /// 在组作用域内使用路由门面登记路由。
    pub fn group(
        self,
        register: impl FnOnce() -> Result<(), RoutingError>,
    ) -> Result<(), RoutingError> {
        crate::support::facades::register_group(self, register)
    }

    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    pub fn name(mut self, prefix: impl Into<String>) -> Self {
        self.name = prefix.into();
        self
    }

    pub fn domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    pub fn r#where(mut self, parameter: &str, expression: &str) -> Result<Self, RoutingError> {
        self.constraints
            .insert(parameter.to_owned(), constraint(parameter, expression)?);
        Ok(self)
    }

    pub fn middleware(mut self, middleware: impl Into<MiddlewareEntry>) -> Self {
        self.middleware.push(middleware.into());
        self
    }

    pub fn without_middleware(mut self, name: impl Into<String>) -> Self {
        self.excluded_middleware.push(name.into());
        self
    }

    pub fn middleware_name(self, name: impl Into<String>) -> Self {
        self.middleware(MiddlewareEntry::named(name))
    }
}
