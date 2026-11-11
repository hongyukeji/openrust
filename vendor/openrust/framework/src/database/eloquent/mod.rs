//! Model persistence, queries, relationships, and attribute lifecycle.

mod builder;
pub mod casts;
mod collection;
pub mod concerns;
pub mod factories;
mod metadata;
mod model;
mod persistence;
mod related_batch;
pub mod relations;
mod route_binding;
mod scope;
mod serialization;
mod soft_deletes;
mod soft_deleting_scope;

use std::collections::BTreeMap;

use crate::database::Value;

mod searchable;

pub use builder::Builder;
pub use collection::Collection;
pub use concerns::{
    ModelState,
    has_relationships::EagerLoads,
    queries_relationships::{RelationAggregate, RelationAggregateQuery},
};
pub use metadata::ModelMetadata;
pub use model::Model;
pub use related_batch::{RelatedBatch, RelatedModels};
pub use scope::Scope;
pub use serialization::VisibilityCondition;

/// The raw attributes of a model, keyed by column name.
pub type Attributes = BTreeMap<String, Value>;

pub use searchable::{SearchBuilder, SearchColumn, Searchable};
