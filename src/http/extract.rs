//! 提取器：使用 [`Validated`] 读取经过校验的输入，使用 [`Inject`] 获取服务。

use std::future::{Future, ready};
use std::ops::Deref;
use std::sync::Arc;

use crate::core::{ApiResponse, AppState, Error, Result, ValidationErrors};
use crate::validation::{Validate, validate};
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRef, FromRequest, FromRequestParts, Request};
use axum::extract::{Json as AxumJson, Path as AxumPath, Query as AxumQuery};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use http::request::Parts;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// JSON 正文提取器，错误和响应遵循 OpenRust 统一封装。
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
            // Axum 会在解析器消息前附加 serde 字段路径。
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

/// 路径参数提取器；失败时返回 OpenRust 统一封装的错误。
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

/// 查询参数提取器；失败时返回 OpenRust 统一封装的错误。
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

/// 获取通过 `AppBuilder::insert` 注册的服务。
///
/// 若服务未注册，将返回包含类型名称的内部错误，不会 panic。
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
        // 查找是同步操作，无需 `.await`。
        ready(AppState::from_ref(state).try_get::<T>().map(Self))
    }
}

/// 反序列化内部提取器的结果，并执行其 `validator` 规则。
///
/// 反序列化失败遵循内部提取器的状态码（`400`、`413`、`415` 或 `422`）。
/// 规则校验失败时返回 `422 validation_failed`，并为每个字段提供一组消息。支持的内部提取器为 [`Json`] 和 [`Query`]。
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
        let value = validate(value)?;
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
        let value = validate(value)?;
        Ok(Self(Query(value)))
    }
}
