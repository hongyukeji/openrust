use serde_json::{Map, Value};

use crate::{
    database::eloquent::Model,
    http::{HttpError, Request, Response, StatusCode},
};

use super::{ResourceResponse, ResourceResult};

/// Transforms an application resource for the current request.
#[allow(non_snake_case, reason = "Matches Laravel's JsonResource API")]
pub trait JsonResource {
    fn toArray(&self, request: &Request) -> ResourceResult<Value>;

    /// The outermost response key. Nested resources resolve directly.
    fn wrap(&self) -> Option<&str> {
        Some("data")
    }

    fn with(&self, _request: &Request) -> ResourceResult<Map<String, Value>> {
        Ok(Map::new())
    }

    fn status(&self) -> StatusCode {
        StatusCode::OK
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
