//! Static entry points for application services, routes and commands.

mod event;
mod facade;
pub use event::Event;
mod redirect;
mod url;
pub use crate::auth::{Auth, access::GateFacade as Gate, passwords::Password};
pub use crate::broadcasting::Broadcast;
pub use crate::bus::Bus;
pub use crate::cache::Cache;
pub use crate::concurrency::Concurrency;
pub use crate::console::scheduling::ScheduleFacade as Schedule;
pub use crate::cookie::facade::Cookie;
pub use crate::database::{DB, schema::Schema};
pub use crate::encryption::Crypt;
pub use crate::filesystem::Storage;
pub use crate::hashing::Hash;
pub use crate::http::client::Http;
pub use crate::image::ImageFacade as Image;
pub use crate::log::Log;
pub use crate::log::context::Context;
pub use crate::mail::Mail;
pub use crate::notifications::NotificationFacade as Notification;
pub use crate::process::Process;
pub use crate::queue::QueueFacade as Queue;
pub use crate::redis::Redis;
pub use crate::translation::Lang;
pub use crate::validation::facade::Validator;
pub use facade::Facade;
pub(crate) use facade::{
    ContextSnapshot, application_scope, context_snapshot, current_container, database_routing,
    isolated_scope, scope,
};
pub use redirect::Redirect;
pub use url::URL;

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};

use http::Uri;

use crate::{
    config::{ConfigError, Value},
    console::{Application, ConsoleError, Output},
    contracts::config::Repository,
    foundation,
    routing::{IntoRouteHandler, Router, RoutingError},
};

/// Resolves the current application's named rate limit policies.
pub struct RateLimiter;

impl Facade for RateLimiter {
    type Service = crate::cache::RateLimiter;

    fn getFacadeAccessor() -> crate::container::ServiceKey {
        "rateLimiter".into()
    }
}

