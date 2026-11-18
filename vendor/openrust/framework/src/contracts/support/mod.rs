//! Service provider extension contracts.
use std::future::Future;

use crate::{
    container::ServiceKey,
    http::{HttpError, Request, Response, ResponseOutput},
};

/// Builds a response for the request that produced this object.
#[allow(non_snake_case, reason = "Matches Laravel's Responsable API")]
pub trait Responsable: Send {
    fn toResponse(
        self,
        request: &mut Request,
    ) -> impl Future<Output = Result<Response, HttpError>> + Send;
}

impl<T: ResponseOutput + Send> Responsable for T {
    async fn toResponse(self, _: &mut Request) -> Result<Response, HttpError> {
        self.into_response()
    }
}

/// Provides the original HTML markup.
#[allow(non_snake_case, reason = "Matches Laravel's Htmlable API")]
pub trait Htmlable {
    fn toHtml(&self) -> &str;
}

/// Static service metadata for a compiled deferred provider factory.
pub trait DeferrableProvider {
    fn provides() -> Vec<ServiceKey>;

    /// Events that register the provider without resolving a service key.
    fn when() -> Vec<&'static str> {
        Vec::new()
    }
}
