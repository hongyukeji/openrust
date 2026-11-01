//! 提取器：使用 [`Validated`] 读取经过校验的输入，使用 [`Inject`] 获取服务。

use std::ops::Deref;
use std::sync::Arc;

use crate::core::container::Scope;
use crate::core::{ApiResponse, AppState, Error, Result, ValidationErrors};
use crate::validation::{InputContext, Validate, validate_input};
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRef, FromRequest, FromRequestParts, Request};
use axum::extract::{Json as AxumJson, Path as AxumPath};
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
        let encoded = normalized_json(req, state).await?;
        decode_json(&encoded).map(Self)
    }
}

async fn normalized_json<S: Send + Sync>(req: Request, state: &S) -> Result<Vec<u8>> {
    let (parts, body) = req.into_parts();
    let policy = super::request::policy(&parts);
    let query = super::request::parse_query(parts.uri.query(), &policy)?;
    // 固定小正文仅复用 Axum 的 MIME 判定，保留 415 优先且不消费真实正文。
    let mut probe = Request::from_parts(parts.clone(), axum::body::Body::from("null"));
    axum::extract::DefaultBodyLimit::disable().apply(&mut probe);
    let _ = AxumJson::<serde::de::IgnoredAny>::from_request(probe, state)
        .await
        .map_err(json_error)?;
    let raw = bytes::Bytes::from_request(Request::from_parts(parts.clone(), body), state)
        .await
        .map_err(|error| super::request::body_error(&error))?;
    super::request::validate_json_depth(&raw)?;
    let _ = AxumJson::<serde::de::IgnoredAny>::from_bytes(&raw).map_err(json_error)?;
    // 有界额外 buffer 只重写字符串值，数字/键/重复字段维持原始词法。
    let encoded = policy.rewrite_json(&raw)?;
    #[cfg(feature = "session")]
    {
        // 可选闪回视图不可表示正文时，仍由 typed DTO 决定请求有效性。
        // 清除先前 Query 提取器的局部视图，避免将其误当完整输入闪回。
        match AxumJson::<serde_json::Value>::from_bytes(&encoded) {
            Ok(AxumJson(body)) => {
                let all =
                    crate::support::data::union(&crate::support::data::cast_json(body), &query);
                crate::support::data::validate_limits(&all)?;
                crate::core::error::bind_exception_input(all);
            }
            Err(_) => crate::core::error::clear_exception_input(),
        }
    }
    #[cfg(not(feature = "session"))]
    let _ = query;
    Ok(encoded)
}

fn decode_json<T: DeserializeOwned>(encoded: &[u8]) -> Result<T> {
    AxumJson::<T>::from_bytes(encoded)
        .map(|AxumJson(value)| value)
        .map_err(json_error)
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

    // Query 的 parts-only 解析无需等待 IO，签名遵循 Axum 契约。
    #[allow(clippy::unused_async_trait_impl)]
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self> {
        let _ = state;
        let policy = super::request::policy(parts);
        let value = super::request::parse_query(parts.uri.query(), &policy)?;
        #[cfg(feature = "session")]
        crate::core::error::bind_exception_input(value.clone());
        super::request::deserialize_query(value).map(Self)
    }
}

/// 从应用容器解析已登记实例或异步工厂；请求内使用独立 scope。
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

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self> {
        if let Some(scope) = parts.extensions.get::<Arc<Scope>>() {
            scope.resolve::<T>().await.map(Self)
        } else {
            AppState::from_ref(state).resolve::<T>().await.map(Self)
        }
    }
}

/// 按同一字段规则验证输入，再提供已构造的 DTO。
///
/// 普通 JSON/Query 保留原 typed 反序列化错误与 serde 默认值。
/// 已由可信 middleware 确认且带 Validate-Only 的请求只验证所选原始字段；
/// 成功立即返回 204，未构造完整 DTO，也不继续提取后续业务参数。
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
    type Rejection = Response;
    async fn from_request(req: Request, state: &S) -> std::result::Result<Self, Response> {
        let selected = if super::precognition::confirmed(req.extensions()) {
            super::precognition::validate_only_header(req.headers()).cloned()
        } else {
            None
        };
        let encoded = normalized_json(req, state)
            .await
            .map_err(IntoResponse::into_response)?;
        if let Some(selected) = selected {
            let raw =
                decode_json::<serde_json::Value>(&encoded).map_err(IntoResponse::into_response)?;
            validate_selected::<T>(&raw, &selected, InputContext::Json)
                .map_err(IntoResponse::into_response)?;
            return Err(precognition_success());
        }
        // 从原编码构造 DTO，不能经 Value 重编码吞掉重复字段等既有 typed 错误。
        let value = decode_json::<T>(&encoded).map_err(IntoResponse::into_response)?;
        let raw =
            decode_json::<serde_json::Value>(&encoded).map_err(IntoResponse::into_response)?;
        validate_input::<T>(&raw, InputContext::Json).map_err(IntoResponse::into_response)?;
        Ok(Self(Json(value)))
    }
}

impl<S, T> FromRequestParts<S> for Validated<Query<T>>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate + Send,
{
    type Rejection = Response;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, Response> {
        let selected = if super::precognition::confirmed(&parts.extensions) {
            super::precognition::validate_only_header(&parts.headers).cloned()
        } else {
            None
        };
        let Query(raw) = Query::<serde_json::Value>::from_request_parts(parts, state)
            .await
            .map_err(IntoResponse::into_response)?;
        if let Some(selected) = selected {
            validate_selected::<T>(&raw, &selected, InputContext::Query)
                .map_err(IntoResponse::into_response)?;
            return Err(precognition_success());
        }
        let value = super::request::deserialize_query::<T>(raw.clone())
            .map_err(IntoResponse::into_response)?;
        validate_input::<T>(&raw, InputContext::Query).map_err(IntoResponse::into_response)?;
        Ok(Self(Query(value)))
    }
}

fn validate_selected<T: Validate>(
    raw: &serde_json::Value,
    header: &http::HeaderValue,
    context: InputContext,
) -> Result<()> {
    if header.as_bytes().len() > 16 * 1024 {
        return Err(Error::custom_with_status(
            StatusCode::PAYLOAD_TOO_LARGE,
            "validation_limits_exceeded",
            "Validate-Only 超过限制",
        ));
    }
    let selected = header
        .to_str()
        .map_err(|_| Error::BadRequest("Validate-Only 必须是有效文本".into()))?;
    let patterns: Vec<_> = selected.split(',').take(129).collect();
    T::rules().validate_only(raw, &patterns, context)
}

fn precognition_success() -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        "precognition-success",
        http::HeaderValue::from_static("true"),
    );
    response
}
