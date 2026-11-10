use std::marker::PhantomData;

use crate::database::eloquent::Model;

/// An Eloquent has many relationship.
pub struct HasMany<P: Model, R: Model> {
    _models: PhantomData<(P, R)>,
}
