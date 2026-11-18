use crate::{
    contracts::support::Responsable,
    http::{Request, Response},
};

use super::{JsonResource, ResourceResult};

/// Owns a resource until it is prepared and rendered for the current request.
pub struct ResourceResponder<R>(R);

impl<R> ResourceResponder<R> {
    pub(super) fn new(resource: R) -> Self {
        Self(resource)
    }
}

impl<R: JsonResource + Send> Responsable for ResourceResponder<R> {
    async fn toResponse(mut self, request: &mut Request) -> ResourceResult<Response> {
        self.0.toResponseAsync(request).await
    }
}
