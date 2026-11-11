use crate::{
    console::ConsoleError,
    foundation::{Application, http::HealthCheck},
    routing::{RouteGroup, Router, RoutingError},
    support::facades::Route,
};

type CustomRoutes = fn(&Application) -> Result<Router, RoutingError>;
type GroupRoutes = fn(&mut Router) -> Result<(), RoutingError>;
type WebRoutes = fn() -> Result<(), RoutingError>;
pub(in crate::foundation) type CommandRoutes = fn() -> Result<(), ConsoleError>;

/// Declares the application's HTTP routes, console routes and health endpoint.
#[derive(Default)]
pub struct Routing {
    custom: Option<CustomRoutes>,
    web: Option<WebRoutes>,
    api: Option<GroupRoutes>,
    health: Option<String>,
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

    /// Declares console routes to load after the console kernel boots the application.
    pub fn commands(mut self, register: CommandRoutes) -> Self {
        self.command_routes = Some(register);
        self
    }

    pub fn health(mut self, path: impl Into<String>) -> Self {
        self.health = Some(path.into());
        self
    }

    pub(crate) fn register(self, app: &Application) -> Result<Router, RoutingError> {
        let mut router = match self.custom {
            Some(register) => register(app)?,
            None => Router::new(),
        };
        if let Some(register) = self.api {
            router.group(
                RouteGroup::new().prefix("api").middleware_name("api"),
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
        Ok(router)
    }
}
