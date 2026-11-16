use std::any::type_name;

use serde_json::{Map, Value};

use crate::{
    database::{Value as AttributeValue, eloquent::Model},
    http::{HttpError, Request, StatusCode},
    support::Str,
};

use super::super::json::{JsonResource, ResourceResult};
use super::{JsonApiAttributes, JsonApiRelationships, add_included};

/// Projects a model into a JSON:API resource object.
#[allow(
    non_snake_case,
    reason = "Matches Laravel's JSON:API resource contract"
)]
pub trait JsonApiResource: Sized {
    type Model: Model;

    fn model(&self) -> &Self::Model;

    fn toId(&self, _request: &Request) -> Option<String> {
        None
    }

    fn toType(&self, _request: &Request) -> Option<String> {
        None
    }

    fn toAttributes(&self, _request: &Request) -> ResourceResult<Map<String, Value>> {
        let mut attributes =
            self.model()
                .attributesToArray()
                .map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?;
        attributes.shift_remove(self.model().getKeyName());
        Ok(attributes)
    }

    fn toLinks(&self, _request: &Request) -> ResourceResult<Map<String, Value>> {
        Ok(Map::new())
    }

    fn lazyAttributes<'a>(&'a self, _request: &'a Request) -> JsonApiAttributes<'a> {
        JsonApiAttributes::default()
    }

    fn toRelationships<'a>(&'a self, _request: &'a Request) -> JsonApiRelationships<'a> {
        JsonApiRelationships::default()
    }

    fn toMeta(&self, _request: &Request) -> ResourceResult<Map<String, Value>> {
        Ok(Map::new())
    }

    fn resolveResourceIdentifier(&self, request: &Request) -> ResourceResult<String> {
        self.toId(request)
            .or_else(|| match self.model().getKey()? {
                AttributeValue::Text(value) => Some(value.clone()),
                AttributeValue::Integer(value) => Some(value.to_string()),
                AttributeValue::Unsigned(value) => Some(value.to_string()),
                _ => None,
            })
            .filter(|id| !id.is_empty())
            .ok_or_else(|| HttpError::Internal {
                source: std::io::Error::other("JSON:API resource identifier is missing or invalid")
                    .into(),
            })
    }

    fn resolveResourceType(&self, request: &Request) -> ResourceResult<String> {
        let resource_type = self.toType(request).unwrap_or_else(|| {
            let name = type_name::<Self>();
            let name = name.split('<').next().unwrap_or(name);
            let name = name.rsplit("::").next().unwrap_or(name);
            let singular = Str::snake(name.strip_suffix("Resource").unwrap_or(name), "_");
            pluralizer::pluralize(&singular, 2, false)
        });
        if resource_type.is_empty() {
            return Err(HttpError::Internal {
                source: std::io::Error::other("JSON:API resource type is missing").into(),
            });
        }
        Ok(resource_type)
    }
}

impl<R: JsonApiResource> JsonResource for R {
    const JSON_API: bool = true;

    fn toArray(&self, request: &Request) -> ResourceResult<Value> {
        let included = includes(request)?;
        project(
            self,
            request,
            &included.iter().map(String::as_str).collect::<Vec<_>>(),
        )
        .map(|(object, _)| object)
    }

    fn resolveResponseData(
        &self,
        request: &Request,
    ) -> ResourceResult<(Value, Map<String, Value>)> {
        let paths = includes(request)?;
        let paths = paths.iter().map(String::as_str).collect::<Vec<_>>();
        let (object, included) = project(self, request, &paths)?;
        let mut fields = Map::new();
        add_included(&mut fields, &object, included);
        Ok((object, fields))
    }

    fn status(&self) -> StatusCode {
        if self.model().state().wasRecentlyCreated() {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        }
    }
}

pub(super) fn project(
    resource: &impl JsonApiResource,
    request: &Request,
    paths: &[&str],
) -> ResourceResult<(Value, Vec<Value>)> {
    let resource_type = resource.resolveResourceType(request)?;
    let id = resource.resolveResourceIdentifier(request)?;
    let mut attributes = resource.toAttributes(request)?;
    for reserved in ["id", "type"] {
        attributes.shift_remove(reserved);
    }
    let fields = request.query(Some("fields"))?;
    let selected = fields
        .as_ref()
        .and_then(|fields| fields.get(&resource_type))
        .map(|fields| fields.as_str().ok_or(HttpError::InvalidRequest))
        .transpose()?;
    let selected =
        |name: &str| selected.is_none_or(|fields| fields.split(',').any(|field| field == name));
    attributes.retain(|name, _| selected(name));
    for (name, resolve) in resource.lazyAttributes(request).0 {
        if !matches!(name, "id" | "type") && selected(name) {
            attributes.insert(name.into(), resolve()?);
        }
    }

    let mut relationships = Map::new();
    let mut included = Vec::new();
    if !paths.is_empty() {
        for (name, resolve) in resource.toRelationships(request).0 {
            if matches!(name, "id" | "type") {
                continue;
            }
            let nested: Vec<_> = paths
                .iter()
                .filter_map(|path| path.strip_prefix(name)?.strip_prefix('.'))
                .collect();
            if nested.is_empty() && !paths.contains(&name) {
                continue;
            }
            let data = resolve(request, &nested, &mut included)?;
            if selected(name) {
                attributes.shift_remove(name);
                relationships.insert(
                    name.into(),
                    Value::Object(Map::from_iter([("data".into(), data)])),
                );
            }
        }
    }
    let mut object = Map::from_iter([
        ("id".into(), Value::String(id)),
        ("type".into(), Value::String(resource_type)),
    ]);
    for (name, fields) in [
        ("attributes", attributes),
        ("relationships", relationships),
        ("links", resource.toLinks(request)?),
        ("meta", resource.toMeta(request)?),
    ] {
        if !fields.is_empty() {
            object.insert(name.into(), Value::Object(fields));
        }
    }
    Ok((Value::Object(object), included))
}

fn includes(request: &Request) -> ResourceResult<Vec<String>> {
    let Some(value) = request.query(Some("include"))? else {
        return Ok(Vec::new());
    };
    let value = value.as_str().ok_or(HttpError::InvalidRequest)?;
    let mut paths = Vec::new();
    for path in value.split(',').filter(|path| !path.is_empty()) {
        if path.split('.').any(str::is_empty) || path.split('.').count() > 5 {
            return Err(HttpError::InvalidRequest);
        }
        paths.push(path.into());
    }
    Ok(paths)
}
