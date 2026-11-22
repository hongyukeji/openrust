use std::io;

use crate::{
    contracts::{container::Container as _, routing::Handler, support::Responsable},
    foundation::http::{FormRequest, ValidatedFormRequest},
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::current_container,
};

pub(super) type Action<C, T = Response> =
    for<'a> fn(&'a C, &'a mut Request) -> HttpFuture<'a, Result<T, HttpError>>;

type Dispatch<C, T> =
    dyn for<'a> Fn(&'a C, &'a mut Request) -> HttpFuture<'a, Result<T, HttpError>> + Send + Sync;

/// Dispatches a typed controller action through the current application's container.
pub struct ControllerDispatcher<C, T = Response> {
    action: Box<Dispatch<C, T>>,
    form_request: bool,
}

impl<C: 'static, T: 'static> ControllerDispatcher<C, T> {
    pub fn new(action: Action<C, T>) -> Self {
        Self {
            action: Box::new(action),
            form_request: false,
        }
    }
}

pub(super) type FormAction<C, F, T = Response> = for<'a> fn(
    &'a C,
    &'a mut Request,
    ValidatedFormRequest<F>,
)
    -> HttpFuture<'a, Result<T, HttpError>>;

impl<C: Sync + 'static, T: Send + 'static> ControllerDispatcher<C, T> {
    #[allow(non_snake_case, reason = "Matches Laravel's FormRequest terminology")]
    pub fn withFormRequest<F: FormRequest + Default + 'static>(
        action: FormAction<C, F, T>,
    ) -> Self {
        Self {
            action: Box::new(move |controller, request| {
                Box::pin(async move {
                    let form = ValidatedFormRequest::resolve(request).await?;
                    crate::foundation::Precognition::abort(request)?;
                    action(controller, request, form).await
                })
            }),
            form_request: true,
        }
    }
}

impl<C: Send + Sync + 'static, T: Responsable + 'static> Handler for ControllerDispatcher<C, T> {
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
            let controller = request.withContainer(container.makeAsync::<C>()).await?;
            if !self.form_request {
                crate::foundation::Precognition::abort(request)?;
            }
            (self.action)(&controller, request)
                .await?
                .toResponse(request)
                .await
        })
    }

    fn handle_precognitive<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        self.handle(request)
    }
}
