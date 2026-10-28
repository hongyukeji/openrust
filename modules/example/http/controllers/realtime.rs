//! 文章所有者的实时频道；认证过期时立即结束 SSE 会话。

use crate::modules::example::models::article;
use openrust::auth::{CurrentUser, JwtTokens};
use openrust::db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use openrust::http::Path;
use openrust::http::axum::{
    http::{HeaderMap, header},
    response::Response,
};
use openrust::prelude::{Db, Inject};
use openrust::realtime::Realtime;
use openrust::{Error, Result};
use std::time::Duration;

pub(super) async fn require_owned(db: &Db, user_id: i64, article_id: i64) -> Result<()> {
    article::Entity::find_by_id(article_id)
        .filter(article::Column::OwnerId.eq(user_id))
        .one(&db.primary_connection()?)
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| Error::NotFound("article not found".into()))?;
    Ok(())
}

pub(super) fn bearer_token(headers: &HeaderMap) -> Result<&str> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(Error::Unauthorized)?;
    let mut parts = value.split_ascii_whitespace();
    let scheme = parts.next().ok_or(Error::Unauthorized)?;
    let token = parts.next().ok_or(Error::Unauthorized)?;
    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() || parts.next().is_some() {
        return Err(Error::Unauthorized);
    }
    Ok(token)
}

pub(crate) async fn events(
    user: CurrentUser,
    headers: HeaderMap,
    Inject(db): Inject<Db>,
    Inject(tokens): Inject<JwtTokens>,
    Inject(realtime): Inject<Realtime>,
    Path(id): Path<i64>,
) -> Result<Response> {
    let (token_user_id, expires_at) =
        tokens.verify_access_token_with_expiry(bearer_token(&headers)?)?;
    if token_user_id != user.id() {
        return Err(Error::Unauthorized);
    }
    require_owned(&db, user.id(), id).await?;
    realtime.validate_origin(&headers)?;
    Ok(realtime
        .connect(&user, format!("example.article.{id}"), expires_at)
        .await?
        .sse())
}

pub(crate) async fn announce(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(realtime): Inject<Realtime>,
    Path(id): Path<i64>,
) -> Result<openrust::http::Json<serde_json::Value>> {
    require_owned(&db, user.id(), id).await?;
    let delivered = realtime
        .send_to_user(
            user.id(),
            &format!("example.article.{id}"),
            &serde_json::json!({"article_id": id, "event": "article.announced"}),
            Duration::from_secs(30),
        )
        .await?;
    Ok(openrust::http::Json(
        serde_json::json!({"local_sessions_queued": delivered}),
    ))
}
