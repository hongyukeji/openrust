use std::marker::PhantomData;

use crate::database::eloquent::Model;

/// An Eloquent morph to relationship.
pub struct MorphTo<P: Model> {
    _models: PhantomData<(P,)>,
}
