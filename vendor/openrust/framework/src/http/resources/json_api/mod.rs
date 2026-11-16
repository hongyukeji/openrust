//! JSON:API resource documents.

mod json_api_resource;
mod lazy_attributes;
mod relationships;

pub use json_api_resource::JsonApiResource;
pub use lazy_attributes::JsonApiAttributes;
pub use relationships::JsonApiRelationships;
pub(super) use relationships::add_included;

pub(super) const MEDIA_TYPE: &str = "application/vnd.api+json";
