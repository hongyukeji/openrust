use std::marker::PhantomData;

use crate::database::eloquent::Model;

/// An Eloquent belongs to relationship.
pub struct BelongsTo<P: Model, R: Model> {
    _models: PhantomData<(P, R)>,
}
