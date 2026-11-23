use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, RwLock, Weak},
};

use axum::body::Bytes;
use tokio::{io::AsyncRead, sync::Semaphore};

use crate::{
    container::{Container, ServiceKey},
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    filesystem::{FilesystemAdapter, FilesystemManager, Storage},
    foundation::Application,
    http::{HttpFuture, UploadedFile, client::Factory},
    support::{ProviderResult, ServiceProvider, facades::Facade},
};

use super::{
    Driver, Error, Image, Limits, Result, TransformationHandler,
    instance::{Source, read_bounded},
    native_driver::NativeDriver,
};

type TransformationHandlers = HashMap<String, indexmap::IndexMap<String, TransformationHandler>>;
type DriverCreator = dyn Fn(&ImageManager) -> Result<Arc<dyn Driver>> + Send + Sync;

#[derive(Default)]
struct DriverState {
    creators: HashMap<String, Arc<DriverCreator>>,
    drivers: HashMap<String, Arc<dyn Driver>>,
    creating: HashSet<String>,
    generation: Arc<()>,
    handlers: TransformationHandlers,
    handlers_generation: Arc<()>,
}

struct DriverCreation<'a> {
    manager: &'a ImageManager,
    name: &'a str,
}

impl Drop for DriverCreation<'_> {
    fn drop(&mut self) {
        self.manager.driver_state().creating.remove(self.name);
    }
}

/// Resolves named drivers and limits processing within one application.
#[derive(Clone)]
pub struct ImageManager {
    default: Arc<str>,
    config: Option<Arc<dyn Repository>>,
    state: Arc<Mutex<DriverState>>,
    container: Arc<RwLock<Weak<Container>>>,
    limits: Limits,
    workers: Arc<Semaphore>,
    http: Option<Arc<Factory>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's image manager API")]
impl ImageManager {
    pub fn new(limits: Limits) -> Self {
        Self {
            default: "image".into(),
            config: None,
            state: Arc::default(),
            container: Arc::default(),
            limits,
            workers: Arc::new(Semaphore::new(
                std::thread::available_parallelism()
                    .map_or(1, usize::from)
                    .min(4),
            )),
            http: None,
        }
    }

    /// 登记惰性创建器；回调通过管理器访问所属容器和配置。
    pub fn extend(
        &self,
        name: impl Into<String>,
        creator: impl Fn(&ImageManager) -> Result<Arc<dyn Driver>> + Send + Sync + 'static,
    ) -> Result<&Self> {
        let previous = self
            .driver_state()
            .creators
            .insert(name.into(), Arc::new(creator));
        drop(previous);
        Ok(self)
    }

    pub fn getContainer(&self) -> Option<Arc<Container>> {
        self.container
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .upgrade()
    }

    pub fn setContainer(&self, container: Arc<Container>) -> &Self {
        *self
            .container
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::downgrade(&container);
        self
    }

    pub fn forgetDrivers(&self) -> &Self {
        let previous = {
            let mut state = self.driver_state();
            state.generation = Arc::new(());
            std::mem::take(&mut state.drivers)
        };
        drop(previous);
        self
    }

    fn driver_state(&self) -> MutexGuard<'_, DriverState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn transformUsing(
        &self,
        driver: &str,
        transformation: &str,
        callback: TransformationHandler,
    ) -> Result<&Self> {
        let (resolved, handlers) = {
            let mut state = self.driver_state();
            state
                .handlers
                .entry(driver.to_owned())
                .or_default()
                .insert(transformation.to_owned(), callback);
            state.handlers_generation = Arc::new(());
            (
                state.drivers.get(driver).cloned(),
                state.handlers[driver].clone(),
            )
        };
        if let Some(resolved) = resolved {
            for (transformation, callback) in handlers {
                resolved.transformUsing(&transformation, callback)?;
            }
        }
        Ok(self)
    }

