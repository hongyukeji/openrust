use std::marker::PhantomData;

use crate::database::eloquent::Model;

/// An Eloquent has one through relationship.
pub struct HasOneThrough<P: Model, I: Model, R: Model> {
    _models: PhantomData<(P, I, R)>,
}
