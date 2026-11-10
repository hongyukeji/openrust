use std::{collections::BTreeMap, io, path::PathBuf};

use super::{Exceptions, Middleware, Routing};
use crate::{
    foundation::{
        Application, BootstrapError,
        application::{CommandRegistrar, MiddlewareConfigurator},
        bootstrap::ConfigLoader,
    },
    support::ServiceProvider,
};

/// Collects the configuration used to create an application.
pub struct ApplicationBuilder {
    base_path: PathBuf,
    load_config: ConfigLoader,
    providers: Vec<Box<dyn ServiceProvider>>,
    routing: Option<Routing>,
    middleware_configurators: Vec<MiddlewareConfigurator>,
    commands: CommandRegistrar,
    exceptions: Option<Exceptions>,
}

#[allow(
    non_snake_case,
    reason = "Application setup uses Laravel's builder naming"
)]
impl ApplicationBuilder {
    pub(crate) fn new(base_path: PathBuf) -> Self {
        Self {
            base_path,
            load_config: |_, _| Ok(BTreeMap::new()),
            providers: Vec::new(),
            routing: None,
            middleware_configurators: Vec::new(),
            commands: |_| Ok(()),
            exceptions: None,
        }
    }

    pub fn withConfig(mut self, load: ConfigLoader) -> Self {
        self.load_config = load;
        self
    }

    pub fn withProviders(mut self, providers: Vec<Box<dyn ServiceProvider>>) -> Self {
        self.providers.extend(providers);
        self
    }

    pub fn withRouting(mut self, routing: Routing) -> Self {
        self.routing = Some(routing);
        self
    }

    /// Configures middleware when the HTTP kernel is constructed.
    pub fn withMiddleware(
        mut self,
        configure: impl FnOnce(&mut Middleware) + Send + Sync + 'static,
    ) -> Self {
        self.middleware_configurators.push(Box::new(configure));
        self
    }

    /// Registers application commands when Artisan starts.
    pub fn withCommands(mut self, register: CommandRegistrar) -> Self {
        self.commands = register;
        self
    }

    /// Configures exception rendering for the HTTP kernel.
    pub fn withExceptions(mut self, configure: impl FnOnce(&mut Exceptions)) -> Self {
        configure(self.exceptions.get_or_insert_with(Exceptions::default));
        self
    }

    pub fn create(self) -> Result<Application, BootstrapError> {
        let invalid_path = |source| BootstrapError::InvalidBasePath {
            path: self.base_path.clone(),
            source,
        };
        let base_path = self.base_path.canonicalize().map_err(invalid_path)?;
        if !base_path.is_dir() {
            return Err(invalid_path(io::Error::new(
                io::ErrorKind::NotADirectory,
                "expected a directory",
            )));
        }
        Ok(Application::new(
            base_path,
            self.load_config,
            self.providers,
            self.routing,
            self.middleware_configurators,
            self.commands,
            self.exceptions,
        ))
    }
}
