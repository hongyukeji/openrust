#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use super::{AuthorizationViewParameters, PendingAuthorization};
use crate::{AuthorizationServer, OauthError, OauthParameters, Passport};
use axum::response::IntoResponse as _;
use openrust::{
    contracts::{auth::StatefulGuard as _, container::Container as _, session::Session as _},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Auth, Config, Facade},
};
use serde_json::{Value, json};
use std::sync::Arc;
use subtle::ConstantTimeEq as _;

pub struct AuthorizationController;
impl AuthorizationController {
    pub fn authorize(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let parameters = OauthParameters::new(
                request
                    .query(None)?
                    .and_then(|v| v.as_object().cloned())
                    .unwrap_or_default(),
            );
            let server = AuthorizationServer::get_facade_root()?;
            let validated = match server.validate_authorization_request(&parameters).await {
                Ok(value) => value,
                Err(error) => return Ok(error.into_response().into()),
            };
            let prompt = parameters
                .parameter("prompt", None)
                .map_err(internal)?
                .unwrap_or_default();
            let none = prompt.split(' ').any(|p| p == "none");
            select_guard(request)?;
            let mut user = Auth::user(request).await?;
            if user.is_some()
                && !none
                && prompt.split(' ').any(|p| p == "login")
                && request.session()?.get("promptedForLogin") != Some(&json!(true))
            {
                Auth::session_guard(request, None)?.logout().await?;
                request.session_mut()?.invalidate().await?;
                request.session_mut()?.regenerate_token()?;
                user = None;
            }
            let Some(user) = user else {
                if none {
                    return Ok(validated
                        .error_redirect(&OauthError::login_required())
                        .map_err(internal)?
                        .into_response()
                        .into());
                }
                request
                    .session_mut()?
                    .put("promptedForLogin", json!(true))?;
                return Err(HttpError::Unauthenticated);
            };
            request.session_mut()?.forget(&["promptedForLogin"]);
            let passport = passport()?;
            let client = passport
                .client_repository()
                .find_active(validated.client_identifier())
                .await
                .map_err(internal)?
                .ok_or(HttpError::NotFound)?;
            let scopes: Vec<_> = passport
                .scopes()
                .into_iter()
                .filter(|scope| validated.scopes().contains(&scope.id))
                .collect();
            let consent = !none && prompt.split(' ').any(|p| p == "consent");
            if !consent
                && (client
                    .skips_authorization(user.as_ref(), &scopes)
                    .await
                    .map_err(internal)?
                    || server
                        .has_granted_scopes(&validated, &user.get_auth_identifier())
                        .await
                        .map_err(internal)?)
            {
                return match server
                    .complete_authorization_request(validated, user.get_auth_identifier(), true)
                    .await
                {
                    Ok(value) => Ok(value.into_response().into()),
                    Err(error) => Ok(error.into_response().into()),
                };
            }
            if none {
                return Ok(validated
                    .error_redirect(&OauthError::consent_required())
                    .map_err(internal)?
                    .into_response()
                    .into());
            }
            let auth_token = nonce()?;
            let pending = serde_json::to_value(PendingAuthorization {
                request: validated.clone(),
                user: user.get_auth_identifier(),
            })
            .map_err(internal)?;
            request.session_mut()?.put("authToken", json!(auth_token))?;
            request.session_mut()?.put("authRequest", pending)?;
            let view = passport.authorization_view_response().ok_or_else(|| {
                internal(OauthError::server_error(
                    "Configure Passport::authorization_view",
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
pub struct ApproveAuthorizationController;
impl ApproveAuthorizationController {
    pub fn approve(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        complete(request, true)
    }
}
pub struct DenyAuthorizationController;
impl DenyAuthorizationController {
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
        let pending = request.session_mut()?.pull("authRequest");
        if !matches!((&supplied,&stored),(Some(Value::String(a)),Some(Value::String(b))) if !a.is_empty() && bool::from(a.as_bytes().ct_eq(b.as_bytes())))
        {
            return Err(openrust::http::abort(
                StatusCode::BAD_REQUEST,
                "Invalid authorization token",
                Default::default(),
            ));
        }
        let pending: PendingAuthorization =
            serde_json::from_value(pending.ok_or(HttpError::InvalidRequest)?)
                .map_err(|_| HttpError::InvalidRequest)?;
        if pending.user != user.get_auth_identifier() {
            return Err(HttpError::Unauthenticated);
        }
        let server = AuthorizationServer::get_facade_root()?;
        Ok(
            match server
                .complete_authorization_request(pending.request, pending.user, approved)
                .await
            {
                Ok(value) => value.into_response().into(),
                Err(error) => error.into_response().into(),
            },
        )
    })
}
pub struct AccessTokenController;
impl AccessTokenController {
    pub fn issue_token(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            // Token requests read the body only; query parameters cannot override client credentials.
            let values = request
                .post(None)
                .await?
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            let parameters = OauthParameters::new(values);
            let header = request.headers().get("authorization").map(|h| h.as_bytes());
            let server = AuthorizationServer::get_facade_root()?;
            let result = server
                .respond_to_access_token_request(&parameters, header)
                .await;
            let response = match result {
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
pub(super) fn select_guard(request: &mut Request) -> Result<(), HttpError> {
    if let Some(Value::String(guard)) = Config::get("passport.guard")? {
        Auth::should_use(request, &guard)?;
    }
    Ok(())
}
pub(super) fn passport() -> Result<Arc<Passport>, HttpError> {
    openrust::container::Container::get_instance()
        .make::<Passport>()
        .map_err(Into::into)
}
pub(super) fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}
pub(super) fn nonce() -> Result<String, HttpError> {
    use rand::RngCore as _;
    let mut bytes = [0; 20];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(internal)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
