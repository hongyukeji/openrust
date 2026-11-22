use std::sync::Arc;

use serde_json::{Map, Value};

use crate::{
    contracts::{container::Container as _, support::Responsable},
    http::{HttpError, Request, Response, StatusCode},
    support::facades::current_container,
};

use super::super::json_api::{JsonApi, MEDIA_TYPE};
use super::{JsonResource, ResourceResult};

/// Configures the default resource wrapper for the application that owns the request.
pub struct ResourceWrapping(Option<String>);

#[allow(non_snake_case, reason = "Matches Laravel's resource wrapping API")]
impl ResourceWrapping {
    pub fn wrap(name: impl Into<String>) -> Self {
        Self(Some(name.into()))
    }

    pub const fn withoutWrapping() -> Self {
        Self(None)
    }

    fn current() -> ResourceResult<Option<Arc<Self>>> {
        let Some(container) = current_container() else {
            return Ok(None);
        };
        if !container.bound::<Self>() {
            return Ok(None);
        }
        container.make::<Self>().map(Some).map_err(Into::into)
    }
}

/// Builds the outer response for a resource, with request-local metadata.
pub struct ResourceResponse<'a, R: ?Sized> {
    resource: &'a R,
    wrapper: Option<Option<String>>,
    additional: Map<String, Value>,
}

#[allow(non_snake_case, reason = "Matches Laravel's resource response API")]
impl<'a, R: JsonResource + ?Sized> ResourceResponse<'a, R> {
    pub fn new(resource: &'a R) -> Self {
        Self {
            resource,
            wrapper: None,
            additional: Map::new(),
        }
    }

    #[must_use]
    pub fn wrap(mut self, name: impl Into<String>) -> Self {
        self.wrapper = Some(Some(name.into()));
        self
    }

    #[must_use]
    pub fn withoutWrapping(mut self) -> Self {
        self.wrapper = Some(None);
        self
    }

    #[must_use]
    pub fn additional(mut self, fields: Map<String, Value>) -> Self {
        merge_fields(&mut self.additional, fields);
        self
    }

    pub fn toResponse(self, request: &Request) -> ResourceResult<Response> {
        if self.resource.isJsonApi() {
            if self
                .wrapper
                .as_ref()
                .is_some_and(|wrapper| wrapper.as_deref() != Some("data"))
                || ["data", "errors"]
                    .iter()
                    .any(|key| self.additional.contains_key(*key))
            {
                return Err(HttpError::Internal {
                    source: std::io::Error::other("JSON:API requires a data document").into(),
                });
            }
            return build(self.resource, request, Some("data"), self.additional);
        }
        let wrapper = self
            .wrapper
            .as_ref()
            .map_or_else(|| self.resource.wrap(), |value| value.as_deref());
        let application = if self.wrapper.is_none() && wrapper == Some("data") {
            ResourceWrapping::current()?
        } else {
            None
        };
        let wrapper = application
            .as_ref()
            .map_or(wrapper, |settings| settings.0.as_deref());
        build(self.resource, request, wrapper, self.additional)
    }
}

impl<R: JsonResource + Sync + ?Sized> Responsable for ResourceResponse<'_, R> {
    async fn toResponse(self, request: &mut Request) -> ResourceResult<Response> {
        ResourceResponse::toResponse(self, request)
    }
}

pub(super) fn build<R: JsonResource + ?Sized>(
    resource: &R,
    request: &Request,
    wrapper: Option<&str>,
    additional: Map<String, Value>,
) -> ResourceResult<Response> {
    let (data, mut with) = resource.resolveResponseData(request)?;
    if resource.isJsonApi()
        && let Some(information) = JsonApi::information()?
    {
        with.insert("jsonapi".into(), information);
    }
    let payload = wrap(data, wrapper, resource.forceWrapping(), with, additional);
    let status = resource.status();
    let mut response = if status == StatusCode::RESET_CONTENT {
        Response::empty(status)
    } else {
        Response::json_with(status, || {
            resource
                .jsonOptions()
                .encode(&payload)
                .map(String::into_bytes)
        })?
    };
    resource.withResponse(request, &mut response)?;
    if resource.isJsonApi() {
        response = response.header(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static(MEDIA_TYPE),
        );
    }
    Ok(response)
}

fn wrap(
    data: Value,
    wrapper: Option<&str>,
    force: bool,
    with: Map<String, Value>,
    additional: Map<String, Value>,
) -> Value {
    let has_metadata = !with.is_empty() || !additional.is_empty();
    let Some(key) = wrapper.or(has_metadata.then_some("data")) else {
        return data;
    };
    let mut payload = match data {
        Value::Object(fields) if wrapper.is_some() && !force && fields.contains_key(key) => fields,
        data => Map::from_iter([(key.to_owned(), data)]),
    };
    merge_fields(&mut payload, with);
    merge_fields(&mut payload, additional);
    Value::Object(payload)
}

pub(super) fn merge_fields(target: &mut Map<String, Value>, incoming: Map<String, Value>) {
    for (key, value) in incoming {
        match target.entry(key) {
            serde_json::map::Entry::Vacant(entry) => {
                entry.insert(value);
            }
            serde_json::map::Entry::Occupied(mut entry) => merge(entry.get_mut(), value),
        }
    }
}

fn merge(target: &mut Value, incoming: Value) {
    match (target, incoming) {
        (Value::Object(target), Value::Object(incoming)) => merge_fields(target, incoming),
        (Value::Array(target), Value::Array(incoming)) => target.extend(incoming),
        (Value::Array(target), incoming) => target.push(incoming),
        (target, Value::Array(incoming)) => {
            let mut merged = Vec::with_capacity(incoming.len() + 1);
            merged.push(std::mem::take(target));
            merged.extend(incoming);
            *target = Value::Array(merged);
        }
        (target, incoming) => *target = Value::Array(vec![std::mem::take(target), incoming]),
    }
}
