use crate::{
    contracts::validation::Validator,
    http::{Body, HttpError, Request, Response, StatusCode},
};
use ::http::{HeaderName, HeaderValue};

/// The validation hook used by precognitive form requests.
pub struct Precognition;

impl Precognition {
    pub fn after_validation_hook(
        request: &Request,
    ) -> impl Fn(&dyn Validator) -> Result<(), HttpError> + use<> {
        let validate_only = request.headers().contains_key("precognition-validate-only");
        move |validator| {
            if validate_only && validator.errors().is_empty() {
                return Err(Self::success().throw_response());
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
        if request.is_precognitive() {
            return Err(Self::success().throw_response());
        }
        Ok(())
    }
}
