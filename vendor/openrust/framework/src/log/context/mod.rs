//! Context associated with the current request or job.

mod facade;
mod repository;

pub use facade::Context;
pub use repository::{ContextError, Repository};
