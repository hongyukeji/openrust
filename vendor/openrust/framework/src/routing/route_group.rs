use std::sync::Arc;

use crate::contracts::http::Middleware;

use super::MiddlewareEntry;

/// Shared prefix and middleware settings for a group of routes.
#[derive(Default)]
pub struct RouteGroup {
    pub(super) prefix: String,
    pub(super) middleware: Vec<MiddlewareEntry>,
}

impl RouteGroup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    pub fn middleware(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.middleware.push(MiddlewareEntry::Instance(middleware));
        self
    }

    pub fn middleware_name(mut self, name: impl Into<String>) -> Self {
        self.middleware.push(MiddlewareEntry::named(name));
        self
    }
}
