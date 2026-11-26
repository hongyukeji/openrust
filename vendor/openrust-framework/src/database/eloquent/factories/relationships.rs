use std::future::Future;

use crate::database::DatabaseResult;

use super::super::{
    Attributes, Model,
    relations::{BelongsTo, HasMany, HasOne, MorphMany, MorphOne, MorphTo},
};

/// A factory relationship that saves a generated child of a persisted model.
pub trait FactoryChild<P: Model, R: Model>: Send + Sync + 'static {
    #[doc(hidden)]
    fn attributes(&self, _parent: &P) -> DatabaseResult<(Attributes, Attributes)> {
        Ok((Attributes::new(), Attributes::new()))
    }

    fn save(&self, parent: &P, child: &mut R) -> impl Future<Output = DatabaseResult<bool>> + Send;
}

macro_rules! factory_children {
    ($($relation:ident),+ $(,)?) => {
        $(impl<P: Model, R: Model> FactoryChild<P, R> for $relation<P, R> {
            fn attributes(&self, parent: &P) -> DatabaseResult<(Attributes, Attributes)> {
                self.factory_attributes(parent)
            }

            fn save(&self, parent: &P, child: &mut R) -> impl Future<Output = DatabaseResult<bool>> + Send {
                self.save_factory_child(parent, child)
            }
        })+
    };
}

factory_children!(HasOne, HasMany, MorphOne, MorphMany);

/// A factory relationship that associates the generated model with an owner.
pub trait FactoryParent<M: Model, R: Model>: Send + Sync + 'static {
    fn associate(&self, model: &mut M, owner: &R) -> DatabaseResult<()>;

    #[doc(hidden)]
    fn factory_foreign_key(&self) -> Option<&str> {
        None
    }

    #[doc(hidden)]
    fn factory_morph_type(&self, _owner: &R) -> DatabaseResult<Option<(&str, &'static str)>> {
        Ok(None)
    }
}

impl<M: Model, R: Model> FactoryParent<M, R> for BelongsTo<M, R> {
    fn associate(&self, model: &mut M, owner: &R) -> DatabaseResult<()> {
        Self::associate(self, model, owner)
    }

    fn factory_foreign_key(&self) -> Option<&str> {
        Some(self.identity_columns().0)
    }
}

impl<M: Model, R: Model> FactoryParent<M, R> for MorphTo<M> {
    fn associate(&self, model: &mut M, owner: &R) -> DatabaseResult<()> {
        self.associate_mapped(model, owner)
    }

    fn factory_foreign_key(&self) -> Option<&str> {
        Some(self.id_column())
    }

    fn factory_morph_type(&self, owner: &R) -> DatabaseResult<Option<(&str, &'static str)>> {
        let alias = owner.get_morph_class()?;
        self.validate_target::<R>(alias)?;
        Ok(Some((self.type_column(), alias)))
    }
}
