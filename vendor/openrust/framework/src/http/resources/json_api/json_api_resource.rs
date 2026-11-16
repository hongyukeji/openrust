use std::any::type_name;

use serde_json::{Map, Value};

use crate::{
    database::{
        Value as AttributeValue,
        eloquent::{Collection, Model},
    },
    http::{HttpError, HttpFuture, Request, StatusCode},
    support::Str,
};

use super::super::json::{JsonResource, ResourceResult};
use super::{JsonApi, JsonApiAttributes, JsonApiRelationships, add_included};

/// Projects a model into a JSON:API resource object.
#[allow(
    non_snake_case,
    reason = "Matches Laravel's JSON:API resource contract"
)]
pub trait JsonApiResource: Sized + Send {
    type Model: Model;

    fn model(&self) -> &Self::Model;

    /// Provides mutable access for automatic loading of registered include paths.
    fn modelMut(&mut self) -> Option<&mut Self::Model> {
        None
    }

    /// Maps an allowed request path to a registered Eloquent relationship path.
    fn includePath(_path: &str) -> Option<&'static str> {
        None
    }

    /// Accepts sparse fieldsets and include paths from the query string.
    fn respectFieldsAndIncludesInQueryString(&self) -> bool {
        true
    }

    /// Selects declared, already loaded relationships in place of query includes.
    fn includePreviouslyLoadedRelationships(&self) -> bool {
        false
    }

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

    fn prepareResponse(&mut self, request: &Request) -> Option<HttpFuture<'_, ResourceResult<()>>> {
        Self::prepareCollection(std::slice::from_mut(self), request)
    }

    fn prepareCollection<'a>(
        resources: &'a mut [Self],
        request: &Request,
    ) -> Option<HttpFuture<'a, ResourceResult<()>>> {
        if !resources.iter().any(queries_includes) {
            return None;
        }
        let paths = match JsonApi::depth().and_then(|depth| includes(request, depth)) {
            Ok(paths) => paths,
            Err(error) => return Some(Box::pin(async move { Err(error) })),
        };
        let paths: Vec<_> = paths
            .iter()
            .filter_map(|path| R::includePath(path))
            .collect();
        if paths.is_empty() {
            return None;
        }
        Some(Box::pin(async move {
            let originals = resources
                .iter_mut()
                .filter(|resource| queries_includes(&**resource))
                .map(|resource| {
                    resource.modelMut().ok_or_else(|| HttpError::Internal {
                        source: std::io::Error::other("JSON:API include loading requires modelMut")
                            .into(),
                    })
                })
                .collect::<ResourceResult<Vec<_>>>()?;
            let mut models = Collection::new(
                originals
                    .iter()
                    .map(|model| R::Model::from_state(model.state().clone()))
                    .collect(),
            );
            models
                .loadMissingPaths(&paths)
                .await
                .map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?;
            for (original, mut loaded) in originals.into_iter().zip(models) {
                *original.state_mut() = std::mem::take(loaded.state_mut());
            }
            Ok(())
        }))
    }

    fn toArray(&self, request: &Request) -> ResourceResult<Value> {
        project_root(self, request).map(|(object, _)| object)
    }

    fn resolveResponseData(
        &self,
        request: &Request,
    ) -> ResourceResult<(Value, Map<String, Value>)> {
        let (object, included) = project_root(self, request)?;
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

fn queries_includes(resource: &impl JsonApiResource) -> bool {
    resource.respectFieldsAndIncludesInQueryString()
        && !resource.includePreviouslyLoadedRelationships()
}

fn project_root(
    resource: &impl JsonApiResource,
    request: &Request,
) -> ResourceResult<(Value, Vec<Value>)> {
    let paths = if queries_includes(resource) {
        includes(request, JsonApi::depth()?)?
    } else {
        Vec::new()
    };
    let paths = paths.iter().map(String::as_str).collect::<Vec<_>>();
    project(
        resource,
        request,
        &paths,
        resource.includePreviouslyLoadedRelationships(),
    )
}

pub(super) fn project<R: JsonApiResource>(
    resource: &R,
    request: &Request,
    paths: &[&str],
    include_loaded: bool,
) -> ResourceResult<(Value, Vec<Value>)> {
    let resource_type = resource.resolveResourceType(request)?;
    let id = resource.resolveResourceIdentifier(request)?;
    let mut attributes = resource.toAttributes(request)?;
    for reserved in ["id", "type"] {
        attributes.shift_remove(reserved);
    }
    let respects_query = resource.respectFieldsAndIncludesInQueryString();
    let paths = if respects_query { paths } else { &[] };
    let fields = if respects_query {
        request.query(Some("fields"))?
    } else {
        None
    };
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
    if include_loaded || !paths.is_empty() {
        for (name, resolve) in resource.toRelationships(request).0 {
            if matches!(name, "id" | "type") {
                continue;
            }
            let nested: Vec<_> = paths
                .iter()
                .filter_map(|path| path.strip_prefix(name)?.strip_prefix('.'))
                .collect();
            let already_loaded = include_loaded
                && resource
                    .model()
                    .relationLoaded(R::includePath(name).unwrap_or(name));
            if !already_loaded && nested.is_empty() && !paths.contains(&name) {
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

fn includes(request: &Request, depth: usize) -> ResourceResult<Vec<String>> {
    let Some(value) = request.query(Some("include"))? else {
        return Ok(Vec::new());
    };
    let value = value.as_str().ok_or(HttpError::InvalidRequest)?;
    let mut paths = Vec::new();
    for path in value.split(',').filter(|path| !path.is_empty()) {
        if path.split('.').any(str::is_empty) {
            return Err(HttpError::InvalidRequest);
        }
        let path = path
            .match_indices('.')
            .nth(depth.saturating_sub(1))
            .map_or(path, |(end, _)| &path[..end]);
        paths.push(path.into());
    }
    Ok(paths)
}
