use serde_json::{Map, Value};

use crate::{
    database::eloquent::Model,
    http::{HttpError, Request, Response, StatusCode},
};

use super::{ResourceCollection, ResourceItems, ResourceResponse, ResourceResult};

/// Transforms an application resource for the current request.
#[allow(non_snake_case, reason = "Matches Laravel's JsonResource API")]
pub trait JsonResource {
    fn toArray(&self, request: &Request) -> ResourceResult<Value>;

    fn make<T>(resource: T) -> Self
    where
        Self: Sized + From<T>,
    {
        Self::from(resource)
    }

    fn collection<T>(source: impl Into<ResourceItems<T>>) -> ResourceCollection<Self>
    where
        Self: Sized + From<T>,
    {
        ResourceCollection::new(source.into().map(Self::make))
    }

    /// The outermost response key. The standard data key uses the application's wrapping policy.
    fn wrap(&self) -> Option<&str> {
        Some("data")
    }

    /// Wraps even when the resource already contains the selected wrapper key.
    fn forceWrapping(&self) -> bool {
        false
    }

    fn with(&self, _request: &Request) -> ResourceResult<Map<String, Value>> {
        Ok(Map::new())
    }

    fn status(&self) -> StatusCode {
        StatusCode::OK
    }

    /// Declares key preservation when this resource is used in a collection.
    fn preservesKeys() -> bool
    where
        Self: Sized,
    {
        false
    }

    fn withResponse(&self, _request: &Request, _response: &mut Response) -> ResourceResult<()> {
        Ok(())
    }

    fn response(&self) -> ResourceResponse<'_, Self> {
        ResourceResponse::new(self)
    }

    fn toResponse(&self, request: &Request) -> ResourceResult<Response> {
        self.response().toResponse(request)
    }
}

/// Uses the Eloquent model's existing serialization and creation state.
pub struct ModelResource<M>(pub M);

impl<M: Model> From<M> for ModelResource<M> {
    fn from(model: M) -> Self {
        Self(model)
    }
}

impl<M: Model> JsonResource for ModelResource<M> {
    fn toArray(&self, _: &Request) -> ResourceResult<Value> {
        self.0
            .toArray()
            .map(Value::Object)
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })
    }

    fn status(&self) -> StatusCode {
        if self.0.state().wasRecentlyCreated() {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        }
    }
}
