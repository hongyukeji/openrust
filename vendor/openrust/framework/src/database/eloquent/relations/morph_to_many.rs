use std::marker::PhantomData;

use crate::database::eloquent::Model;

/// An Eloquent morph to many relationship.
pub struct MorphToMany<P: Model, R: Model> {
    _models: PhantomData<(P, R)>,
}
