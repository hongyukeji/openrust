use serde_json::{Map, Value};

use crate::http::{Request, Response, StatusCode};

use super::{JsonResource, ResourceResult};

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
        let wrapper = self
            .wrapper
            .as_ref()
            .map_or_else(|| self.resource.wrap(), |value| value.as_deref());
        build(self.resource, request, wrapper, self.additional)
    }
}

pub(super) fn build<R: JsonResource + ?Sized>(
    resource: &R,
    request: &Request,
    wrapper: Option<&str>,
    additional: Map<String, Value>,
) -> ResourceResult<Response> {
    let data = resource.toArray(request)?;
    let with = resource.with(request)?;
    let payload = wrap(data, wrapper, with, additional);
    let status = resource.status();
    let mut response = if status == StatusCode::RESET_CONTENT {
        Response::empty(status)
    } else {
        Response::json(status, payload)?
    };
    resource.withResponse(request, &mut response)?;
    Ok(response)
}

fn wrap(
    data: Value,
    wrapper: Option<&str>,
    with: Map<String, Value>,
    additional: Map<String, Value>,
) -> Value {
    let has_metadata = !with.is_empty() || !additional.is_empty();
    let Some(key) = wrapper.or(has_metadata.then_some("data")) else {
        return data;
    };
    let mut payload = match data {
        Value::Object(fields) if fields.contains_key(key) => fields,
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