    pub fn getDefaultDriver(&self) -> String {
        self.config
            .as_ref()
            .and_then(|config| config.get("images.default"))
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| self.default.to_string())
    }

    pub fn getDrivers(&self) -> HashMap<String, Arc<dyn Driver>> {
        self.driver_state().drivers.clone()
    }

    pub fn driver(&self, name: Option<&str>) -> Result<Arc<dyn Driver>> {
        let name = name
            .filter(|name| !matches!(*name, "" | "0"))
            .map(str::to_owned)
            .unwrap_or_else(|| self.getDefaultDriver());
        let (creator, generation) = {
            let mut state = self.driver_state();
            if let Some(driver) = state.drivers.get(&name) {
                return Ok(driver.clone());
            }
            let creator = state.creators.get(&name).cloned();
            if creator.is_none() && name != "image" {
                return Err(Error::UnknownDriver(name));
            }
            if !state.creating.insert(name.clone()) {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "图片驱动正在创建，不能递归解析同一驱动",
                )));
            }
            (creator, state.generation.clone())
        };
        let _creation = DriverCreation {
            manager: self,
            name: &name,
        };
        let driver: Arc<dyn Driver> = match creator {
            Some(creator) => creator(self)?,
            None => Arc::new(NativeDriver::default()),
        };
        driver.ensureRequirementsAreMet()?;
        loop {
            let (handlers, revision) = {
                let state = self.driver_state();
                (
                    state.handlers.get(&name).cloned().unwrap_or_default(),
                    state.handlers_generation.clone(),
                )
            };
            for (transformation, callback) in handlers {
                driver.transformUsing(&transformation, callback)?;
            }
            let mut state = self.driver_state();
            if !Arc::ptr_eq(&state.handlers_generation, &revision) {
                continue;
            }
            if Arc::ptr_eq(&state.generation, &generation) {
                state.drivers.insert(name.clone(), driver.clone());
            }
            break;
        }
        Ok(driver)
    }

    pub(super) fn limits(&self) -> Limits {
        self.limits
    }

    pub(super) async fn blocking<T: Send + 'static>(
        &self,
        process: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let permit = self
            .workers
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| Error::Io(std::io::Error::other(error)))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            process()
        })
        .await
        .map_err(|error| Error::Io(std::io::Error::other(error)))?
    }

    pub fn fromBytes(&self, bytes: impl Into<Bytes>) -> Image {
        Image::new(self.clone(), Source::Bytes(bytes.into()))
    }

    pub async fn fromStream(&self, reader: impl AsyncRead) -> Result<Image> {
        Ok(self.fromBytes(read_bounded(reader, self.limits).await?))
    }

    pub fn fromBase64(&self, encoded: impl Into<Arc<str>>) -> Image {
        Image::new(self.clone(), Source::Base64(encoded.into()))
    }

    pub fn fromPath(&self, path: impl Into<PathBuf>) -> Image {
        Image::new(self.clone(), Source::Path(path.into()))
    }

    pub fn fromUpload(&self, file: &UploadedFile) -> Image {
        Image::new(self.clone(), Source::Upload(file.clone()))
    }

    pub fn fromDisk(&self, disk: Arc<FilesystemAdapter>, path: impl Into<String>) -> Image {
        Image::new(self.clone(), Source::Storage(disk, path.into()))
    }

    pub fn fromStorage(&self, path: impl Into<String>, disk: Option<&str>) -> Result<Image> {
        Ok(Image::new(
            self.clone(),
            Source::ConfiguredStorage(disk.map(str::to_owned), path.into()),
        ))
    }

    pub fn fromUrl(&self, url: impl Into<String>) -> Result<Image> {
        Ok(Image::new(self.clone(), Source::Url(url.into())))
    }

    pub(super) fn storage(&self, disk: Option<&str>) -> Result<Arc<FilesystemAdapter>> {
        match self.getContainer() {
            Some(container) => container
                .get::<FilesystemManager>("filesystem")
                .map_err(Error::Unavailable)?
                .disk(disk)
                .map_err(Error::Filesystem),
            None => Storage::disk(disk).map_err(Error::Filesystem),
        }
    }

    pub(super) fn http(&self) -> Result<Arc<Factory>> {
        match self.getContainer() {
            Some(container) => container.make::<Factory>().map_err(Error::Unavailable),
            None => match &self.http {
                Some(factory) => Ok(factory.clone()),
                None => Ok(Arc::new(Factory::new().map_err(Error::Http)?)),
            },
        }
    }
}

