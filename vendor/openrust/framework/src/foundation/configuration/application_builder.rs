use std::{collections::BTreeMap, io, path::PathBuf, sync::Arc};

use super::routing::ChannelRoutes;
use super::{Exceptions, Middleware, Routing};
use crate::{
    contracts::container::Container as _,
    database::{
        migrations::{MigrationEntry, MigrationRegistry},
        seeder::{SeederEntry, SeederRegistry},
    },
    foundation::{
        Application, BootstrapError, Runner,
        application::{CommandRegistrar, MiddlewareConfigurator, RunnerRegistrar},
        bootstrap::ConfigLoader,
    },
    grpc,
    support::ServiceProvider,
};

/// Collects the configuration used to create an application.
pub struct ApplicationBuilder {
    base_path: PathBuf,
    lifecycle: [Vec<super::super::application::LifecycleCallback>; 3],
    schedule_configurators: Vec<super::super::application::ScheduleConfigurator>,
    load_config: ConfigLoader,
    providers: Vec<Box<dyn ServiceProvider>>,
    packages: crate::foundation::PackageManifest,
    migrations: Option<&'static [MigrationEntry]>,
    seeders: Option<&'static [SeederEntry]>,
    routing: Option<Routing>,
    channels: Option<ChannelRoutes>,
    middleware_configurators: Vec<MiddlewareConfigurator>,
    commands: CommandRegistrar,
    command_types: Vec<crate::foundation::console::command_types::CommandType>,
    grpc: Option<grpc::Registrar>,
    exceptions: Option<Exceptions>,
    runners: Option<RunnerRegistrar>,
}

impl ApplicationBuilder {
    pub(crate) fn new(base_path: PathBuf) -> Self {
        Self {
            base_path,
            lifecycle: Default::default(),
            schedule_configurators: Vec::new(),
            load_config: |_, _| Ok(BTreeMap::new()),
            providers: Vec::new(),
            packages: Default::default(),
            migrations: None,
            seeders: None,
            routing: None,
            channels: None,
            middleware_configurators: Vec::new(),
            commands: |_| Ok(()),
            command_types: Vec::new(),
            grpc: None,
            exceptions: None,
            runners: None,
        }
    }

    pub fn registered(
        mut self,
        callback: impl FnOnce(&Application) -> crate::support::ProviderResult + Send + Sync + 'static,
    ) -> Self {
        self.lifecycle[0].push(Box::new(callback));
        self
    }
    pub fn booting(
        mut self,
        callback: impl FnOnce(&Application) -> crate::support::ProviderResult + Send + Sync + 'static,
    ) -> Self {
        self.lifecycle[1].push(Box::new(callback));
        self
    }
    pub fn booted(
        mut self,
        callback: impl FnOnce(&Application) -> crate::support::ProviderResult + Send + Sync + 'static,
    ) -> Self {
        self.lifecycle[2].push(Box::new(callback));
        self
    }

    /// Rust 按类型注册单项工厂，可连续调用以声明多个绑定。
    pub fn with_bindings<T, F>(self, factory: F) -> Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(
                &crate::container::Container,
            ) -> Result<Arc<T>, crate::contracts::container::BindingResolutionError>
            + Send
            + Sync
            + 'static,
    {
        self.registered(move |app| {
            app.container_handle().bind::<T, _>(factory)?;
            Ok(())
        })
    }
    pub fn with_singletons<T, F>(self, factory: F) -> Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(
                &crate::container::Container,
            ) -> Result<Arc<T>, crate::contracts::container::BindingResolutionError>
            + Send
            + Sync
            + 'static,
    {
        self.registered(move |app| {
            app.container_handle().singleton::<T, _>(factory)?;
            Ok(())
        })
    }
    pub fn with_scoped_singletons<T, F>(self, factory: F) -> Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(
                &crate::container::Container,
            ) -> Result<Arc<T>, crate::contracts::container::BindingResolutionError>
            + Send
            + Sync
            + 'static,
    {
        self.registered(move |app| {
            app.container_handle().scoped::<T, _>(factory)?;
            Ok(())
        })
    }
    pub fn with_schedule(
        mut self,
        callback: impl Fn(
            &mut crate::console::scheduling::Schedule,
        ) -> Result<(), crate::console::ConsoleError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.schedule_configurators.push(Box::new(callback));
        self
    }
    pub fn prefers_json_responses(self, prefer: bool) -> Self {
        if prefer {
            self.booted(|app| {
                app.shared
                    .prefer_json
                    .store(true, std::sync::atomic::Ordering::Release);
                Ok(())
            })
        } else {
            self
        }
    }

    pub fn with_config(mut self, load: ConfigLoader) -> Self {
        self.load_config = load;
        self
    }

