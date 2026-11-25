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
    console::{ConsoleError, task_command::TaskCommand},
    database::migrations::{MigrationEntry, Migrator},
    foundation::{Application, Runner},
    http::HttpFuture,
    routing::RoutingError,
    translation::Translator,
    view::{Component, Factory, ViewData, ViewResult},
};

/// The result of a service provider lifecycle hook.
pub type ProviderResult = Result<(), Box<dyn Error + Send + Sync>>;

/// Compiled constructor used for a fresh deferred registration attempt.
pub type ProviderFactory = dyn Fn() -> Box<dyn ServiceProvider> + Send + Sync;

/// Registers service bindings before starting application services.
pub trait ServiceProvider: Any + Send + Sync {
    /// Names the provider in startup diagnostics.
    fn name(&self) -> &'static str {
        type_name::<Self>()
    }

    #[doc(hidden)]
    fn provider_class(&self) -> &'static str {
        type_name::<Self>()
    }

    #[doc(hidden)]
    fn provider_type(&self) -> TypeId {
        self.type_id()
    }

    #[doc(hidden)]
    fn deferred_factory(&self) -> Option<Arc<ProviderFactory>> {
        None
    }

    /// Declare deferred service keys instead of eagerly registering this provider.
    fn is_deferred(&self) -> bool {
        false
    }

    /// Exact typed or named service identities provided by a deferred provider.
    fn provides(&self) -> Vec<crate::container::ServiceKey> {
        Vec::new()
    }

    /// Event names that load this provider when it is deferred.
    fn when(&self) -> Vec<&'static str> {
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

    /// Provides runners from this booted service provider.
    fn runners(&self, _app: &Application) -> Vec<Arc<dyn Runner>> {
        Vec::new()
    }

    /// Add package defaults during registration. Existing application keys win at the top level.
    fn merge_config_from(&self, app: &Application, defaults: Value, key: &str) -> ProviderResult {
        merge_config(app, defaults, key, false)
    }

    /// Add nested defaults during registration. Application keys and array indices win.
    fn replace_config_recursively_from(
        &self,
        app: &Application,
        defaults: Value,
        key: &str,
    ) -> ProviderResult {
        merge_config(app, defaults, key, true)
    }

    /// Load a compiled route registrar during eager provider boot.
    fn load_routes_from(
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

    /// Register an Artisan command to run during a service reload.
    fn reloads(&self, app: &Application, command: &str, key: Option<&str>) {
        app.register_console_task(TaskCommand::Reload, provider_key(self.name(), key), command);
    }

    /// Register package commands for deployment optimization and cache clearing.
    fn optimizes(
        &self,
        app: &Application,
        optimize: Option<&str>,
        clear: Option<&str>,
        key: Option<&str>,
    ) {
        for (group, command) in [
            (TaskCommand::Optimize, optimize),
            (TaskCommand::OptimizeClear, clear),
        ] {
            if let Some(command) = command {
                app.register_console_task(group, provider_key(self.name(), key), command);
            }
        }
    }

    /// Register package views, preceded by application overrides under each view root.
    fn load_views_from<'a>(
        &'a self,
        app: &'a Application,
        path: &'a Path,
        namespace: &'a str,
    ) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<Factory>()
                .await?
                .get_finder()
                .add_package_namespace(namespace, path)?;
            Ok(())
        })
    }

    /// Register namespaced translations with application override support.
    fn load_translations_from<'a>(
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

    /// Register a package's anonymous JSON translations.
    fn load_json_translations_from<'a>(
        &'a self,
        app: &'a Application,
        path: &'a Path,
    ) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<Translator>()
                .await?
                .add_package_json_path(path.to_owned());
            Ok(())
        })
    }

    /// Register a compiled Blade component under a package prefix and alias.
    fn load_view_components_as<'a, C: Component + 'static>(
        &'a self,
        app: &'a Application,
        prefix: &'a str,
        alias: &'a str,
        resolve: fn(&ViewData) -> ViewResult<C>,
    ) -> HttpFuture<'a, ProviderResult>
    where
        Self: Sized,
    {
        Box::pin(async move {
            let name = if prefix.is_empty() {
                alias.to_owned()
            } else {
                format!("{prefix}-{alias}")
            };
            app.make::<Factory>().await?.component(name, resolve);
            Ok(())
        })
    }

    /// Publish compiled package resources under the given Artisan tag.
    fn publishes(
        &self,
        app: &Application,
        files: &[(&str, &'static [u8])],
        tag: &str,
    ) -> ProviderResult {
        crate::support::publishing::register(app, self.name(), files, tag, false)
    }

    /// 注册包文件或目录，目标支持应用内相对路径及明确指定的绝对路径。
    fn publishes_paths(
        &self,
        app: &Application,
        paths: &[(&Path, &Path)],
        tag: &str,
    ) -> ProviderResult {
        crate::support::publishing::register_paths(app, self.name(), paths, tag, false)
    }

    /// Register migration files or directories supplied by a package.
    fn publishes_migration_paths(
        &self,
        app: &Application,
        paths: &[(&Path, &Path)],
        tag: &str,
    ) -> ProviderResult {
        crate::support::publishing::register_paths(app, self.name(), paths, tag, true)
    }

    /// Publish package migration sources with fresh timestamps on first copy.
    fn publishes_migrations(
        &self,
        app: &Application,
        files: &[(&str, &'static [u8])],
        tag: &str,
    ) -> ProviderResult {
        crate::support::publishing::register(app, self.name(), files, tag, true)
    }

    /// Add compiled package migrations to the application's existing migration ledger.
    fn load_migrations_from<'a>(
        &'a self,
        app: &'a Application,
        migrations: &'static [MigrationEntry],
    ) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<Migrator>()
                .await?
                .add_package_migrations(migrations, false);
            Ok(())
        })
    }

    /// 使用已编译的应用同名迁移覆盖包迁移，保留包路径作为选择别名。
    fn load_migrations_with_application_overrides<'a>(
        &'a self,
        app: &'a Application,
        migrations: &'static [MigrationEntry],
    ) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<Migrator>()
                .await?
                .add_package_migrations(migrations, true);
            Ok(())
        })
    }
}

fn provider_key(name: &str, key: Option<&str>) -> String {
    let provider = name.rsplit("::").next().unwrap_or(name);
    let key = key.map(str::to_owned).unwrap_or_else(|| {
        super::Str::kebab(
            provider
                .split_once("ServiceProvider")
                .map_or(provider, |(name, _)| name),
        )
    });
    if key.is_empty() { provider.into() } else { key }
}

fn merge_config(
    app: &Application,
    mut defaults: Value,
    key: &str,
    recursive: bool,
) -> ProviderResult {
    if app.configuration_is_cached() {
        return Ok(());
    }
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
