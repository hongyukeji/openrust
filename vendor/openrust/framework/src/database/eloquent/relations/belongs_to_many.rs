use std::marker::PhantomData;

use crate::database::eloquent::Model;

/// An Eloquent belongs to many relationship.
pub struct BelongsToMany<P: Model, R: Model> {
    _models: PhantomData<(P, R)>,
}
