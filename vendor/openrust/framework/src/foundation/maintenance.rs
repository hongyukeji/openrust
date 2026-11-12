//! Application maintenance state shared by the runtime and command entry points.

use std::{
    error::Error,
    fmt,
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, RwLock},
};

use serde_json::{Map, Value};

use crate::{
    cache::{CacheError, CacheManager, Repository, Value as CacheValue},
    container::Container,
    contracts::{
        cache::Repository as _,
        config::Repository as Config,
        container::{BindingResolutionError, Container as _},
        foundation::{Application as _, MaintenanceMode},
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

const CACHE_KEY: &str = "illuminate:foundation:down";

pub type MaintenancePayload = Map<String, Value>;
pub type MaintenanceFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, MaintenanceError>> + Send + 'a>>;

#[derive(Debug)]
pub enum MaintenanceError {
    InvalidDriver,
    InvalidPayload,
    Io(io::Error),
    Json(serde_json::Error),
    Cache(CacheError),
    WriteRejected,
    Unavailable(BindingResolutionError),
}

impl fmt::Display for MaintenanceError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDriver => output.write_str("invalid application maintenance driver"),
            Self::InvalidPayload => output.write_str("maintenance state must be a JSON object"),
            Self::Io(error) => write!(output, "maintenance state I/O failed: {error}"),
            Self::Json(error) => write!(output, "invalid maintenance state: {error}"),
            Self::Cache(error) => write!(output, "maintenance cache failed: {error}"),
            Self::WriteRejected => output.write_str("maintenance cache rejected the update"),
            Self::Unavailable(error) => write!(output, "maintenance service unavailable: {error}"),
        }
    }
}

impl Error for MaintenanceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Cache(error) => Some(error),
            Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }
}

enum Backend {
    File(PathBuf),
    Cache(Arc<Repository>),
    Array(RwLock<Option<MaintenancePayload>>),
}

impl MaintenanceMode for Backend {
    fn activate<'a>(&'a self, payload: &'a MaintenancePayload) -> MaintenanceFuture<'a, ()> {
        Box::pin(async move {
            match self {
                Self::File(path) => {
                    let path = path.clone();
                    let payload =
                        serde_json::to_vec_pretty(payload).map_err(MaintenanceError::Json)?;
                    tokio::task::spawn_blocking(move || {
                        use std::io::Write as _;
                        let directory = path.parent().expect("maintenance path has a parent");
                        std::fs::create_dir_all(directory)?;
                        let mut file = tempfile::NamedTempFile::new_in(directory)?;
                        file.write_all(&payload)?;
                        file.persist(path).map_err(|error| error.error)?;
                        Ok::<(), io::Error>(())
                    })
                    .await
                    .map_err(|error| MaintenanceError::Io(io::Error::other(error)))?
                    .map_err(MaintenanceError::Io)
                }
                Self::Cache(cache) => {
                    if cache
                        .forever(CACHE_KEY, &CacheValue::Json(Value::Object(payload.clone())))
                        .await
                        .map_err(MaintenanceError::Cache)?
                    {
                        Ok(())
                    } else {
                        Err(MaintenanceError::WriteRejected)
                    }
                }
                Self::Array(state) => {
                    *state
                        .write()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(payload.clone());
                    Ok(())
                }
            }
        })
    }

    fn deactivate(&self) -> MaintenanceFuture<'_, ()> {
        Box::pin(async move {
            match self {
                Self::File(path) => match tokio::fs::remove_file(path).await {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                    result => result.map_err(MaintenanceError::Io),
                },
                Self::Cache(cache) => cache
                    .forget(CACHE_KEY)
                    .await
                    .map(|_| ())
                    .map_err(MaintenanceError::Cache),
                Self::Array(state) => {
                    *state
                        .write()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                    Ok(())
                }
            }
        })
    }

    fn active(&self) -> MaintenanceFuture<'_, bool> {
        Box::pin(async move {
            match self {
                Self::File(path) => tokio::fs::try_exists(path)
                    .await
                    .map_err(MaintenanceError::Io),
                Self::Cache(cache) => cache.has(CACHE_KEY).await.map_err(MaintenanceError::Cache),
                Self::Array(state) => Ok(state
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_some()),
            }
        })
    }

    fn data(&self) -> MaintenanceFuture<'_, Option<MaintenancePayload>> {
        Box::pin(async move {
            match self {
                Self::File(path) => read_file(path).await,
                Self::Cache(cache) => match cache
                    .get(CACHE_KEY)
                    .await
                    .map_err(MaintenanceError::Cache)?
                {
                    Some(CacheValue::Json(Value::Object(payload))) => Ok(Some(payload)),
                    None | Some(CacheValue::Json(Value::Null)) => Ok(None),
                    Some(_) => Err(MaintenanceError::InvalidPayload),
                },
                Self::Array(state) => Ok(state
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()),
            }
        })
    }
}

pub(super) fn file_path(base_path: &Path) -> PathBuf {
    base_path.join("storage/framework/down")
}

pub(super) async fn read_file(path: &Path) -> Result<Option<MaintenancePayload>, MaintenanceError> {
    match tokio::fs::read(path).await {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(MaintenanceError::Json),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(MaintenanceError::Io(error)),
    }
}

fn configured_driver(container: &Container, file: &Path) -> Result<Backend, MaintenanceError> {
    let config = container
        .get::<dyn Config>("config")
        .map_err(MaintenanceError::Unavailable)?;
    match config.get("app.maintenance.driver").as_ref() {
        None | Some(Value::Null) => Ok(Backend::File(file.to_owned())),
        Some(Value::String(driver)) if driver == "file" => Ok(Backend::File(file.to_owned())),
        Some(Value::String(driver)) if driver == "array" => Ok(Backend::Array(RwLock::default())),
        Some(Value::String(driver)) if driver == "cache" => {
            let store = config.get("app.maintenance.store");
            let cache = container
                .make::<CacheManager>()
                .map_err(MaintenanceError::Unavailable)?;
            let name = store
                .as_ref()
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty());
            Ok(Backend::Cache(
                cache.store(name).map_err(MaintenanceError::Cache)?,
            ))
        }
        _ => Err(MaintenanceError::InvalidDriver),
    }
}

pub(crate) struct MaintenanceServiceProvider;

impl ServiceProvider for MaintenanceServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let file = file_path(app.base_path());
            app.container()
                .singleton::<dyn MaintenanceMode, _>(move |container| {
                    let driver = configured_driver(container, &file).map_err(|source| {
                        BindingResolutionError::FactoryFailed {
                            service: "maintenance",
                            source: Box::new(source),
                        }
                    })?;
                    Ok(Arc::new(driver))
                })?;
            Ok(())
        })
    }
}
