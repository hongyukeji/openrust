use std::{
    any::{Any, TypeId, type_name},
    error::Error,
    future::Future,
    path::Path,
    pin::Pin,
    sync::Arc,
};

use crate::{
    config::{ConfigError, Value},
    console::ConsoleError,
    database::migrations::{MigrationEntry, Migrator},
    foundation::Application,
    http::HttpFuture,
    routing::RoutingError,
    translation::Translator,
    view::Factory,
};

/// The result of a service provider lifecycle hook.
pub type ProviderResult = Result<(), Box<dyn Error + Send + Sync>>;

/// Compiled constructor used for a fresh deferred registration attempt.
pub type ProviderFactory = dyn Fn() -> Box<dyn ServiceProvider> + Send + Sync;

/// Registers service bindings before starting application services.
#[allow(non_snake_case, reason = "Matches Laravel's provider API")]
pub trait ServiceProvider: Any + Send + Sync {
    /// Names the provider in startup diagnostics.
    fn name(&self) -> &'static str {
        type_name::<Self>()
    }

    #[doc(hidden)]
    #[allow(non_snake_case, reason = "Rust provider type identity adapter")]
    fn providerType(&self) -> TypeId {
        self.type_id()
    }

    #[doc(hidden)]
    #[allow(non_snake_case, reason = "Rust compiled provider constructor adapter")]
    fn deferredFactory(&self) -> Option<Arc<ProviderFactory>> {
        None
    }

    /// Declare deferred service keys instead of eagerly registering this provider.
    #[allow(non_snake_case, reason = "Matches Laravel's provider API")]
    fn isDeferred(&self) -> bool {
        false
    }

    /// Exact typed or named service identities provided by a deferred provider.
    fn provides(&self) -> Vec<crate::container::ServiceKey> {
        Vec::new()
    }

    /// Register bindings without starting services.
    fn register<'a>(
        &'a mut self,
        _app: &'a Application,
    ) -> Pin<Box<dyn Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    /// Initialize services after all providers have registered.
    fn boot<'a>(
        &'a mut self,
        _app: &'a Application,
    ) -> Pin<Box<dyn Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    /// Add package defaults during registration. Existing application keys win at the top level.
    fn mergeConfigFrom(&self, app: &Application, defaults: Value, key: &str) -> ProviderResult {
        merge_config(app, defaults, key, false)
    }

    /// Add nested defaults during registration. Application keys and array indices win.
    fn replaceConfigRecursivelyFrom(
        &self,
        app: &Application,
        defaults: Value,
        key: &str,
    ) -> ProviderResult {
        merge_config(app, defaults, key, true)
    }

    /// Load a compiled route registrar during eager provider boot.
    fn loadRoutesFrom(
        &self,
        app: &Application,
        register: fn() -> Result<(), RoutingError>,
    ) -> ProviderResult {
        Ok(app.register_package_routes(register)?)
    }

    /// Register compiled Artisan commands when the console kernel boots.
    fn commands(&self, app: &Application, register: fn() -> Result<(), ConsoleError>) {
        app.register_package_commands(register);
    }

    /// Register package views, preceded by application overrides under each view root.
    fn loadViewsFrom<'a>(
        &'a self,
        app: &'a Application,
        path: &'a Path,
        namespace: &'a str,
    ) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<Factory>()
                .await?
                .getFinder()
                .add_package_namespace(namespace, path)?;
            Ok(())
        })
    }

    /// Register namespaced translations with application override support.
    fn loadTranslationsFrom<'a>(
        &'a self,
        app: &'a Application,
        path: &'a Path,
        namespace: &'a str,
    ) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<Translator>()
                .await?
                .add_package_namespace(namespace, path.to_owned())?;
            Ok(())
        })
    }

    /// Add compiled package migrations to the application's existing migration ledger.
    fn loadMigrationsFrom<'a>(
        &'a self,
        app: &'a Application,
        migrations: &'static [MigrationEntry],
    ) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<Migrator>()
                .await?
                .add_package_migrations(migrations);
            Ok(())
        })
    }
}

fn merge_config(
    app: &Application,
    mut defaults: Value,
    key: &str,
    recursive: bool,
) -> ProviderResult {
    let repository = app.configuration_repository()?;
    if !matches!(&defaults, Value::Object(_) | Value::Array(_)) {
        return Err(Box::new(invalid_package_config(key)));
    }
    if let Some(overrides) = repository.get(key) {
        match (&mut defaults, overrides) {
            (Value::Object(defaults), Value::Object(overrides)) if !recursive => {
                defaults.extend(overrides);
            }
            (Value::Array(defaults), Value::Array(overrides)) if !recursive => {
                defaults.extend(overrides);
            }
            (defaults @ (Value::Object(_) | Value::Array(_)), overrides)
                if recursive
                    && defaults.is_object() == overrides.is_object()
                    && defaults.is_array() == overrides.is_array() =>
            {
                replace_config(defaults, overrides);
            }
            _ => return Err(Box::new(invalid_package_config(key))),
        }
    }
    Ok(repository.set(key, defaults)?)
}

fn invalid_package_config(key: &str) -> ConfigError {
    ConfigError::InvalidType {
        key: key.to_owned(),
        expected: "package defaults and overrides of the same object or array type",
    }
}

fn replace_config(defaults: &mut Value, overrides: Value) {
    match (defaults, overrides) {
        (Value::Object(defaults), Value::Object(overrides)) => {
            for (key, value) in overrides {
                replace_config(defaults.entry(key).or_insert(Value::Null), value);
            }
        }
        (Value::Array(defaults), Value::Array(overrides)) => {
            for (index, value) in overrides.into_iter().enumerate() {
                if let Some(default) = defaults.get_mut(index) {
                    replace_config(default, value);
                } else {
                    defaults.push(value);
                }
            }
        }
        (default, value) => *default = value,
    }
}
