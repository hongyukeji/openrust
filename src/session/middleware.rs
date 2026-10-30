//! 保存发生于响应头发出前；取消只关闭本请求句柄，不后台保存。
use super::{
    Expiry, Session, SessionId, SessionRequestContext, SessionStore,
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
struct Close(Option<Session>, Option<crate::http::CookieJar>);
impl Drop for Close {
    fn drop(&mut self) {
        if let Some(session) = &self.0 {
            session.close();
        }
        if let Some(jar) = &self.1 {
            let _ = jar.flush_queued_cookies();
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
        Err(error) => {
            // 请求驱动绑定或加载失败时，不发送它此前排入的 Cookie。
            if let Some(jar) = request.extensions().get::<crate::http::CookieJar>() {
                let _ = jar.flush_queued_cookies();
            }
            return crate::http::cookie::private_error_response(error);
        }
    };
    let _close = Close(Some(session.clone()), None);
    let secure = match crate::http::transport::is_secure(&request) {
        Ok(value) => value,
        Err(error) => return crate::http::cookie::private_error_response(error),
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
            crate::http::cookie::private_error_response(error)
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
    let mut close = Close(
        Some(session.clone()),
        request
            .extensions()
            .get::<crate::http::CookieJar>()
            .cloned(),
    );
    if session.handler_needs_request()? {
        let jar = request
            .extensions()
            .get::<crate::http::CookieJar>()
            .cloned()
            .ok_or_else(missing)?;
        session
            .set_request_on_handler(SessionRequestContext::from_request(
                parsed.value,
                jar,
                request
                    .extensions()
                    .get::<crate::http::cookie::CookieProtection>()
                    .cloned(),
            )?)
            .await?;
    }
    session.start().await?;
    if lottery_wins(layer.settings.config.lottery)? {
        session
            .collect_garbage(layer.settings.config.lifetime_seconds()?)
            .await?;
    }
    close.0 = None;
    close.1 = None;
    Ok(session)
}

fn lottery_wins(lottery: [u32; 2]) -> Result<bool> {
    lottery_with(lottery, || {
        let mut bytes = [0_u8; 8];
        getrandom::fill(&mut bytes).map_err(|_| randomness_unavailable())?;
        Ok(u64::from_ne_bytes(bytes))
    })
}

fn randomness_unavailable() -> crate::Error {
    crate::Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "session_random_unavailable",
        "Session 抽签随机源不可用",
    )
}

fn lottery_with(lottery: [u32; 2], mut random: impl FnMut() -> Result<u64>) -> Result<bool> {
    let [numerator, denominator] = lottery;
    if denominator == 0 || numerator > denominator {
        return Err(crate::Error::Config(crate::config::ConfigError::Invalid(
            format!("session.lottery: {lottery:?}"),
        )));
    }
    if numerator == 0 || numerator == denominator {
        return Ok(numerator != 0);
    }
    let denominator = u64::from(denominator);
    let threshold = denominator.wrapping_neg() % denominator;
    // 64位拒绝采样保持无偏；有限重试防异常随机源形成无界循环。
    for _ in 0..8 {
        let value = random()?;
        if value >= threshold {
            return Ok(value % denominator < u64::from(numerator));
        }
    }
    Err(randomness_unavailable())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lottery_deterministic_edges_do_not_consume_entropy() -> Result<()> {
        assert!(!lottery_with([0, 100], || Err(randomness_unavailable()))?);
        assert!(lottery_with([100, 100], || Err(randomness_unavailable()))?);
        assert!(lottery_with([0, 0], || Ok(1)).is_err());
        assert!(lottery_with([3, 2], || Ok(1)).is_err());
        Ok(())
    }

    #[test]
    fn lottery_rejects_biased_prefix_and_bounds_entropy_failure() -> Result<()> {
        let mut calls = 0;
        assert!(lottery_with([2, 100], || {
            calls += 1;
            Ok(if calls == 1 { 0 } else { 100 })
        })?);
        assert_eq!(calls, 2);
        assert!(!lottery_with([2, 100], || Ok(199))?);
        let mut exhausted = 0;
        assert!(
            lottery_with([1, 3], || {
                exhausted += 1;
                Ok(0)
            })
            .is_err()
        );
        assert_eq!(exhausted, 8);
        assert!(lottery_with([1, 3], || Err(randomness_unavailable())).is_err());
        Ok(())
    }
}
