use std::marker::PhantomData;

use crate::database::eloquent::Model;

/// An Eloquent has many through relationship.
pub struct HasManyThrough<P: Model, I: Model, R: Model> {
    _models: PhantomData<(P, I, R)>,
}
