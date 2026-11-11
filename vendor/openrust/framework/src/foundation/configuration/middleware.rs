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
    pub(crate) prepended: usize,
    pub(crate) registry: MiddlewareRegistry,
}

impl Default for Middleware {
    fn default() -> Self {
        let mut middleware = Self {
            global: Vec::new(),
            prepended: 0,
            registry: MiddlewareRegistry::default(),
        };
        let bindings = Arc::new(SubstituteBindings);
        middleware.registry.groups.insert(
            "web".into(),
            vec![MiddlewareEntry::Instance(bindings.clone())],
        );
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
        self.prepended += 1;
        self
    }

    #[allow(
        non_snake_case,
        reason = "Matches the application middleware configuration API"
    )]
    pub fn appendTerminable<M: crate::contracts::http::TerminableMiddleware>(
        &mut self,
        middleware: Arc<M>,
    ) -> &mut Self {
        self.append(crate::foundation::http::terminable(middleware))
    }

    #[allow(
        non_snake_case,
        reason = "Matches the application middleware configuration API"
    )]
    pub fn prependTerminable<M: crate::contracts::http::TerminableMiddleware>(
        &mut self,
        middleware: Arc<M>,
    ) -> &mut Self {
        self.prepend(crate::foundation::http::terminable(middleware))
    }

    #[allow(
        non_snake_case,
        reason = "Matches the application middleware configuration API"
    )]
    pub fn aliasTerminable<M: crate::contracts::http::TerminableMiddleware>(
        &mut self,
        name: impl Into<String>,
        middleware: Arc<M>,
    ) -> &mut Self {
        self.alias(name, crate::foundation::http::terminable(middleware))
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
