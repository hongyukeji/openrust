//! 文章所有者的只读 WebSocket；订阅 topic 和认证截止时间均由服务端确定。

use super::realtime::{bearer_token, require_owned};
use openrust::auth::{CurrentUser, JwtTokens};
use openrust::http::axum::{extract::ws::WebSocketUpgrade, http::HeaderMap, response::Response};
use openrust::http::{Inject, Path};
use openrust::realtime::Realtime;
use openrust::{Db, Error, Result};

/// 此握手使用 Bearer header，供能设置 Authorization 的原生/服务客户端使用。
/// 浏览器原生 WebSocket 无自定义请求头能力；不以 URL token 降低凭据保护。
pub(crate) async fn connect(
    user: CurrentUser,
    headers: HeaderMap,
    Inject(db): Inject<Db>,
    Inject(tokens): Inject<JwtTokens>,
    Inject(realtime): Inject<Realtime>,
    Path(id): Path<i64>,
    upgrade: WebSocketUpgrade,
) -> Result<Response> {
    let (token_user, expires_at) =
        tokens.verify_access_token_with_expiry(bearer_token(&headers)?)?;
    if token_user != user.integer_id()? {
        return Err(Error::Unauthorized);
    }
    require_owned(&db, user.integer_id()?, id).await?;
    realtime.validate_origin(&headers)?;
    Ok(realtime
        .connect(&user, format!("example.article.{id}"), expires_at)
        .await?
        .websocket(upgrade))
}

/// 浏览器使用同请求已认证的 Cookie Session；握手不传 URL 凭据或自定义 CSRF header。
/// 精确 Origin 防跨站 WebSocket 劫持，连接上限固定为当前持久 Session 截止时间。
/// 已建连接不宣称在其他请求退出登录后即时撤销。
#[allow(clippy::too_many_arguments)] // 显式提取认证 Session、URI 和存储，避免隐藏握手安全边界。
pub(crate) async fn browser_connect(
    user: CurrentUser,
    session: openrust::auth::Session,
    headers: HeaderMap,
    openrust::http::axum::extract::OriginalUri(uri): openrust::http::axum::extract::OriginalUri,
    Inject(db): Inject<Db>,
    Inject(sessions): Inject<openrust::auth::SessionStoreHandle>,
    Inject(auth): Inject<openrust::auth::AuthManager>,
    Inject(realtime): Inject<Realtime>,
    Path(id): Path<i64>,
    upgrade: WebSocketUpgrade,
) -> Result<Response> {
    if headers.get_all("origin").iter().count() != 1 {
        return Err(Error::Forbidden(
            "浏览器实时连接要求唯一且精确的 Origin".into(),
        ));
    }
    realtime.validate_origin(&headers)?;
    if user.source() != openrust::auth::AuthSource::Session
        || headers.contains_key("authorization")
        || uri.query().is_some()
    {
        return Err(Error::Unauthorized);
    }
    require_owned(&db, user.integer_id()?, id).await?;
    let guard = auth.guard(user.guard_name()).await?;
    let expires_at = sessions
        .authorized_until_guard(&session, guard.as_stateful().ok_or(Error::Unauthorized)?)
        .await?;
    Ok(realtime
        .connect(&user, format!("example.article.{id}"), expires_at)
        .await?
        .websocket(upgrade))
}
