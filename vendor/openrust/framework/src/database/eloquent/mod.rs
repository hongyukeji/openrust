//! Eloquent 的通用模型基础；提供显式元数据、持久身份、属性原值与变化快照。
//!
//! 这些基础不代表完整 Model CRUD、关系查询、mass assignment 或事件已实现。

pub(crate) mod attribute_cast;
mod attributes;
pub use attribute_cast::AttributeCast;
mod attribute_store;
mod belongs_to;
mod belongs_to_many;
pub use attribute_store::{AttributeGetter, AttributeMutation, AttributeSetter, ModelAttributes};
#[cfg(feature = "hashing")]
pub mod casts;
mod has_many;
mod mass_assignment;
mod model;
mod model_events;
mod one_of_many;
mod refresh;
mod relations;
mod save;
mod timestamps;
mod unique_ids;
pub use attributes::attributes_from_row;
pub use belongs_to::BelongsTo;
pub use belongs_to_many::{
    BelongsToMany, CustomPivot, Pivot, PivotAttachment, PivotChanges, PivotKeys, PivotSource,
    PivotToggleChanges,
};
pub use has_many::{HasMany, HasOne, HasRelationships};
pub use mass_assignment::{DiscardedAttributeViolation, MassAssignment, is_unguarded, unguarded};

pub use model::{ModelIdentity, ModelMetadata, MorphIdentity};
pub use model_events::{
    MappedModelEvent, dispatch_mapped_model_event, fire_model_event, without_model_events,
};
pub use one_of_many::RelationAggregate;
pub use refresh::{HydrateModel, fresh, fresh_with, refresh, refresh_for_update};
pub use relations::{LoadedRelation, ModelRelations, RelationLoad};
pub use save::{
    SaveContext, SaveModel, force_create, save, save_or_fail, save_quietly, save_with_query,
    update, update_or_fail, update_quietly,
};
pub use timestamps::without_timestamps_on;
#[cfg(feature = "db-uuid")]
pub use unique_ids::new_uuid_v7;
