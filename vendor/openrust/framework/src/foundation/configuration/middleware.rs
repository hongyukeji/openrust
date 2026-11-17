use std::{any::TypeId, collections::HashSet, sync::Arc};

use crate::{
    config::{ConfigError, Value},
    contracts::{config::Repository, http::Middleware as HttpMiddleware},
    cookie::middleware::{AddQueuedCookiesToResponse, EncryptCookies},
    foundation::http::middleware::PreventRequestForgery,
    http::{Request, middleware::CorsSkipCallback},
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
    pub(crate) trusted_proxies: Option<crate::http::ProxyPolicy>,
    pub(crate) trusted_hosts: Option<crate::http::TrustedHosts>,
    pub(crate) cors_skip_callbacks: Vec<CorsSkipCallback>,
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
            trusted_proxies: None,
            trusted_hosts: None,
            cors_skip_callbacks: Vec::new(),
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
            TypeId::of::<crate::auth::middleware::AuthenticateWithBasicAuth>(),
            TypeId::of::<crate::auth::middleware::AuthenticateSession>(),
            TypeId::of::<SubstituteBindings>(),
            TypeId::of::<crate::auth::middleware::Authorize>(),
        ]);
        middleware.alias(
            "auth",
            Arc::new(crate::auth::middleware::Authenticate::default()),
        );
        middleware.alias(
            "auth.basic",
            Arc::new(crate::auth::middleware::AuthenticateWithBasicAuth::default()),
        );
        middleware.alias(
            "auth.session",
            Arc::new(crate::auth::middleware::AuthenticateSession),
        );
        middleware.alias(
            "password.confirm",
            Arc::new(crate::auth::middleware::RequirePassword::default()),
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

    /// Skips CORS for requests matching this callback in the current application.
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's middleware configuration names"
    )]
    pub fn skipCorsWhen(
        &mut self,
        callback: impl Fn(&Request) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.cors_skip_callbacks.push(Arc::new(callback));
        self
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's middleware configuration API"
    )]
    pub fn trustProxies(&mut self, at: &[&str], headers: u8) -> Result<&mut Self, ConfigError> {
        self.trusted_proxies = Some(crate::http::ProxyPolicy::new(at, headers)?);
        Ok(self)
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's middleware configuration API"
    )]
    pub fn trustHosts(&mut self, at: &[&str], subdomains: bool) -> Result<&mut Self, ConfigError> {
        self.trusted_hosts = Some(crate::http::TrustedHosts::new(at, subdomains)?);
        Ok(self)
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's dynamic trusted-host configuration"
    )]
    pub fn trustHostsWith<Fut>(
        &mut self,
        at: impl Fn() -> Fut + Send + Sync + 'static,
        subdomains: bool,
    ) -> &mut Self
    where
        Fut: std::future::Future<Output = Result<Vec<String>, ConfigError>> + Send + 'static,
    {
        self.trusted_hosts = Some(crate::http::TrustedHosts::using(at, subdomains));
        self
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
        use crate::sanctum::{AuthenticateSession, EnsureFrontendRequestsAreStateful};
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
        let replacement = |key: &str,
                           value: Option<Value>|
         -> Result<Option<Arc<dyn HttpMiddleware>>, ConfigError> {
            match value {
                None | Some(Value::Null | Value::Bool(false)) => Ok(None),
                Some(Value::String(alias)) if self.registry.aliases.contains_key(&alias) => {
                    Ok(self.registry.aliases.get(&alias).cloned())
                }
                _ => Err(ConfigError::InvalidType {
                    key: key.into(),
                    expected: "a registered middleware alias, null, or false",
                }),
            }
        };
        let encrypt = "sanctum.middleware.encrypt_cookies";
        let csrf = "sanctum.middleware.validate_csrf_token";
        let authenticate = "sanctum.middleware.authenticate_session";
        let session = match config.get(authenticate) {
            None => Some(Arc::new(AuthenticateSession::new(config).map_err(|_| {
                ConfigError::InvalidType {
                    key: "sanctum.guard".into(),
                    expected: "configured session guard names",
                }
            })?) as Arc<dyn HttpMiddleware>),
            value => replacement(authenticate, value)?,
        };
        let mut stack = Vec::new();
        let encrypt_cookies = match config.get(encrypt) {
            None => Some(Arc::new(EncryptCookies::for_application(
                self.cookie_exceptions.clone(),
            )) as Arc<dyn HttpMiddleware>),
            value => replacement(encrypt, value)?,
        };
        let validate_csrf = match config
            .get(csrf)
            .or_else(|| config.get("sanctum.middleware.verify_csrf_token"))
        {
            None => Some(Arc::new(self.csrf.clone()) as Arc<dyn HttpMiddleware>),
            value => replacement(csrf, value)?,
        };
        stack.extend(encrypt_cookies);
        stack.push(Arc::new(AddQueuedCookiesToResponse));
        stack.push(Arc::new(StartSession));
        stack.extend(validate_csrf);
        stack.extend(session);
        let mut seen = Vec::new();
        stack.retain(|item| {
            if seen.iter().any(|previous| Arc::ptr_eq(previous, item)) {
                return false;
            }
            seen.push(item.clone());
            true
        });
        let middleware = EnsureFrontendRequestsAreStateful::new(config, stack)?;
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
