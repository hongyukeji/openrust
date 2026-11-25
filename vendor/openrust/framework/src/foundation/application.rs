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
    BootstrapError, Runner,
    bootstrap::{ConfigLoader, LoadConfiguration},
    configuration::{ApplicationBuilder, CommandRoutes, Exceptions, Middleware, Routing},
    console::Kernel,
    environment_detector,
    exceptions::Handler,
    http,
};
use crate::{
    config::{ConfigError, Repository, Value, cache::ConfigurationCache},
    console::{Application as Artisan, ConsoleError, Output, task_command::TaskCommand},
    container::{Container, EnvironmentQuery, EnvironmentResult, ServiceKey},
    contracts::{
        config,
        console::Kernel as _,
        container::Container as _,
        debug::{Exception, ExceptionHandler},
        foundation,
    },
    grpc,
    routing::{Router, route_cache::RouteCache},
    support::ServiceProvider,
};

pub(super) type LifecycleCallback =
    Box<dyn FnOnce(&Application) -> crate::support::ProviderResult + Send + Sync>;
pub(super) type ScheduleConfigurator = Box<
    dyn Fn(&mut crate::console::scheduling::Schedule) -> Result<(), ConsoleError> + Send + Sync,
>;

pub(super) type RunnerRegistrar = Box<dyn FnOnce() -> Vec<Arc<dyn Runner>> + Send>;
pub(super) type CommandRegistrar = fn(&mut Artisan) -> Result<(), ConsoleError>;
pub(crate) type PackageRoutes = fn() -> Result<(), crate::routing::RoutingError>;
pub(crate) type PackageCommands = fn() -> Result<(), ConsoleError>;
pub(super) type MiddlewareConfigurator = Box<dyn FnOnce(&mut Middleware) + Send + Sync>;

/// Owns the application root, service container, and provider lifecycle.
pub struct Application {
    pub(super) shared: Arc<ApplicationState>,
}

pub(super) struct ApplicationState {
    base_path: PathBuf,
    pub(super) lifecycle: Mutex<[Vec<LifecycleCallback>; 3]>,
    pub(super) prefer_json: AtomicBool,
    pub(super) schedule_configurators: Mutex<Vec<ScheduleConfigurator>>,
    public_path: RwLock<Option<PathBuf>>,
    paths: Arc<super::Paths>,
    load_config: ConfigLoader,
    testing_environment: AtomicBool,
    pub(super) container: Arc<Container>,
    providers: Mutex<Vec<Box<dyn ServiceProvider>>>,
    loaded_providers: Mutex<HashSet<&'static str>>,
    registered_runners: Mutex<Vec<Arc<dyn Runner>>>,
    custom_runners: Mutex<Option<RunnerRegistrar>>,
    deferred: OnceLock<Arc<super::provider_repository::ProviderRepository>>,
    state: AtomicU8,
    booting_started: AtomicBool,
    routing: Mutex<Option<Routing>>,
    pub(super) api_prefix: OnceLock<Arc<str>>,
    pub(super) grpc_registrar: Mutex<Option<grpc::Registrar>>,
    package_routes: Mutex<Option<Vec<PackageRoutes>>>,
    pub(crate) publishable: Mutex<Vec<crate::support::PublishableResource>>,
    pub(crate) keep_published_migration_dates: AtomicBool,
    pub(super) package_manifest: OnceLock<super::PackageManifest>,
    package_cache_path: OnceLock<PathBuf>,
    route_cache: OnceLock<(PathBuf, bool)>,
    event_cache_path: OnceLock<PathBuf>,
    pub(super) package_commands: Mutex<Vec<PackageCommands>>,
    console_tasks: Mutex<Vec<(TaskCommand, String, String)>>,
    middleware_configurators: Mutex<Vec<MiddlewareConfigurator>>,
    middleware: OnceLock<Middleware>,
    pub(super) commands: CommandRegistrar,
    pub(super) command_routes: Option<CommandRoutes>,
    pub(super) routes: Mutex<Option<Router>>,
}

tokio::task_local! {
    // 启动回调也在容器作用域中执行，但其中的配置与语言必须成为应用默认值。
    static BOOTSTRAPPING: Arc<Container>;
}

const PENDING: u8 = 0;
const FAILED: u8 = 1;
const BOOTED: u8 = 2;

