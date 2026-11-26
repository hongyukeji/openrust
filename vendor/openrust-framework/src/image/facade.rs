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

    pub fn get_container(&self) -> Option<Arc<Container>> {
        self.container
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .upgrade()
    }

    pub fn set_container(&self, container: Arc<Container>) -> &Self {
        *self
            .container
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::downgrade(&container);
        self
    }

    pub fn forget_drivers(&self) -> &Self {
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

    pub fn transform_using(
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
                resolved.transform_using(&transformation, callback)?;
            }
        }
        Ok(self)
    }

    pub fn get_default_driver(&self) -> String {
        self.config
            .as_ref()
            .and_then(|config| config.get("images.default"))
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| self.default.to_string())
    }

    pub fn get_drivers(&self) -> HashMap<String, Arc<dyn Driver>> {
        self.driver_state().drivers.clone()
    }

    pub fn driver(&self, name: Option<&str>) -> Result<Arc<dyn Driver>> {
        let name = name
            .filter(|name| !matches!(*name, "" | "0"))
            .map(str::to_owned)
            .unwrap_or_else(|| self.get_default_driver());
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
        driver.ensure_requirements_are_met()?;
        loop {
            let (handlers, revision) = {
                let state = self.driver_state();
                (
                    state.handlers.get(&name).cloned().unwrap_or_default(),
                    state.handlers_generation.clone(),
                )
            };
            for (transformation, callback) in handlers {
                driver.transform_using(&transformation, callback)?;
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

    pub(super) fn with_limits(&self, limits: Limits) -> Self {
        let mut bounded = self.clone();
        bounded.limits = Limits {
            max_input_bytes: self.limits.max_input_bytes.min(limits.max_input_bytes),
            max_dimension: self.limits.max_dimension.min(limits.max_dimension),
            max_pixels: self.limits.max_pixels.min(limits.max_pixels),
        };
        bounded
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

    pub fn from_bytes(&self, bytes: impl Into<Bytes>) -> Image {
        Image::new(self.clone(), Source::Bytes(bytes.into()))
    }

    pub async fn from_stream(&self, reader: impl AsyncRead) -> Result<Image> {
        Ok(self.from_bytes(read_bounded(reader, self.limits).await?))
    }

    pub fn from_base64(&self, encoded: impl Into<Arc<str>>) -> Image {
        Image::new(self.clone(), Source::Base64(encoded.into()))
    }

    pub fn from_path(&self, path: impl Into<PathBuf>) -> Image {
        Image::new(self.clone(), Source::Path(path.into()))
    }

    pub fn from_upload(&self, file: &UploadedFile) -> Image {
        Image::new(self.clone(), Source::Upload(file.clone()))
    }

    pub fn from_disk(&self, disk: Arc<FilesystemAdapter>, path: impl Into<String>) -> Image {
        Image::new(self.clone(), Source::Storage(disk, path.into()))
    }

    pub fn from_storage(&self, path: impl Into<String>, disk: Option<&str>) -> Result<Image> {
        Ok(Image::new(
            self.clone(),
            Source::ConfiguredStorage(disk.map(str::to_owned), path.into()),
        ))
    }

    pub fn from_url(&self, url: impl Into<String>) -> Result<Image> {
        Ok(Image::new(self.clone(), Source::Url(url.into())))
    }

    pub(super) fn storage(&self, disk: Option<&str>) -> Result<Arc<FilesystemAdapter>> {
        match self.get_container() {
            Some(container) => container
                .get::<FilesystemManager>("filesystem")
                .map_err(Error::Unavailable)?
                .disk(disk)
                .map_err(Error::Filesystem),
            None => Storage::disk(disk).map_err(Error::Filesystem),
        }
    }

    pub(super) fn http(&self) -> Result<Arc<Factory>> {
        match self.get_container() {
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

    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<ImageManager>()
    }
}

impl ImageFacade {
    pub fn from_bytes(bytes: impl Into<Bytes>) -> Result<Image> {
        Ok(Self::get_facade_root()?.from_bytes(bytes))
    }

    pub async fn from_stream(reader: impl AsyncRead) -> Result<Image> {
        Self::get_facade_root()?.from_stream(reader).await
    }

    pub fn from_base64(encoded: impl Into<Arc<str>>) -> Result<Image> {
        Ok(Self::get_facade_root()?.from_base64(encoded))
    }

    pub fn from_path(path: impl Into<PathBuf>) -> Result<Image> {
        Ok(Self::get_facade_root()?.from_path(path))
    }

    pub fn from_upload(file: &UploadedFile) -> Result<Image> {
        Ok(Self::get_facade_root()?.from_upload(file))
    }

    pub fn from_disk(disk: Arc<FilesystemAdapter>, path: impl Into<String>) -> Result<Image> {
        Ok(Self::get_facade_root()?.from_disk(disk, path))
    }

    pub fn from_storage(path: impl Into<String>, disk: Option<&str>) -> Result<Image> {
        Self::get_facade_root()?.from_storage(path, disk)
    }

    pub fn from_url(url: impl Into<String>) -> Result<Image> {
        Self::get_facade_root()?.from_url(url)
    }

    pub fn driver(name: Option<&str>) -> Result<Arc<dyn Driver>> {
        Self::get_facade_root()?.driver(name)
    }

    pub fn transform_using(
        driver: &str,
        transformation: &str,
        callback: TransformationHandler,
    ) -> Result<()> {
        Self::get_facade_root()?
            .transform_using(driver, transformation, callback)
            .map(|_| ())
    }

    pub fn get_default_driver() -> Result<String> {
        Ok(Self::get_facade_root()?.get_default_driver())
    }

    pub fn get_drivers() -> Result<HashMap<String, Arc<dyn Driver>>> {
        Ok(Self::get_facade_root()?.get_drivers())
    }

    pub fn get_container() -> Result<Option<Arc<Container>>> {
        Ok(Self::get_facade_root()?.get_container())
    }

    pub fn set_container(container: Arc<Container>) -> Result<()> {
        Self::get_facade_root()?.set_container(container);
        Ok(())
    }

    pub fn forget_drivers() -> Result<()> {
        Self::get_facade_root()?.forget_drivers();
        Ok(())
    }

    pub fn extend(
        name: impl Into<String>,
        creator: impl Fn(&ImageManager) -> Result<Arc<dyn Driver>> + Send + Sync + 'static,
    ) -> Result<()> {
        Self::get_facade_root()?.extend(name, creator).map(|_| ())
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
