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
pub use attributes::attributes_from_row;
pub use mass_assignment::{DiscardedAttributeViolation, MassAssignment, is_unguarded, unguarded};

pub use model::{ModelIdentity, ModelMetadata, MorphIdentity};
