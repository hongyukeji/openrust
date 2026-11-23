#![allow(non_snake_case, reason = "保留既定公开接口及命名")]
//! Device/browser endpoints with the existing authentication, Session and CSRF path.
use super::{
    AuthorizationViewParameters,
    controllers::{internal, nonce, passport, select_guard},
};
use crate::{AuthorizationServer, OauthError, OauthParameters};
use axum::response::IntoResponse as _;
use openrust::{
    contracts::{routing::UrlGenerator as _, session::Session as _},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteParameter, UrlValue},
    support::{
        MessageBag,
        facades::{Auth, Facade, Url},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use subtle::ConstantTimeEq as _;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingDevice {
    code: String,
    user: openrust::auth::AuthIdentifier,
}
pub struct DeviceCodeController;
impl DeviceCodeController {
    pub fn issue(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let values = request
                .post(None)
                .await?
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            let server = AuthorizationServer::get_facade_root().map_err(internal)?;
            let uri = Url::for_request(request)
                .map_err(internal)?
                .route("passport.device", &[], true)
                .map_err(internal)?;
            let authorization = request.headers().get("authorization").map(|v| v.as_bytes());
            let response = match server
                .respond_to_device_authorization_request(
                    &OauthParameters::new(values),
                    authorization,
                    uri,
                )
                .await
            {
                Ok(value) => Response::json(StatusCode::OK, value)?,
                Err(error) => error.into_response().into(),
            };
            Ok(response
                .header(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("no-store"),
                )
                .header(
                    axum::http::header::PRAGMA,
                    axum::http::HeaderValue::from_static("no-cache"),
                ))
        })
    }
}
pub struct DeviceUserCodeController;
impl DeviceUserCodeController {
    pub fn show(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            if let Some(code) = user_code(request)? {
                return redirect(
                    request,
                    "passport.device.authorizations.authorize",
                    Some(&code),
                );
            }
            let view = passport()?
                .device_user_code_view_response()
                .ok_or_else(|| {
                    internal(OauthError::server_error(
                        "Configure Passport::device_user_code_view",
                    ))
                })?;
            view(request).await
        })
    }
}
pub struct DeviceAuthorizationController;
impl DeviceAuthorizationController {
    pub fn authorize(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            select_guard(request)?;
            let user = Auth::user(request)
                .await?
                .ok_or(HttpError::Unauthenticated)?;
            let Some(code) = user_code(request)? else {
                return redirect(request, "passport.device", None);
            };
            let server = AuthorizationServer::get_facade_root().map_err(internal)?;
            let Some(device) = server
                .device_code_by_user_code(&code)
                .await
                .map_err(internal)?
            else {
                request
                    .session_mut()?
                    .flash_input(json!({"user_code":code}))?;
                let mut errors = MessageBag::default();
                errors.add("user_code", "Incorrect code.");
                request.session_mut()?.flash_errors("default", &errors)?;
                return redirect(request, "passport.device", None);
            };
            use openrust::database::Value as DbValue;
            let id = match device.get_key() {
                Some(DbValue::Text(id)) => id.clone(),
                _ => return Err(HttpError::InvalidRequest),
            };
            let client_id = match device.attribute("client_id") {
                Some(DbValue::Text(id)) => id.clone(),
                _ => return Err(HttpError::InvalidRequest),
            };
            let passport = passport()?;
            let client = passport
                .client_repository()
                .find(client_id.as_str())
                .await
                .map_err(internal)?
                .ok_or(HttpError::NotFound)?;
            let requested = match device.value("scopes").map_err(internal)? {
                Some(DbValue::Json(value)) => {
                    serde_json::from_value::<Vec<String>>(value).map_err(internal)?
                }
                _ => return Err(HttpError::InvalidRequest),
            };
            let scopes = passport
                .scopes()
                .into_iter()
                .filter(|scope| requested.contains(&scope.id))
                .collect();
            let auth_token = nonce()?;
            request.session_mut()?.put("authToken", json!(auth_token))?;
            request.session_mut()?.put(
                "deviceCode",
                serde_json::to_value(PendingDevice {
                    code: id,
                    user: user.get_auth_identifier(),
                })
                .map_err(internal)?,
            )?;
            let view = passport
                .device_authorization_view_response()
                .ok_or_else(|| {
                    internal(OauthError::server_error(
                        "Configure Passport::device_authorization_view",
                    ))
                })?;
            view(
                request,
                AuthorizationViewParameters {
                    client,
                    user,
                    scopes,
                    auth_token,
                },
            )
            .await
        })
    }
}
pub struct ApproveDeviceAuthorizationController;
impl ApproveDeviceAuthorizationController {
    pub fn approve(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        complete(request, true)
    }
}
pub struct DenyDeviceAuthorizationController;
impl DenyDeviceAuthorizationController {
    pub fn deny(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        complete(request, false)
    }
}
fn complete(request: &mut Request, approved: bool) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        select_guard(request)?;
        let user = Auth::user(request)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        let supplied = request.input(Some("auth_token")).await?;
        let stored = request.session_mut()?.pull("authToken");
        let pending = request.session_mut()?.pull("deviceCode");
        if !matches!((&supplied,&stored),(Some(Value::String(a)),Some(Value::String(b))) if !a.is_empty() && bool::from(a.as_bytes().ct_eq(b.as_bytes())))
        {
            return Err(openrust::http::abort(
                StatusCode::BAD_REQUEST,
                "Invalid authorization token",
                Default::default(),
            ));
        }
        let pending: PendingDevice =
            serde_json::from_value(pending.ok_or(HttpError::InvalidRequest)?)
                .map_err(|_| HttpError::InvalidRequest)?;
        if pending.user != user.get_auth_identifier() {
            return Err(HttpError::Unauthenticated);
        }
        let server = AuthorizationServer::get_facade_root().map_err(internal)?;
        if let Err(error) = server
            .complete_device_authorization_request(&pending.code, pending.user, approved)
            .await
        {
            return Ok(error.into_response().into());
        }
        request.session_mut()?.flash(
            "status",
            json!(if approved {
                "authorization-approved"
            } else {
                "authorization-denied"
            }),
        )?;
        redirect(request, "passport.device", None)
    })
}
fn user_code(request: &Request) -> Result<Option<String>, HttpError> {
    match request.query(Some("user_code"))? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.is_empty() || value == "0" => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(HttpError::InvalidRequest),
    }
}
fn redirect(request: &Request, route: &str, code: Option<&str>) -> Result<Response, HttpError> {
    let parameters = code
        .map(|v| {
            vec![RouteParameter::Named(
                "user_code".into(),
                UrlValue::String(v.into()),
            )]
        })
        .unwrap_or_default();
    let target = Url::for_request(request)
        .map_err(internal)?
        .route(route, &parameters, true)
        .map_err(internal)?;
    Ok(Response::empty(StatusCode::FOUND).header(
        axum::http::header::LOCATION,
        axum::http::HeaderValue::from_str(&target).map_err(internal)?,
    ))
}
