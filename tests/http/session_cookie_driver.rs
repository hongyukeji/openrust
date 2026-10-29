//! 内置 Cookie Session 的公开驱动与正式 Provider 链；oneshot 不代替 TCP/TLS。
#![cfg(feature = "session")]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use openrust::http::{
    Cookie, CookieFactory, CookieJar, CookieOptions, CookieServiceProvider, HttpServiceProvider,
    Inject, Json,
};
use openrust::session::{
    CookieSessionStore, DEFAULT_MAX_BYTES, MemorySessionStore, Session, SessionId, SessionRecord,
    SessionRequestContext, SessionServiceProvider, SessionStore, SessionStoreGuarantees,
    SessionStoreHandle, SessionWriteMode, SessionWriteOutcome,
};
use openrust::{
    App, Error, Result, ServiceExt,
    core::{BootContext, BoxFuture, RegisterContext, ServiceProvider},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use time::{Duration, OffsetDateTime};

const NAME: &str = "cookie_driver_id";
fn fail() -> Error {
    Error::custom("cookie_driver_test_failed", "Cookie driver 测试断言失败")
}
fn context(cookies: Value, jar: CookieJar) -> Result<SessionRequestContext> {
    SessionRequestContext::new(cookies, jar)
}
fn jar() -> CookieJar {
    CookieJar::new(CookieFactory::default())
}
fn code<T>(result: Result<T>) -> Option<String> {
    result.err().map(|error| error.code().to_owned())
}
fn record(id: SessionId, data: Value) -> SessionRecord {
    SessionRecord {
        id,
        data,
        expiry_date: OffsetDateTime::now_utc() + Duration::minutes(2),
    }
}

#[derive(Default)]
struct Captured {
    session: Mutex<Option<Session>>,
    jar: Mutex<Option<CookieJar>>,
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}
struct Routes(Arc<Captured>);
impl ServiceProvider for Routes {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.instance(self.0.clone());
        Ok(())
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            #[cfg(feature = "auth")]
            app.routes(|routes| routes.get("/cookie/auth-save-failed", auth_save_failed))?;
            app.routes(|routes| {
                routes
                    .get("/cookie/one", one)
                    .get("/cookie/two", two)
                    .get("/cookie/read", read)
                    .get("/cookie/renew", renew)
                    .get("/cookie/logout", logout)
                    .get("/cookie/cancel", cancel)
                    .get("/cookie/renew-failed", renew_failed)
            })?;
            Ok(())
        })
    }
}
async fn one(session: Session) -> Result<Json<Value>> {
    session.put("value", "one")?;
    read(session).await
}
async fn two(session: Session) -> Result<Json<Value>> {
    session.put("value", "two")?;
    read(session).await
}
async fn read(session: Session) -> Result<Json<Value>> {
    Ok(Json(
        json!({"value":session.get("value")?,"snapshot":session.handler()?.guarantees()==SessionStoreGuarantees::ClientSideSnapshot}),
    ))
}
async fn renew(session: Session) -> Result<Json<Value>> {
    session.migrate(true).await?;
    read(session).await
}
async fn logout(session: Session) -> Result<Json<Value>> {
    session.invalidate().await?;
    read(session).await
}
async fn renew_failed(session: Session) -> Result<Json<Value>> {
    session.migrate(true).await?;
    session.handler()?.delete(&session.id()?).await?;
    read(session).await
}
async fn cancel(
    session: Session,
    jar: CookieJar,
    Inject(captured): Inject<Captured>,
) -> Result<Json<Value>> {
    session.put("value", "cancelled")?;
    *captured.session.lock().map_err(|_| fail())? = Some(session);
    *captured.jar.lock().map_err(|_| fail())? = Some(jar);
    if let Some(entered) = captured.entered.lock().map_err(|_| fail())?.take() {
        entered.send(()).map_err(|()| fail())?;
    }
    std::future::pending().await
}
async fn app(expire_on_close: bool, protected: bool, active: &str) -> Result<(App, Arc<Captured>)> {
    let mut config = openrust::config::FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let mut builder = openrust::core::ProviderBuilder::new();
    builder.provider(HttpServiceProvider);
    #[cfg(feature = "encryption")]
    if protected {
        builder
            .provider(openrust::security::EncryptionServiceProvider)
            .provider(CookieServiceProvider::new());
    } else {
        builder.provider(CookieServiceProvider::plain());
    }
    #[cfg(not(feature = "encryption"))]
    {
        if protected {
            return Err(fail());
        }
        builder.provider(CookieServiceProvider::plain());
    }
    builder.provider(SessionServiceProvider::new().with_cookie_config(
        openrust::config::SessionConfig {
            driver: Some("cookie".into()),
            cookie: Some(NAME.into()),
            lifetime: 2,
            expire_on_close,
            secure: Some(false),
            http_only: false,
            partitioned: true,
            ..Default::default()
        },
    ));
    let captured = Arc::new(Captured::default());
    builder.provider(Routes(captured.clone()));
    let source = figment::Figment::from(figment::providers::Serialized::defaults(config)).merge(
        figment::providers::Serialized::defaults(
            json!({"security":{"encryption":{"active_key_id":active,"keys":{
                "old":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=",
                "new":"KysrKysrKysrKysrKysrKysrKysrKysrKysrKysrKys="
            }}}}),
        ),
    );
    Ok((
        App::from_provider_runtime(builder.start(source).await?)?,
        captured,
    ))
}
async fn send(app: &App, path: &str, cookies: Option<&str>) -> Result<Response> {
    let mut request = Request::get(path).header("host", "localhost");
    if let Some(cookies) = cookies {
        request = request.header("cookie", cookies);
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        openrust::http::build_router(app)?
            .oneshot(request.body(Body::empty()).map_err(Error::internal)?)
            .await
            .map_err(Error::internal)
    })
    .await
    .map_err(Error::internal)?
}
fn cookies(response: &Response) -> Result<Vec<Cookie>> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|header| Cookie::from_header(header.to_str().map_err(Error::internal)?))
        .collect()
}
fn wire(response: &Response) -> Result<String> {
    let mut pairs = Vec::new();
    for header in response.headers().get_all("set-cookie") {
        let text = header.to_str().map_err(Error::internal)?;
        let modeled = Cookie::from_header(text)?;
        if modeled
            .expires()
            .is_some_and(|expiry| expiry < OffsetDateTime::now_utc())
        {
            continue;
        }
        pairs.push(text.split(';').next().ok_or_else(fail)?.to_owned());
    }
    Ok(pairs.join("; "))
}
async fn decoded_wire(response: &Response) -> Result<openrust::http::RequestInfo> {
    use axum::extract::FromRequestParts;
    let (mut parts, _) = Request::get("/")
        .header("host", "localhost")
        .header("cookie", wire(response)?)
        .body(Body::empty())
        .map_err(Error::internal)?
        .into_parts();
    openrust::http::RequestInfo::from_request_parts(&mut parts, &()).await
}
async fn success(response: Response, value: Value) -> Result<()> {
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096)
        .await
        .map_err(Error::internal)?;
    assert_eq!(
        serde_json::from_slice::<Value>(&body).map_err(Error::internal)?,
        json!({"success":true,"code":"ok","message":"ok","data":{"value":value,"snapshot":true}})
    );
    Ok(())
}
async fn finish(app: &App, outcome: Result<()>) -> Result<()> {
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    outcome
}

