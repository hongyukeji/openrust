use std::{any::TypeId, sync::Arc};

use crate::{
    contracts::http::Middleware as HttpMiddleware,
    routing::{
        MiddlewareEntry,
        middleware::{MiddlewareRegistry, SubstituteBindings},
    },
};

/// Configures global and route middleware during application assembly.
pub struct Middleware {
    pub(crate) global: Vec<Arc<dyn HttpMiddleware>>,
    pub(crate) registry: MiddlewareRegistry,
}

impl Default for Middleware {
    fn default() -> Self {
        let mut middleware = Self {
            global: Vec::new(),
            registry: MiddlewareRegistry::default(),
        };
        let bindings = Arc::new(SubstituteBindings);
        middleware.group("web", [MiddlewareEntry::Instance(bindings.clone())]);
        middleware.group("api", [MiddlewareEntry::Instance(bindings)]);
        middleware
    }
}

impl Middleware {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, middleware: Arc<dyn HttpMiddleware>) -> &mut Self {
        self.global.push(middleware);
        self
    }

    pub fn prepend(&mut self, middleware: Arc<dyn HttpMiddleware>) -> &mut Self {
        self.global.insert(0, middleware);
        self
    }

    pub fn group(
        &mut self,
        name: impl Into<String>,
        entries: impl IntoIterator<Item = MiddlewareEntry>,
    ) -> &mut Self {
        self.registry
            .groups
            .insert(name.into(), entries.into_iter().collect());
        self
    }

    pub fn alias(
        &mut self,
        name: impl Into<String>,
        middleware: Arc<dyn HttpMiddleware>,
    ) -> &mut Self {
        self.registry.aliases.insert(name.into(), middleware);
        self
    }

    pub fn priority(&mut self, types: impl IntoIterator<Item = TypeId>) -> &mut Self {
        self.registry.priority = types.into_iter().collect();
        self
    }
}
