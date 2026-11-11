use std::{collections::HashMap, sync::Arc};

use crate::contracts::http::Middleware;

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

    pub fn middleware(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.middleware.push(MiddlewareEntry::Instance(middleware));
        self
    }

    #[allow(non_snake_case, reason = "Matches Laravel's route middleware API")]
    pub fn withoutMiddleware(mut self, name: impl Into<String>) -> Self {
        self.excluded_middleware.push(name.into());
        self
    }

    pub fn middleware_name(mut self, name: impl Into<String>) -> Self {
        self.middleware.push(MiddlewareEntry::named(name));
        self
    }
}
