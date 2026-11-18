use percent_encoding::percent_decode_str;

use crate::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode, abort},
};

/// Rejects paths whose decoded bytes are not valid UTF-8.
pub struct ValidatePathEncoding;

impl ValidatePathEncoding {
    pub fn validate(path: &str) -> Result<(), HttpError> {
        percent_decode_str(path)
            .decode_utf8()
            .map(|_| ())
            .map_err(|_| {
                abort(
                    StatusCode::BAD_REQUEST,
                    "Malformed URL.",
                    Default::default(),
                )
            })
    }
}

impl Middleware for ValidatePathEncoding {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            Self::validate(request.parts().uri.path())?;
            Ok(next.run(request).await)
        })
    }
}
