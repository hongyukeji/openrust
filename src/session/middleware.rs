//! 保存发生于响应头发出前；取消只关闭本请求句柄，不后台保存。
use super::{
    Expiry, Session, SessionId, SessionStore,
    provider::{SessionSettings, SessionSlot, missing},
};
use crate::{
    Result,
    http::{CookieFactory, CookieOptions, CookieResponseExt},
};
use axum::{
    extract::{FromRequestParts, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct SessionLayer {
    pub(crate) settings: SessionSettings,
    pub(crate) store: Arc<dyn SessionStore>,
}
struct Close(Option<Session>);
impl Drop for Close {
    fn drop(&mut self) {
        if let Some(session) = &self.0 {
            session.close();
        }
    }
}
impl<S: Send + Sync> FromRequestParts<S> for Session {
    type Rejection = crate::Error;
    #[allow(clippy::unused_async_trait_impl)]
    async fn from_request_parts(parts: &mut http::request::Parts, _state: &S) -> Result<Self> {
        parts.extensions.get::<Self>().cloned().ok_or_else(missing)
    }
}
pub(crate) async fn handle(
    State(layer): State<Option<SessionLayer>>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(layer) = layer else {
        return next.run(request).await;
    };
    let result = prepare(&layer, &mut request).await;
    let session = match result {
        Ok(session) => session,
        Err(error) => return error.into_response(),
    };
    let _close = Close(Some(session.clone()));
    let secure = match crate::http::transport::is_secure(&request) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let jar = request
        .extensions()
        .get::<crate::http::CookieJar>()
        .cloned();
    let response = next.run(request).await;
    let result = async {
        session.save().await?;
        let config = &layer.settings.config;
        let mut factory = CookieFactory::default();
        factory.set_default_path_and_domain(
            config.path.clone(),
            config.domain.clone(),
            config.secure.or(Some(secure)),
            config.validate()?,
        )?;
        let minutes = if config.expire_on_close {
            0
        } else {
            i64::try_from(config.lifetime).map_err(crate::Error::internal)?
        };
        let cookie = factory
            .make(
                session.name()?,
                session.id()?.to_string(),
                minutes,
                CookieOptions {
                    http_only: config.http_only,
                    ..CookieOptions::default()
                },
            )?
            .with_partitioned(config.partitioned);
        response.with_cookie(cookie)
    }
    .await;
    match result {
        Ok(response) => response.into_response(),
        Err(error) => {
            if let Some(jar) = jar {
                let _ = jar.flush_queued_cookies();
            }
            error.into_response()
        }
    }
}
async fn prepare(layer: &SessionLayer, request: &mut Request) -> Result<Session> {
    let parsed = crate::http::cookie::parse_headers(request.headers())?;
    let id = parsed
        .value
        .get(&layer.settings.cookie_name)
        .and_then(serde_json::Value::as_str)
        .and_then(|value| SessionId::parse(value).ok());
    let session = Session::with_options(
        id,
        layer.store.clone(),
        Some(Expiry::OnInactivity(time::Duration::seconds(
            layer.settings.config.lifetime_seconds()?,
        ))),
        layer.settings.config.max_bytes,
        layer.settings.cookie_name.clone(),
    )?;
    let scope = request
        .extensions()
        .get::<Arc<crate::core::container::Scope>>()
        .ok_or_else(missing)?
        .clone();
    scope
        .resolve::<SessionSlot>()
        .await?
        .0
        .set(session.clone())
        .map_err(|_| missing())?;
    request.extensions_mut().insert(session.clone());
    let mut close = Close(Some(session.clone()));
    session.start().await?;
    close.0 = None;
    Ok(session)
}
