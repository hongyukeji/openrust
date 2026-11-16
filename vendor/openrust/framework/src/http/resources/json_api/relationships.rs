use indexmap::IndexMap;
use serde_json::{Map, Value};

use crate::http::Request;

use super::{super::json::ResourceResult, JsonApiResource, json_api_resource::project};

type Resolver<'a> =
    Box<dyn FnOnce(&Request, &[&str], &mut Vec<Value>) -> ResourceResult<Value> + 'a>;

/// Named relationship resolvers evaluated for requested include paths.
#[derive(Default)]
pub struct JsonApiRelationships<'a>(pub(super) IndexMap<&'a str, Resolver<'a>>);

#[allow(non_snake_case, reason = "Matches JSON:API relationship terminology")]
impl<'a> JsonApiRelationships<'a> {
    #[must_use]
    pub fn toOne<R: JsonApiResource>(
        mut self,
        name: &'a str,
        resolve: impl FnOnce() -> ResourceResult<Option<R>> + 'a,
    ) -> Self {
        self.0.insert(
            name,
            Box::new(move |request, paths, included| {
                resolve()?.map_or(Ok(Value::Null), |resource| {
                    include(&resource, request, paths, included)
                })
            }),
        );
        self
    }

    #[must_use]
    pub fn toMany<R: JsonApiResource>(
        mut self,
        name: &'a str,
        resolve: impl FnOnce() -> ResourceResult<Vec<R>> + 'a,
    ) -> Self {
        self.0.insert(
            name,
            Box::new(move |request, paths, included| {
                resolve()?
                    .iter()
                    .map(|resource| include(resource, request, paths, included))
                    .collect()
            }),
        );
        self
    }
}

fn include(
    resource: &impl JsonApiResource,
    request: &Request,
    paths: &[&str],
    included: &mut Vec<Value>,
) -> ResourceResult<Value> {
    let (object, descendants) = project(resource, request, paths)?;
    let identifier = Value::Object(Map::from_iter([
        ("id".into(), object["id"].clone()),
        ("type".into(), object["type"].clone()),
    ]));
    included.push(object);
    included.extend(descendants);
    Ok(identifier)
}

pub(in crate::http::resources) fn add_included(
    fields: &mut Map<String, Value>,
    primary: &Value,
    mut included: Vec<Value>,
) {
    use std::collections::HashSet;
    let mut identities = HashSet::new();
    for object in primary
        .as_array()
        .map_or(std::slice::from_ref(primary), Vec::as_slice)
    {
        identities.insert(identity(object));
    }
    included.retain(|object| identities.insert(identity(object)));
    if !included.is_empty() {
        fields.insert("included".into(), Value::Array(included));
    }
}

fn identity(object: &Value) -> (Option<String>, Option<String>) {
    (
        object["type"].as_str().map(str::to_owned),
        object["id"].as_str().map(str::to_owned),
    )
}