/// The application facade exported as `support::facades::Image`.
pub struct ImageFacade;

impl Facade for ImageFacade {
    type Service = ImageManager;

    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<ImageManager>()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Image facade")]
impl ImageFacade {
    pub fn fromBytes(bytes: impl Into<Bytes>) -> Result<Image> {
        Ok(Self::getFacadeRoot()?.fromBytes(bytes))
    }

    pub async fn fromStream(reader: impl AsyncRead) -> Result<Image> {
        Self::getFacadeRoot()?.fromStream(reader).await
    }

    pub fn fromBase64(encoded: impl Into<Arc<str>>) -> Result<Image> {
        Ok(Self::getFacadeRoot()?.fromBase64(encoded))
    }

    pub fn fromPath(path: impl Into<PathBuf>) -> Result<Image> {
        Ok(Self::getFacadeRoot()?.fromPath(path))
    }

    pub fn fromUpload(file: &UploadedFile) -> Result<Image> {
        Ok(Self::getFacadeRoot()?.fromUpload(file))
    }

    pub fn fromDisk(disk: Arc<FilesystemAdapter>, path: impl Into<String>) -> Result<Image> {
        Ok(Self::getFacadeRoot()?.fromDisk(disk, path))
    }

    pub fn fromStorage(path: impl Into<String>, disk: Option<&str>) -> Result<Image> {
        Self::getFacadeRoot()?.fromStorage(path, disk)
    }

    pub fn fromUrl(url: impl Into<String>) -> Result<Image> {
        Self::getFacadeRoot()?.fromUrl(url)
    }

    pub fn driver(name: Option<&str>) -> Result<Arc<dyn Driver>> {
        Self::getFacadeRoot()?.driver(name)
    }

    pub fn transformUsing(
        driver: &str,
        transformation: &str,
        callback: TransformationHandler,
    ) -> Result<()> {
        Self::getFacadeRoot()?
            .transformUsing(driver, transformation, callback)
            .map(|_| ())
    }

    pub fn getDefaultDriver() -> Result<String> {
        Ok(Self::getFacadeRoot()?.getDefaultDriver())
    }

    pub fn getDrivers() -> Result<HashMap<String, Arc<dyn Driver>>> {
        Ok(Self::getFacadeRoot()?.getDrivers())
    }

    pub fn getContainer() -> Result<Option<Arc<Container>>> {
        Ok(Self::getFacadeRoot()?.getContainer())
    }

    pub fn setContainer(container: Arc<Container>) -> Result<()> {
        Self::getFacadeRoot()?.setContainer(container);
        Ok(())
    }

    pub fn forgetDrivers() -> Result<()> {
        Self::getFacadeRoot()?.forgetDrivers();
        Ok(())
    }

    pub fn extend(
        name: impl Into<String>,
        creator: impl Fn(&ImageManager) -> Result<Arc<dyn Driver>> + Send + Sync + 'static,
    ) -> Result<()> {
        Self::getFacadeRoot()?.extend(name, creator).map(|_| ())
    }
}

pub(crate) struct ImageServiceProvider;

impl ServiceProvider for ImageServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let owner = Arc::downgrade(&app.container_handle());
            app.container()
                .singleton::<ImageManager, _>(move |container| {
                    let config = container.get::<dyn Repository>("config")?;
                    let mut manager = ImageManager::new(Limits::default());
                    *manager
                        .container
                        .write()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = owner.clone();
                    manager.config = Some(config);
                    manager.http = Some(container.make::<Factory>()?);
                    Ok(Arc::new(manager))
                })?;
            Ok(())
        })
    }
}
