use serde_json::{Map, Value};

use crate::{
    database::eloquent::Model,
    http::{HttpFuture, Request, Response, StatusCode},
};

use super::{JsonOptions, JsonResource, ResourceCollection, ResourceItems, ResourceResult};

/// Declares the default resource associated with an Eloquent model.
pub trait UseResource: Model {
    type Resource: JsonResource + From<Self> + Send;

    /// Builds the default collection response from its original source.
    fn resource_collection(source: ResourceItems<Self>) -> impl JsonResource + Send {
        Self::Resource::collection(source)
    }
}

/// Transforms model collections and paginator results into JSON resources.
pub trait TransformsToResourceCollection<M: Model>: Sized + Into<ResourceItems<M>> {
    fn to_resource_collection(self) -> impl JsonResource + Send
    where
        M: UseResource,
    {
        let source = self.into();
        if source.items().is_empty() {
            ResolvedCollection::Empty(Box::new(M::Resource::collection(source)))
        } else {
            ResolvedCollection::Declared(M::resource_collection(source))
        }
    }

    fn to_resource_collection_with<R: JsonResource + From<M>>(self) -> ResourceCollection<R> {
        R::collection(self)
    }

    fn to_resource_collection_using<R: JsonResource>(
        self,
        factory: impl FnOnce(ResourceItems<M>) -> R,
    ) -> R {
        factory(self.into())
    }
}

impl<M: Model, S: Into<ResourceItems<M>>> TransformsToResourceCollection<M> for S {}

enum ResolvedCollection<C, R> {
    Declared(C),
    Empty(Box<ResourceCollection<R>>),
}

impl<C: JsonResource, R: JsonResource> JsonResource for ResolvedCollection<C, R> {
    fn prepare_response(
        &mut self,
        request: &Request,
    ) -> Option<HttpFuture<'_, ResourceResult<()>>> {
        match self {
            Self::Declared(resource) => resource.prepare_response(request),
            Self::Empty(resource) => resource.prepare_response(request),
        }
    }

    fn resolve_response_data(
        &self,
        request: &Request,
    ) -> ResourceResult<(Value, Map<String, Value>)> {
        match self {
            Self::Declared(resource) => resource.resolve_response_data(request),
            Self::Empty(resource) => resource.resolve_response_data(request),
        }
    }

    fn is_json_api(&self) -> bool {
        match self {
            Self::Declared(resource) => resource.is_json_api(),
            Self::Empty(resource) => resource.is_json_api(),
        }
    }

    fn json_options(&self) -> JsonOptions {
        match self {
            Self::Declared(resource) => resource.json_options(),
            Self::Empty(resource) => resource.json_options(),
        }
    }

    fn to_array(&self, request: &Request) -> ResourceResult<Value> {
        match self {
            Self::Declared(resource) => resource.to_array(request),
            Self::Empty(resource) => resource.to_array(request),
        }
    }

    fn wrap(&self) -> Option<&str> {
        match self {
            Self::Declared(resource) => resource.wrap(),
            Self::Empty(resource) => resource.wrap(),
        }
    }

    fn force_wrapping(&self) -> bool {
        match self {
            Self::Declared(resource) => resource.force_wrapping(),
            Self::Empty(resource) => resource.force_wrapping(),
        }
    }

    fn with(&self, request: &Request) -> ResourceResult<Map<String, Value>> {
        match self {
            Self::Declared(resource) => resource.with(request),
            Self::Empty(resource) => resource.with(request),
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::Declared(resource) => resource.status(),
            Self::Empty(resource) => resource.status(),
        }
    }

    fn with_response(&self, request: &Request, response: &mut Response) -> ResourceResult<()> {
        match self {
            Self::Declared(resource) => resource.with_response(request, response),
            Self::Empty(resource) => resource.with_response(request, response),
        }
    }

    fn to_response(&self, request: &Request) -> ResourceResult<Response> {
        match self {
            Self::Declared(resource) => resource.to_response(request),
            Self::Empty(resource) => resource.to_response(request),
        }
    }
}
