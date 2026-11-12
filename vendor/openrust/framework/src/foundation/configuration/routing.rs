use std::sync::Arc;

use crate::{
    broadcasting::{BroadcastController, BroadcastResult},
    config::{ConfigError, Value},
    console::ConsoleError,
    contracts::config::Repository,
    foundation::{Application, http::HealthCheck},
    routing::{RouteGroup, Router, RoutingError},
    support::facades::Route,
};

type CustomRoutes = fn(&Application) -> Result<Router, RoutingError>;
type GroupRoutes = fn(&mut Router) -> Result<(), RoutingError>;
type WebRoutes = fn() -> Result<(), RoutingError>;
pub(in crate::foundation) type CommandRoutes = fn() -> Result<(), ConsoleError>;
pub(super) type ChannelRoutes = fn() -> BroadcastResult<()>;

/// Declares the application's HTTP routes, console routes and health endpoint.
#[derive(Default)]
pub struct Routing {
    custom: Option<CustomRoutes>,
    web: Option<WebRoutes>,
    api: Option<GroupRoutes>,
    api_prefix: Option<String>,
    health: Option<String>,
    channels: Option<ChannelRoutes>,
    pub(in crate::foundation) command_routes: Option<CommandRoutes>,
}

impl Routing {
    pub fn new(register: CustomRoutes) -> Self {
        Self {
            custom: Some(register),
            ..Self::default()
        }
    }

    pub fn web(mut self, register: WebRoutes) -> Self {
        self.web = Some(register);
        self
    }

    pub fn api(mut self, register: GroupRoutes) -> Self {
        self.api = Some(register);
        self
    }

    /// Overrides the configured URL prefix for API routes.
    #[allow(non_snake_case, reason = "Matches Laravel's apiPrefix option")]
    pub fn apiPrefix(mut self, prefix: impl Into<String>) -> Self {
        self.api_prefix = Some(prefix.into());
        self
    }

    /// Enables broadcast authentication and loads the application's channel rules.
    pub fn channels(mut self, register: ChannelRoutes) -> Self {
        self.channels = Some(register);
        self
    }

    /// Declares console routes to load after the console kernel boots the application.
    pub fn commands(mut self, register: CommandRoutes) -> Self {
        self.command_routes = Some(register);
        self
    }

    pub fn health(mut self, path: impl Into<String>) -> Self {
        self.health = Some(path.into());
        self
    }

    pub(crate) fn resolve_api_prefix(
        &self,
        config: &dyn Repository,
    ) -> Result<Arc<str>, ConfigError> {
        if let Some(prefix) = &self.api_prefix {
            return Ok(Arc::from(prefix.trim_matches('/')));
        }
        match config.get("routing.api_prefix") {
            None => Ok(Arc::from("api")),
            Some(Value::String(prefix)) => Ok(Arc::from(prefix.trim_matches('/'))),
            Some(_) => Err(ConfigError::InvalidType {
                key: "routing.api_prefix".into(),
                expected: "a string",
            }),
        }
    }

    pub(crate) fn register(
        self,
        app: &Application,
        api_prefix: &str,
    ) -> Result<Router, RoutingError> {
        let mut router = match self.custom {
            Some(register) => register(app)?,
            None => Router::new(),
        };
        if let Some(register) = self.api {
            router.group(
                RouteGroup::new().prefix(api_prefix).middleware_name("api"),
                register,
            )?;
        }
        if let Some(path) = self.health {
            router.get(&path, HealthCheck::new(app.container_handle()))?;
        }
        if let Some(register) = self.web {
            router.group(RouteGroup::new().middleware_name("web"), |router| {
                Route::register(router, app, register)
            })?;
        }
        if let Some(register) = self.channels {
            BroadcastController::register(app, &mut router, register)?;
        }
        Ok(router)
    }
}
