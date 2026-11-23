use crate::{
    contracts::validation::Validator,
    http::{Body, HttpError, Request, Response, StatusCode},
};
use ::http::{HeaderName, HeaderValue};

/// The validation hook used by precognitive form requests.
pub struct Precognition;

impl Precognition {
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn afterValidationHook(
        request: &Request,
    ) -> impl Fn(&dyn Validator) -> Result<(), HttpError> + use<> {
        let validate_only = request.headers().contains_key("precognition-validate-only");
        move |validator| {
            if validate_only && validator.errors().is_empty() {
                return Err(Self::success().throwResponse());
            }
            Ok(())
        }
    }

    pub(crate) fn success() -> Response {
        Response::new(StatusCode::NO_CONTENT, Body::empty()).header(
            HeaderName::from_static("precognition-success"),
            HeaderValue::from_static("true"),
        )
    }

    pub(crate) fn abort(request: &Request) -> Result<(), HttpError> {
        if request.isPrecognitive() {
            return Err(Self::success().throwResponse());
        }
        Ok(())
    }
}
