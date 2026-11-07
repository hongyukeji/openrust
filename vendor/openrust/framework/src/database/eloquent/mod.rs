//! Eloquent 的通用模型基础；提供显式元数据、持久身份、属性原值与变化快照。
//!
//! 这些基础不代表完整 Model CRUD、关系查询、mass assignment 或事件已实现。

pub(crate) mod attribute_cast;
mod attributes;
pub use attribute_cast::AttributeCast;
mod attribute_store;
pub use attribute_store::{AttributeGetter, AttributeMutation, AttributeSetter, ModelAttributes};
#[cfg(feature = "hashing")]
pub mod casts;
mod mass_assignment;
mod model;
mod model_events;
mod refresh;
mod save;
mod timestamps;
mod unique_ids;
pub use attributes::attributes_from_row;
pub use mass_assignment::{DiscardedAttributeViolation, MassAssignment, is_unguarded, unguarded};

pub use model::{ModelIdentity, ModelMetadata, MorphIdentity};
pub use model_events::{
    MappedModelEvent, dispatch_mapped_model_event, fire_model_event, without_model_events,
};
pub use refresh::{HydrateModel, fresh, refresh, refresh_for_update};
pub use save::{
    SaveContext, SaveModel, force_create, save, save_or_fail, save_quietly, save_with_query,
    update, update_or_fail, update_quietly,
};
pub use timestamps::without_timestamps_on;
#[cfg(feature = "db-uuid")]
pub use unique_ids::new_uuid_v7;
