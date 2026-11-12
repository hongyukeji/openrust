use std::sync::Arc;

use crate::{
    contracts::{
        broadcasting::Factory,
        container::Container as _,
        foundation::Application as _,
        routing::{Handler, Registrar},
        session::Session as _,
    },
    foundation::Application,
    http::{HttpError, HttpFuture, Method, Request, Response},
    routing::{RouteGroup, Router, RoutingError},
    support::facades::Broadcast,
};

use super::{BroadcastException, BroadcastResult, ChannelRegistry};

/// Handles broadcast channel authentication.
pub struct BroadcastController {
    channels: Arc<ChannelRegistry>,
    factory: Option<Arc<dyn Factory>>,
}

impl BroadcastController {
    pub(crate) fn register(
        app: &Application,
        router: &mut Router,
        register: fn() -> BroadcastResult<()>,
    ) -> Result<(), RoutingError> {
        let channels = Arc::new(
            Broadcast::register(register)
                .map_err(|source| RoutingError::BroadcastRegistration { source })?,
        );
        let container = app.container();
        container
            .instance(channels.clone())
            .map_err(|source| RoutingError::ContainerFailed { source })?;
        let factory = if container.bound::<dyn Factory>() {
            Some(container.make::<dyn Factory>().map_err(|error| {
                RoutingError::BroadcastRegistration {
                    source: BroadcastException::Backend(Box::new(error)),
                }
            })?)
        } else {
            None
        };
        router.group(RouteGroup::new().middleware_name("web"), |router| {
            router
                .add_route(
                    &[Method::GET, Method::POST],
                    "/broadcasting/auth",
                    Arc::new(Self { channels, factory }),
                )?
                .withoutMiddleware("framework.prevent_request_forgery");
            Ok(())
        })
    }
}

impl Handler for BroadcastController {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let factory = self
                .factory
                .as_ref()
                .filter(|_| !self.channels.is_empty())
                .ok_or(HttpError::AuthorizationDenied(
                    "Broadcast channel access denied.".into(),
                ))?;
            let broadcaster = factory.connection(None).map_err(http_error)?;
            if let Ok(session) = request.session_mut() {
                session.reflash();
            }
            broadcaster.auth(request).await.map_err(http_error)
        })
    }
}

fn http_error(error: BroadcastException) -> HttpError {
    match error {
        BroadcastException::AccessDenied => {
            HttpError::AuthorizationDenied("Broadcast channel access denied.".into())
        }
        BroadcastException::InvalidChannel => HttpError::InvalidRequest,
        source => HttpError::Internal {
            source: Box::new(source),
        },
    }
}