#[tokio::test]
async fn provider_cookie_driver_uses_two_cookies_with_distinct_security_options() -> Result<()> {
    for expire_on_close in [false, true] {
        let (app, _) = app(expire_on_close, false, "old").await?;
        let outcome = async {
            let first = send(&app, "/cookie/one", None).await?;
            let modeled = cookies(&first)?;
            assert_eq!(modeled.len(), 2);
            let id = modeled
                .iter()
                .find(|cookie| cookie.name() == NAME)
                .ok_or_else(fail)?;
            let data = modeled
                .iter()
                .find(|cookie| cookie.name() != NAME)
                .ok_or_else(fail)?;
            assert!(!id.http_only());
            assert!(data.http_only());
            let headers = first
                .headers()
                .get_all("set-cookie")
                .iter()
                .map(|header| header.to_str().map_err(Error::internal))
                .collect::<Result<Vec<_>>>()?;
            assert!(headers.iter().any(|header| header.starts_with(NAME)
                && header.to_ascii_lowercase().contains("partitioned")));
            assert!(headers.iter().any(|header| !header.starts_with(NAME)
                && !header.to_ascii_lowercase().contains("partitioned")));
            assert_eq!(id.expires().is_none(), expire_on_close);
            assert_eq!(data.expires().is_none(), expire_on_close);
            // Set-Cookie 模型保留 raw 百分编码；公开 RequestInfo 执行入站解码。
            let decoded = decoded_wire(&first).await?;
            let payload: Value =
                serde_json::from_value::<String>(decoded.cookie(data.name()).ok_or_else(fail)?)
                    .map_err(Error::internal)
                    .and_then(|text| serde_json::from_str(&text).map_err(Error::internal))?;
            assert!(payload["data"].is_string());
            assert!(payload["expires"].is_i64());
            assert_eq!(
                serde_json::from_str::<Value>(payload["data"].as_str().ok_or_else(fail)?)
                    .map_err(Error::internal)?["value"],
                "one"
            );
            let saved = wire(&first)?;
            success(first, json!("one")).await?;
            success(
                send(&app, "/cookie/read", Some(&saved)).await?,
                json!("one"),
            )
            .await
        }
        .await;
        finish(&app, outcome).await?;
    }
    Ok(())
}

