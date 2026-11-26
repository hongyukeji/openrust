#![forbid(unsafe_code)]
mod builder;
mod collection;
pub use collection::SearchableCollection;
mod commands;
mod engine_manager;
pub mod engines;
mod importer;
pub mod jobs;
mod model_observer;
mod provider;
mod searchable;
pub mod facades {
    use openrust::{container::ServiceKey, support::facades::Facade};
    pub struct Scout;
    impl Facade for Scout {
        type Service = crate::EngineManager;
        fn get_facade_accessor() -> ServiceKey {
            ServiceKey::typed::<crate::EngineManager>()
        }
    }
}
pub use builder::Builder;
pub use engine_manager::EngineManager;
pub use model_observer::ModelObserver;
pub use provider::ScoutServiceProvider;
pub use searchable::Searchable;
#[path = "../config/scout.rs"]
mod configuration;
pub type Result<T> = std::result::Result<T, Error>;
pub type ScoutFuture<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>>;
#[derive(Debug)]
pub enum Error {
    Database(openrust::database::DatabaseError),
    Container(openrust::contracts::container::BindingResolutionError),
    Queue(openrust::queue::QueueError),
    Pagination(openrust::pagination::PaginationError),
    InvalidConfiguration(String),
    NotSupported(String),
    InvalidPayload(serde_json::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(e) => write!(f, "Scout database operation failed: {e}"),
            Self::Container(e) => write!(f, "Scout service resolution failed: {e}"),
            Self::Queue(e) => write!(f, "Scout queue operation failed: {e}"),
            Self::Pagination(e) => write!(f, "Scout pagination failed: {e}"),
            Self::InvalidConfiguration(e) => write!(f, "Invalid Scout configuration: {e}"),
            Self::NotSupported(e) => f.write_str(e),
            Self::InvalidPayload(e) => write!(f, "Invalid Scout payload: {e}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            Self::Container(e) => Some(e),
            Self::Queue(e) => Some(e),
            Self::Pagination(e) => Some(e),
            Self::InvalidPayload(e) => Some(e),
            _ => None,
        }
    }
}
macro_rules! from_error {
    ($ty:ty,$variant:ident) => {
        impl From<$ty> for Error {
            fn from(e: $ty) -> Self {
                Self::$variant(e)
            }
        }
    };
}
from_error!(openrust::database::DatabaseError, Database);
from_error!(
    openrust::contracts::container::BindingResolutionError,
    Container
);
from_error!(openrust::queue::QueueError, Queue);
from_error!(openrust::pagination::PaginationError, Pagination);
from_error!(serde_json::Error, InvalidPayload);
pub fn openrust_providers() -> Vec<Box<dyn openrust::support::ServiceProvider>> {
    vec![Box::new(ScoutServiceProvider)]
}

pub(crate) fn database_error(source: Error) -> openrust::database::DatabaseError {
    openrust::database::DatabaseError::ModelEvent(Box::new(
        openrust::events::EventError::Listener {
            source: Box::new(source),
        },
    ))
}
