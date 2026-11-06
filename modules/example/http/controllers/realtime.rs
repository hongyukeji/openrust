//! 文章所有者的实时频道；认证过期时立即结束 SSE 会话。

use crate::modules::example::models::article;
use openrust::auth::{CurrentUser, authenticate_access_credential};
use openrust::database::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use openrust::http::Path;
use openrust::http::axum::{
    http::{HeaderMap, header},
    response::Response,
};
use openrust::prelude::{Db, Inject};
use openrust::realtime::Realtime;
use openrust::{Error, Result};
use std::time::Duration;

struct ArticleAccessValidator {
    state: openrust::foundation::ApplicationState,
    db: Db,
    user_id: i64,
    article_id: i64,
    token: zeroize::Zeroizing<String>,
}
#[async_trait::async_trait]
impl openrust::realtime::RealtimeValidator for ArticleAccessValidator {
    async fn validate(&self) -> Result<std::time::SystemTime> {
        let (user_id, expiry) = authenticate_access_credential(&self.state, &self.token).await?;
        if user_id != self.user_id {
            return Err(Error::Unauthorized);
        }
        require_owned(&self.db, self.user_id, self.article_id).await?;
        Ok(expiry)
    }
}
pub(super) fn article_access_validator(
    state: openrust::foundation::ApplicationState,
    db: Db,
    user_id: i64,
    article_id: i64,
    token: &str,
) -> std::sync::Arc<dyn openrust::realtime::RealtimeValidator> {
    std::sync::Arc::new(ArticleAccessValidator {
        state,
        db,
        user_id,
        article_id,
        token: zeroize::Zeroizing::new(token.to_owned()),
    })
}
#[cfg(feature = "auth-fortify")]
pub(super) struct BrowserArticleValidator {
    pub credential: openrust::auth::LiveSessionCredential,
    pub authority: std::sync::Arc<openrust::auth::fortify::CredentialAuthority>,
    pub db: Db,
    pub user_id: i64,
    pub article_id: i64,
}
#[cfg(feature = "auth-fortify")]
#[async_trait::async_trait]
impl openrust::realtime::RealtimeValidator for BrowserArticleValidator {
    async fn validate(&self) -> Result<std::time::SystemTime> {
        let expiry = self.credential.authorize(&self.authority).await?;
        require_owned(&self.db, self.user_id, self.article_id).await?;
        Ok(expiry)
    }
}

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
    openrust::http::axum::Extension(state): openrust::http::axum::Extension<
        openrust::foundation::ApplicationState,
    >,
    Inject(realtime): Inject<Realtime>,
    Path(id): Path<i64>,
) -> Result<Response> {
    let (token_user_id, expires_at) =
        authenticate_access_credential(&state, bearer_token(&headers)?).await?;
    if token_user_id != user.integer_id()? {
        return Err(Error::Unauthorized);
    }
    require_owned(&db, user.integer_id()?, id).await?;
    realtime.validate_origin(&headers)?;
    Ok(realtime
        .connect_with_validator(
            &user,
            format!("example.article.{id}"),
            expires_at,
            article_access_validator(
                state,
                (*db).clone(),
                user.integer_id()?,
                id,
                bearer_token(&headers)?,
            ),
        )
        .await?
        .sse())
}

pub(crate) async fn announce(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(realtime): Inject<Realtime>,
    Path(id): Path<i64>,
) -> Result<openrust::http::Json<serde_json::Value>> {
    require_owned(&db, user.integer_id()?, id).await?;
    let delivered = realtime
        .send_to_user(
            user.integer_id()?,
            &format!("example.article.{id}"),
            &serde_json::json!({"article_id": id, "event": "article.announced"}),
            Duration::from_secs(30),
        )
        .await?;
    Ok(openrust::http::Json(
        serde_json::json!({"local_sessions_queued": delivered}),
    ))
}