#[tokio::test]
async fn cookie_driver_current_logout_and_migration_do_not_claim_replay_revocation() -> Result<()> {
    let (app, _) = app(false, false, "old").await?;
    let outcome = async {
        let first = send(&app, "/cookie/one", None).await?;
        let old = wire(&first)?;
        let original_id = cookies(&first)?
            .into_iter()
            .find(|cookie| cookie.name() == NAME)
            .and_then(|cookie| cookie.value().map(str::to_owned))
            .ok_or_else(fail)?;
        let old_data = cookies(&first)?
            .into_iter()
            .find(|cookie| cookie.name() != NAME)
            .ok_or_else(fail)?;
        let moved = send(&app, "/cookie/renew", Some(&old)).await?;
        let deleted_header = moved
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|header| header.to_str().ok())
            .find(|header| {
                header
                    .split(';')
                    .next()
                    .and_then(|pair| pair.split_once('='))
                    .is_some_and(|(name, _)| name == old_data.name())
            })
            .ok_or_else(fail)?;
        let deleted = Cookie::from_header(deleted_header)?;
        assert_eq!(deleted.value(), Some("deleted"));
        assert!(
            deleted
                .expires()
                .is_some_and(|expiry| expiry < OffsetDateTime::now_utc())
        );
        assert!(deleted_header.to_ascii_lowercase().contains("; max-age=0"));
        let moved_id = cookies(&moved)?
            .into_iter()
            .find(|cookie| cookie.name() == NAME)
            .and_then(|cookie| cookie.value().map(str::to_owned))
            .ok_or_else(fail)?;
        assert_ne!(
            SessionId::parse(&original_id)?,
            SessionId::parse(&moved_id)?
        );
        let moved_jar = wire(&moved)?;
        success(moved, json!("one")).await?;
        success(
            send(&app, "/cookie/read", Some(&moved_jar)).await?,
            json!("one"),
        )
        .await?;
        let logged_out = send(&app, "/cookie/logout", Some(&moved_jar)).await?;
        let current = wire(&logged_out)?;
        success(logged_out, Value::Null).await?;
        success(
            send(&app, "/cookie/read", Some(&current)).await?,
            Value::Null,
        )
        .await?;
        // 另一个客户端持有的旧双 Cookie 仍可重放；这正是公开 Snapshot 限制。
        success(send(&app, "/cookie/read", Some(&old)).await?, json!("one")).await?;
        let second = send(&app, "/cookie/two", None).await?;
        let other = wire(&second)?;
        success(
            send(&app, "/cookie/read", Some(&other)).await?,
            json!("two"),
        )
        .await?;
        success(send(&app, "/cookie/read", Some(&old)).await?, json!("one")).await
    }
    .await;
    finish(&app, outcome).await
}

