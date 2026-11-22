use serde_json::{Map, Value};

use crate::{
    database::eloquent::Model,
    http::{HttpFuture, Request, Response, StatusCode},
};

use super::{
    JsonOptions, ResourceCollection, ResourceItems, ResourceResponder, ResourceResponse,
    ResourceResult,
};

/// Transforms an application resource for the current request.
#[allow(non_snake_case, reason = "Matches Laravel's JsonResource API")]
pub trait JsonResource {
    #[doc(hidden)]
    const JSON_API: bool = false;

    #[doc(hidden)]
    fn isJsonApi(&self) -> bool {
        Self::JSON_API
    }

    fn toArray(&self, request: &Request) -> ResourceResult<Value>;

    #[doc(hidden)]
    fn prepareResponse(
        &mut self,
        _request: &Request,
    ) -> Option<HttpFuture<'_, ResourceResult<()>>> {
        None
    }

    #[doc(hidden)]
    fn prepareCollection<'a>(
        _items: &'a mut [Self],
        _request: &Request,
    ) -> Option<HttpFuture<'a, ResourceResult<()>>>
    where
        Self: Sized,
    {
        None
    }

    /// Prepares resources that need I/O and then builds the response.
    fn toResponseAsync<'a>(
        &'a mut self,
        request: &'a mut Request,
    ) -> impl Future<Output = ResourceResult<Response>> + Send + 'a
    where
        Self: Send,
    {
        async move {
            if let Some(prepare) = self.prepareResponse(request) {
                prepare.await?;
            }
            self.toResponse(request)
        }
    }

    #[doc(hidden)]
    fn resolveResponseData(
        &self,
        request: &Request,
    ) -> ResourceResult<(Value, Map<String, Value>)> {
        Ok((self.toArray(request)?, self.with(request)?))
    }

    fn toJson(&self, request: &Request) -> ResourceResult<String> {
        self.jsonOptions().encode(&self.toArray(request)?)
    }

    fn toPrettyJson(&self, request: &Request) -> ResourceResult<String> {
        self.jsonOptions()
            .prettyPrint()
            .encode(&self.toArray(request)?)
    }

    fn jsonOptions(&self) -> JsonOptions {
        Self::defaultJsonOptions()
    }

    /// Declares options that also apply to a collection without items.
    fn defaultJsonOptions() -> JsonOptions {
        JsonOptions::default()
    }

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

    /// Moves the resource into the request-aware response lifecycle.
    fn intoResponsable(self) -> ResourceResponder<Self>
    where
        Self: Sized + Send,
    {
        ResourceResponder::new(self)
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
        self.0.toArray().map(Value::Object).map_err(Into::into)
    }

    fn status(&self) -> StatusCode {
        if self.0.state().wasRecentlyCreated() {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        }
    }
}
