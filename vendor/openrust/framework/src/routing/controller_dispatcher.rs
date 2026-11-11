use std::{io, marker::PhantomData};

use crate::{
    contracts::{container::Container as _, routing::Handler},
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::current_container,
};

type Action<C> = for<'a> fn(&'a C, &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>>;

/// Dispatches a typed controller action through the current application's container.
pub struct ControllerDispatcher<C> {
    action: Action<C>,
    controller: PhantomData<fn() -> C>,
}

impl<C> ControllerDispatcher<C> {
    pub fn new(action: Action<C>) -> Self {
        Self {
            action,
            controller: PhantomData,
        }
    }
}

impl<C: Send + Sync + 'static> Handler for ControllerDispatcher<C> {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let container = current_container().ok_or_else(|| HttpError::Internal {
                source: Box::new(io::Error::other(
                    "Controller dispatch requires an application scope.",
                )),
            })?;
            let controller =
                container
                    .makeAsync::<C>()
                    .await
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
            (self.action)(&controller, request).await
        })
    }
}
