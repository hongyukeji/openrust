use std::marker::PhantomData;

use crate::database::eloquent::Model;

/// An Eloquent morph one relationship.
pub struct MorphOne<P: Model, R: Model> {
    _models: PhantomData<(P, R)>,
}
