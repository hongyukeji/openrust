use ::http::{HeaderValue, header::CONTENT_TYPE};

use crate::http::{Body, HttpError, HttpFuture, Request, Response, StatusCode};

/// Responds when the application has booted and can handle HTTP requests.
pub fn health_check(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut response = Response::new(StatusCode::OK, Body::from("OK"));
        response.parts_mut().headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        Ok(response)
    })
}
