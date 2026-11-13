use crate::{
    contracts::routing::UrlRoutable,
    database::{DatabaseError, DatabaseResult, Value, connection::Pool},
    http::{HttpError, HttpFuture},
    routing::{BindingContext, UrlValue},
};

use super::{Builder, Model};

impl<M: Model> UrlRoutable for M {
    fn get_route_key_name() -> &'static str {
        M::getRouteKeyName()
    }

    fn get_route_key(&self) -> Option<crate::routing::UrlValue> {
        let key = match M::getRouteKey(self)? {
            Value::Integer(value) => crate::routing::UrlValue::Number((*value).into()),
            Value::Unsigned(value) => crate::routing::UrlValue::Number((*value).into()),
            Value::Text(value) => crate::routing::UrlValue::String(value.clone()),
            _ => return None,
        };
        Some(key)
    }

    fn route_url_attributes(&self) -> std::collections::HashMap<String, UrlValue> {
        self.state()
            .getAttributes()
            .iter()
            .filter_map(|(name, value)| {
                let value = match value {
                    Value::Integer(value) => UrlValue::Number((*value).into()),
                    Value::Unsigned(value) => UrlValue::Number((*value).into()),
                    Value::Text(value) => UrlValue::String(value.clone()),
                    Value::Bool(value) => UrlValue::Bool(*value),
                    Value::Real(value) => UrlValue::Number(serde_json::Number::from_f64(*value)?),
                    _ => return None,
                };
                Some((name.clone(), value))
            })
            .collect()
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
