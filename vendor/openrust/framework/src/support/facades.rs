//! Static entry points for the application's route and command registration.

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
                    .mount_static(&prefix, Path::new(&root))
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    pub fn get<Arguments>(
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<(), RoutingError> {
        ROUTER
            .try_with(|scope| scope.router.borrow_mut().get(uri, handler).map(|_| ()))
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    pub(crate) fn register(
        router: &mut Router,
        application: &foundation::Application,
        register: fn() -> Result<(), RoutingError>,
    ) -> Result<(), RoutingError> {
        let scope = RouteRegistration {
            router: RefCell::new(std::mem::take(router)),
            config: application.configuration_repository(),
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
