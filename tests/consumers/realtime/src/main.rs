//! 双节点实时路由验收宿主。

use std::time::{Duration, SystemTime};

use openrust::{
    Application, BoxFuture, Error, NoConfig, Result,
    auth::{
        AuthCredential, AuthLayer, AuthMode, AuthPrincipal, AuthServiceProvider, Authenticator,
        CurrentUser,
    },
    config::RealtimeConfig,
    core::{BootContext, ServiceProvider},
    http::{
        HttpServiceProvider,
        axum::{
            extract::{Path, ws::WebSocketUpgrade},
            http::HeaderMap,
            response::{IntoResponse, Response},
        },
    },
    prelude::{Inject, Json, Realtime},
    realtime::RealtimeServiceProvider,
};
use serde_json::{Value, json};

struct RealtimeNode;

struct TokenAuthenticator;

impl Authenticator for TokenAuthenticator {
    fn authenticate<'a>(
        &'a self,
        credential: AuthCredential<'a>,
    ) -> BoxFuture<'a, Result<AuthPrincipal>> {
        Box::pin(async move {
            let Some(value) = credential
                .value()
                .and_then(|value| value.strip_prefix("user-"))
            else {
                return Err(Error::Unauthorized);
            };
            let user_id = value.parse::<i64>().map_err(|_| Error::Unauthorized)?;
            if user_id <= 0 {
                return Err(Error::Unauthorized);
            }
            Ok(AuthPrincipal::new(
                user_id,
                Some(vec![
                    "realtime:connect".into(),
                    "realtime:publish".into(),
                    "realtime:revoke".into(),
                ]),
            ))
        })
    }
}

impl Application for RealtimeNode {
    type Config = NoConfig;

    fn providers(_config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let redis_url = std::env::var("OPENRUST_REALTIME_REDIS_URL")
            .unwrap_or_else(|_| "redis://127.0.0.1:6379/".into());
        let config = RealtimeConfig::new()
            .with_redis_url(redis_url)
            .with_allowed_origins(["http://localhost".to_owned(), "http://127.0.0.1".to_owned()]);
        vec![
            Box::new(HttpServiceProvider),
            Box::new(AuthServiceProvider::with_authenticator(TokenAuthenticator)),
            Box::new(RealtimeServiceProvider::with_config(config)),
            Box::new(RealtimeRoutes),
        ]
    }
}

struct RealtimeRoutes;
impl ServiceProvider for RealtimeRoutes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes
                    .get("/realtime/ws", websocket)
                    .get("/realtime/sse", sse)
                    .post("/realtime/send/{user_id}", send)
                    .post("/realtime/revoke/{user_id}", revoke)
                    .layer(AuthLayer::required(AuthMode::PersonalAccessToken))
            })?;
            Ok(())
        })
    }
}

async fn websocket(
    headers: HeaderMap,
    user: CurrentUser,
    Inject(realtime): Inject<Realtime>,
    upgrade: WebSocketUpgrade,
) -> Result<Response> {
    require(&user, "realtime:connect")?;
    realtime.validate_origin(&headers)?;
    let session = realtime
        .connect(&user, "notifications", authorized_until())
        .await?;
    Ok(session.websocket(upgrade))
}

async fn sse(
    headers: HeaderMap,
    user: CurrentUser,
    Inject(realtime): Inject<Realtime>,
) -> Result<Response> {
    require(&user, "realtime:connect")?;
    realtime.validate_origin(&headers)?;
    let session = realtime
        .connect(&user, "notifications", authorized_until())
        .await?;
    Ok(session.sse().into_response())
}

async fn send(
    user: CurrentUser,
    Inject(realtime): Inject<Realtime>,
    Path(user_id): Path<i64>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>> {
    require(&user, "realtime:publish")?;
    let local_deliveries = realtime
        .send_to_user(user_id, "notifications", &payload, Duration::from_secs(10))
        .await?;
    Ok(Json(json!({"local_deliveries": local_deliveries})))
}

async fn revoke(
    user: CurrentUser,
    Inject(realtime): Inject<Realtime>,
    Path(user_id): Path<i64>,
) -> Result<Json<Value>> {
    require(&user, "realtime:revoke")?;
    let local_closed = realtime.revoke_user(user_id).await?;
    Ok(Json(json!({"local_closed": local_closed})))
}

fn require(user: &CurrentUser, ability: &str) -> Result<()> {
    if user.has_ability(ability) {
        Ok(())
    } else {
        Err(Error::Forbidden("无权执行实时操作".into()))
    }
}

fn authorized_until() -> SystemTime {
    SystemTime::now() + Duration::from_secs(300)
}

fn main() -> std::process::ExitCode {
    openrust::run::<RealtimeNode>()
}
