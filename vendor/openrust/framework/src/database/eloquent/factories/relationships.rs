use std::future::Future;

use crate::database::DatabaseResult;

use super::super::{
    Model,
    relations::{BelongsTo, HasMany, HasOne, MorphMany, MorphOne, MorphTo},
};

/// A factory relationship that saves a generated child of a persisted model.
pub trait FactoryChild<P: Model, R: Model>: Send + Sync + 'static {
    fn save(&self, parent: &P, child: &mut R) -> impl Future<Output = DatabaseResult<bool>> + Send;
}

macro_rules! factory_children {
    ($($relation:ident),+ $(,)?) => {
        $(impl<P: Model, R: Model> FactoryChild<P, R> for $relation<P, R> {
            fn save(&self, parent: &P, child: &mut R) -> impl Future<Output = DatabaseResult<bool>> + Send {
                Self::save(self, parent, child)
            }
        })+
    };
}

factory_children!(HasOne, HasMany, MorphOne, MorphMany);

/// A factory relationship that associates the generated model with an owner.
pub trait FactoryParent<M: Model, R: Model>: Send + Sync + 'static {
    fn associate(&self, model: &mut M, owner: &R) -> DatabaseResult<()>;
}

impl<M: Model, R: Model> FactoryParent<M, R> for BelongsTo<M, R> {
    fn associate(&self, model: &mut M, owner: &R) -> DatabaseResult<()> {
        Self::associate(self, model, owner)
    }
}

impl<M: Model, R: Model> FactoryParent<M, R> for MorphTo<M> {
    fn associate(&self, model: &mut M, owner: &R) -> DatabaseResult<()> {
        self.associateMapped(model, owner)
    }
}