#[tokio::test]
async fn public_cookie_store_reads_own_writes_deletes_locally_and_reports_capability() -> Result<()>
{
    let prototype = CookieSessionStore::new(2, false)?;
    assert_eq!(
        prototype.guarantees(),
        SessionStoreGuarantees::ClientSideSnapshot
    );
    assert_eq!(
        MemorySessionStore::default().guarantees(),
        SessionStoreGuarantees::ServerSide
    );
    let id = SessionId::generate()?;
    let value = record(id.clone(), json!({"value":"owned"}));
    assert_eq!(
        code(
            prototype
                .load(&id, DEFAULT_MAX_BYTES, value.expiry_date)
                .await
        )
        .as_deref(),
        Some("session_request_unbound")
    );
    let jar = jar();
    let bound = prototype
        .bind_request(context(json!({}), jar.clone())?)?
        .ok_or_else(fail)?;
    assert_eq!(
        bound
            .write(&value, SessionWriteMode::Insert, DEFAULT_MAX_BYTES)
            .await?,
        SessionWriteOutcome::Written
    );
    assert_eq!(
        bound
            .load(&id, DEFAULT_MAX_BYTES, value.expiry_date)
            .await?
            .ok_or_else(fail)?
            .data,
        value.data
    );
    bound.delete(&id).await?;
    assert!(
        bound
            .load(&id, DEFAULT_MAX_BYTES, value.expiry_date)
            .await?
            .is_none()
    );
    assert_eq!(
        bound
            .write(&value, SessionWriteMode::Update, DEFAULT_MAX_BYTES)
            .await?,
        SessionWriteOutcome::Missing
    );
    assert!(jar.queued(id.as_str(), None)?.is_some());
    let handle = SessionStoreHandle::new(CookieSessionStore::new(2, false)?);
    assert_eq!(
        handle.guarantees(),
        SessionStoreGuarantees::ClientSideSnapshot
    );
    assert_eq!(
        code(handle.delete(id.as_str()).await).as_deref(),
        Some("session_store_guarantee_unsupported")
    );
    Ok(())
}

async fn decode_case(payload: Value) -> Result<Option<SessionRecord>> {
    let id = SessionId::generate()?;
    let mut cookies = json!({});
    cookies[id.as_str()] = payload;
    let bound = CookieSessionStore::new(2, false)?
        .bind_request(context(cookies, jar())?)?
        .ok_or_else(fail)?;
    bound
        .load(
            &id,
            DEFAULT_MAX_BYTES,
            OffsetDateTime::now_utc() + Duration::minutes(2),
        )
        .await
}
#[tokio::test]
async fn malformed_cookie_envelope_and_data_follow_the_documented_error_boundaries() -> Result<()> {
    let future = OffsetDateTime::now_utc().unix_timestamp() + 60;
    for payload in [
        json!("not json"),
        json!("[]"),
        json!(json!({"data":"[]"}).to_string()),
        json!(json!({"expires":null,"data":"[]"}).to_string()),
        json!(json!({"expires":future-120,"data":"[]"}).to_string()),
    ] {
        assert!(decode_case(payload).await?.is_none());
    }
    for payload in [
        json!(json!({"expires":"soon","data":"[]"}).to_string()),
        json!(json!({"expires":1.5,"data":"[]"}).to_string()),
        json!(json!({"expires":future}).to_string()),
        json!(json!({"expires":future,"data":17}).to_string()),
        json!(json!({"expires":future,"data":null}).to_string()),
        json!(json!({"expires":i64::MAX,"data":"[]"}).to_string()),
    ] {
        assert_eq!(
            code(decode_case(payload).await).as_deref(),
            Some("session_cookie_payload_invalid")
        );
    }
    for data in ["bad json", "17", "null", "false"] {
        assert_eq!(
            decode_case(json!(json!({"expires":future,"data":data}).to_string()))
                .await?
                .ok_or_else(fail)?
                .data,
            json!([])
        );
    }
    Ok(())
}

