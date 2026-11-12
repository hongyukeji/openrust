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
    BootstrapError, RuntimeService,
    bootstrap::{ConfigLoader, LoadConfiguration, LoadEnvironmentVariables},
    configuration::{ApplicationBuilder, CommandRoutes, Exceptions, Middleware, Routing},
    console::Kernel,
    environment_detector,
    exceptions::Handler,
    http,
};
use crate::{
    config::{ConfigError, Repository, Value},
    console::{Application as Artisan, ConsoleError, Output},
    container::{Container, EnvironmentQuery, EnvironmentResult, ServiceKey},
    contracts::{
        config,
        console::Kernel as _,
        container::Container as _,
        debug::{Exception, ExceptionHandler},
        foundation,
    },
    grpc,
    routing::Router,
    support::ServiceProvider,
};

pub(super) type CommandRegistrar = fn(&mut Artisan) -> Result<(), ConsoleError>;
pub(crate) type PackageRoutes = fn() -> Result<(), crate::routing::RoutingError>;
pub(crate) type PackageCommands = fn() -> Result<(), ConsoleError>;
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
    runtime_services: Mutex<Vec<Arc<dyn RuntimeService>>>,
    deferred: OnceLock<Arc<super::provider_repository::ProviderRepository>>,
    state: AtomicU8,
    booting_started: AtomicBool,
    routing: Mutex<Option<Routing>>,
    pub(super) grpc_registrar: Mutex<Option<grpc::Registrar>>,
    package_routes: Mutex<Option<Vec<PackageRoutes>>>,
    pub(super) package_commands: Mutex<Vec<PackageCommands>>,
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

    /// Registers an optional listener during application assembly.
    #[allow(non_snake_case, reason = "Matches the application configuration API")]
    pub fn registerRuntimeService(&self, service: Arc<dyn RuntimeService>) {
        self.shared.runtime_services.lock().unwrap().push(service);
    }

    pub(super) fn take_runtime_services(&self) -> Vec<Arc<dyn RuntimeService>> {
        std::mem::take(&mut *self.shared.runtime_services.lock().unwrap())
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

    /// Returns the detected name with `None`, or matches any supplied pattern.
    pub fn environment(
        &self,
        patterns: Option<EnvironmentQuery<'_>>,
    ) -> Result<EnvironmentResult, crate::contracts::container::BindingResolutionError> {
        environment_detector::query(&self.shared.container, patterns)
    }

    pub(crate) fn register_package_routes(&self, register: PackageRoutes) -> std::io::Result<()> {
        self.shared
            .package_routes
            .lock()
            .expect("package route list lock poisoned")
            .as_mut()
            .ok_or_else(|| {
                io::Error::other("Package routes must be registered before route assembly.")
            })?
            .push(register);
        Ok(())
    }

    pub(crate) fn register_package_commands(&self, register: PackageCommands) {
        self.shared
            .package_commands
            .lock()
            .expect("package command list lock poisoned")
            .push(register);
    }

    pub(crate) fn route_info(&self) -> Vec<crate::routing::RouteInfo> {
        self.shared
            .routes
            .lock()
            .expect("application routes lock poisoned")
            .as_ref()
            .map(Router::getRoutes)
            .unwrap_or_default()
    }

    pub(crate) fn configuration_repository(
        &self,
    ) -> Result<Arc<dyn config::Repository>, crate::contracts::container::BindingResolutionError>
    {
        self.shared
            .container
            .get::<dyn config::Repository>("config")
    }

    /// Reports an application error through this application's exception handler.
    pub async fn report(
        &self,
        error: &(dyn Error + Send + Sync + 'static),
    ) -> crate::log::LogResult<()> {
        self.shared
            .exceptions
            .report(&Exception::Error(error))
            .await
    }

    pub(crate) async fn render_console_exception(
        &self,
        output: &mut Output,
        error: &(dyn Error + Send + Sync + 'static),
    ) {
        let exception = Exception::Error(error);
        let _ = self.shared.exceptions.report(&exception).await;
        self.shared.exceptions.renderForConsole(output, &exception);
    }

    /// Executes an Artisan invocation and terminates its console kernel.
    #[allow(non_snake_case, reason = "Matches Laravel's Application API")]
    pub async fn handleCommand(
        self,
        arguments: &[OsString],
    ) -> Result<i32, Box<dyn Error + Send + Sync>> {
        let mut output = Output::for_terminal();
        let status = self.run_console(arguments, &mut output).await;
        io::stdout().write_all(output.stdout.as_bytes())?;
        io::stderr().write_all(output.stderr.as_bytes())?;
        Ok(status)
    }

    pub(crate) async fn run_console(self, arguments: &[OsString], output: &mut Output) -> i32 {
        let mut kernel = Kernel::for_artisan(self);
        let status = kernel.handle(arguments, output).await;
        kernel.terminate(arguments, status).await;
        status
    }

    pub(crate) fn share(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
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
        let base_providers: [Box<dyn ServiceProvider>; 23] = [
            Box::new(crate::log::LogServiceProvider),
            Box::new(crate::events::EventServiceProvider),
            Box::new(crate::broadcasting::BroadcastServiceProvider),
            Box::new(crate::database::DatabaseServiceProvider),
            Box::new(crate::database::MigrationServiceProvider),
            Box::new(crate::filesystem::FilesystemServiceProvider),
            Box::new(crate::encryption::EncryptionServiceProvider),
            Box::new(crate::cookie::CookieServiceProvider),
            Box::new(crate::session::SessionServiceProvider),
            Box::new(crate::hashing::HashingServiceProvider),
            Box::new(crate::http::client::HttpClientServiceProvider),
            Box::new(crate::image::ImageServiceProvider),
            Box::new(crate::process::ProcessServiceProvider),
            Box::new(crate::concurrency::ConcurrencyServiceProvider),
            Box::new(crate::redis::RedisServiceProvider),
            Box::new(crate::queue::QueueServiceProvider),
            Box::new(crate::cache::CacheServiceProvider),
            Box::new(crate::view::ViewServiceProvider),
            Box::new(crate::translation::TranslationServiceProvider),
            Box::new(crate::mail::MailServiceProvider),
            Box::new(crate::notifications::NotificationServiceProvider),
            Box::new(crate::validation::ValidationServiceProvider),
            Box::new(crate::auth::AuthServiceProvider),
        ];
        let providers: Vec<_> = base_providers
            .into_iter()
            .chain(providers)
            .filter(|provider| seen.insert(provider.providerType()))
            .collect();
        let (deferred, providers): (Vec<_>, Vec<_>) = providers
            .into_iter()
            .partition(|provider| provider.isDeferred());
        let exceptions = Arc::new(Handler::new(exceptions, Arc::downgrade(&container)));
        container
            .instance::<dyn ExceptionHandler>(exceptions.clone())
            .expect("the application exception handler is initially unbound");
        let shared = Arc::new(ApplicationState {
            basePath,
            publicPath: RwLock::new(None),
            load_config,
            container,
            providers: Mutex::new(providers),
            runtime_services: Mutex::default(),
            deferred: OnceLock::new(),
            state: AtomicU8::new(PENDING),
            booting_started: AtomicBool::new(false),
            routing: Mutex::new(routing),
            grpc_registrar: Mutex::new(None),
            package_routes: Mutex::new(Some(Vec::new())),
            package_commands: Mutex::default(),
            middleware_configurators: Mutex::new(middleware_configurators),
            commands,
            command_routes,
            exceptions,
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

    /// Return a lazy application-aware resolver; no service is resolved at creation.
    pub fn factory<'a, T: ?Sized + Send + Sync + 'static>(
        &'a self,
    ) -> impl Fn() -> crate::container::FactoryFuture<'a, T> + Send + Sync {
        move || Box::pin(self.make::<T>())
    }

    #[allow(non_snake_case, reason = "Named Rust application factory overload")]
    pub fn factoryNamed<'a, T: ?Sized + Send + Sync + 'static>(
        &'a self,
        key: ServiceKey,
    ) -> impl Fn() -> crate::container::FactoryFuture<'a, T> + Send + Sync {
        move || Box::pin(self.get::<T>(key.clone()))
    }

    /// Defer repeated calls, establishing this application's Facade context each time.
    pub fn wrap<'a, R, F, Fut>(
        &'a self,
        callback: crate::container::Callback<'a, F>,
        parameters: crate::container::ParameterOverrides,
    ) -> impl Fn() -> crate::container::CallFuture<'a, R> + Send + Sync + 'a
    where
        R: Send + 'a,
        F: Fn(crate::container::CallArguments) -> Fut + Send + Sync + 'a,
        Fut: std::future::Future<
                Output = Result<R, crate::contracts::container::BindingResolutionError>,
            > + Send
            + 'a,
    {
        let wrapped = self.shared.container.wrap(callback, parameters);
        move || {
            Box::pin(crate::support::facades::application_scope(
                self.container_handle(),
                wrapped(),
            ))
        }
    }

    /// Invoke a compiled callable inside this application's Facade context.
    pub async fn call<'a, R: Send + 'a, Target: crate::container::Callable<'a, R> + 'a>(
        &'a self,
        target: Target,
        parameters: crate::container::ParameterOverrides,
    ) -> Result<R, crate::contracts::container::BindingResolutionError> {
        crate::support::facades::application_scope(
            self.container_handle(),
            self.shared.container.call(target, parameters),
        )
        .await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's application API")]
    pub async fn callMethodBinding<C: ?Sized + Send + Sync + 'static, R: Send + 'static>(
        &self,
        method: &str,
        receiver: Arc<C>,
    ) -> Result<R, crate::contracts::container::BindingResolutionError> {
        crate::support::facades::application_scope(
            self.container_handle(),
            self.shared
                .container
                .callMethodBinding::<C, R>(method, receiver),
        )
        .await
    }

    #[allow(non_snake_case, reason = "Typed Rust static method overload")]
    pub async fn callStaticMethodBinding<C: ?Sized + 'static, R: Send + 'static>(
        &self,
        method: &str,
    ) -> Result<R, crate::contracts::container::BindingResolutionError> {
        crate::support::facades::application_scope(
            self.container_handle(),
            self.shared
                .container
                .callStaticMethodBinding::<C, R>(method),
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
        self.boot_with_environment(None).await
    }

    pub(super) async fn boot_with_environment(
        &mut self,
        selected: Option<&str>,
    ) -> Result<(), BootstrapError> {
        crate::support::facades::scope(
            self.shared.container.clone(),
            self.boot_application(selected),
        )
        .await
    }

    async fn boot_application(&mut self, selected: Option<&str>) -> Result<(), BootstrapError> {
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
        let environment = LoadEnvironmentVariables::bootstrap(&self.shared.basePath, selected)
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
        self.shared
            .container
            .instanceNamed("env.file", Arc::new(environment.file))
            .map_err(|source| BootstrapError::ContainerFailed { source })?;
        let name = match selected {
            Some(name) => Arc::from(name),
            None => match config::Repository::get(config.as_ref(), "app.env") {
                None => Arc::from("production"),
                Some(Value::String(name)) => Arc::from(name),
                Some(_) => {
                    return Err(configuration_failed(ConfigError::InvalidType {
                        key: "app.env".into(),
                        expected: "a string",
                    }));
                }
            },
        };
        environment_detector::install(&self.shared.container, name)
            .map_err(|source| BootstrapError::ContainerFailed { source })?;
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
        let mut routes = match routing {
            Some(routing) => routing.register(self).map_err(routing_failed)?,
            None => Router::new(),
        };
        let package_routes = self
            .shared
            .package_routes
            .lock()
            .expect("package route list lock poisoned")
            .take()
            .expect("package routes assembled once");
        for register in package_routes {
            crate::support::facades::Route::register(&mut routes, self, register)
                .map_err(routing_failed)?;
        }
        routes.resolve_static_directories(&self.shared.basePath);
        let config = self
            .configuration_repository()
            .map_err(|source| BootstrapError::ContainerFailed { source })?;
        crate::filesystem::register_signed_routes(&mut routes, config.as_ref())
            .map_err(routing_failed)?;
        self.shared
            .container
            .instanceNamed(
                "url",
                Arc::new(crate::routing::UrlGenerator::new(
                    routes.named_routes(),
                    config.clone(),
                )),
            )
            .map_err(|source| BootstrapError::ContainerFailed { source })?;
        self.shared
            .container
            .alias("url", ServiceKey::typed::<crate::routing::UrlGenerator>())
            .map_err(|source| BootstrapError::ContainerFailed { source })?;
        *self
            .shared
            .routes
            .lock()
            .expect("application routes lock poisoned") = Some(routes);
        let grpc = self
            .shared
            .grpc_registrar
            .lock()
            .expect("gRPC registration lock poisoned")
            .take();
        if let Some(register) = grpc {
            let mut routes = grpc::RoutesBuilder::default();
            register(self, &mut routes)
                .map_err(|source| BootstrapError::GrpcRegistrationFailed { source })?;
            self.registerRuntimeService(Arc::new(grpc::ConfiguredServer::new(
                routes.routes(),
                config,
            )));
        }

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
