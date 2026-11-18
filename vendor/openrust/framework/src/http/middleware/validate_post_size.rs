use crate::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode, abort},
};

/// Rejects an oversized declared body without polling its stream.
pub struct ValidatePostSize;

impl Middleware for ValidatePostSize {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if request.declared_body_exceeds_limit() {
                return Err(abort(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "The POST data is too large.",
                    Default::default(),
                ));
            }
            Ok(next.run(request).await)
        })
    }
}