#[tokio::test]
async fn failed_cookie_write_budget_queues_nothing_and_cancelled_handler_does_not_save()
-> Result<()> {
    let jar = jar();
    let bound = CookieSessionStore::new(2, false)?
        .bind_request(context(json!({}), jar.clone())?)?
        .ok_or_else(fail)?;
    let value = record(
        SessionId::generate()?,
        json!({"large":"x".repeat(DEFAULT_MAX_BYTES)}),
    );
    assert_eq!(
        code(
            bound
                .write(&value, SessionWriteMode::Insert, DEFAULT_MAX_BYTES)
                .await
        )
        .as_deref(),
        Some("session_limits_exceeded")
    );
    assert!(jar.get_queued_cookies()?.is_empty());
    let (app, captured) = app(false, false, "old").await?;
    let outcome = async {
        let (entered, observed) = tokio::sync::oneshot::channel();
        *captured.entered.lock().map_err(|_| fail())? = Some(entered);
        let mut task = Box::pin(send(&app, "/cookie/cancel", None));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::select! {
                result=task.as_mut()=>{ result?; Err(fail()) },
                result=observed=>result.map_err(Error::internal),
            }
        })
        .await
        .map_err(Error::internal)??;
        drop(task);
        assert_eq!(
            code(
                captured
                    .session
                    .lock()
                    .map_err(|_| fail())?
                    .as_ref()
                    .ok_or_else(fail)?
                    .set_id(None)
            )
            .as_deref(),
            Some("session_closed")
        );
        let queued = captured
            .jar
            .lock()
            .map_err(|_| fail())?
            .as_ref()
            .ok_or_else(fail)?
            .get_queued_cookies()?;
        assert!(queued.is_empty());
        Ok(())
    }
    .await;
    finish(&app, outcome).await
}

#[cfg(feature = "encryption")]
#[tokio::test]
async fn protected_cookie_session_roundtrips_with_historical_keys_and_rejects_tampering()
-> Result<()> {
    let (old, _) = app(false, true, "old").await?;
    let (new, _) = app(false, true, "new").await?;
    let outcome = async {
        let response = send(&old, "/cookie/one", None).await?;
        let saved = wire(&response)?;
        let data_name = cookies(&response)?
            .into_iter()
            .find(|cookie| cookie.name() != NAME)
            .map(|cookie| cookie.name().to_owned())
            .ok_or_else(fail)?;
        let decoded = decoded_wire(&response).await?;
        let protected_id = decoded
            .cookie(NAME)
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(fail)?;
        let id_is_opaque = protected_id != data_name;
        assert!(id_is_opaque);
        assert!(SessionId::parse(&protected_id).is_err());
        let opaque = decoded
            .cookie(&data_name)
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(fail)?;
        let exposed = serde_json::from_str::<Value>(&opaque).ok();
        assert!(
            exposed.is_none_or(|value| !value["data"].is_string() || !value["expires"].is_i64())
        );
        assert!(!opaque.contains("\"value\":\"one\""));
        success(
            send(&new, "/cookie/read", Some(&saved)).await?,
            json!("one"),
        )
        .await?;
        for target in [NAME, data_name.as_str()] {
            let tampered = saved
                .split("; ")
                .map(|pair| {
                    pair.split_once('=').map_or_else(
                        || pair.to_owned(),
                        |(name, value)| {
                            if name == target {
                                format!("{name}={value}x")
                            } else {
                                pair.to_owned()
                            }
                        },
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            success(
                send(&new, "/cookie/read", Some(&tampered)).await?,
                Value::Null,
            )
            .await?;
        }
        Ok(())
    }
    .await;
    let first = finish(&old, Ok(())).await;
    let second = finish(&new, outcome).await;
    first?;
    second
}

#[tokio::test]
async fn queued_cookie_origin_survives_explicit_transfer_and_repeated_start() -> Result<()> {
    let queue = jar();
    let session = Session::new(None, Arc::new(CookieSessionStore::new(2, false)?), None)?;
    session
        .set_request_on_handler(context(json!({}), queue.clone())?)
        .await?;
    session.start().await?;
    session
        .put("value", "nonempty")?
        .put("other", 7)?
        .save()
        .await?;
    let original = session.id()?;
    let loaded = session
        .handler()?
        .load(&original, DEFAULT_MAX_BYTES, session.expiry_date()?)
        .await?
        .ok_or_else(fail)?;
    assert_eq!(loaded.data["value"], "nonempty");
    assert_eq!(loaded.data["other"], 7);
    session.set_id(Some(&"X".repeat(40)))?;
    session.save().await?;
    session.start().await?;
    assert_eq!(session.get("value")?, Some(json!("nonempty")));
    assert_eq!(session.get("other")?, Some(json!(7)));
    session.migrate(true).await?;
    session.save().await?;
    session.start().await?;
    assert_eq!(session.get("value")?, Some(json!("nonempty")));
    assert_eq!(session.get("other")?, Some(json!(7)));
    Ok(())
}

#[tokio::test]
async fn cookie_delete_uses_exact_path_and_failed_migration_flushes_all_response_cookies()
-> Result<()> {
    let mut factory = CookieFactory::default();
    factory.set_default_path_and_domain("/scope".into(), None, Some(false), None)?;
    let queue = CookieJar::new(factory);
    let id = SessionId::generate()?;
    let alternate =
        CookieFactory::default().make(id.as_str(), "unrelated", 0, CookieOptions::default())?;
    queue.queue(alternate)?;
    let bound = CookieSessionStore::new(2, false)?
        .bind_request(context(json!({}), queue.clone())?)?
        .ok_or_else(fail)?;
    let value = record(id.clone(), json!({"value":"owned"}));
    bound
        .write(&value, SessionWriteMode::Insert, DEFAULT_MAX_BYTES)
        .await?;
    bound.delete(&id).await?;
    assert_eq!(
        queue
            .queued(id.as_str(), Some("/"))?
            .and_then(|cookie| cookie.value().map(str::to_owned)),
        Some("unrelated".into())
    );
    assert!(
        queue
            .queued(id.as_str(), Some("/scope"))?
            .ok_or_else(fail)?
            .value()
            .is_none_or(str::is_empty)
    );
    let (app, _) = app(false, false, "old").await?;
    let outcome = async {
        let first = send(&app, "/cookie/one", None).await?;
        let saved = wire(&first)?;
        let failed = send(&app, "/cookie/renew-failed", Some(&saved)).await?;
        assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            failed
                .headers()
                .get_all("set-cookie")
                .iter()
                .next()
                .is_none()
        );
        let bytes = to_bytes(failed.into_body(), 4096)
            .await
            .map_err(Error::internal)?;
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).map_err(Error::internal)?["code"],
            "session_store_write_conflict"
        );
        success(
            send(&app, "/cookie/read", Some(&saved)).await?,
            json!("one"),
        )
        .await
    }
    .await;
    finish(&app, outcome).await
}

