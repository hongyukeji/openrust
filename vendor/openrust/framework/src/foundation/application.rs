use std::{
    collections::HashSet,
    error::Error,
    ffi::OsString,
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::Arc,
};

use super::{
    BootstrapError,
    bootstrap::{ConfigLoader, LoadConfiguration, LoadEnvironmentVariables},
    configuration::{ApplicationBuilder, CommandRoutes, Exceptions, Middleware, Routing},
    console::Kernel,
    exceptions::Handler,
    http,
};
use crate::{
    config::{ConfigError, Repository, Value},
    console::{Application as Artisan, ConsoleError, Output},
    container::{Container, ServiceKey},
    contracts::{
        config,
        console::Kernel as _,
        container::Container as _,
        debug::{Exception, ExceptionHandler as _},
        foundation,
    },
    routing::Router,
    support::ServiceProvider,
};

pub(super) type CommandRegistrar = fn(&mut Artisan) -> Result<(), ConsoleError>;
pub(super) type MiddlewareConfigurator = Box<dyn FnOnce(&mut Middleware) + Send + Sync>;

/// Owns the application root, service container, and provider lifecycle.
#[allow(non_snake_case, reason = "Retains Laravel's basePath property name")]
pub struct Application {
    basePath: PathBuf,
    publicPath: Option<PathBuf>,
    config: Arc<Repository>,
    load_config: ConfigLoader,
    container: Arc<Container>,
    providers: Vec<Box<dyn ServiceProvider>>,
    state: BootState,
    routing: Option<Routing>,
    pub(super) middleware_configurators: Vec<MiddlewareConfigurator>,
    pub(super) commands: CommandRegistrar,
    pub(super) command_routes: Option<CommandRoutes>,
    pub(super) exceptions: Arc<Handler>,
    pub(super) routes: Router,
}

enum BootState {
    Pending,
    Failed,
    Booted,
}

impl Application {
    #[allow(non_snake_case, reason = "Uses Laravel's basePath argument name")]
    pub fn configure(basePath: PathBuf) -> ApplicationBuilder {
        ApplicationBuilder::new(basePath)
    }

