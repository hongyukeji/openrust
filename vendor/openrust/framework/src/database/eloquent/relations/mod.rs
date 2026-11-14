//! Typed model relationship descriptors.

mod belongs_to;
mod belongs_to_many;
pub(super) mod custom_pivot;
mod default_model;
mod has_many;
mod has_many_through;
mod has_one;
mod has_one_through;
mod morph_many;
mod morph_one;
mod morph_pivot;
mod morph_to;
mod morph_to_many;
mod one_of_many;
mod pivot;
mod pivot_constraints;
pub(super) mod relation;

pub use belongs_to::BelongsTo;
pub(super) use belongs_to_many::PivotTouchIdentity;
pub use belongs_to_many::{BelongsToMany, RelatedValues, SyncChanges, ToggleChanges};
pub use has_many::HasMany;
pub use has_many_through::HasManyThrough;
pub use has_one::HasOne;
pub use has_one_through::HasOneThrough;
pub use morph_many::MorphMany;
pub use morph_one::MorphOne;
pub use morph_pivot::MorphPivot;
pub use morph_to::{MorphIdentity, MorphTarget, MorphTo};
pub use morph_to_many::MorphToMany;
pub use one_of_many::OneOfManyAggregate;
pub use pivot::Pivot;
pub use relation::{ConstrainableRelation, ExistenceRelation, NestedRelation, Relation};

fn require_soft_deletes<R: super::Model>() -> crate::database::DatabaseResult<()> {
    R::metadata()
        .deleted_at
        .map(|_| ())
        .ok_or_else(super::soft_deletes::unsupported)
}
