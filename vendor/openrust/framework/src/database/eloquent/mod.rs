//! Model persistence, queries, relationships, and attribute lifecycle.

mod builder;
pub mod casts;
mod collection;
pub mod concerns;
mod metadata;
mod model;
mod persistence;
pub mod relations;
mod scope;
mod soft_deletes;
mod soft_deleting_scope;

use std::collections::BTreeMap;

use crate::database::Value;

pub use builder::Builder;
pub use collection::Collection;
pub use concerns::ModelState;
pub use metadata::ModelMetadata;
pub use model::Model;
pub use scope::Scope;

/// The raw attributes of a model, keyed by column name.
pub type Attributes = BTreeMap<String, Value>;