#[cfg(feature = "auth")]
async fn auth_save_failed(session: Session, jar: CookieJar) -> Result<Response> {
    session.put("openrust.auth.user_id", 42_i64)?;
    jar.queue(CookieFactory::default().make(
        "queued_private",
        "secret",
        0,
        CookieOptions::default(),
    )?)?;
    Response::builder()
        .status(StatusCode::OK)
        .header("cache-control", "no-store")
        .header("pragma", "no-cache")
        .header("set-cookie", "downstream_private=secret")
        .header("content-length", "17")
        .body(Body::from("downstream-secret"))
        .map_err(Error::internal)
}

#[cfg(feature = "auth")]
#[tokio::test]
async fn plain_cookie_auth_save_error_preserves_privacy_and_discards_downstream_response()
-> Result<()> {
    let (app, _) = app(false, false, "old").await?;
    let outcome = async {
        let response = send(&app, "/cookie/auth-save-failed", None).await?;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            response
                .headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok()),
            Some("no-store")
        );
        assert_eq!(
            response
                .headers()
                .get("pragma")
                .and_then(|v| v.to_str().ok()),
            Some("no-cache")
        );
        assert!(response.headers().get("set-cookie").is_none());
        assert_ne!(
            response
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok()),
            Some("17")
        );
        let bytes = to_bytes(response.into_body(), 4096)
            .await
            .map_err(Error::internal)?;
        let body: Value = serde_json::from_slice(&bytes).map_err(Error::internal)?;
        assert_eq!(body["success"], false);
        assert_eq!(body["code"], "session_cookie_protection_required");
        assert_eq!(body["message"], "internal server error");
        assert!(!String::from_utf8_lossy(&bytes).contains("downstream-secret"));
        Ok(())
    }
    .await;
    finish(&app, outcome).await
}
