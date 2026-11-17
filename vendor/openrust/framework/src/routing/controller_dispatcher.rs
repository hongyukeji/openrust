use std::io;

use crate::{
    contracts::{container::Container as _, routing::Handler},
    foundation::http::{FormRequest, ValidatedFormRequest},
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::current_container,
};

pub(super) type Action<C> =
    for<'a> fn(&'a C, &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>>;

type Dispatch<C> = dyn for<'a> Fn(&'a C, &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>>
    + Send
    + Sync;

/// Dispatches a typed controller action through the current application's container.
pub struct ControllerDispatcher<C> {
    action: Box<Dispatch<C>>,
}

impl<C: 'static> ControllerDispatcher<C> {
    pub fn new(action: Action<C>) -> Self {
        Self {
            action: Box::new(action),
        }
    }
}

pub(super) type FormAction<C, F> = for<'a> fn(
    &'a C,
    &'a mut Request,
    ValidatedFormRequest<F>,
) -> HttpFuture<'a, Result<Response, HttpError>>;

impl<C: Sync + 'static> ControllerDispatcher<C> {
    #[allow(non_snake_case, reason = "Matches Laravel's FormRequest terminology")]
    pub fn withFormRequest<F: FormRequest + Default + 'static>(action: FormAction<C, F>) -> Self {
        Self {
            action: Box::new(move |controller, request| {
                Box::pin(async move {
                    let form = ValidatedFormRequest::resolve(request).await?;
                    action(controller, request, form).await
                })
            }),
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
            let controller = request
                .withContainer(container.makeAsync::<C>())
                .await
                .map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?;
            (self.action)(&controller, request).await
        })
    }
}
