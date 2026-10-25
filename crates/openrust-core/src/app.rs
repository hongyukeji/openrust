//! Application assembly: [`Application`], [`AppBuilder`], [`App`] and [`AppState`].

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::sync::Arc;

use figment::Figment;
use serde::de::DeserializeOwned;

use crate::command::Command;
use crate::config::{ConfigError, FrameworkConfig};
use crate::error::{Error, Result};
use crate::provider::ServiceProvider;

/// Implemented once per project to describe how the application is assembled.
///
/// ```
/// use openrust_core::{AppBuilder, Application};
/// use openrust_core::config::NoConfig;
///
/// struct App;
///
/// impl Application for App {
///     type Config = NoConfig;
///
///     fn build(app: &mut AppBuilder) {
///         // app.provider(DatabaseProvider).routes(routes::register);
///         let _ = app;
///     }
/// }
/// ```
pub trait Application: Send + Sync + 'static {
    /// Application-specific configuration, extracted from the same layered sources as the
    /// framework configuration and registered as a service. Use
    /// [`NoConfig`](crate::config::NoConfig) when there is none.
    type Config: DeserializeOwned + Send + Sync + 'static;

    /// Registers providers, routes, commands and services.
    fn build(app: &mut AppBuilder);
}

/// Error returned when a handler or command asks for a service nobody registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("service `{0}` is not registered; call `AppBuilder::provide` from a ServiceProvider")]
pub struct ServiceNotRegistered(pub &'static str);

#[derive(Default)]
struct Registry {
    items: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl Registry {
    fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.items.insert(TypeId::of::<T>(), Arc::new(value));
    }

    fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.items
            .get(&TypeId::of::<T>())
            .and_then(|item| Arc::clone(item).downcast::<T>().ok())
    }
}

type Extensions = HashMap<TypeId, Box<dyn Any + Send + Sync>>;

/// Mutable assembly stage. Providers add services, commands and crate-specific hooks here.
pub struct AppBuilder {
    framework: FrameworkConfig,
    figment: Figment,
    services: Registry,
    providers: Vec<Box<dyn ServiceProvider>>,
    commands: Vec<Box<dyn Command>>,
    extensions: Extensions,
}

impl AppBuilder {
    /// Starts assembly from loaded framework configuration and its source [`Figment`].
    pub fn new(framework: FrameworkConfig, figment: Figment) -> Self {
        Self {
            framework,
            figment,
            services: Registry::default(),
            providers: Vec::new(),
            commands: Vec::new(),
            extensions: Extensions::new(),
        }
    }

    /// Framework configuration.
    pub fn config(&self) -> &FrameworkConfig {
        &self.framework
    }

    /// Extracts one configuration section, e.g. `section::<DatabaseConfig>("database")`.
    pub fn section<T: DeserializeOwned>(&self, key: &str) -> Result<T, ConfigError> {
        Ok(self.figment.extract_inner(key)?)
    }

    /// Queues a provider. Providers added while another provider registers are processed too.
    pub fn provider(&mut self, provider: impl ServiceProvider) -> &mut Self {
        self.providers.push(Box::new(provider));
        self
    }

    /// Registers a service, retrievable later via [`AppState::get`]. One instance per type.
    pub fn provide<T: Send + Sync + 'static>(&mut self, service: T) -> &mut Self {
        self.services.insert(service);
        self
    }

    /// Looks up a service already registered by an earlier provider.
    pub fn service<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.services.get()
    }

    /// Registers a runtime command.
    pub fn command(&mut self, command: impl Command) -> &mut Self {
        self.commands.push(Box::new(command));
        self
    }

    /// Mutable access to a crate-specific hook container, created on first use.
    ///
    /// Capability crates use this to collect registrations (route builders, migrations, jobs)
    /// during `register` and read them back from [`App::extension`] after the build.
    pub fn extension_mut<T: Default + Send + Sync + 'static>(&mut self) -> &mut T {
        let entry = self
            .extensions
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(T::default()));
        // The entry was inserted under `TypeId::of::<T>()`, so the downcast cannot fail.
        entry
            .downcast_mut::<T>()
            .unwrap_or_else(|| unreachable!("extension stored under a mismatched TypeId"))
    }

    pub(crate) fn take_providers(&mut self) -> Vec<Box<dyn ServiceProvider>> {
        std::mem::take(&mut self.providers)
    }

    pub(crate) fn has_pending_providers(&self) -> bool {
        !self.providers.is_empty()
    }

    /// Freezes the builder. Prefer [`finalize`](crate::lifecycle::finalize), which also runs providers.
    pub fn build(self) -> App {
        App {
            state: AppState {
                inner: Arc::new(StateInner {
                    framework: self.framework,
                    services: self.services,
                }),
            },
            figment: self.figment,
            commands: self.commands,
            extensions: self.extensions,
        }
    }
}