    /// Resolves an explicit override, a source root, or the executable's directory.
    #[allow(non_snake_case, reason = "Matches Laravel's Application API")]
    pub fn inferBasePath(source_root: Option<&str>) -> Result<PathBuf, BootstrapError> {
        let basePath = match (std::env::var_os("APP_BASE_PATH"), source_root) {
            (Some(path), _) => PathBuf::from(path),
            (None, Some(path)) => PathBuf::from(path),
            (None, None) => executable_base_path()
                .map_err(|source| BootstrapError::ExecutablePathUnavailable { source })?,
        };
        if !basePath.is_absolute() {
            return Err(BootstrapError::InvalidBasePath {
                path: basePath,
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "expected an absolute application root",
                ),
            });
        }
        Ok(basePath)
    }

    /// Runs the HTTP service through the HTTP kernel.
    pub async fn start(self) -> Result<(), Box<dyn Error + Send + Sync>> {
        http::Kernel::new(self).serve().await
    }

    /// Returns the application's configured public directory.
    #[allow(non_snake_case, reason = "Matches Laravel's Application API")]
    pub fn publicPath(&self) -> PathBuf {
        self.publicPath
            .clone()
            .unwrap_or_else(|| self.basePath.join("public"))
    }

    /// Overrides the public directory. Relative paths are resolved from the application root.
    #[allow(non_snake_case, reason = "Matches Laravel's Application API")]
    pub fn usePublicPath(&mut self, path: impl AsRef<Path>) -> &mut Self {
        self.publicPath = Some(self.basePath.join(path));
        self
    }

    pub(crate) fn configuration_repository(&self) -> Arc<dyn config::Repository> {
        self.config.clone()
    }

    pub(crate) fn render_console_exception(
        &self,
        output: &mut Output,
        error: &(dyn Error + Send + Sync + 'static),
    ) {
        let exception = Exception::Error(error);
        let _ = self.exceptions.report(&exception);
        self.exceptions.renderForConsole(output, &exception);
    }

    /// Executes an Artisan invocation and terminates its console kernel.
    #[allow(non_snake_case, reason = "Matches Laravel's Application API")]
    pub async fn handleCommand(
        self,
        arguments: &[OsString],
    ) -> Result<i32, Box<dyn Error + Send + Sync>> {
        let mut kernel = Kernel::for_artisan(self);
        let mut output = Output::for_terminal();
        let status = kernel.handle(arguments, &mut output).await;
        kernel.terminate(arguments, status).await;
        io::stdout().write_all(output.stdout.as_bytes())?;
        io::stderr().write_all(output.stderr.as_bytes())?;
        Ok(status)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's constructor")]
    pub(super) fn new(
        basePath: PathBuf,
        load_config: ConfigLoader,
        providers: Vec<Box<dyn ServiceProvider>>,
        routing: Option<Routing>,
        middleware_configurators: Vec<MiddlewareConfigurator>,
        commands: CommandRegistrar,
        exceptions: Option<Exceptions>,
    ) -> Self {
        let config = Arc::new(Repository::new(Default::default()));
        let container = Arc::new(Container::default());
        let repository: Arc<dyn config::Repository> = config.clone();
        container.instance(repository);
        container
            .alias(ServiceKey::typed::<dyn config::Repository>(), "config")
            .expect("typed and named configuration keys are distinct");
        let command_routes = routing.as_ref().and_then(|routing| routing.command_routes);
        let mut seen = HashSet::new();
        let providers = providers
            .into_iter()
            .filter(|provider| seen.insert(provider.as_ref().type_id()))
            .collect();
        Self {
            basePath,
            publicPath: None,
            config,
            load_config,
            container,
            providers,
            state: BootState::Pending,
            routing,
            middleware_configurators,
            commands,
            command_routes,
            exceptions: Arc::new(Handler::new(exceptions)),
            routes: Router::new(),
        }
    }

    pub(crate) fn container_handle(&self) -> Arc<Container> {
        self.container.clone()
    }

    pub async fn boot(&mut self) -> Result<(), BootstrapError> {
        crate::support::facades::scope(self.container.clone(), self.boot_application()).await
    }

    async fn boot_application(&mut self) -> Result<(), BootstrapError> {
        match self.state {
            BootState::Booted => return Ok(()),
            BootState::Failed => return Err(BootstrapError::PreviousBootFailed),
            BootState::Pending => self.state = BootState::Failed,
        }
        let configuration_failed = |source| BootstrapError::ConfigurationFailed { source };
        let environment = LoadEnvironmentVariables::bootstrap(&self.basePath, None)
            .map_err(configuration_failed)?;
        let config = LoadConfiguration::bootstrap(&self.basePath, &environment, self.load_config)
            .map_err(configuration_failed)?;
        if self.publicPath.is_none() {
            match config::Repository::get(&config, "app.public_path") {
                None | Some(Value::Null) => {}
                Some(Value::String(path)) if !path.trim().is_empty() => {
                    self.usePublicPath(path);
                }
                _ => {
                    return Err(configuration_failed(ConfigError::InvalidType {
                        key: "app.public_path".into(),
                        expected: "a non-empty directory path",
                    }));
                }
            }
        }
        self.config.replace_with(config);

        let mut providers = std::mem::take(&mut self.providers);
        for provider in &mut providers {
            let (provider_type, name) = (provider.as_ref().type_id(), provider.name());
            provider.register(self).map_err(|source| {
                BootstrapError::ProviderRegistrationFailed {
                    provider: provider_type,
                    name,
                    source,
                }
            })?;
        }
        for provider in &mut providers {
            let (provider_type, name) = (provider.as_ref().type_id(), provider.name());
            provider
                .boot(self)
                .await
                .map_err(|source| BootstrapError::ProviderBootFailed {
                    provider: provider_type,
                    name,
                    source,
                })?;
        }
        let routing_failed = |source| BootstrapError::RoutingFailed { source };
        self.routes = match self.routing.take() {
            Some(routing) => routing.register(self).map_err(routing_failed)?,
            None => Router::new(),
        };
        self.providers = providers;
        self.state = BootState::Booted;
        Ok(())
    }
}

fn executable_base_path() -> io::Result<PathBuf> {
    let mut path = std::env::current_exe()?.canonicalize()?;
    if !path.pop() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "executable has no parent directory",
        ));
    }
    Ok(path)
}

impl foundation::Application for Application {
    type Container = Container;

    fn base_path(&self) -> &Path {
        &self.basePath
    }
    fn config(&self) -> &dyn config::Repository {
        self.config.as_ref()
    }
    fn container(&self) -> &Container {
        &self.container
    }
    fn is_booted(&self) -> bool {
        matches!(self.state, BootState::Booted)
    }
}
