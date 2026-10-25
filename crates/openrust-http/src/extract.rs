//! Extractors: [`Validated`] for validated input and [`Inject`] for services.

use std::future::{Future, ready};
use std::ops::Deref;
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRef, FromRequest, FromRequestParts, Request};
use axum::extract::{Json as AxumJson, Path as AxumPath, Query as AxumQuery};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use http::request::Parts;
use openrust_core::{ApiResponse, AppState, Error, Result, ValidationErrors};
use serde::Serialize;
use serde::de::DeserializeOwned;
use validator::{Validate, ValidationErrorsKind};

/// JSON body extractor with OpenRust envelope errors and responses.
pub struct Json<T>(pub T);

impl<S, T> FromRequest<S> for Json<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = Error;

    async fn from_request(req: Request, state: &S) -> Result<Self> {
        AxumJson::<T>::from_request(req, state)
            .await
            .map(|AxumJson(value)| Self(value))
            .map_err(json_error)
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        ApiResponse::success(self.0).into_response()
    }
}

fn json_error(rejection: JsonRejection) -> Error {
    let detail = rejection.body_text();
    match rejection {
        JsonRejection::MissingJsonContentType(_) => Error::custom_with_status(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            detail,
        ),
        JsonRejection::JsonDataError(_) => {
            let mut errors = ValidationErrors::new();
            // Axum includes the serde path before the parser's message.
            let reason = detail
                .split_once(": ")
                .map_or(detail.as_str(), |(_, reason)| reason);
            if let Some((path, _)) = reason.split_once(": ") {
                if !path.is_empty() && !path.contains(' ') {
                    errors.add(path, reason);
                }
            } else if let Some(field) = reason.split('`').nth(1) {
                errors.add(field, reason);
            }
            Error::validation_with_detail(detail, errors)
        }
        JsonRejection::BytesRejection(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            Error::custom_with_status(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", detail)
        }
        _ => Error::BadRequest(detail),
    }
}

/// Path parameter extractor with OpenRust envelope errors on failure.
pub struct Path<T>(pub T);

impl<S, T> FromRequestParts<S> for Path<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self> {
        AxumPath::<T>::from_request_parts(parts, state)
            .await
            .map(|AxumPath(value)| Self(value))
            .map_err(|rejection| Error::BadRequest(rejection.body_text()))
    }
}

/// Query parameter extractor with OpenRust envelope errors on failure.
pub struct Query<T>(pub T);

impl<S, T> FromRequestParts<S> for Query<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self> {
        AxumQuery::<T>::from_request_parts(parts, state)
            .await
            .map(|AxumQuery(value)| Self(value))
            .map_err(|rejection| Error::BadRequest(rejection.body_text()))
    }
}

/// Fetches a service registered with `AppBuilder::insert`.
///
/// Missing registrations surface as an internal error naming the type, never as a panic.
pub struct Inject<T>(pub Arc<T>);

impl<T> Deref for Inject<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<S, T> FromRequestParts<S> for Inject<T>
where
    S: Send + Sync,
    AppState: FromRef<S>,
    T: Send + Sync + 'static,
{
    type Rejection = Error;

    fn from_request_parts(
        _parts: &mut Parts,
        state: &S,
    ) -> impl Future<Output = Result<Self>> + Send {
        // Lookup is synchronous; no `.await` is needed.
        ready(AppState::from_ref(state).try_get::<T>().map(Self))
    }
}

/// Deserialises the inner extractor and runs its `validator` rules.
///
/// Deserialisation failures follow the inner extractor (`400`, `413`, `415`, or `422`).
/// Rule failures answer `422 validation_failed` with one message list per field. Supported inner extractors: [`Json`] and [`Query`].
pub struct Validated<T>(pub T);

impl<T> Deref for Validated<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<S, T> FromRequest<S> for Validated<Json<T>>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate + Send,
{
    type Rejection = Error;

    async fn from_request(req: Request, state: &S) -> Result<Self> {
        let Json(value) = Json::<T>::from_request(req, state).await?;
        value.validate().map_err(|errors| convert(&errors))?;
        Ok(Self(Json(value)))
    }
}

impl<S, T> FromRequestParts<S> for Validated<Query<T>>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate + Send,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self> {
        let Query(value) = Query::<T>::from_request_parts(parts, state).await?;
        value.validate().map_err(|errors| convert(&errors))?;
        Ok(Self(Query(value)))
    }
}

/// Flattens nested `validator` errors into `field.path` keys.
fn convert(errors: &validator::ValidationErrors) -> Error {
    let mut out = ValidationErrors::new();
    collect(&mut out, "", errors);
    Error::Validation(out)
}

fn collect(out: &mut ValidationErrors, prefix: &str, errors: &validator::ValidationErrors) {
    for (field, kind) in errors.errors() {
        let path = if prefix.is_empty() {
            field.to_string()
        } else {
            format!("{prefix}.{field}")
        };
        match kind {
            ValidationErrorsKind::Field(failures) => {
                for failure in failures {
                    let message = failure
                        .message
                        .as_ref()
                        .map_or_else(|| failure.code.to_string(), ToString::to_string);
                    out.add(path.clone(), message);
                }
            }
            ValidationErrorsKind::Struct(nested) => collect(out, &path, nested),
            ValidationErrorsKind::List(items) => {
                for (index, nested) in items {
                    collect(out, &format!("{path}[{index}]"), nested);
                }
            }
        }
    }
}
