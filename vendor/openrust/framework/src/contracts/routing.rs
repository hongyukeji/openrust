mod url_generator;
pub use url_generator::UrlGenerator;

use std::sync::Arc;

use crate::{
    http::{HttpError, HttpFuture, Method, Request, Response},
    routing::{BindingContext, Route, RoutingError},
};

/// Handles a matched route after middleware has run.
pub trait Handler: Send + Sync {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>>;

    /// Route dispatch hook; parameter-aware adapters resolve dependencies first.
    #[doc(hidden)]
    fn handle_precognitive<'a>(
        &'a self,
        _request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { Ok(crate::foundation::Precognition::success()) })
    }
}

impl<F> Handler for F
where
    F: for<'a> Fn(&'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> + Send + Sync,
{
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        self(request)
    }
}

/// Registers handlers with HTTP methods and URI patterns.
pub trait Registrar {
    #[track_caller]
    fn add_route(
        &mut self,
        methods: &[Method],
        uri: &str,
        handler: Arc<dyn Handler>,
    ) -> Result<&mut Route, RoutingError>;
}

/// A model that can be looked up using a matched route parameter.
pub trait UrlRoutable: Send + Sync + 'static {
    fn get_route_key_name() -> &'static str
    where
        Self: Sized,
    {
        "id"
    }

    /// Projects the model's key for use in a named URL.
    fn get_route_key(
        &self,
    ) -> crate::routing::UrlResult<Option<crate::routing::UrlValue<'static>>> {
        Ok(None)
    }

    /// Reads the attribute selected by the route's custom binding field.
    fn route_url_attribute(
        &self,
        _field: &str,
    ) -> crate::routing::UrlResult<Option<crate::routing::UrlValue<'static>>> {
        Ok(None)
    }

    fn resolve_route_binding(
        binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>>
    where
        Self: Sized;

    fn resolve_child_route_binding(
        _binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>>
    where
        Self: Sized,
    {
        Box::pin(async {
            Err(HttpError::Internal {
                source: std::io::Error::other("model has no scoped route binding resolver").into(),
            })
        })
    }
}
