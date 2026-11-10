use std::marker::PhantomData;

use crate::{contracts::auth::Authenticatable, database::eloquent::Model};

/// Retrieves authentication identities through a registered Eloquent model.
pub struct EloquentUserProvider<M: Model + Authenticatable> {
    _model: PhantomData<M>,
}
