//! 保存发生于响应头发出前；取消只关闭本请求句柄，不后台保存。
use super::{
    Expiry, Session, SessionId, SessionRequestContext, SessionStore,
    provider::{SessionSettings, SessionSlot, missing},
};
use crate::{
    Result,
    http::{CookieFactory, CookieOptions, RequestInfo},
};
use axum::{
    extract::{FromRequestParts, Request, State},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct SessionLayer {
    pub(crate) settings: SessionSettings,
    pub(crate) store: Arc<dyn SessionStore>,
    pub(crate) error_bag_codecs: Arc<super::ErrorBagCodecs>,
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
    let (mut parts, body) = request.into_parts();
    let info = match RequestInfo::from_request_parts(&mut parts, &()).await {
        Ok(info) => info,
        Err(error) => return crate::http::cookie::private_error_response(error),
    };
    request = Request::from_parts(parts, body);
    let result = prepare(&layer, &mut request, &info).await;
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
    crate::core::error::bind_exception_session(session.clone());
    let jar = request
        .extensions()
        .get::<crate::http::CookieJar>()
        .cloned();
    let mut close = Close(Some(session.clone()), jar);
    let result = async {
        let secure = crate::http::transport::is_secure(&request)?;
        // Route 与 Precognition 共享请求私有状态；不得在下游前猜测最终命中或确认。
        let response = next.run(request).await;
        complete(&layer, &session, &info, secure, response).await
    }
    .await;
    match result {
        Ok(response) => {
            close.1 = None;
            response
        }
        Err(error) => crate::http::cookie::private_error_response(error),
    }
}

async fn complete(
    layer: &SessionLayer,
    session: &Session,
    info: &RequestInfo,
    secure: bool,
    mut response: Response,
) -> Result<Response> {
    if info.method() == http::Method::GET
        && info.route()?.is_some()
        && !info.ajax()?
        && !info.prefetch()?
        && !info.is_precognitive()
    {
        session.set_previous_url(&info.full_url()?)?;
        // 上游写 URL 后再次调用公开 resolver；有状态 resolver 的第二次结果不能复用。
        let route = info.route()?.ok_or_else(|| {
            crate::Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "session_previous_route_missing",
                "记录上一请求名称时路由已不存在",
            )
        })?;
        session.set_previous_route(route.name())?;
    }
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
    crate::http::cookie::attach_response_cookies(&mut response, [cookie], false)?;
    // 已确认的预检仍已执行 start/GC，并保留响应 Cookie；仅跳过自动持久化。
    if !info.is_precognitive() {
        session.save().await?;
    }
    Ok(response)
}
async fn prepare(
    layer: &SessionLayer,
    request: &mut Request,
    info: &RequestInfo,
) -> Result<Session> {
    let parsed = crate::http::cookie::parse_headers(request.headers())?;
    let id = parsed
        .value
        .get(&layer.settings.cookie_name)
        .and_then(serde_json::Value::as_str)
        .and_then(|value| SessionId::parse(value).ok());
    let session = Session::with_options_and_codecs(
        id,
        layer.store.clone(),
        Some(Expiry::OnInactivity(time::Duration::seconds(
            layer.settings.config.lifetime_seconds()?,
        ))),
        layer.settings.config.max_bytes,
        layer.settings.cookie_name.clone(),
        Arc::clone(&layer.error_bag_codecs),
    )?;
    session.set_object_encoding(if layer.settings.config.serializer == "json" {
        crate::support::ObjectEncoding::JsonProjection
    } else {
        crate::support::ObjectEncoding::Reject
    })?;
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
    if session.handler_accepts_request()? {
        let jar = request
            .extensions()
            .get::<crate::http::CookieJar>()
            .cloned()
            .ok_or_else(missing)?;
        let context = SessionRequestContext::from_request(
            parsed.value,
            jar,
            request
                .extensions()
                .get::<crate::http::cookie::CookieProtection>()
                .cloned(),
        )?;
        let context =
            if session.handler()?.semantics() == super::SessionStoreSemantics::NativeDatabase {
                context.with_database_metadata(
                    super::DatabaseSessionMetadata {
                        user_id: None,
                        ip_address: Some(info.client_ip()?.map(|ip| ip.to_string())),
                        user_agent: None,
                    }
                    .with_user_agent_bytes(
                        request
                            .headers()
                            .get(http::header::USER_AGENT)
                            .map_or(&[][..], http::HeaderValue::as_bytes),
                    ),
                )
            } else {
                context
            };
        session.set_request_on_handler(context).await?;
    }
    session.start().await?;
    if lottery_wins(layer.settings.config.lottery)? {
        session
            .collect_garbage(layer.settings.config.lifetime_seconds()?)
            .await?;
    }
    #[cfg(feature = "view")]
    request
        .extensions_mut()
        .insert(session.shared_view_errors()?);
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
