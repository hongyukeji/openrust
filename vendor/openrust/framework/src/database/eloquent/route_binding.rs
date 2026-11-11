use crate::{
    contracts::routing::UrlRoutable,
    database::{DatabaseError, DatabaseResult, Value, connection::Pool},
    http::{HttpError, HttpFuture},
    routing::BindingContext,
};

use super::{Builder, Model};

impl<M: Model> UrlRoutable for M {
    fn get_route_key_name() -> &'static str {
        M::getRouteKeyName()
    }

    fn resolve_route_binding(
        binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>> {
        M::resolveRouteBinding(binding)
    }

    fn resolve_child_route_binding(
        binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>> {
        M::resolveChildRouteBinding(binding)
    }
}

pub(super) async fn lookup<M: Model>(
    mut query: Builder<'static, M>,
    binding: BindingContext<'_>,
) -> DatabaseResult<Option<M>> {
    let field = binding.field().unwrap_or_else(|| M::getRouteKeyName());
    let Some(value) = M::routeBindingValue(field, binding.value()) else {
        return Ok(None);
    };
    if matches!(value, Value::Unsigned(value) if i64::try_from(value).is_err())
        && matches!(query.connection.pool, Pool::Sqlite(_))
    {
        return Ok(None);
    }
    if binding.allowsTrashedBindings() && M::metadata().deleted_at.is_some() {
        query = query.withTrashed();
    }
    query.r#where(&query.qualifyColumn(field), "=", value);
    query.first().await
}

pub(super) fn http_error(source: DatabaseError) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
