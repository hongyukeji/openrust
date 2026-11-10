use std::{
    collections::HashSet,
    error::Error,
    ffi::OsString,
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock, RwLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
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
    pub(super) shared: Arc<ApplicationState>,
}

#[allow(non_snake_case, reason = "Retains Laravel application state names")]
pub(super) struct ApplicationState {
    basePath: PathBuf,
    publicPath: RwLock<Option<PathBuf>>,
    load_config: ConfigLoader,
    pub(super) container: Arc<Container>,
    providers: Mutex<Vec<Box<dyn ServiceProvider>>>,
    deferred: OnceLock<Arc<super::provider_repository::ProviderRepository>>,
    state: AtomicU8,
    booting_started: AtomicBool,
    routing: Mutex<Option<Routing>>,
    pub(super) middleware_configurators: Mutex<Vec<MiddlewareConfigurator>>,
    pub(super) commands: CommandRegistrar,
    pub(super) command_routes: Option<CommandRoutes>,
    pub(super) exceptions: Arc<Handler>,
    pub(super) routes: Mutex<Option<Router>>,
}

const PENDING: u8 = 0;
const FAILED: u8 = 1;
const BOOTED: u8 = 2;

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
        self.shared
            .publicPath
            .read()
            .expect("application path lock poisoned")
            .clone()
            .unwrap_or_else(|| self.shared.basePath.join("public"))
    }

    /// Overrides the public directory. Relative paths are resolved from the application root.
    #[allow(non_snake_case, reason = "Matches Laravel's Application API")]
    pub fn usePublicPath(&mut self, path: impl AsRef<Path>) -> &mut Self {
        *self
            .shared
            .publicPath
            .write()
            .expect("application path lock poisoned") = Some(self.shared.basePath.join(path));
        self
    }

    pub(crate) fn configuration_repository(
        &self,
    ) -> Result<Arc<dyn config::Repository>, crate::contracts::container::BindingResolutionError>
    {
        self.shared
            .container
            .get::<dyn config::Repository>("config")
    }

    pub(crate) fn render_console_exception(
        &self,
        output: &mut Output,
        error: &(dyn Error + Send + Sync + 'static),
    ) {
        let exception = Exception::Error(error);
        let _ = self.shared.exceptions.report(&exception);
        self.shared.exceptions.renderForConsole(output, &exception);
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
        let container = Arc::new(Container::default());
        container
            .alias("config", ServiceKey::typed::<dyn config::Repository>())
            .expect("typed and named configuration keys are distinct");
        let command_routes = routing.as_ref().and_then(|routing| routing.command_routes);
        let mut seen = HashSet::new();
        let providers: Vec<_> = providers
            .into_iter()
            .filter(|provider| seen.insert(provider.providerType()))
            .collect();
        let (deferred, providers): (Vec<_>, Vec<_>) = providers
            .into_iter()
            .partition(|provider| provider.isDeferred());
        let shared = Arc::new(ApplicationState {
            basePath,
            publicPath: RwLock::new(None),
            load_config,
            container,
            providers: Mutex::new(providers),
            deferred: OnceLock::new(),
            state: AtomicU8::new(PENDING),
            booting_started: AtomicBool::new(false),
            routing: Mutex::new(routing),
            middleware_configurators: Mutex::new(middleware_configurators),
            commands,
            command_routes,
            exceptions: Arc::new(Handler::new(exceptions)),
            routes: Mutex::new(None),
        });
        let deferred =
            super::provider_repository::ProviderRepository::new(Arc::downgrade(&shared), deferred);
        if !deferred.is_empty() {
            shared.container.install_deferred(deferred.clone());
        }
        assert!(
            shared.deferred.set(deferred).is_ok(),
            "provider registry already initialized"
        );
        Self { shared }
    }

    /// Resolve a service, awaiting deferred provider boot when the application is ready.
    pub async fn make<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> Result<Arc<T>, crate::contracts::container::BindingResolutionError> {
        crate::support::facades::application_scope(
            self.container_handle(),
            self.shared.container.makeAsync::<T>(),
        )
        .await
    }

    pub async fn get<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<ServiceKey> + Send,
    ) -> Result<Arc<T>, crate::contracts::container::BindingResolutionError> {
        crate::support::facades::application_scope(
            self.container_handle(),
            self.shared.container.getAsync::<T>(key),
        )
        .await
    }

    /// Resolve a fresh service when explicit parameters are supplied.
    #[allow(non_snake_case, reason = "Matches Laravel's application API")]
    pub async fn makeWith<T: ?Sized + Send + Sync + 'static>(
        &self,
        parameters: crate::container::ParameterOverrides,
    ) -> Result<Arc<T>, crate::contracts::container::BindingResolutionError> {
        crate::support::facades::application_scope(
            self.container_handle(),
            self.shared.container.makeWithAsync::<T>(parameters),
        )
        .await
    }
    #[allow(non_snake_case, reason = "Named Rust overload")]
    pub async fn makeWithNamed<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<ServiceKey> + Send,
        parameters: crate::container::ParameterOverrides,
    ) -> Result<Arc<T>, crate::contracts::container::BindingResolutionError> {
        crate::support::facades::application_scope(
            self.container_handle(),
            self.shared
                .container
                .makeWithAsyncNamed::<T>(key, parameters),
        )
        .await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's application API")]
    pub fn isDeferredService(&self, key: impl Into<ServiceKey>) -> bool {
        use crate::container::deferred::DeferredServices as _;
        self.shared
            .deferred
            .get()
            .expect("provider registry initialized")
            .contains(&key.into())
    }

    #[allow(non_snake_case, reason = "Matches Laravel's application API")]
    pub async fn loadDeferredProviders(
        &self,
    ) -> Result<(), crate::contracts::container::BindingResolutionError> {
        self.shared
            .deferred
            .get()
            .expect("provider registry initialized")
            .load_all()
            .await
    }

    pub(super) fn booting_started(&self) -> bool {
        self.shared.booting_started.load(Ordering::Acquire)
    }

    pub(crate) fn container_handle(&self) -> Arc<Container> {
        self.shared.container.clone()
    }

    pub async fn boot(&mut self) -> Result<(), BootstrapError> {
        crate::support::facades::scope(self.shared.container.clone(), self.boot_application()).await
    }

    async fn boot_application(&mut self) -> Result<(), BootstrapError> {
        match self.shared.state.compare_exchange(
            PENDING,
            FAILED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {}
            Err(BOOTED) => return Ok(()),
            Err(_) => return Err(BootstrapError::PreviousBootFailed),
        }

        let configuration_failed = |source| BootstrapError::ConfigurationFailed { source };
        let environment = LoadEnvironmentVariables::bootstrap(&self.shared.basePath, None)
            .map_err(configuration_failed)?;
        let config = Arc::new(Repository::new(Default::default()));
        self.shared
            .container
            .instanceNamed::<dyn config::Repository>("config", config.clone())
            .map_err(|source| BootstrapError::ContainerFailed { source })?;
        let loaded = LoadConfiguration::bootstrap(
            &self.shared.basePath,
            &environment,
            self.shared.load_config,
        )
        .map_err(configuration_failed)?;
        config.replace_with(loaded);
        if self
            .shared
            .publicPath
            .read()
            .expect("application path lock poisoned")
            .is_none()
        {
            match config::Repository::get(config.as_ref(), "app.public_path") {
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

        let mut providers = std::mem::take(
            &mut *self
                .shared
                .providers
                .lock()
                .expect("provider list lock poisoned"),
        );
        for provider in &mut providers {
            let (provider_type, name) = (provider.providerType(), provider.name());
            provider.register(self).await.map_err(|source| {
                BootstrapError::ProviderRegistrationFailed {
                    provider: provider_type,
                    name,
                    source,
                }
            })?;
        }
        self.shared.booting_started.store(true, Ordering::Release);
        self.shared
            .deferred
            .get()
            .expect("provider registry initialized")
            .boot_prepared()
            .await
            .map_err(|source| BootstrapError::ContainerFailed { source })?;
        for provider in &mut providers {
            let (provider_type, name) = (provider.providerType(), provider.name());
            provider
                .boot(self)
                .await
                .map_err(|source| BootstrapError::ProviderBootFailed {
                    provider: provider_type,
                    name,
                    source,
                })?;
        }
        self.shared
            .deferred
            .get()
            .expect("provider registry initialized")
            .boot_registered()
            .await
            .map_err(|source| BootstrapError::ContainerFailed { source })?;
        let routing_failed = |source| BootstrapError::RoutingFailed { source };
        let routing = self
            .shared
            .routing
            .lock()
            .expect("application routing lock poisoned")
            .take();
        let routes = match routing {
            Some(routing) => routing.register(self).map_err(routing_failed)?,
            None => Router::new(),
        };
        *self
            .shared
            .routes
            .lock()
            .expect("application routes lock poisoned") = Some(routes);
        *self
            .shared
            .providers
            .lock()
            .expect("provider list lock poisoned") = providers;
        self.shared
            .deferred
            .get()
            .expect("provider registry initialized")
            .boot_registered()
            .await
            .map_err(|source| BootstrapError::ContainerFailed { source })?;
        self.shared.state.store(BOOTED, Ordering::Release);
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
        &self.shared.basePath
    }
    fn config(
        &self,
    ) -> Result<Arc<dyn config::Repository>, crate::contracts::container::BindingResolutionError>
    {
        self.configuration_repository()
    }
    fn container(&self) -> &Container {
        &self.shared.container
    }
    fn is_booted(&self) -> bool {
        self.shared.state.load(Ordering::Acquire) == BOOTED
    }
}
