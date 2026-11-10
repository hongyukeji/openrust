use std::marker::PhantomData;

use crate::database::eloquent::Model;

/// An Eloquent morph many relationship.
pub struct MorphMany<P: Model, R: Model> {
    _models: PhantomData<(P, R)>,
}
