use std::any::type_name;

use serde_json::{Map, Value};

use crate::{
    http::{HttpError, HttpFuture, Request, StatusCode},
    support::Str,
};

use super::super::json::{JsonResource, ResourceResult};
use super::{JsonApi, JsonApiAttributes, JsonApiRelationships, JsonApiSource, add_included};

/// Projects a data source into a JSON:API resource object.
pub trait JsonApiResource: Sized + Send {
    /// Eloquent models and JSON object maps are supported data sources.
    type Model: JsonApiSource;

    fn model(&self) -> &Self::Model;

    /// Provides mutable access for automatic loading of registered include paths.
    fn model_mut(&mut self) -> Option<&mut Self::Model> {
        None
    }

    /// Maps an allowed request path to a registered Eloquent relationship path.
    fn include_path(_path: &str) -> Option<&'static str> {
        None
    }

    /// Accepts sparse fieldsets and include paths from the query string.
    fn respect_fields_and_includes_in_query_string(&self) -> bool {
        true
    }

    /// Selects declared, already loaded relationships in place of query includes.
    fn include_previously_loaded_relationships(&self) -> bool {
        false
    }

    fn to_id(&self, _request: &Request) -> Option<String> {
        None
    }

    fn to_type(&self, _request: &Request) -> Option<String> {
        None
    }

    fn to_attributes(&self, _request: &Request) -> ResourceResult<Map<String, Value>> {
        self.model().attributes()
    }

    fn to_links(&self, _request: &Request) -> ResourceResult<Map<String, Value>> {
        Ok(Map::new())
    }

    fn lazy_attributes<'a>(&'a self, _request: &'a Request) -> JsonApiAttributes<'a> {
        JsonApiAttributes::default()
    }

    fn to_relationships<'a>(&'a self, _request: &'a Request) -> JsonApiRelationships<'a> {
        JsonApiRelationships::default()
    }

    fn to_meta(&self, _request: &Request) -> ResourceResult<Map<String, Value>> {
        Ok(Map::new())
    }

    fn resolve_resource_identifier(&self, request: &Request) -> ResourceResult<String> {
        self.to_id(request)
            .or_else(|| self.model().identifier())
            .filter(|id| !id.is_empty())
            .ok_or_else(|| HttpError::Internal {
                source: std::io::Error::other("JSON:API resource identifier is missing or invalid")
                    .into(),
            })
    }

    fn resolve_resource_type(&self, request: &Request) -> ResourceResult<String> {
        let resource_type = self.to_type(request).unwrap_or_else(|| {
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

    fn prepare_response(
        &mut self,
        request: &Request,
    ) -> Option<HttpFuture<'_, ResourceResult<()>>> {
        Self::prepare_collection(std::slice::from_mut(self), request)
    }

    fn prepare_collection<'a>(
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
            .filter_map(|path| R::include_path(path))
            .collect();
        if paths.is_empty() {
            return None;
        }
        Some(Box::pin(async move {
            let originals = resources
                .iter_mut()
                .filter(|resource| queries_includes(&**resource))
                .map(|resource| {
                    resource.model_mut().ok_or_else(|| HttpError::Internal {
                        source: std::io::Error::other(
                            "JSON:API include loading requires model_mut",
                        )
                        .into(),
                    })
                })
                .collect::<ResourceResult<Vec<_>>>()?;
            R::Model::load_missing_paths(originals, paths).await
        }))
    }

    fn to_array(&self, request: &Request) -> ResourceResult<Value> {
        project_root(self, request).map(|(object, _)| object)
    }

    fn resolve_response_data(
        &self,
        request: &Request,
    ) -> ResourceResult<(Value, Map<String, Value>)> {
        let (object, included) = project_root(self, request)?;
        let mut fields = Map::new();
        add_included(&mut fields, &object, included);
        Ok((object, fields))
    }

    fn status(&self) -> StatusCode {
        self.model().status()
    }
}

fn queries_includes(resource: &impl JsonApiResource) -> bool {
    resource.respect_fields_and_includes_in_query_string()
        && !resource.include_previously_loaded_relationships()
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
        resource.include_previously_loaded_relationships(),
    )
}

pub(super) fn project<R: JsonApiResource>(
    resource: &R,
    request: &Request,
    paths: &[&str],
    include_loaded: bool,
) -> ResourceResult<(Value, Vec<Value>)> {
    let resource_type = resource.resolve_resource_type(request)?;
    let id = resource.resolve_resource_identifier(request)?;
    let mut attributes = resource.to_attributes(request)?;
    for reserved in ["id", "type"] {
        attributes.shift_remove(reserved);
    }
    let respects_query = resource.respect_fields_and_includes_in_query_string();
    let paths = if respects_query { paths } else { &[] };
    let fields = if respects_query {
        request.query(Some("fields"))?
    } else {
        None
    };
    let selected = fields
        .as_ref()
        .and_then(|fields| fields.get(&resource_type))
        .map(|fields| fields.as_str().unwrap_or_default());
    let selected =
        |name: &str| selected.is_none_or(|fields| fields.split(',').any(|field| field == name));
    attributes.retain(|name, _| selected(name));
    for (name, resolve) in resource.lazy_attributes(request).0 {
        if !matches!(name, "id" | "type") && selected(name) {
            attributes.insert(name.into(), resolve()?);
        }
    }

    let mut relationships = Map::new();
    let mut included = Vec::new();
    if include_loaded || !paths.is_empty() {
        for (name, resolve) in resource.to_relationships(request).0 {
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
                    .relation_loaded(R::include_path(name).unwrap_or(name));
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
        ("links", resource.to_links(request)?),
        ("meta", resource.to_meta(request)?),
    ] {
        if !fields.is_empty() {
            object.insert(name.into(), Value::Object(fields));
        }
    }
    Ok((Value::Object(object), included))
}

fn includes(request: &Request, depth: usize) -> ResourceResult<Vec<String>> {
    let value = request.query(Some("include"))?;
    let Some(value) = value.as_ref().and_then(Value::as_str) else {
        return Ok(Vec::new());
    };
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
