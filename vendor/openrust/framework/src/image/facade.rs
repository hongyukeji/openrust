use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock},
};

use axum::body::Bytes;
use tokio::{io::AsyncRead, sync::Semaphore};

use crate::{
    container::ServiceKey,
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    filesystem::{FilesystemAdapter, Storage},
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

/// Resolves named drivers and limits processing within one application.
#[derive(Clone)]
pub struct ImageManager {
    default: Arc<str>,
    config: Option<Arc<dyn Repository>>,
    drivers: Arc<RwLock<HashMap<String, Arc<dyn Driver>>>>,
    transformation_handlers: Arc<RwLock<TransformationHandlers>>,
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
            drivers: Arc::new(RwLock::new(HashMap::from([(
                "image".into(),
                Arc::new(NativeDriver::default()) as Arc<dyn Driver>,
            )]))),
            transformation_handlers: Arc::default(),
            limits,
            workers: Arc::new(Semaphore::new(
                std::thread::available_parallelism()
                    .map_or(1, usize::from)
                    .min(4),
            )),
            http: None,
        }
    }

    pub fn extend(&self, name: impl Into<String>, driver: impl Driver) -> Result<&Self> {
        let name = name.into();
        let handlers = self
            .transformation_handlers
            .read()
            .map_err(|_| Error::Io(std::io::Error::other("图片转换处理器锁中毒")))?
            .get(&name)
            .cloned()
            .unwrap_or_default();
        for (transformation, callback) in handlers {
            driver.transformUsing(&transformation, callback)?;
        }
        self.drivers
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name, Arc::new(driver));
        Ok(self)
    }

    pub fn transformUsing(
        &self,
        driver: &str,
        transformation: &str,
        callback: TransformationHandler,
    ) -> Result<&Self> {
        self.transformation_handlers
            .write()
            .map_err(|_| Error::Io(std::io::Error::other("图片转换处理器锁中毒")))?
            .entry(driver.to_owned())
            .or_default()
            .insert(transformation.to_owned(), callback.clone());
        let resolved = self
            .drivers
            .read()
            .map_err(|_| Error::Io(std::io::Error::other("图片驱动锁中毒")))?
            .get(driver)
            .cloned();
        if let Some(resolved) = resolved {
            resolved.transformUsing(transformation, callback)?;
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
        self.drivers
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn driver(&self, name: Option<&str>) -> Result<Arc<dyn Driver>> {
        let name = name
            .map(str::to_owned)
            .unwrap_or_else(|| self.getDefaultDriver());
        self.drivers
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&name)
            .cloned()
            .ok_or_else(|| Error::UnknownDriver(name))
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
        Ok(self.fromDisk(Storage::disk(disk).map_err(Error::Filesystem)?, path))
    }

    pub fn fromUrl(&self, url: impl Into<String>) -> Result<Image> {
        let factory = match &self.http {
            Some(factory) => factory.clone(),
            None => Arc::new(Factory::new().map_err(Error::Http)?),
        };
        Ok(Image::new(self.clone(), Source::Url(factory, url.into())))
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
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .fromBytes(bytes))
    }

    pub async fn fromStream(reader: impl AsyncRead) -> Result<Image> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .fromStream(reader)
            .await
    }

    pub fn fromBase64(encoded: impl Into<Arc<str>>) -> Result<Image> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .fromBase64(encoded))
    }

    pub fn fromPath(path: impl Into<PathBuf>) -> Result<Image> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .fromPath(path))
    }

    pub fn fromUpload(file: &UploadedFile) -> Result<Image> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .fromUpload(file))
    }

    pub fn fromDisk(disk: Arc<FilesystemAdapter>, path: impl Into<String>) -> Result<Image> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .fromDisk(disk, path))
    }

    pub fn fromStorage(path: impl Into<String>, disk: Option<&str>) -> Result<Image> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .fromStorage(path, disk)
    }

    pub fn fromUrl(url: impl Into<String>) -> Result<Image> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .fromUrl(url)
    }

    pub fn driver(name: Option<&str>) -> Result<Arc<dyn Driver>> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .driver(name)
    }

    pub fn transformUsing(
        driver: &str,
        transformation: &str,
        callback: TransformationHandler,
    ) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .transformUsing(driver, transformation, callback)
            .map(|_| ())
    }

    pub fn getDefaultDriver() -> Result<String> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .getDefaultDriver())
    }

    pub fn getDrivers() -> Result<HashMap<String, Arc<dyn Driver>>> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .getDrivers())
    }

    pub fn extend(name: impl Into<String>, driver: impl Driver) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .extend(name, driver)
            .map(|_| ())
    }
}

pub(crate) struct ImageServiceProvider;

impl ServiceProvider for ImageServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().singleton::<ImageManager, _>(|container| {
                let config = container.get::<dyn Repository>("config")?;
                let mut manager = ImageManager::new(Limits::default());
                manager.config = Some(config);
                manager.http = Some(container.make::<Factory>()?);
                Ok(Arc::new(manager))
            })?;
            Ok(())
        })
    }
}
