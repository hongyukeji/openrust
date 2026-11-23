use crate::{
    contracts::routing::UrlRoutable,
    database::{DatabaseResult, Value},
    http::{HttpError, HttpFuture},
    routing::{BindingContext, UrlError, UrlResult, UrlValue},
};

use super::{Builder, Model};

impl<M: Model> UrlRoutable for M {
    fn get_route_key_name() -> &'static str {
        M::getRouteKeyName()
    }

    fn get_route_key(&self) -> UrlResult<Option<UrlValue<'static>>> {
        url_value(M::getRouteKey(self))
    }

    fn route_url_attribute(&self, field: &str) -> UrlResult<Option<UrlValue<'static>>> {
        url_value(self.getAttributeValue(field))
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

fn url_value(value: DatabaseResult<Option<Value>>) -> UrlResult<Option<UrlValue<'static>>> {
    match value.map_err(UrlError::ModelAttribute)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Integer(value)) => Ok(Some(UrlValue::Number(value.into()))),
        Some(Value::Unsigned(value)) => Ok(Some(UrlValue::Number(value.into()))),
        Some(Value::Text(value)) => Ok(Some(UrlValue::String(value))),
        Some(Value::Bool(value)) => Ok(Some(UrlValue::Bool(value))),
        Some(Value::Real(value)) => serde_json::Number::from_f64(value)
            .map(|value| Some(UrlValue::Number(value)))
            .ok_or(UrlError::InvalidParameter),
        _ => Err(UrlError::InvalidParameter),
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
        && query.connection.is_sqlite()
    {
        return Ok(None);
    }
    if binding.allows_trashed_bindings() && M::metadata().deleted_at.is_some() {
        query = query.with_trashed();
    }
    query.r#where(&query.qualifyColumn(field), "=", value);
    query.first().await
}