    pub fn with_providers(mut self, providers: Vec<Box<dyn ServiceProvider>>) -> Self {
        self.providers.extend(providers);
        self
    }

    /// Installs the package catalog generated for this application's Cargo target.
    pub fn with_packages(mut self, manifest: crate::foundation::PackageManifest) -> Self {
        self.packages = manifest;
        self
    }

    /// Registers the application's compiled migration entries for Artisan.
    pub fn with_migrations(mut self, entries: &'static [MigrationEntry]) -> Self {
        self.migrations = Some(entries);
        self
    }

    /// Registers the application's compiled seeder entries for Artisan.
    pub fn with_seeders(mut self, entries: &'static [SeederEntry]) -> Self {
        self.seeders = Some(entries);
        self
    }

    /// Adds a compiled manifest of event listeners to application boot.
    pub fn with_events(mut self, manifest: crate::foundation::events::DiscoverEvents) -> Self {
        self.providers.push(Box::new(
            crate::foundation::support::providers::EventServiceProvider::new(manifest),
        ));
        self
    }

    /// Adds custom runners after provider contributions.
    pub fn with_runners(
        mut self,
        runners: impl FnOnce() -> Vec<Arc<dyn Runner>> + Send + 'static,
    ) -> Self {
        self.runners = Some(Box::new(runners));
        self
    }

    pub fn with_routing(mut self, routing: Routing) -> Self {
        self.routing = Some(routing);
        self
    }

    /// Enables broadcast authentication and loads the application's channel rules.
    pub fn with_broadcasting(mut self, register: ChannelRoutes) -> Self {
        self.channels = Some(register);
        self
    }

    /// Registers gRPC services during application boot.
    pub fn with_grpc(mut self, register: grpc::Registrar) -> Self {
        self.grpc = Some(register);
        self
    }

    /// Configures this application’s middleware when the configuration is first needed.
    pub fn with_middleware(
        mut self,
        configure: impl FnOnce(&mut Middleware) + Send + Sync + 'static,
    ) -> Self {
        self.middleware_configurators.push(Box::new(configure));
        self
    }

    /// Registers application commands when Artisan starts.
    pub fn with_commands(mut self, register: CommandRegistrar) -> Self {
        self.commands = register;
        self
    }

    /// 通过应用容器构造并注册命令类型，复用同一应用的实际命令实例。
    pub fn with_command<C: crate::console::Command + 'static>(mut self) -> Self {
        let command = crate::foundation::console::command_types::CommandType::of::<C>();
        if !self
            .command_types
            .iter()
            .any(|entry| entry.id == command.id)
        {
            self.command_types.push(command);
        }
        self
    }

    /// Configures exception rendering for the HTTP kernel.
    pub fn with_exceptions(mut self, configure: impl FnOnce(&mut Exceptions)) -> Self {
        configure(self.exceptions.get_or_insert_with(Exceptions::default));
        self
    }

    pub fn create(mut self) -> Result<Application, BootstrapError> {
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
        let routing = match self.channels {
            Some(register) => Some(self.routing.unwrap_or_default().channels(register)),
            None => self.routing,
        };
        if let Some(path) = routing.as_ref().and_then(Routing::health_path) {
            let path = path.trim_matches('/');
            let path = if path.is_empty() { "/" } else { path }.to_owned();
            self.middleware_configurators
                .push(Box::new(move |middleware| {
                    middleware.prevent_requests_during_maintenance([path]);
                }));
        }
        let mut providers = self.packages.providers();
        providers.extend(self.providers);
        let application = Application::new(
            base_path,
            self.load_config,
            providers,
            routing,
            self.middleware_configurators,
            self.commands,
            self.exceptions,
        );
        *application
            .shared
            .lifecycle
            .lock()
            .expect("应用生命周期锁中毒") = self.lifecycle;
        *application
            .shared
            .schedule_configurators
            .lock()
            .expect("调度配置锁中毒") = self.schedule_configurators;
        let _ = application.shared.package_manifest.set(self.packages);
        let container = &application.shared.container;
        if !self.command_types.is_empty() {
            container.instance(Arc::new(
                crate::foundation::console::command_types::CommandTypeCatalog(self.command_types),
            ))?;
        }
        if let Some(entries) = self.migrations {
            container.instance(Arc::new(MigrationRegistry(entries)))?;
        }
        if let Some(entries) = self.seeders {
            container.instance(Arc::new(SeederRegistry(entries)))?;
        }
        *application
            .shared
            .grpc_registrar
            .lock()
            .expect("gRPC registration lock poisoned") = self.grpc;
        application.set_custom_runners(self.runners);
        Ok(application)
    }
}
