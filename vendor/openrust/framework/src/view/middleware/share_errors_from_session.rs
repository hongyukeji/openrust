use crate::{
    contracts::{http::Middleware, session::Session},
    http::{HttpError, HttpFuture, Next, Request, Response},
    view::ViewValue,
};

tokio::task_local! {
    static ERRORS: Option<ViewValue>;
}

/// Makes the session's validation error bags available to views in the current request.
pub struct ShareErrorsFromSession;

impl Middleware for ShareErrorsFromSession {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let errors = request
                .session()
                .ok()
                .and_then(|session| session.get("errors"))
                .filter(|errors| errors.is_object())
                .cloned()
                .unwrap_or_else(|| ViewValue::Object(serde_json::Map::new()));
            Ok(ERRORS.scope(Some(errors), next.run(request)).await)
        })
    }
}

pub(crate) fn shared_errors() -> Option<ViewValue> {
    ERRORS.try_with(Clone::clone).ok().flatten()
}

pub(crate) async fn request_scope<F: std::future::Future>(future: F) -> F::Output {
    ERRORS.scope(None, future).await
}

pub(crate) fn render_scope<T>(errors: Option<ViewValue>, render: impl FnOnce() -> T) -> T {
    ERRORS.sync_scope(errors, render)
}
