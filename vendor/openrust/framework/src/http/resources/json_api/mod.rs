//! JSON:API resource documents.

mod json_api_resource;

pub use json_api_resource::JsonApiResource;

pub(super) const MEDIA_TYPE: &str = "application/vnd.api+json";
