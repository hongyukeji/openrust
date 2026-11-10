use std::marker::PhantomData;

use crate::database::query;

use super::Model;

/// A model-aware query built on the database query builder.
pub struct Builder<'connection, M: Model> {
    _query: query::Builder<'connection>,
    _model: PhantomData<M>,
}
