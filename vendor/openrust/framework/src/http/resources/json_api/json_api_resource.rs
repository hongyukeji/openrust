use std::any::type_name;

use serde_json::{Map, Value};

use crate::{
    database::{Value as AttributeValue, eloquent::Model},
    http::{HttpError, Request, StatusCode},
    support::Str,
};

use super::super::json::{JsonResource, ResourceResult};

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
        let resource_type = self.resolveResourceType(request)?;
        let id = self.resolveResourceIdentifier(request)?;
        let mut attributes = self.toAttributes(request)?;
        for reserved in ["id", "type"] {
            attributes.shift_remove(reserved);
        }
        if let Some(fields) = request.query(Some("fields"))?
            && let Some(selected) = fields.get(&resource_type)
        {
            let selected = selected.as_str().ok_or(HttpError::InvalidRequest)?;
            attributes.retain(|name, _| selected.split(',').any(|field| field == name));
        }

        let mut object = Map::from_iter([
            ("id".into(), Value::String(id)),
            ("type".into(), Value::String(resource_type)),
        ]);
        for (name, fields) in [
            ("attributes", attributes),
            ("links", self.toLinks(request)?),
            ("meta", self.toMeta(request)?),
        ] {
            if !fields.is_empty() {
                object.insert(name.into(), Value::Object(fields));
            }
        }
        Ok(Value::Object(object))
    }

    fn status(&self) -> StatusCode {
        if self.model().state().wasRecentlyCreated() {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        }
    }
}