struct StateInner {
    framework: FrameworkConfig,
    services: Registry,
}

/// Cheaply cloneable handle shared with every request and job: configuration plus services.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<StateInner>,
}

impl AppState {
    /// Framework configuration.
    pub fn config(&self) -> &FrameworkConfig {
        &self.inner.framework
    }

    /// Returns the registered service of type `T`, or [`ServiceNotRegistered`] as an internal error.
    pub fn get<T: Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        self.try_get()
            .ok_or_else(|| Error::internal(ServiceNotRegistered(type_name::<T>())))
    }

    /// Returns the registered service of type `T`, if any.
    pub fn try_get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.inner.services.get()
    }
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("app", &self.inner.framework.app.name)
            .field("services", &self.inner.services.items.len())
            .finish()
    }
}

/// Fully assembled application: immutable state, commands and crate hooks.
pub struct App {
    state: AppState,
    figment: Figment,
    commands: Vec<Box<dyn Command>>,
    extensions: Extensions,
}

impl App {
    /// Shared state handed to HTTP handlers, jobs and commands.
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Framework configuration.
    pub fn config(&self) -> &FrameworkConfig {
        self.state.config()
    }

    /// Merged configuration source, for commands such as `config:show`.
    pub fn figment(&self) -> &Figment {
        &self.figment
    }

    /// Extracts one configuration section.
    pub fn section<T: DeserializeOwned>(&self, key: &str) -> Result<T, ConfigError> {
        Ok(self.figment.extract_inner(key)?)
    }

    /// Registered commands in registration order.
    pub fn commands(&self) -> impl Iterator<Item = &dyn Command> {
        self.commands.iter().map(AsRef::as_ref)
    }

    /// Crate-specific hook container populated during registration.
    pub fn extension<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.extensions
            .get(&TypeId::of::<T>())
            .and_then(|item| item.downcast_ref::<T>())
    }

    /// Runs the command called `name` with `args`.
    pub async fn run_command(&self, name: &str, args: Vec<String>) -> Result<()> {
        let command = self.commands().find(|c| c.name() == name).ok_or_else(|| {
            Error::custom(
                http::StatusCode::BAD_REQUEST,
                "unknown_command",
                format!("unknown command `{name}`; run `help` to list commands"),
            )
        })?;
        command.run(self, args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Counter(u32);

    #[derive(Default)]
    struct Hooks(Vec<&'static str>);

    #[test]
    fn services_are_typed_and_shared() {
        let mut builder = AppBuilder::new(FrameworkConfig::default(), Figment::new());
        builder.provide(Counter(7));
        let app = builder.build();
        assert_eq!(app.state().get::<Counter>().ok().map(|c| c.0), Some(7));
        assert!(app.state().try_get::<String>().is_none());
        let error = app
            .state()
            .get::<String>()
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("alloc::string::String"),
            "message was: {error}"
        );
    }

    #[test]
    fn extensions_accumulate_across_registrations() {
        let mut builder = AppBuilder::new(FrameworkConfig::default(), Figment::new());
        builder.extension_mut::<Hooks>().0.push("a");
        builder.extension_mut::<Hooks>().0.push("b");
        let app = builder.build();
        assert_eq!(
            app.extension::<Hooks>().map(|h| h.0.clone()),
            Some(vec!["a", "b"])
        );
    }

    #[tokio::test]
    async fn unknown_command_is_reported() {
        let app = AppBuilder::new(FrameworkConfig::default(), Figment::new()).build();
        let error = app.run_command("nope", Vec::new()).await.err();
        assert_eq!(error.as_ref().map(Error::code), Some("unknown_command"));
    }
}
