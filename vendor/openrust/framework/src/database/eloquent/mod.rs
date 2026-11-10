//! Model persistence, queries, relationships, and attribute lifecycle.

mod builder;
pub mod casts;
mod collection;
pub mod concerns;
mod model;
pub mod relations;
mod scope;

use std::collections::BTreeMap;

use crate::database::Value;

pub use builder::Builder;
pub use collection::Collection;
pub use model::Model;
pub use scope::Scope;

/// The raw attributes of a model, keyed by column name.
pub type Attributes = BTreeMap<String, Value>;

/// The table, connection, primary key, and other options of a model type.
pub struct ModelMetadata {
    _private: (),
}

/// A model's attributes, persistence status, and loaded relations.
pub struct ModelState {
    _private: (),
}
