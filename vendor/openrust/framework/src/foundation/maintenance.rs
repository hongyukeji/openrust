//! Application maintenance state shared by the runtime and command entry points.

use std::{
    error::Error,
    fmt,
    future::Future,
    io,
    path::Path,
    pin::Pin,
    sync::{Arc, RwLock},
};

use serde_json::{Map, Value};

use crate::{
    cache::{CacheError, CacheManager, Value as CacheValue},
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

mod manager;
pub use manager::MaintenanceModeManager;

const CACHE_KEY: &str = "illuminate:foundation:down";

pub type MaintenancePayload = Map<String, Value>;
pub type MaintenanceFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, MaintenanceError>> + Send + 'a>>;

#[derive(Debug)]
pub enum MaintenanceError {
    InvalidDriver,
    DriverResolutionCycle,
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
            Self::InvalidDriver => output.write_str("维护模式驱动配置无效"),
            Self::DriverResolutionCycle => output.write_str("维护驱动解析存在循环依赖"),
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
    File(Arc<super::Paths>),
    Cache(Arc<CacheManager>, String),
    Array(RwLock<Option<MaintenancePayload>>),
}

impl MaintenanceMode for Backend {
    fn activate<'a>(&'a self, payload: &'a MaintenancePayload) -> MaintenanceFuture<'a, ()> {
        Box::pin(async move {
            match self {
                Self::File(path) => {
                    let path = path.storage_path("framework/down");
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
                Self::Cache(manager, name) => {
                    let cache = manager.store(Some(name)).map_err(MaintenanceError::Cache)?;
                    if cache
                        .put_with_default(
                            CACHE_KEY,
                            &CacheValue::Json(Value::Object(payload.clone())),
                        )
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
                Self::File(path) => {
                    match tokio::fs::remove_file(path.storage_path("framework/down")).await {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                        result => result.map_err(MaintenanceError::Io),
                    }
                }
                Self::Cache(manager, name) => manager
                    .store(Some(name))
                    .map_err(MaintenanceError::Cache)?
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
                Self::File(path) => tokio::fs::try_exists(path.storage_path("framework/down"))
                    .await
                    .map_err(MaintenanceError::Io),
                Self::Cache(manager, name) => manager
                    .store(Some(name))
                    .map_err(MaintenanceError::Cache)?
                    .has(CACHE_KEY)
                    .await
                    .map_err(MaintenanceError::Cache),
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
                Self::File(path) => read_file(&path.storage_path("framework/down")).await,
                Self::Cache(manager, name) => match manager
                    .store(Some(name))
                    .map_err(MaintenanceError::Cache)?
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

pub(super) async fn read_file(path: &Path) -> Result<Option<MaintenancePayload>, MaintenanceError> {
    match tokio::fs::read(path).await {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(MaintenanceError::Json),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(MaintenanceError::Io(error)),
    }
}

pub(crate) struct MaintenanceServiceProvider;

impl ServiceProvider for MaintenanceServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let owner = Arc::downgrade(&app.container_handle());
            app.container()
                .singleton::<MaintenanceModeManager, _>(move |container| {
                    Ok(Arc::new(MaintenanceModeManager::for_container(
                        owner.clone(),
                        container.get::<dyn Config>("config")?,
                    )))
                })?;
            app.container()
                .bind::<dyn MaintenanceMode, _>(|container| {
                    container
                        .make::<MaintenanceModeManager>()?
                        .driver(None)
                        .map_err(|source| BindingResolutionError::FactoryFailed {
                            service: "maintenance",
                            source: Box::new(source),
                        })
                })?;
            Ok(())
        })
    }
}

impl From<crate::contracts::container::BindingResolutionError> for MaintenanceError {
    fn from(source: crate::contracts::container::BindingResolutionError) -> Self {
        Self::Unavailable(source)
    }
}
