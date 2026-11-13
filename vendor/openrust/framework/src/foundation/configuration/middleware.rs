use std::{any::TypeId, collections::HashSet, sync::Arc};

use crate::{
    config::{ConfigError, Value},
    contracts::{config::Repository, http::Middleware as HttpMiddleware},
    cookie::middleware::{AddQueuedCookiesToResponse, EncryptCookies},
    foundation::http::middleware::PreventRequestForgery,
    routing::{
        MiddlewareEntry,
        middleware::{MiddlewareRegistry, SubstituteBindings},
    },
    session::middleware::StartSession,
    view::middleware::ShareErrorsFromSession,
};

/// Configures global and route middleware during application assembly.
#[derive(Clone)]
pub struct Middleware {
    pub(crate) global: Vec<Arc<dyn HttpMiddleware>>,
    pub(crate) prepended: usize,
    pub(crate) maintenance_exceptions: Vec<String>,
    pub(crate) registry: MiddlewareRegistry,
    default_web: bool,
    #[cfg(feature = "sanctum")]
    stateful_api: bool,
    cookie_exceptions: Arc<HashSet<String>>,
    csrf: PreventRequestForgery,
}

impl Default for Middleware {
    fn default() -> Self {
        let mut middleware = Self {
            global: Vec::new(),
            prepended: 0,
            maintenance_exceptions: Vec::new(),
            registry: MiddlewareRegistry::default(),
            default_web: true,
            #[cfg(feature = "sanctum")]
            stateful_api: false,
            cookie_exceptions: Arc::default(),
            csrf: PreventRequestForgery::default(),
        };
        let bindings = Arc::new(SubstituteBindings);
        middleware.registry.groups.insert(
            "web".into(),
            vec![MiddlewareEntry::Instance(bindings.clone())],
        );
        middleware.group("api", [MiddlewareEntry::Instance(bindings)]);
        middleware.registry.priority.extend([
            TypeId::of::<crate::auth::middleware::Authenticate>(),
            TypeId::of::<SubstituteBindings>(),
            TypeId::of::<crate::auth::middleware::Authorize>(),
        ]);
        middleware.alias(
            "auth",
            Arc::new(crate::auth::middleware::Authenticate::default()),
        );
        middleware.alias(
            "verified",
            Arc::new(crate::auth::middleware::EnsureEmailIsVerified::default()),
        );
        middleware.alias(
            "guest",
            Arc::new(crate::auth::middleware::RedirectIfAuthenticated::default()),
        );
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

    /// Excludes the named cookies from encryption in the default web group.
    #[allow(non_snake_case, reason = "Matches Laravel's Middleware API")]
    pub fn encryptCookies(
        &mut self,
        except: impl IntoIterator<Item = impl Into<String>>,
    ) -> &mut Self {
        Arc::make_mut(&mut self.cookie_exceptions).extend(except.into_iter().map(Into::into));
        self
    }

    /// Allows the given paths to remain accessible during maintenance mode.
    #[allow(non_snake_case, reason = "Matches Laravel's Middleware API")]
    pub fn preventRequestsDuringMaintenance(
        &mut self,
        except: impl IntoIterator<Item = impl Into<String>>,
    ) -> &mut Self {
        self.maintenance_exceptions
            .extend(except.into_iter().map(Into::into));
        self
    }

    /// Configures request forgery protection in the default web group.
    #[allow(non_snake_case, reason = "Matches Laravel's Middleware API")]
    pub fn preventRequestForgery(
        &mut self,
        except: impl IntoIterator<Item = impl Into<String>>,
        origin_only: bool,
        allow_same_site: bool,
    ) -> &mut Self {
        self.csrf
            .except(except)
            .useOriginOnly(origin_only)
            .allowSameSite(allow_same_site);
        self
    }

    pub(crate) fn configure_web(&mut self, config: &dyn Repository) -> Result<(), ConfigError> {
        #[cfg(feature = "sanctum")]
        self.configure_stateful_api(config)?;
        if !self.default_web || matches!(config.get("session.driver"), None | Some(Value::Null)) {
            return Ok(());
        }
        let name = config.get("session.cookie");
        let name = name
            .as_ref()
            .and_then(Value::as_str)
            .unwrap_or("openrust_session");
        if self.cookie_exceptions.contains(name) || self.cookie_exceptions.contains("XSRF-TOKEN") {
            return Err(ConfigError::InvalidType {
                key: "session.cookie".into(),
                expected: "encrypted session and XSRF cookies in the web middleware group",
            });
        }
        let csrf: Arc<dyn HttpMiddleware> = Arc::new(self.csrf.clone());
        self.registry
            .aliases
            .insert("framework.prevent_request_forgery".into(), csrf.clone());
        let stack: [Arc<dyn HttpMiddleware>; 6] = [
            Arc::new(EncryptCookies::for_application(
                self.cookie_exceptions.clone(),
            )),
            Arc::new(AddQueuedCookiesToResponse),
            Arc::new(StartSession),
            Arc::new(ShareErrorsFromSession),
            csrf,
            Arc::new(SubstituteBindings),
        ];
        self.registry.groups.insert(
            "web".into(),
            stack.into_iter().map(MiddlewareEntry::Instance).collect(),
        );
        Ok(())
    }

    /// Enables first-party session authentication on the API middleware group.
    #[cfg(feature = "sanctum")]
    #[allow(non_snake_case, reason = "Matches Laravel's Middleware API")]
    pub fn statefulApi(&mut self) -> &mut Self {
        self.stateful_api = true;
        self
    }

    #[cfg(feature = "sanctum")]
    fn configure_stateful_api(&mut self, config: &dyn Repository) -> Result<(), ConfigError> {
        use crate::sanctum::EnsureFrontendRequestsAreStateful;
        if !self.stateful_api {
            return Ok(());
        }
        let cookie = config.get("session.cookie");
        let name = cookie
            .as_ref()
            .and_then(Value::as_str)
            .unwrap_or("openrust_session");
        if matches!(config.get("session.driver"), None | Some(Value::Null))
            || self.cookie_exceptions.contains(name)
            || self.cookie_exceptions.contains("XSRF-TOKEN")
        {
            return Err(ConfigError::InvalidType {
                key: "session".into(),
                expected: "a session driver and encrypted session and XSRF cookies for stateful API requests",
            });
        }
        let middleware = EnsureFrontendRequestsAreStateful::new(
            config,
            [
                Arc::new(EncryptCookies::for_application(
                    self.cookie_exceptions.clone(),
                )),
                Arc::new(AddQueuedCookiesToResponse),
                Arc::new(StartSession),
                Arc::new(self.csrf.clone()),
            ],
        )?;
        self.registry
            .groups
            .entry("api".into())
            .or_default()
            .insert(0, MiddlewareEntry::Instance(Arc::new(middleware)));
        self.registry
            .priority
            .insert(0, TypeId::of::<EnsureFrontendRequestsAreStateful>());
        Ok(())
    }

    /// Resolves the throttle alias through the Redis duration limiter.
    #[allow(non_snake_case, reason = "Matches Laravel's Middleware API")]
    pub fn throttleWithRedis(&mut self) -> &mut Self {
        self.registry.throttle_with_redis = true;
        self
    }

    pub fn group(
        &mut self,
        name: impl Into<String>,
        entries: impl IntoIterator<Item = MiddlewareEntry>,
    ) -> &mut Self {
        let name = name.into();
        if name == "web" {
            self.default_web = false;
        }
        self.registry
            .groups
            .insert(name, entries.into_iter().collect());
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