impl Application {
    pub(crate) fn is_bootstrapping(container: &Container) -> bool {
        BOOTSTRAPPING
            .try_with(|owner| std::ptr::eq(owner.as_ref(), container))
            .unwrap_or(false)
    }

    /// 框架包的编译期版本，与应用配置版本相互独立。
    pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    /// 框架源码仓库。
    pub const REPOSITORY: &'static str = env!("CARGO_PKG_REPOSITORY");

    /// 返回当前框架版本。
    pub fn version(&self) -> &'static str {
        Self::VERSION
    }

    fn run_lifecycle(&self, phase: usize) -> Result<(), BootstrapError> {
        let callbacks =
            std::mem::take(&mut self.shared.lifecycle.lock().expect("应用生命周期锁中毒")[phase]);
        for callback in callbacks {
            callback(self).map_err(|source| BootstrapError::LifecycleFailed { source })?;
        }
        Ok(())
    }

    /// Checks a compiled provider class name after successful registration.
    pub fn provider_is_loaded(&self, provider: &str) -> bool {
        self.shared
            .loaded_providers
            .lock()
            .expect("loaded provider registry poisoned")
            .contains(provider)
    }

    pub(super) fn mark_as_registered(&self, provider: &dyn ServiceProvider) {
        let class = provider.provider_class();
        self.shared
            .loaded_providers
            .lock()
            .expect("loaded provider registry poisoned")
            .insert(class);
    }

    pub fn configure(base_path: PathBuf) -> ApplicationBuilder {
        ApplicationBuilder::new(base_path)
    }

    /// Resolves an explicit override, a source root, or the executable's directory.
    pub fn infer_base_path(source_root: Option<&str>) -> Result<PathBuf, BootstrapError> {
        let base_path = match (std::env::var_os("APP_BASE_PATH"), source_root) {
            (Some(path), _) => PathBuf::from(path),
            (None, Some(path)) => PathBuf::from(path),
            (None, None) => executable_base_path()
                .map_err(|source| BootstrapError::ExecutablePathUnavailable { source })?,
        };
        if !base_path.is_absolute() {
            return Err(BootstrapError::InvalidBasePath {
                path: base_path,
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "expected an absolute application root",
                ),
            });
        }
        Ok(base_path)
    }

    /// The packages linked by this application's Cargo build.
    pub fn package_manifest(&self) -> super::PackageManifest {
        self.shared
            .package_manifest
            .get()
            .copied()
            .unwrap_or_default()
    }

    /// Runs the HTTP kernel and any configured companion services.
    pub async fn start(self) -> Result<(), Box<dyn Error + Send + Sync>> {
        http::Kernel::new(self).serve().await
    }

    fn register_runner(&self, runner: Arc<dyn Runner>) {
        self.shared.registered_runners.lock().unwrap().push(runner);
    }

    pub(super) fn set_custom_runners(&self, runners: Option<RunnerRegistrar>) {
        *self.shared.custom_runners.lock().unwrap() = runners;
    }

    pub(super) async fn provider_runners(&self) -> Vec<Arc<dyn Runner>> {
        let mut runners: Vec<_> = self
            .shared
            .providers
            .lock()
            .unwrap()
            .iter()
            .flat_map(|provider| provider.runners(self))
            .collect();
        runners.extend(
            self.shared
                .deferred
                .get()
                .expect("provider registry initialized")
                .runners(self)
                .await,
        );
        runners.extend(std::mem::take(
            &mut *self.shared.registered_runners.lock().unwrap(),
        ));
        runners
    }

    pub(super) fn take_custom_runners(&self) -> Option<RunnerRegistrar> {
        self.shared.custom_runners.lock().unwrap().take()
    }

    /// Returns the application's configured public directory.
    pub fn public_path(&self) -> PathBuf {
        self.shared
            .public_path
            .read()
            .expect("application path lock poisoned")
            .clone()
            .unwrap_or_else(|| self.shared.base_path.join("public"))
    }

    /// Overrides the public directory. Relative paths are resolved from the application root.
    pub fn use_public_path(&mut self, path: impl AsRef<Path>) -> &mut Self {
        *self
            .shared
            .public_path
            .write()
            .expect("application path lock poisoned") = Some(self.shared.base_path.join(path));
        self
    }

    pub fn storage_path(&self, path: &str) -> PathBuf {
        self.shared.paths.storage_path(path)
    }

    pub fn use_storage_path(
        &self,
        path: impl Into<PathBuf>,
    ) -> Result<&Self, crate::contracts::container::BindingResolutionError> {
        let path = path.into();
        self.shared.paths.use_storage_path(path.clone());
        self.shared
            .container
            .instance_named("path.storage", Arc::new(path))?;
        Ok(self)
    }

    /// 返回应用语言目录及可选后缀。
    pub fn lang_path(&self, path: &str) -> PathBuf {
        self.shared.paths.lang_path(path)
    }

    /// 覆盖语言目录；应在解析翻译加载器之前调用。
    pub fn use_lang_path(
        &self,
        path: impl Into<PathBuf>,
    ) -> Result<&Self, crate::contracts::container::BindingResolutionError> {
        let path = path.into();
        self.shared
            .container
            .instance_named("path.lang", Arc::new(path.clone()))?;
        self.shared.paths.use_lang_path(path);
        Ok(self)
    }

    /// Whether bootstrap loaded an existing configuration snapshot.
    pub fn configuration_is_cached(&self) -> bool {
        self.shared
            .container
            .get::<bool>("config_loaded_from_cache")
            .is_ok_and(|cached| *cached)
    }

    /// Returns the detected name with `None`, or matches any supplied pattern.
    pub fn environment(
        &self,
        patterns: Option<EnvironmentQuery<'_>>,
    ) -> Result<EnvironmentResult, crate::contracts::container::BindingResolutionError> {
        environment_detector::query(&self.shared.container, patterns)
    }

    /// Checks the bound environment name, independently of the test harness.
    pub fn running_unit_tests(
        &self,
    ) -> Result<bool, crate::contracts::container::BindingResolutionError> {
        foundation::Application::running_unit_tests(self)
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

    pub(crate) fn publishable(&self) -> &Mutex<Vec<crate::support::PublishableResource>> {
        &self.shared.publishable
    }

    pub(crate) fn register_package_commands(&self, register: PackageCommands) {
        self.shared
            .package_commands
            .lock()
            .expect("package command list lock poisoned")
            .push(register);
    }

    pub(crate) fn register_console_task(&self, group: TaskCommand, key: String, command: &str) {
        let mut commands = self
            .shared
            .console_tasks
            .lock()
            .expect("console task commands lock");
        if let Some((_, _, registered)) = commands
            .iter_mut()
            .find(|(kind, name, _)| *kind == group && *name == key)
        {
            command.clone_into(registered);
        } else {
            commands.push((group, key, command.to_owned()));
        }
    }

    pub(crate) fn console_tasks(&self, group: TaskCommand) -> Vec<(String, String)> {
        let mut tasks: Vec<_> = group
            .defaults()
            .iter()
            .map(|&(key, command)| (key.to_owned(), command.to_owned()))
            .collect();
        let commands = self
            .shared
            .console_tasks
            .lock()
            .expect("console task commands lock");
        for (kind, key, command) in commands.iter() {
            if *kind == group {
                if let Some((_, target)) = tasks.iter_mut().find(|(name, _)| name == key) {
                    target.clone_from(command);
                } else {
                    tasks.push((key.clone(), command.clone()));
                }
            }
        }
        tasks
    }

    pub(crate) fn route_info(
        &self,
        expanded_middleware: bool,
    ) -> Result<Vec<crate::routing::RouteInfo>, BootstrapError> {
        let middleware = if expanded_middleware {
            let mut middleware = self.configured_middleware().clone();
            let config = self.configuration_repository()?;
            middleware.configure_web(config.as_ref())?;
            Some(middleware)
        } else {
            None
        };
        let routes = self
            .shared
            .routes
            .lock()
            .expect("application routes lock poisoned");
        match (routes.as_ref(), middleware) {
            (Some(router), Some(middleware)) => router
                .resolved_route_info(&middleware.registry)
                .map_err(|source| BootstrapError::RoutingFailed { source }),
            (Some(router), None) => Ok(router.get_routes()),
            (None, _) => Ok(Vec::new()),
        }
    }

    /// Returns the route cache path after the application resolves its environment.
    pub fn get_cached_routes_path(&self) -> Option<&Path> {
        self.shared
            .route_cache
            .get()
            .map(|(path, _)| path.as_path())
    }

    /// Reports whether this application loaded a valid route matching index during boot.
    pub fn routes_are_cached(&self) -> bool {
        self.shared
            .route_cache
            .get()
            .is_some_and(|(_, loaded)| *loaded)
    }

    /// Returns the event cache path after the application's environment is resolved.
    pub fn get_cached_events_path(&self) -> Option<&Path> {
        self.shared.event_cache_path.get().map(PathBuf::as_path)
    }

    /// Returns the package manifest export path after the application's environment is resolved.
    pub fn get_cached_packages_path(&self) -> Option<&Path> {
        self.shared.package_cache_path.get().map(PathBuf::as_path)
    }

    pub(crate) fn compile_route_cache(&self) -> io::Result<RouteCache> {
        self.shared
            .routes
            .lock()
            .expect("application routes lock poisoned")
            .as_ref()
            .and_then(Router::compile_cache)
            .ok_or_else(|| io::Error::other("Your application doesn't have any routes."))
    }

    pub(crate) fn configuration_repository(
        &self,
    ) -> Result<Arc<dyn config::Repository>, crate::contracts::container::BindingResolutionError>
    {
        self.shared
            .container
            .get::<dyn config::Repository>("config")
    }

    pub(crate) fn exception_handler(
        &self,
    ) -> Result<Arc<dyn ExceptionHandler>, crate::contracts::container::BindingResolutionError>
    {
        self.shared.container.make::<dyn ExceptionHandler>()
    }

    /// Reports an application error through this application's exception handler.
    pub async fn report(
        &self,
        error: &(dyn Error + Send + Sync + 'static),
    ) -> crate::log::LogResult<()> {
        self.exception_handler()?
            .report(&Exception::Error(error))
            .await
    }

    /// 报告并保留原始错误所有权，供替身及异步消费者安全持有。
    pub async fn report_owned(
        &self,
        error: impl Error + Send + Sync + 'static,
    ) -> crate::log::LogResult<()> {
        let shared = crate::contracts::debug::SharedException::new(error);
        self.report(&shared).await
    }

    pub(crate) async fn render_console_exception(
        &self,
        output: &mut Output,
        error: impl Error + Send + Sync + 'static,
    ) {
        let error = crate::contracts::debug::SharedException::new(error);
        if let Some(dump) = Exception::Error(&error)
            .downcast_ref::<crate::console::ConsoleError>()
            .and_then(crate::console::ConsoleError::dump_output)
        {
            output.stdout.push_str(dump);
            return;
        }
        let exception = Exception::Error(&error);
        match self.exception_handler() {
            Ok(handler) => {
                let _ = handler.report(&exception).await;
                handler.render_for_console(output, &exception);
            }
            Err(source) => {
                use std::fmt::Write as _;
                let _ = writeln!(
                    output.stderr,
                    "异常处理器解析失败：{source}；原始异常：{error}"
                );
            }
        }
    }

    /// Executes an Artisan invocation and terminates its console kernel.
    pub async fn handle_command(
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

    pub(super) fn new(
        base_path: PathBuf,
        load_config: ConfigLoader,
        providers: Vec<Box<dyn ServiceProvider>>,
        routing: Option<Routing>,
        middleware_configurators: Vec<MiddlewareConfigurator>,
        commands: CommandRegistrar,
        exceptions: Option<Exceptions>,
    ) -> Self {
        let container = Arc::new(Container::default());
        container
            .instance(Arc::new(crate::console::ProgressDefinitions::default()))
            .expect("进度条定义初始未绑定");
        container
            .instance(Arc::new(crate::console::TableDefinitions::default()))
            .expect("表格样式定义初始未绑定");
        container
            .instance(Arc::new(
                crate::console::view::components::ComponentRegistry::default(),
            ))
            .expect("组件注册表初始未绑定");
        let paths = Arc::new(super::Paths::new(base_path.clone()));
        container
            .instance(paths.clone())
            .expect("application paths are initially unbound");
        container
            .instance_named("path.base", Arc::new(base_path.clone()))
            .expect("初始应用根路径尚未绑定");
        container
            .instance_named("path.lang", Arc::new(paths.lang_path("")))
            .expect("初始语言路径尚未绑定");
        container
            .instance_named("path.storage", Arc::new(paths.storage_path("")))
            .expect("the initial storage path is unbound");
        container
            .alias("config", ServiceKey::typed::<dyn config::Repository>())
            .expect("typed and named configuration keys are distinct");
        let command_routes = routing.as_ref().and_then(|routing| routing.command_routes);
        let mut seen = HashSet::new();
        let base_providers: [Box<dyn ServiceProvider>; _] = [
            Box::new(crate::support::date_factory::DateServiceProvider),
            Box::new(crate::log::LogServiceProvider),
            Box::new(super::console::RunnerServiceProvider),
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
            Box::new(crate::http::ResponseServiceProvider),
            Box::new(crate::http::RequestServiceProvider),
            Box::new(crate::image::ImageServiceProvider),
            Box::new(crate::process::ProcessServiceProvider),
            Box::new(crate::concurrency::ConcurrencyServiceProvider),
            Box::new(crate::redis::RedisServiceProvider),
            Box::new(crate::queue::QueueServiceProvider),
            Box::new(super::cloud::CloudServiceProvider),
            Box::new(crate::cache::CacheServiceProvider),
            Box::new(super::maintenance::MaintenanceServiceProvider),
            Box::new(crate::view::ViewServiceProvider),
            Box::new(super::vite::ViteServiceProvider),
            Box::new(crate::pagination::PaginationServiceProvider),
            Box::new(crate::localization::LocalizationServiceProvider),
            Box::new(crate::mail::MailServiceProvider),
            Box::new(crate::notifications::NotificationServiceProvider),
            Box::new(crate::validation::ValidationServiceProvider),
            Box::new(crate::auth::AuthServiceProvider),
        ];
        let providers: Vec<_> = base_providers
            .into_iter()
            .chain(providers)
            .filter(|provider| seen.insert(provider.provider_type()))
            .collect();
        let (deferred, providers): (Vec<_>, Vec<_>) = providers
            .into_iter()
            .partition(|provider| provider.is_deferred());
        let exceptions = Arc::new(Handler::new(exceptions, Arc::downgrade(&container)));
        container
            .instance::<dyn ExceptionHandler>(exceptions.clone())
            .expect("the application exception handler is initially unbound");
        let shared = Arc::new(ApplicationState {
            lifecycle: Mutex::new(Default::default()),
            prefer_json: AtomicBool::new(false),
            schedule_configurators: Mutex::new(Vec::new()),
            base_path,
            public_path: RwLock::new(None),
            paths,
            load_config,
            testing_environment: AtomicBool::new(false),
            container,
            providers: Mutex::new(providers),
            loaded_providers: Mutex::default(),
            registered_runners: Mutex::default(),
            custom_runners: Mutex::default(),
            deferred: OnceLock::new(),
            state: AtomicU8::new(PENDING),
            booting_started: AtomicBool::new(false),
            routing: Mutex::new(routing),
            api_prefix: OnceLock::new(),
            grpc_registrar: Mutex::new(None),
            package_routes: Mutex::new(Some(Vec::new())),
            publishable: Mutex::default(),
            keep_published_migration_dates: AtomicBool::new(false),
            package_manifest: OnceLock::new(),
            package_cache_path: OnceLock::new(),
            route_cache: OnceLock::new(),
            event_cache_path: OnceLock::new(),
            package_commands: Mutex::default(),
            console_tasks: Mutex::default(),
            middleware_configurators: Mutex::new(middleware_configurators),
            middleware: OnceLock::new(),
            commands,
            command_routes,
            routes: Mutex::new(None),
        });
        let owner = Arc::downgrade(&shared);
        shared
            .container
            .bind_named("app", move |_| {
                owner
                    .upgrade()
                    .map(|shared| Arc::new(Application { shared }))
                    .ok_or(crate::contracts::container::BindingResolutionError::FacadeNotSet)
            })
            .expect("应用入口尚未绑定");
        shared
            .container
            .alias("app", ServiceKey::typed::<Application>())
            .expect("应用类型别名尚未绑定");
        let deferred =
            super::provider_repository::ProviderRepository::new(Arc::downgrade(&shared), deferred);
        super::console::bind_application(&shared);
        shared
            .container
            .instance(Arc::new(super::dev_commands::Registry::new(
                shared.base_path.clone(),
                Arc::downgrade(&shared),
            )))
            .expect("development command registry is initially unbound");
        if !deferred.is_empty() {
            shared.container.install_deferred(deferred.clone());
        }
        assert!(
            shared.deferred.set(deferred).is_ok(),
            "provider registry already initialized"
        );
        // 应用本身已构造完成：发布解析状态，但不把强引用存回自身容器。
        shared
            .container
            .get::<Self>("app")
            .expect("应用入口解析失败");
        Self { shared }
    }

    /// Resolve a service, awaiting deferred provider boot when the application is ready.
    pub async fn make<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> Result<Arc<T>, crate::contracts::container::BindingResolutionError> {
        crate::support::facades::application_scope(
            self.container_handle(),
            self.shared.container.make_async::<T>(),
        )
        .await
    }

    pub async fn get<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<ServiceKey> + Send,
    ) -> Result<Arc<T>, crate::contracts::container::BindingResolutionError> {
        crate::support::facades::application_scope(
            self.container_handle(),
            self.shared.container.get_async::<T>(key),
        )
        .await
    }

    /// Resolve a fresh service when explicit parameters are supplied.
    pub async fn make_with<T: ?Sized + Send + Sync + 'static>(
        &self,
        parameters: crate::container::ParameterOverrides,
    ) -> Result<Arc<T>, crate::contracts::container::BindingResolutionError> {
        crate::support::facades::application_scope(
            self.container_handle(),
            self.shared.container.make_with_async::<T>(parameters),
        )
        .await
    }
    pub async fn make_with_named<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<ServiceKey> + Send,
        parameters: crate::container::ParameterOverrides,
    ) -> Result<Arc<T>, crate::contracts::container::BindingResolutionError> {
        crate::support::facades::application_scope(
            self.container_handle(),
            self.shared
                .container
                .make_with_async_named::<T>(key, parameters),
        )
        .await
    }

    /// Return a lazy application-aware resolver; no service is resolved at creation.
    pub fn factory<'a, T: ?Sized + Send + Sync + 'static>(
        &'a self,
    ) -> impl Fn() -> crate::container::FactoryFuture<'a, T> + Send + Sync {
        move || Box::pin(self.make::<T>())
    }

    pub fn factory_named<'a, T: ?Sized + Send + Sync + 'static>(
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

    pub async fn call_method_binding<C: ?Sized + Send + Sync + 'static, R: Send + 'static>(
        &self,
        method: &str,
        receiver: Arc<C>,
    ) -> Result<R, crate::contracts::container::BindingResolutionError> {
        crate::support::facades::application_scope(
            self.container_handle(),
            self.shared
                .container
                .call_method_binding::<C, R>(method, receiver),
        )
        .await
    }

    pub async fn call_static_method_binding<C: ?Sized + 'static, R: Send + 'static>(
        &self,
        method: &str,
    ) -> Result<R, crate::contracts::container::BindingResolutionError> {
        crate::support::facades::application_scope(
            self.container_handle(),
            self.shared
                .container
                .call_static_method_binding::<C, R>(method),
        )
        .await
    }

    pub fn is_deferred_service(&self, key: impl Into<ServiceKey>) -> bool {
        use crate::container::deferred::DeferredServices as _;
        self.shared
            .deferred
            .get()
            .expect("provider registry initialized")
            .contains(&key.into())
    }

    pub async fn load_deferred_providers(
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

    pub(super) fn configured_middleware(&self) -> &Middleware {
        self.shared.middleware.get_or_init(|| {
            let mut middleware = Middleware::default();
            let configure = std::mem::take(
                &mut *self
                    .shared
                    .middleware_configurators
                    .lock()
                    .expect("middleware configuration lock poisoned"),
            );
            for configure in configure {
                configure(&mut middleware);
            }
            middleware
        })
    }

    pub(crate) fn container_handle(&self) -> Arc<Container> {
        self.shared.container.clone()
    }

    pub(crate) fn register_deferred_load_events(
        &self,
        dispatcher: &crate::events::Dispatcher,
    ) -> crate::events::EventResult<()> {
        self.shared
            .deferred
            .get()
            .expect("provider registry initialized")
            .register_load_events(dispatcher)
    }

    pub fn maintenance_mode(
        &self,
    ) -> Result<
        Arc<dyn crate::contracts::foundation::MaintenanceMode>,
        crate::contracts::container::BindingResolutionError,
    > {
        self.shared.container.make()
    }

    pub async fn is_down_for_maintenance(
        &self,
    ) -> Result<bool, super::maintenance::MaintenanceError> {
        self.maintenance_mode()?.active().await
    }

    pub async fn boot(&mut self) -> Result<(), BootstrapError> {
        self.boot_with_environment(None).await
    }

    pub(crate) async fn boot_with_environment(
        &mut self,
        selected: Option<&str>,
    ) -> Result<(), BootstrapError> {
        self.boot_with_configuration_signal(selected, None).await
    }

    pub(crate) fn use_testing_environment(&self) {
        self.shared
            .testing_environment
            .store(true, Ordering::Relaxed);
    }

    pub(super) async fn boot_with_configuration_signal(
        &mut self,
        selected: Option<&str>,
        configuration_ready: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> Result<(), BootstrapError> {
        BOOTSTRAPPING
            .scope(
                self.shared.container.clone(),
                crate::support::facades::scope(
                    self.shared.container.clone(),
                    self.boot_application(selected, configuration_ready, true),
                ),
            )
            .await
    }

    pub(super) async fn boot_without_config_cache(
        &mut self,
        selected: Option<&str>,
    ) -> Result<(), BootstrapError> {
        BOOTSTRAPPING
            .scope(
                self.shared.container.clone(),
                crate::support::facades::scope(
                    self.shared.container.clone(),
                    self.boot_application(selected, None, false),
                ),
            )
            .await
    }

    async fn boot_application(
        &mut self,
        selected: Option<&str>,
        configuration_ready: Option<tokio::sync::oneshot::Sender<()>>,
        use_cache: bool,
    ) -> Result<(), BootstrapError> {
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
        let ConfigurationCache {
            path: cache_path,
            mut environment,
            values: cached,
        } = ConfigurationCache::bootstrap(
            &self.shared.base_path,
            selected,
            use_cache,
            self.shared.testing_environment.load(Ordering::Relaxed),
        )
        .map_err(configuration_failed)?;
        environment.paths = Arc::downgrade(&self.shared.paths);
        let environment = Arc::new(environment);
        self.shared.paths.environment(environment.clone());
        let package_cache = self.shared.base_path.join(
            environment
                .values
                .get("APP_PACKAGES_CACHE")
                .filter(|path| !path.is_empty())
                .map_or_else(|| Path::new("bootstrap/cache/packages.json"), Path::new),
        );
        let _ = self.shared.package_cache_path.set(package_cache);
        let route_cache_path = self.shared.base_path.join(
            environment
                .values
                .get("APP_ROUTES_CACHE")
                .filter(|path| !path.is_empty())
                .map_or_else(
                    || Path::new("storage/framework/cache/routes.json"),
                    Path::new,
                ),
        );
        let event_cache_path = self.shared.base_path.join(
            environment
                .values
                .get("APP_EVENTS_CACHE")
                .filter(|path| !path.is_empty())
                .map_or_else(
                    || Path::new("storage/framework/cache/events.json"),
                    Path::new,
                ),
        );
        let _ = self.shared.event_cache_path.set(event_cache_path);
        self.shared
            .container
            .instance_named("config.cache.path", Arc::new(cache_path))?;
        self.shared
            .container
            .instance_named("config_loaded_from_cache", Arc::new(cached.is_some()))?;
        self.shared
            .container
            .scoped::<crate::config::LocaleOverrides, _>(|_| Ok(Arc::default()))?;
        self.shared
            .container
            .instance(Arc::new(crate::config::ConfigMacroRegistry::default()))?;
        self.shared
            .container
            .singleton::<crate::testing::ParallelTesting, _>(|_| {
                Ok(Arc::new(
                    crate::testing::ParallelTesting::default().with_default_isolation(),
                ))
            })?;
        let config = Arc::new(
            Repository::new(Default::default()).with_owner(Arc::downgrade(&self.shared.container)),
        );
        self.shared
            .container
            .instance_named::<dyn config::Repository>("config", config.clone())?;
        let grpc_options = Arc::new(grpc::GrpcServerOptions::default());
        self.shared.container.instance(grpc_options.clone())?;
        let loaded = match cached {
            Some(values) => Repository::new(values),
            None => LoadConfiguration::bootstrap(
                &self.shared.base_path,
                &environment,
                self.shared.load_config,
            )
            .map_err(configuration_failed)?,
        };
        config.replace_with(loaded);
        self.shared
            .container
            .instance(Arc::new(grpc::GrpcClients::new(config.clone())))?;
        self.shared
            .container
            .instance_named("env.file", Arc::new(environment.file.clone()))?;
        self.shared.container.instance(environment)?;
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
        environment_detector::install(&self.shared.container, name)?;
        if self
            .shared
            .public_path
            .read()
            .expect("application path lock poisoned")
            .is_none()
        {
            let (key, directory) =
                match config::Repository::get(config.as_ref(), "filesystems.disks.web.root") {
                    Some(value) => ("filesystems.disks.web.root", Some(value)),
                    None => (
                        "app.public_path",
                        config::Repository::get(config.as_ref(), "app.public_path"),
                    ),
                };
            match directory {
                None | Some(Value::Null) => {}
                Some(Value::String(path)) if !path.trim().is_empty() => {
                    self.use_public_path(path);
                }
                _ => {
                    return Err(configuration_failed(ConfigError::InvalidType {
                        key: key.into(),
                        expected: "a non-empty directory path",
                    }));
                }
            }
        }
        if config::Repository::has(config.as_ref(), "filesystems.disks.web") {
            let path = self.public_path();
            let path = path.to_str().ok_or_else(|| {
                configuration_failed(ConfigError::InvalidType {
                    key: "filesystems.disks.web.root".into(),
                    expected: "a UTF-8 directory path",
                })
            })?;
            config::Repository::set(
                config.as_ref(),
                "filesystems.disks.web.root",
                Value::String(path.into()),
            )
            .map_err(configuration_failed)?;
        }

        if let Some(ready) = configuration_ready {
            let _ = ready.send(());
        }

        let mut providers = std::mem::take(
            &mut *self
                .shared
                .providers
                .lock()
                .expect("provider list lock poisoned"),
        );
        let deferred = self
            .shared
            .deferred
            .get()
            .expect("provider registry initialized");
        let mut boot_positions = Vec::with_capacity(providers.len());
        for provider in &mut providers {
            let (provider_type, name) = (provider.provider_type(), provider.name());
            provider.register(self).await.map_err(|source| {
                BootstrapError::ProviderRegistrationFailed {
                    provider: provider_type,
                    name,
                    source,
                }
            })?;
            self.mark_as_registered(provider.as_ref());
            boot_positions.push(deferred.registered_count());
        }
        self.run_lifecycle(0)?;
        self.run_lifecycle(1)?;
        self.shared.booting_started.store(true, Ordering::Release);
        deferred.boot_prepared().await?;
        let mut previous_position = 0;
        for (provider, position) in providers.iter_mut().zip(boot_positions) {
            if previous_position != position {
                deferred
                    .boot_registration_range(previous_position..position)
                    .await?;
                previous_position = position;
            }
            let (provider_type, name) = (provider.provider_type(), provider.name());
            provider
                .boot(self)
                .await
                .map_err(|source| BootstrapError::ProviderBootFailed {
                    provider: provider_type,
                    name,
                    source,
                })?;
        }
        deferred.boot_registered().await?;
        crate::localization::default_source::boot(&self.shared.container)
            .await
            .map_err(|source| BootstrapError::LocalizationFailed { source })?;
        let routing_failed = |source| BootstrapError::RoutingFailed { source };
        let routing = self
            .shared
            .routing
            .lock()
            .expect("application routing lock poisoned")
            .take()
            .unwrap_or_default();
        let routing_config = self.configuration_repository()?;
        let api_prefix = routing
            .resolve_api_prefix(routing_config.as_ref())
            .map_err(configuration_failed)?;
        let mut routes = routing
            .register(self, &api_prefix)
            .map_err(routing_failed)?;
        self.shared
            .api_prefix
            .set(api_prefix)
            .expect("application API prefix resolved once");
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
        routes.resolve_static_directories(&self.shared.base_path);
        let config = self.configuration_repository()?;
        crate::filesystem::register_signed_routes(&mut routes, config.as_ref())
            .map_err(routing_failed)?;
        let loaded = routes.load_cache(&route_cache_path);
        self.shared
            .route_cache
            .set((route_cache_path, loaded))
            .expect("application route cache loaded once");
        self.shared.container.instance_named(
            "url",
            Arc::new(
                crate::routing::UrlGenerator::new(routes.url_routes(), config.clone())
                    .with_owner(Arc::downgrade(&self.container_handle())),
            ),
        )?;
        self.shared
            .container
            .alias("url", ServiceKey::typed::<crate::routing::UrlGenerator>())?;
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
            self.register_runner(Arc::new(grpc::ConfiguredServer::new(
                routes.routes(),
                config,
                grpc_options,
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
            .await?;
        self.shared.state.store(BOOTED, Ordering::Release);
        self.run_lifecycle(2)?;
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
        &self.shared.base_path
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
    fn lang_path(&self, path: &str) -> PathBuf {
        self.shared.paths.lang_path(path)
    }
    fn storage_path(&self, path: &str) -> PathBuf {
        self.shared.paths.storage_path(path)
    }
}
