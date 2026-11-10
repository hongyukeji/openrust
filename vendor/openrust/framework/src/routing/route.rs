use std::{collections::HashMap, sync::Arc};

use crate::contracts::{
    http::Middleware,
    routing::{Handler, UrlRoutable},
};

use super::{
    MiddlewareEntry, RoutingError,
    binding::{self, Binding, BindingPlan, Resolver},
    middleware::MiddlewareStack,
    route_pattern::{RoutePattern, constraint},
};

/// A registered route and its metadata.
pub struct Route {
    pub(super) uri: String,
    pub(super) handler: Arc<dyn Handler>,
    pub(super) middleware: Vec<MiddlewareEntry>,
    pub(super) resolved_middleware: Option<MiddlewareStack>,
    constraints: HashMap<String, String>,
    defaults: HashMap<String, String>,
    pattern: RoutePattern,
    models: HashMap<String, Resolver>,
    scope_bindings: Option<bool>,
    missing: Option<Arc<dyn Handler>>,
}

impl Route {
    pub(super) fn new(
        uri: String,
        handler: Arc<dyn Handler>,
        middleware: Vec<MiddlewareEntry>,
        constraints: HashMap<String, String>,
    ) -> Result<Self, RoutingError> {
        let pattern = RoutePattern::compile(&uri, &constraints)?;
        Ok(Self {
            uri,
            handler,
            middleware,
            resolved_middleware: None,
            constraints,
            defaults: HashMap::new(),
            pattern,
            models: HashMap::new(),
            scope_bindings: None,
            missing: None,
        })
    }

    pub fn middleware(&mut self, middleware: Arc<dyn Middleware>) -> &mut Self {
        self.middleware.push(MiddlewareEntry::Instance(middleware));
        self.resolved_middleware = None;
        self
    }

    pub fn middleware_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.middleware.push(MiddlewareEntry::named(name));
        self.resolved_middleware = None;
        self
    }

    pub fn model<T: UrlRoutable>(&mut self, name: &str) -> Result<&mut Self, RoutingError> {
        if !self
            .pattern
            .binding_fields()
            .iter()
            .any(|(parameter, _)| parameter == name)
        {
            return Err(RoutingError::UnknownRouteParameter {
                name: name.to_owned(),
                uri: self.uri.clone(),
            });
        }
        self.models.insert(name.to_owned(), binding::model::<T>());
        Ok(self)
    }

    pub fn scope_bindings(&mut self) -> &mut Self {
        self.scope_bindings = Some(true);
        self
    }

    pub fn without_scoped_bindings(&mut self) -> &mut Self {
        self.scope_bindings = Some(false);
        self
    }

    pub fn missing(&mut self, handler: impl Handler + 'static) -> &mut Self {
        self.missing = Some(Arc::new(handler));
        self
    }

    pub(super) fn binding_plan(
        &self,
        explicit: &HashMap<String, Resolver>,
        parameters: &[(String, String)],
    ) -> BindingPlan {
        let fields = self.pattern.binding_fields();
        let mut bindings = Vec::new();
        for (name, _) in parameters {
            if let Some(resolver) = explicit.get(name) {
                let field = fields
                    .iter()
                    .find(|(parameter, _)| parameter == name)
                    .and_then(|(_, field)| field.clone());
                bindings.push(Binding {
                    name: name.clone(),
                    field,
                    parent: None,
                    resolver: resolver.clone(),
                });
            }
        }
        for (index, (name, field)) in fields.iter().enumerate() {
            if let Some(resolver) = self.models.get(name) {
                let scoped = self.scope_bindings.unwrap_or(field.is_some());
                let parent = index
                    .checked_sub(1)
                    .filter(|_| scoped)
                    .map(|parent| fields[parent].0.clone());
                bindings.push(Binding {
                    name: name.clone(),
                    field: field.clone(),
                    parent,
                    resolver: resolver.clone(),
                });
            }
        }
        BindingPlan {
            bindings,
            missing: self.missing.clone(),
        }
    }

    pub fn defaults(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.defaults.insert(name.into(), value.into());
        self
    }

    pub fn r#where(&mut self, name: &str, expression: &str) -> Result<&mut Self, RoutingError> {
        let mut constraints = self.constraints.clone();
        constraints.insert(name.to_owned(), constraint(name, expression)?);
        self.pattern = RoutePattern::compile(&self.uri, &constraints)?;
        self.constraints = constraints;
        Ok(self)
    }

    pub fn where_alpha(&mut self, name: &str) -> Result<&mut Self, RoutingError> {
        self.r#where(name, "[a-zA-Z]+")
    }

    pub fn where_alpha_numeric(&mut self, name: &str) -> Result<&mut Self, RoutingError> {
        self.r#where(name, "[a-zA-Z0-9]+")
    }

    pub fn where_number(&mut self, name: &str) -> Result<&mut Self, RoutingError> {
        self.r#where(name, "[0-9]+")
    }

    pub fn where_uuid(&mut self, name: &str) -> Result<&mut Self, RoutingError> {
        self.r#where(
            name,
            "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
        )
    }

    pub fn where_ulid(&mut self, name: &str) -> Result<&mut Self, RoutingError> {
        self.r#where(name, "[0-7][0-9a-hjkmnp-tv-zA-HJKMNP-TV-Z]{25}")
    }

    pub fn where_in(&mut self, name: &str, values: &[&str]) -> Result<&mut Self, RoutingError> {
        self.r#where(name, &values.join("|"))
    }

    pub(super) fn matches(&self, path: &str) -> bool {
        self.pattern.matches(path)
    }

    pub(super) fn parameters(&self, path: &str) -> Vec<(String, String)> {
        self.pattern.parameters(path, &self.defaults)
    }
}
