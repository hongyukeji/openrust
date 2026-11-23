//! Service provider extension contracts.
use std::future::Future;

use crate::{
    container::ServiceKey,
    http::{HttpError, Request, Response, ResponseOutput},
};

/// Builds a response for the request that produced this object.
pub trait Responsable: Send {
    fn to_response(
        self,
        request: &mut Request,
    ) -> impl Future<Output = Result<Response, HttpError>> + Send;
}

impl<T: ResponseOutput + Send> Responsable for T {
    async fn to_response(self, _: &mut Request) -> Result<Response, HttpError> {
        self.into_response()
    }
}

/// Provides the original HTML markup.
pub trait Htmlable {
    fn to_html(&self) -> &str;
}

/// Static service metadata for a compiled deferred provider factory.
pub trait DeferrableProvider {
    fn provides() -> Vec<ServiceKey>;

    /// Events that register the provider without resolving a service key.
    fn when() -> Vec<&'static str> {
        Vec::new()
    }
}
