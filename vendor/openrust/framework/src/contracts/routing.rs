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
pub trait UrlRoutable: Sized + Send + Sync + 'static {
    fn get_route_key_name() -> &'static str {
        "id"
    }

    fn resolve_route_binding(
        binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>>;

    fn resolve_child_route_binding(
        _binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>> {
        Box::pin(async {
            Err(HttpError::Internal {
                source: std::io::Error::other("model has no scoped route binding resolver").into(),
            })
        })
    }
}
