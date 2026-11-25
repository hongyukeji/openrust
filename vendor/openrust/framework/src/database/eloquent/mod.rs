//! Model persistence, queries, relationships, and attribute lifecycle.

mod builder;
pub mod casts;
mod collection;
pub mod concerns;
mod detached_autoload;
mod dynamic_relations;
pub mod factories;
mod fields;
mod metadata;
mod model;
mod named_relations;
mod observer;
mod persistence;
mod pruning;
mod queueable;
mod related_batch;
pub mod relations;
mod repository;
mod route_binding;
mod scope;
mod serialization;
mod soft_deletes;
mod soft_deleting_scope;
mod strictness;
mod touches;
mod unique_ids;

use std::collections::BTreeMap;

use crate::database::Value;

mod searchable;

pub use crate::model_fields;
pub use builder::{Builder, Key};
pub use collection::{
    BorrowedRelatedModel, Collection, CollectionModel, MorphCollectionModel, RelatedCollectionModel,
};
pub(crate) use collection::{load_morph_aggregates, load_morph_relation};
pub(crate) use concerns::has_events::ModelEvents;
pub use concerns::{
    ModelState,
    has_relationships::EagerLoads,
    queries_relationships::{RelationAggregate, RelationAggregateQuery},
};
pub use dynamic_relations::DynamicRelation;
pub(crate) use dynamic_relations::RelationResolvers;
#[doc(hidden)]
pub use fields::FieldModel as __FieldModel;
pub(crate) use fields::FieldModel;
#[doc(hidden)]
pub use inventory as __inventory;
pub use metadata::ModelMetadata;
pub use model::{Model, ModelKeyType};
pub use named_relations::{MissingRelation, NamedRelations};
pub use observer::Observer;
pub use queueable::{ModelCollectionIdentifier, ModelIdentifier, QueueablePivot};
pub use related_batch::{RelatedBatch, RelatedCollection, RelatedModels};
pub use repository::{EloquentRepository, Repository};
pub use scope::Scope;
pub use serialization::VisibilityCondition;
pub use soft_deleting_scope::SoftDeletingScope;
pub(crate) use strictness::ModelPolicies;
pub use touches::Touches;
pub use unique_ids::UniqueId;

/// The raw attributes of a model, keyed by column name.
pub type Attributes = BTreeMap<String, Value>;

pub use pruning::{ModelPruner, ModelPruners};
pub use searchable::{SearchBuilder, SearchColumn, Searchable};
