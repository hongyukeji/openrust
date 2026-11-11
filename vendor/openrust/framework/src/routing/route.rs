use std::{collections::HashMap, panic::Location, sync::Arc};

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
    pub(super) name: String,
    pub(super) domain: Option<String>,
    pub(super) action: &'static str,
    pub(super) definition: &'static Location<'static>,
    host_pattern: Option<RoutePattern>,
    pub(super) handler: Arc<dyn Handler>,
    pub(super) middleware: Vec<MiddlewareEntry>,
    pub(super) excluded_middleware: Vec<String>,
    pub(super) resolved_middleware: Option<MiddlewareStack>,
    constraints: HashMap<String, String>,
    defaults: HashMap<String, String>,
    pattern: RoutePattern,
    models: HashMap<String, Resolver>,
    scope_bindings: Option<bool>,
    with_trashed_bindings: bool,
    missing: Option<Arc<dyn Handler>>,
}

impl Route {
    #[track_caller]
    pub(super) fn new(
        uri: String,
        domain: Option<String>,
        handler: Arc<dyn Handler>,
        middleware: Vec<MiddlewareEntry>,
        constraints: HashMap<String, String>,
    ) -> Result<Self, RoutingError> {
        let pattern = RoutePattern::compile(&uri, &constraints)?;
        let host_pattern = domain
            .as_deref()
            .map(|host| RoutePattern::compile_host(host, &constraints))
            .transpose()?;
        Ok(Self {
            uri,
            name: String::new(),
            domain,
            action: "Closure",
            definition: Location::caller(),
            host_pattern,
            handler,
            middleware,
            excluded_middleware: Vec::new(),
            resolved_middleware: None,
            constraints,
            defaults: HashMap::new(),
            pattern,
            models: HashMap::new(),
            scope_bindings: None,
            with_trashed_bindings: false,
            missing: None,
        })
    }

    pub fn name(&mut self, name: &str) -> &mut Self {
        self.name.push_str(name);
        self
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Route API")]
    pub fn getName(&self) -> Option<&str> {
        (!self.name.is_empty()).then_some(&self.name)
    }

    pub fn middleware(&mut self, middleware: Arc<dyn Middleware>) -> &mut Self {
        self.middleware.push(MiddlewareEntry::Instance(middleware));
        self.resolved_middleware = None;
        self
    }

    /// Applies the named authorization ability after resolving its route arguments.
    pub fn can(&mut self, ability: &str, parameters: &[&str]) -> &mut Self {
        let mut alias = format!("can:{ability}");
        for parameter in parameters {
            alias.push(',');
            alias.push_str(parameter);
        }
        self.middleware_name(alias)
    }

    pub fn middleware_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.middleware.push(MiddlewareEntry::named(name));
        self.resolved_middleware = None;
        self
    }

    #[allow(non_snake_case, reason = "Matches Laravel's route middleware API")]
    pub fn withoutMiddleware(&mut self, name: impl Into<String>) -> &mut Self {
        self.excluded_middleware.push(name.into());
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

    #[allow(non_snake_case, reason = "Matches Laravel's Route API")]
    pub fn scopeBindings(&mut self) -> &mut Self {
        self.scope_bindings()
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Route API")]
    pub fn withoutScopedBindings(&mut self) -> &mut Self {
        self.without_scoped_bindings()
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Route API")]
    pub fn withTrashed(&mut self) -> &mut Self {
        self.with_trashed_bindings = true;
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
                    with_trashed: self.with_trashed_bindings,
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
                    with_trashed: self.with_trashed_bindings,
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
        let pattern = RoutePattern::compile(&self.uri, &constraints)?;
        let host_pattern = self
            .domain
            .as_deref()
            .map(|host| RoutePattern::compile_host(host, &constraints))
            .transpose()?;
        self.pattern = pattern;
        self.host_pattern = host_pattern;
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

    pub(super) fn matches(&self, host: &str, path: &str) -> bool {
        self.host_pattern
            .as_ref()
            .is_none_or(|pattern| pattern.matches(host))
            && self.pattern.matches(path)
    }

    pub(super) fn parameters(&self, host: &str, path: &str) -> Vec<(String, String)> {
        let Some(pattern) = &self.host_pattern else {
            return self.pattern.parameters(path, &self.defaults);
        };
        let mut parameters = pattern.parameters(host, &HashMap::new());
        let defaults = self
            .defaults
            .iter()
            .filter(|(key, _)| !parameters.iter().any(|(name, _)| name == *key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        for (key, value) in self.pattern.parameters(path, &defaults) {
            if let Some((_, previous)) = parameters.iter_mut().find(|(name, _)| *name == key) {
                *previous = value;
            } else {
                parameters.push((key, value));
            }
        }
        parameters
    }
}
