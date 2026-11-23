use std::{any::TypeId, collections::HashSet, sync::Arc};

use crate::{
    config::{ConfigError, Value},
    contracts::{config::Repository, http::Middleware as HttpMiddleware},
    cookie::middleware::{AddQueuedCookiesToResponse, EncryptCookies},
    foundation::http::middleware::{ConvertEmptyStringsToNull, PreventRequestForgery, TrimStrings},
    http::{
        Request,
        middleware::{CorsSkipCallback, SetLocale},
    },
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
    pub(crate) trim_strings: TrimStrings,
    pub(crate) empty_strings_to_null: ConvertEmptyStringsToNull,
    pub(crate) set_locale: SetLocale,
    default_web: bool,
    default_api: bool,
    api_limiter: Option<String>,
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
            trim_strings: TrimStrings::default(),
            empty_strings_to_null: ConvertEmptyStringsToNull::default(),
            set_locale: SetLocale::default(),
            default_web: true,
            default_api: true,
            api_limiter: None,
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
        middleware
            .registry
            .groups
            .insert("api".into(), vec![MiddlewareEntry::Instance(bindings)]);
        middleware.registry.priority.extend([
            TypeId::of::<crate::foundation::http::middleware::HandlePrecognitiveRequests>(),
            TypeId::of::<EncryptCookies>(),
            TypeId::of::<AddQueuedCookiesToResponse>(),
            TypeId::of::<StartSession>(),
            TypeId::of::<ShareErrorsFromSession>(),
            TypeId::of::<crate::auth::middleware::Authenticate>(),
            TypeId::of::<crate::auth::middleware::AuthenticateWithBasicAuth>(),
            TypeId::of::<crate::routing::middleware::ThrottleRequests>(),
            TypeId::of::<crate::routing::middleware::ThrottleRequestsWithRedis>(),
            TypeId::of::<crate::auth::middleware::AuthenticateSession>(),
            TypeId::of::<SubstituteBindings>(),
            TypeId::of::<crate::auth::middleware::Authorize>(),
        ]);
        middleware.alias(
            "auth",
            Arc::new(crate::auth::middleware::Authenticate::default()),
        );
        middleware.alias(
            "precognitive",
            Arc::new(crate::foundation::http::middleware::HandlePrecognitiveRequests),
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

    /// 覆盖启动时自动发现的语言候选；空列表关闭识别，非法标签保留原设置。
    pub fn set_locale(
        &mut self,
        locales: &[&str],
    ) -> crate::translation::TranslationResult<&mut Self> {
        self.set_locale.set_locale(locales)?;
        Ok(self)
    }

    /// 配置当前应用的语言选择回调；候选与回调分别保存。
    pub fn locale_using(
        &mut self,
        callback: impl Fn(&Request) -> Result<Option<String>, crate::http::HttpError>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        self.set_locale.locale_using(callback);
        self
    }

    /// Skips CORS for requests matching this callback in the current application.
    pub fn skip_cors_when(
        &mut self,
        callback: impl Fn(&Request) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.cors_skip_callbacks.push(Arc::new(callback));
        self
    }

    /// Adds field patterns excluded from the default trimming middleware.
    pub fn trim_strings(
        &mut self,
        except: impl IntoIterator<Item = impl Into<String>>,
    ) -> &mut Self {
        self.trim_strings.except(except);
        self
    }

    /// Skips trimming for requests which match this callback.
    pub fn skip_trim_strings_when(
        &mut self,
        callback: impl Fn(&Request) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.trim_strings.skip_when(callback);
        self
    }

    /// Skips empty-string conversion for requests which match this callback.
    pub fn convert_empty_strings_to_null(
        &mut self,
        except: impl Fn(&Request) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.empty_strings_to_null.skip_when(except);
        self
    }

    pub fn trust_proxies(&mut self, at: &[&str], headers: u8) -> Result<&mut Self, ConfigError> {
        self.trusted_proxies = Some(crate::http::ProxyPolicy::new(at, headers)?);
        Ok(self)
    }

    pub fn trust_hosts(&mut self, at: &[&str], subdomains: bool) -> Result<&mut Self, ConfigError> {
        self.trusted_hosts = Some(crate::http::TrustedHosts::new(at, subdomains)?);
        Ok(self)
    }

    pub fn trust_hosts_with<Fut>(
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
    pub fn append_terminable<M: crate::contracts::http::TerminableMiddleware>(
        &mut self,
        middleware: Arc<M>,
    ) -> &mut Self {
        self.append(crate::foundation::http::terminable(middleware))
    }

    #[allow(
        non_snake_case,
        reason = "Matches the application middleware configuration API"
    )]
    pub fn prepend_terminable<M: crate::contracts::http::TerminableMiddleware>(
        &mut self,
        middleware: Arc<M>,
    ) -> &mut Self {
        self.prepend(crate::foundation::http::terminable(middleware))
    }

    #[allow(
        non_snake_case,
        reason = "Matches the application middleware configuration API"
    )]
    pub fn alias_terminable<M: crate::contracts::http::TerminableMiddleware>(
        &mut self,
        name: impl Into<String>,
        middleware: Arc<M>,
    ) -> &mut Self {
        self.alias(name, crate::foundation::http::terminable(middleware))
    }

    /// Excludes the named cookies from encryption in the default web group.
    pub fn encrypt_cookies(
        &mut self,
        except: impl IntoIterator<Item = impl Into<String>>,
    ) -> &mut Self {
        Arc::make_mut(&mut self.cookie_exceptions).extend(except.into_iter().map(Into::into));
        self
    }

    /// Allows the given paths to remain accessible during maintenance mode.
    pub fn prevent_requests_during_maintenance(
        &mut self,
        except: impl IntoIterator<Item = impl Into<String>>,
    ) -> &mut Self {
        self.maintenance_exceptions
            .extend(except.into_iter().map(Into::into));
        self
    }

    /// Configures request forgery protection in the default web group.
    pub fn prevent_request_forgery(
        &mut self,
        except: impl IntoIterator<Item = impl Into<String>>,
        origin_only: bool,
        allow_same_site: bool,
    ) -> &mut Self {
        self.csrf
            .except(except)
            .use_origin_only(origin_only)
            .allow_same_site(allow_same_site);
        self
    }

    pub(crate) fn configure_web(&mut self, config: &dyn Repository) -> Result<(), ConfigError> {
        if self.default_api {
            let mut stack = Vec::new();
            if let Some(limiter) = &self.api_limiter
                && !limiter.is_empty()
                && limiter != "0"
            {
                stack.push(MiddlewareEntry::named(format!("throttle:{limiter}")));
            }
            stack.push(MiddlewareEntry::Instance(Arc::new(SubstituteBindings)));
            self.registry.groups.insert("api".into(), stack);
        }
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
    pub fn stateful_api(&mut self) -> &mut Self {
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

    /// 为默认 API 组指定限流策略；传入 `None` 可关闭组内限流。
    ///
    /// 使用 `Some("api")` 和 `false` 对应默认策略及缓存后端。
    pub fn throttle_api(&mut self, limiter: Option<&str>, redis: bool) -> &mut Self {
        self.api_limiter = limiter.map(str::to_owned);
        if redis {
            self.throttle_with_redis();
        }
        self
    }

    /// Resolves the throttle alias through the Redis duration limiter.
    pub fn throttle_with_redis(&mut self) -> &mut Self {
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
        if name == "api" {
            self.default_api = false;
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
