//! Extractors: [`Validated`] for validated input and [`Inject`] for services.

use std::future::{Future, ready};
use std::ops::Deref;
use std::sync::Arc;

use axum::extract::{FromRef, FromRequest, FromRequestParts, Json, Query, Request};
use http::request::Parts;
use openrust_core::{AppState, Error, Result, ValidationErrors};
use serde::de::DeserializeOwned;
use validator::{Validate, ValidationErrorsKind};

/// Fetches a service registered with `AppBuilder::provide`.
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
        ready(AppState::from_ref(state).get::<T>().map(Self))
    }
}

/// Deserialises the inner extractor and runs its `validator` rules.
///
/// Deserialisation failures answer `400 bad_request`; rule failures answer `422 validation_failed`
/// with one message list per field. Supported inner extractors: [`Json`] and [`Query`].
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
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|rejection| Error::BadRequest(rejection.body_text()))?;
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
        let Query(value) = Query::<T>::from_request_parts(parts, state)
            .await
            .map_err(|rejection| Error::BadRequest(rejection.body_text()))?;
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
