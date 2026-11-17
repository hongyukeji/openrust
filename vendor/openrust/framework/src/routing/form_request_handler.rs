use std::{future::Future, marker::PhantomData, sync::Arc};

use crate::{
    contracts::routing::Handler,
    foundation::http::{FormRequest, ValidatedFormRequest},
    http::{HttpError, HttpFuture, Request, Response},
};

use super::IntoRouteHandler;

struct FormRequestHandler<H, F> {
    action: H,
    form: PhantomData<fn() -> F>,
}

impl<H, F, Fut> IntoRouteHandler<(ValidatedFormRequest<F>, Fut)> for H
where
    H: Fn(ValidatedFormRequest<F>) -> Fut + Send + Sync + 'static,
    F: FormRequest + Default + 'static,
    Fut: Future<Output = Result<Response, HttpError>> + Send,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(FormRequestHandler {
            action: self,
            form: PhantomData,
        })
    }
}

impl<H, F, Fut> Handler for FormRequestHandler<H, F>
where
    H: Fn(ValidatedFormRequest<F>) -> Fut + Send + Sync,
    F: FormRequest + Default + 'static,
    Fut: Future<Output = Result<Response, HttpError>> + Send,
{
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async {
            let form = ValidatedFormRequest::resolve(request).await?;
            (self.action)(form).await
        })
    }
}
