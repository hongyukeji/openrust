use std::marker::PhantomData;

use crate::database::eloquent::Model;

/// An Eloquent has one relationship.
pub struct HasOne<P: Model, R: Model> {
    _models: PhantomData<(P, R)>,
}
