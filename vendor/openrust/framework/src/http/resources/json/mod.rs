//! JSON resource transformation and response metadata.

mod conditional;
mod json_resource;
mod paginated_resource_response;
mod resource_collection;
mod resource_response;
mod route_response;

pub use conditional::Conditional;
pub use json_resource::{JsonResource, ModelResource};
pub use paginated_resource_response::PaginatedResourceResponse;
pub use resource_collection::{ResourceCollection, ResourceItems};
pub use resource_response::ResourceResponse;
#[doc(hidden)]
pub use route_response::ResourceRoute;

pub type ResourceResult<T> = Result<T, crate::http::HttpError>;

/// Navigation links exposed by paginated resource responses.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PaginationLinks {
    pub first: Option<String>,
    pub last: Option<String>,
    pub prev: Option<String>,
    pub next: Option<String>,
}

/// The default data passed to a collection's pagination-information hook.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct PaginationInformation {
    pub links: PaginationLinks,
    pub meta: serde_json::Map<String, serde_json::Value>,
}
