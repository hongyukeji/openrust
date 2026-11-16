use serde_json::{Map, Value};

use crate::{
    database::eloquent::Model,
    http::{Request, Response, StatusCode},
};

use super::{JsonOptions, JsonResource, ResourceCollection, ResourceItems, ResourceResult};

/// Declares the default resource associated with an Eloquent model.
#[allow(non_snake_case, reason = "Matches Laravel's resource API")]
pub trait UseResource: Model {
    type Resource: JsonResource + From<Self> + Send;

    /// Builds the default collection response from its original source.
    fn resourceCollection(source: ResourceItems<Self>) -> impl JsonResource + Send {
        Self::Resource::collection(source)
    }
}

/// Transforms model collections and paginator results into JSON resources.
#[allow(non_snake_case, reason = "Matches Laravel's resource API")]
pub trait TransformsToResourceCollection<M: Model>: Sized + Into<ResourceItems<M>> {
    fn toResourceCollection(self) -> impl JsonResource + Send
    where
        M: UseResource,
    {
        let source = self.into();
        if source.items().is_empty() {
            ResolvedCollection::Empty(Box::new(M::Resource::collection(source)))
        } else {
            ResolvedCollection::Declared(M::resourceCollection(source))
        }
    }

    fn toResourceCollectionWith<R: JsonResource + From<M>>(self) -> ResourceCollection<R> {
        R::collection(self)
    }

    fn toResourceCollectionUsing<R: JsonResource>(
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
    fn jsonOptions(&self) -> JsonOptions {
        match self {
            Self::Declared(resource) => resource.jsonOptions(),
            Self::Empty(resource) => resource.jsonOptions(),
        }
    }

    fn toArray(&self, request: &Request) -> ResourceResult<Value> {
        match self {
            Self::Declared(resource) => resource.toArray(request),
            Self::Empty(resource) => resource.toArray(request),
        }
    }

    fn wrap(&self) -> Option<&str> {
        match self {
            Self::Declared(resource) => resource.wrap(),
            Self::Empty(resource) => resource.wrap(),
        }
    }

    fn forceWrapping(&self) -> bool {
        match self {
            Self::Declared(resource) => resource.forceWrapping(),
            Self::Empty(resource) => resource.forceWrapping(),
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

    fn withResponse(&self, request: &Request, response: &mut Response) -> ResourceResult<()> {
        match self {
            Self::Declared(resource) => resource.withResponse(request, response),
            Self::Empty(resource) => resource.withResponse(request, response),
        }
    }

    fn toResponse(&self, request: &Request) -> ResourceResult<Response> {
        match self {
            Self::Declared(resource) => resource.toResponse(request),
            Self::Empty(resource) => resource.toResponse(request),
        }
    }
}