impl RateLimiter {
    pub fn r#for<R>(
        name: impl Into<String>,
        callback: impl Fn(&crate::http::Request) -> R + Send + Sync + 'static,
    ) -> crate::cache::CacheResult<()>
    where
        R: Into<crate::cache::rate_limiting::LimiterResponse>,
    {
        Self::getFacadeRoot()
            .map_err(crate::cache::CacheError::Unavailable)?
            .r#for(name, callback);
        Ok(())
    }

    pub async fn attempt<T>(
        key: &str,
        max_attempts: u64,
        action: impl std::future::Future<Output = crate::cache::CacheResult<T>>,
        decay_seconds: u64,
    ) -> crate::cache::CacheResult<crate::cache::rate_limiting::Attempt<T>> {
        Self::getFacadeRoot()
            .map_err(crate::cache::CacheError::Unavailable)?
            .attempt(key, max_attempts, action, decay_seconds)
            .await
    }

    pub async fn hit(key: &str, decay_seconds: u64) -> crate::cache::CacheResult<u64> {
        Self::getFacadeRoot()
            .map_err(crate::cache::CacheError::Unavailable)?
            .hit(key, decay_seconds)
            .await
    }

    pub async fn attempts(key: &str) -> crate::cache::CacheResult<u64> {
        Self::getFacadeRoot()
            .map_err(crate::cache::CacheError::Unavailable)?
            .attempts(key)
            .await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's RateLimiter API")]
    pub async fn tooManyAttempts(key: &str, max_attempts: u64) -> crate::cache::CacheResult<bool> {
        Self::getFacadeRoot()
            .map_err(crate::cache::CacheError::Unavailable)?
            .tooManyAttempts(key, max_attempts)
            .await
    }

    pub async fn remaining(key: &str, max_attempts: u64) -> crate::cache::CacheResult<u64> {
        Self::getFacadeRoot()
            .map_err(crate::cache::CacheError::Unavailable)?
            .remaining(key, max_attempts)
            .await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's RateLimiter API")]
    pub async fn availableIn(key: &str) -> crate::cache::CacheResult<u64> {
        Self::getFacadeRoot()
            .map_err(crate::cache::CacheError::Unavailable)?
            .availableIn(key)
            .await
    }

    pub async fn clear(key: &str) -> crate::cache::CacheResult<()> {
        Self::getFacadeRoot()
            .map_err(crate::cache::CacheError::Unavailable)?
            .clear(key)
            .await
    }

    pub fn limiter(
        name: &str,
    ) -> crate::cache::CacheResult<Option<Arc<crate::cache::rate_limiting::Limiter>>> {
        Ok(Self::getFacadeRoot()
            .map_err(crate::cache::CacheError::Unavailable)?
            .limiter(name))
    }
}

/// Resolves the view factory for the current application.
pub struct View;

impl Facade for View {
    type Service = crate::view::Factory;

    fn getFacadeAccessor() -> crate::container::ServiceKey {
        crate::container::ServiceKey::typed::<Self::Service>()
    }
}

impl View {
    /// Resolves the application view factory through its public contract.
    pub fn contract() -> crate::view::ViewResult<Arc<dyn crate::contracts::view::Factory>> {
        let factory = Self::getFacadeRoot().map_err(crate::view::ViewError::Unavailable)?;
        Ok(factory)
    }

    pub fn make(
        name: impl Into<std::borrow::Cow<'static, str>>,
        data: crate::view::ViewData,
    ) -> crate::view::ViewResult<crate::view::View> {
        Self::getFacadeRoot()
            .map_err(crate::view::ViewError::Unavailable)?
            .make(name, data)
    }

    pub fn first(
        names: &[&str],
        data: crate::view::ViewData,
    ) -> crate::view::ViewResult<crate::view::View> {
        Self::getFacadeRoot()
            .map_err(crate::view::ViewError::Unavailable)?
            .first(names, data)
    }

    pub fn share(
        key: impl Into<String>,
        value: impl Into<crate::view::ViewValue>,
    ) -> crate::view::ViewResult<()> {
        Self::getFacadeRoot()
            .map_err(crate::view::ViewError::Unavailable)?
            .share(key, value);
        Ok(())
    }

    pub fn composer(
        name: impl Into<String>,
        callback: impl Fn(&mut crate::view::View) -> crate::view::ViewResult<()> + Send + Sync + 'static,
    ) -> crate::view::ViewResult<()> {
        Self::getFacadeRoot()
            .map_err(crate::view::ViewError::Unavailable)?
            .composer(name, callback);
        Ok(())
    }

    pub fn creator(
        name: impl Into<String>,
        callback: impl Fn(&mut crate::view::View) -> crate::view::ViewResult<()> + Send + Sync + 'static,
    ) -> crate::view::ViewResult<()> {
        Self::getFacadeRoot()
            .map_err(crate::view::ViewError::Unavailable)?
            .creator(name, callback);
        Ok(())
    }

    pub fn exists(name: &str) -> crate::view::ViewResult<bool> {
        Ok(Self::getFacadeRoot()
            .map_err(crate::view::ViewError::Unavailable)?
            .exists(name))
    }
}

tokio::task_local! {
    static ROUTER: RouteRegistration;
    static CONSOLE: RefCell<Application>;
}

struct RouteRegistration {
    router: RefCell<Router>,
    config: Arc<dyn Repository>,
    public_path: PathBuf,
}

/// Registers routes on the router bound by the current application.
pub struct Route;

impl Route {
    /// Registers an explicit model binding in the current application's router.
    pub fn model<T: crate::contracts::routing::UrlRoutable>(
        name: &str,
    ) -> Result<(), RoutingError> {
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .try_borrow_mut()
                    .map_err(|_| RoutingError::RegistrationBorrowed)?
                    .model::<T>(name);
                Ok(())
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    /// Registers a custom binding in the current application's router.
    pub fn bind<T, F>(name: &str, resolve: F) -> Result<(), RoutingError>
    where
        T: Send + Sync + 'static,
        F: for<'a> Fn(
                crate::routing::BindingContext<'a>,
            )
                -> crate::http::HttpFuture<'a, Result<Option<T>, crate::http::HttpError>>
            + Send
            + Sync
            + 'static,
    {
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .try_borrow_mut()
                    .map_err(|_| RoutingError::RegistrationBorrowed)?
                    .bind(name, resolve);
                Ok(())
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    /// Mounts files from a directory as a GET and HEAD fallback.
    pub fn r#static(prefix: &str, directory: impl AsRef<Path>) -> Result<(), RoutingError> {
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .borrow_mut()
                    .mount_static(prefix, directory.as_ref())
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    /// Mounts the application public directory at its configured URL prefix.
    #[allow(non_snake_case, reason = "Matches Laravel-style public API naming")]
    pub fn staticPublic() -> Result<(), RoutingError> {
        ROUTER
            .try_with(|scope| {
                let prefix = config_string(scope.config.as_ref(), "app.public_url_prefix")?;
                scope
                    .router
                    .borrow_mut()
                    .mount_static(prefix.as_deref().unwrap_or("/"), &scope.public_path)
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    /// Mounts an explicitly public local disk using its configured root and URL.
    #[allow(non_snake_case, reason = "Matches Laravel-style public API naming")]
    pub fn staticDisk(disk: &str) -> Result<(), RoutingError> {
        ROUTER
            .try_with(|scope| {
                let config = scope.config.as_ref();
                let key = format!("filesystems.disks.{disk}");
                for (field, expected) in [("driver", "local"), ("visibility", "public")] {
                    let key = format!("{key}.{field}");
                    if config_string(config, &key)?.as_deref() != Some(expected) {
                        return Err(static_config_error(&key, expected));
                    }
                }
                let root_key = format!("{key}.root");
                let root = config_string(config, &root_key)?
                    .ok_or_else(|| static_config_error(&root_key, "a directory path"))?;
                let prefix = disk_url_prefix(config, &format!("{key}.url"))?;
                scope
                    .router
                    .borrow_mut()
                    .mount_public_disk(&prefix, Path::new(&root))
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    #[track_caller]
    pub fn get<Arguments>(
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<(), RoutingError> {
        Self::r#match(
            &[crate::http::Method::GET, crate::http::Method::HEAD],
            uri,
            handler,
        )
    }

    #[track_caller]
    pub fn redirect(
        uri: &str,
        target: &str,
        status: crate::http::StatusCode,
    ) -> Result<(), RoutingError> {
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .try_borrow_mut()
                    .map_err(|_| RoutingError::RegistrationBorrowed)?
                    .redirect(uri, target, status)
                    .map(|_| ())
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    #[track_caller]
    #[allow(non_snake_case, reason = "Matches Laravel's Route facade")]
    pub fn permanentRedirect(uri: &str, target: &str) -> Result<(), RoutingError> {
        Self::redirect(uri, target, crate::http::StatusCode::MOVED_PERMANENTLY)
    }

    pub fn group(
        group: crate::routing::RouteGroup,
        register: impl FnOnce(&mut Router) -> Result<(), RoutingError>,
    ) -> Result<(), RoutingError> {
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .try_borrow_mut()
                    .map_err(|_| RoutingError::RegistrationBorrowed)?
                    .group(group, register)
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    #[track_caller]
    pub fn r#match<Arguments>(
        methods: &[crate::http::Method],
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<(), RoutingError> {
        let definition = std::panic::Location::caller();
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .try_borrow_mut()
                    .map_err(|_| RoutingError::RegistrationBorrowed)?
                    .register_with(methods, uri, handler, definition)
                    .map(|_| ())
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    pub(crate) fn register(
        router: &mut Router,
        application: &foundation::Application,
        register: fn() -> Result<(), RoutingError>,
    ) -> Result<(), RoutingError> {
        let scope = RouteRegistration {
            router: RefCell::new(std::mem::take(router)),
            config: application
                .configuration_repository()
                .map_err(|source| RoutingError::ContainerFailed { source })?,
            public_path: application.publicPath(),
        };
        ROUTER.sync_scope(scope, || {
            let result = register();
            *router = ROUTER.with(|active| std::mem::take(&mut *active.router.borrow_mut()));
            result
        })
    }
}

fn config_string(config: &dyn Repository, key: &str) -> Result<Option<String>, RoutingError> {
    match config.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value)),
        _ => Err(static_config_error(key, "a non-empty string")),
    }
}

fn disk_url_prefix(config: &dyn Repository, key: &str) -> Result<String, RoutingError> {
    let invalid_url = || static_config_error(key, "an absolute URL path or a same-origin HTTP URL");
    let url = config_string(config, key)?.ok_or_else(invalid_url)?;
    let uri = url.parse::<Uri>().map_err(|_| invalid_url())?;
    if url.contains('#') || uri.query().is_some() {
        return Err(invalid_url());
    }
    if url.starts_with('/') && !url.starts_with("//") {
        return Ok(uri.path().into());
    }
    let app_url = config_string(config, "app.url")?
        .and_then(|value| value.parse::<Uri>().ok())
        .ok_or_else(invalid_url)?;
    let default_port = match uri.scheme_str() {
        Some("http") => 80,
        Some("https") => 443,
        _ => return Err(invalid_url()),
    };
    let same_host = uri
        .host()
        .zip(app_url.host())
        .is_some_and(|(host, app_host)| host.eq_ignore_ascii_case(app_host));
    if !same_host
        || uri.scheme() != app_url.scheme()
        || uri.port_u16().unwrap_or(default_port) != app_url.port_u16().unwrap_or(default_port)
    {
        return Err(invalid_url());
    }
    Ok(uri.path().into())
}

fn static_config_error(key: &str, expected: &'static str) -> RoutingError {
    RoutingError::StaticMountConfiguration {
        source: ConfigError::InvalidType {
            key: key.into(),
            expected,
        },
    }
}

/// Registers commands on the console bound by the current application.
pub struct Artisan;

impl Artisan {
    pub fn command(
        name: &'static str,
        callback: impl Fn(&mut Output) + Send + Sync + 'static,
    ) -> Result<RegisteredCommand, ConsoleError> {
        CONSOLE
            .try_with(|console| {
                let mut console = console.borrow_mut();
                let command = console.command(name, callback)?;
                Ok(RegisteredCommand {
                    name,
                    definition: command.registration_key(),
                })
            })
            .map_err(|_| ConsoleError::new("No active Artisan registration scope."))?
    }

    /// Adds a typed command to the console in the active registration scope.
    pub fn add(command: Box<dyn crate::console::Command>) -> Result<(), ConsoleError> {
        CONSOLE
            .try_with(|console| console.borrow_mut().add_command(command))
            .map_err(|_| ConsoleError::new("No active Artisan registration scope."))?
    }

    pub(crate) fn register(
        console: &mut Application,
        register: fn() -> Result<(), ConsoleError>,
    ) -> Result<(), ConsoleError> {
        CONSOLE.sync_scope(RefCell::new(std::mem::take(console)), || {
            let result = register();
            *console = CONSOLE.with(|active| std::mem::take(&mut *active.borrow_mut()));
            result
        })
    }
}

/// Configures the command just registered through the Artisan facade.
pub struct RegisteredCommand {
    name: &'static str,
    definition: Weak<clap::Command>,
}

impl RegisteredCommand {
    pub fn purpose(self, description: &'static str) -> Result<(), ConsoleError> {
        CONSOLE
            .try_with(|console| {
                console
                    .borrow_mut()
                    .set_closure_purpose(self.name, &self.definition, description)
            })
            .map_err(|_| ConsoleError::new("No active Artisan registration scope."))?
    }
}
