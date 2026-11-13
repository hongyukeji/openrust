//! Model persistence, queries, relationships, and attribute lifecycle.

mod builder;
pub mod casts;
mod collection;
pub mod concerns;
mod detached_autoload;
mod dynamic_relations;
pub mod factories;
mod metadata;
mod model;
mod named_relations;
mod observer;
mod persistence;
mod queueable;
mod related_batch;
pub mod relations;
mod route_binding;
mod scope;
mod serialization;
mod soft_deletes;
mod soft_deleting_scope;
mod touches;
mod unique_ids;

use std::collections::BTreeMap;

use crate::database::Value;

mod searchable;

pub use builder::Builder;
pub use collection::{
    BorrowedRelatedModel, Collection, CollectionModel, MorphCollectionModel, RelatedCollectionModel,
};
pub(crate) use concerns::has_events::ModelEvents;
pub use concerns::{
    ModelState,
    has_relationships::EagerLoads,
    queries_relationships::{RelationAggregate, RelationAggregateQuery},
};
pub use dynamic_relations::DynamicRelation;
pub(crate) use dynamic_relations::RelationResolvers;
pub use metadata::ModelMetadata;
pub use model::Model;
pub use named_relations::NamedRelations;
pub use observer::Observer;
pub use queueable::{ModelCollectionIdentifier, ModelIdentifier};
pub use related_batch::{RelatedBatch, RelatedModels};
pub use scope::Scope;
pub use serialization::VisibilityCondition;
pub use touches::Touches;
pub use unique_ids::UniqueId;

/// The raw attributes of a model, keyed by column name.
pub type Attributes = BTreeMap<String, Value>;

pub use searchable::{SearchBuilder, SearchColumn, Searchable};
