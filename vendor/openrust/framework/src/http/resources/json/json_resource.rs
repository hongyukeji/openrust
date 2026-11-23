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
pub trait JsonResource {
    #[doc(hidden)]
    const JSON_API: bool = false;

    #[doc(hidden)]
    fn is_json_api(&self) -> bool {
        Self::JSON_API
    }

    fn to_array(&self, request: &Request) -> ResourceResult<Value>;

    #[doc(hidden)]
    fn prepare_response(
        &mut self,
        _request: &Request,
    ) -> Option<HttpFuture<'_, ResourceResult<()>>> {
        None
    }

    #[doc(hidden)]
    fn prepare_collection<'a>(
        _items: &'a mut [Self],
        _request: &Request,
    ) -> Option<HttpFuture<'a, ResourceResult<()>>>
    where
        Self: Sized,
    {
        None
    }

    /// Prepares resources that need I/O and then builds the response.
    fn to_response_async<'a>(
        &'a mut self,
        request: &'a mut Request,
    ) -> impl Future<Output = ResourceResult<Response>> + Send + 'a
    where
        Self: Send,
    {
        async move {
            if let Some(prepare) = self.prepare_response(request) {
                prepare.await?;
            }
            self.to_response(request)
        }
    }

    #[doc(hidden)]
    fn resolve_response_data(
        &self,
        request: &Request,
    ) -> ResourceResult<(Value, Map<String, Value>)> {
        Ok((self.to_array(request)?, self.with(request)?))
    }

    fn to_json(&self, request: &Request) -> ResourceResult<String> {
        self.json_options().encode(&self.to_array(request)?)
    }

    fn to_pretty_json(&self, request: &Request) -> ResourceResult<String> {
        self.json_options()
            .pretty_print()
            .encode(&self.to_array(request)?)
    }

    fn json_options(&self) -> JsonOptions {
        Self::default_json_options()
    }

    /// Declares options that also apply to a collection without items.
    fn default_json_options() -> JsonOptions {
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
    fn force_wrapping(&self) -> bool {
        false
    }

    fn with(&self, _request: &Request) -> ResourceResult<Map<String, Value>> {
        Ok(Map::new())
    }

    fn status(&self) -> StatusCode {
        StatusCode::OK
    }

    /// Declares key preservation when this resource is used in a collection.
    fn preserves_keys() -> bool
    where
        Self: Sized,
    {
        false
    }

    fn with_response(&self, _request: &Request, _response: &mut Response) -> ResourceResult<()> {
        Ok(())
    }

    fn response(&self) -> ResourceResponse<'_, Self> {
        ResourceResponse::new(self)
    }

    /// Moves the resource into the request-aware response lifecycle.
    fn into_responsable(self) -> ResourceResponder<Self>
    where
        Self: Sized + Send,
    {
        ResourceResponder::new(self)
    }

    fn to_response(&self, request: &Request) -> ResourceResult<Response> {
        self.response().to_response(request)
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
    fn to_array(&self, _: &Request) -> ResourceResult<Value> {
        self.0.to_array().map(Value::Object).map_err(Into::into)
    }

    fn status(&self) -> StatusCode {
        if self.0.state().wasRecentlyCreated() {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        }
    }
}
