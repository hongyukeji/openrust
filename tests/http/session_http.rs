//! 正式 Provider/Router 路径上的进程内 Session HTTP 生命周期回归。
//! 不启动监听端口；Memory store 与受控保存失败不替代双库证据。
#![cfg(feature = "session")]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Request as Wire, StatusCode},
    response::{IntoResponse, Response},
};
use openrust::config::{FrameworkConfig, HttpStatusMode, SessionConfig};
use openrust::http::{
    CookieJar, CookieOptions, CookieResponseExt, CookieServiceProvider, HttpServiceProvider,
    Inject, Json, RequestInfo, build_router,
};
use openrust::session::{
    MemorySessionStore, Session, SessionId, SessionRecord, SessionServiceProvider, SessionStore,
    SessionWriteMode, SessionWriteOutcome,
};
use openrust::{
    App, Error, Result, ServiceExt,
    core::{BootContext, BoxFuture, RegisterContext, ServiceProvider},
};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio::sync::oneshot;

#[derive(Clone, Default)]
struct Probe {
    memory: MemorySessionStore,
    writes: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
}
impl SessionStore for Probe {
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        maximum: usize,
        expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        self.memory.load(id, maximum, expiry)
    }
    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        maximum: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async move {
            self.writes.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                return Err(Error::custom_with_status(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "session_test_save_failed",
                    "受控 Session 保存失败",
                ));
            }
            self.memory.write(record, mode, maximum).await
        })
    }
    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        self.memory.delete(id)
    }
}

#[derive(Default)]
struct Control {
    leaked: Mutex<Option<Session>>,
    entered: Mutex<Option<oneshot::Sender<()>>>,
}
struct TestRoutes(Arc<Control>);
impl ServiceProvider for TestRoutes {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.instance(self.0.clone());
        Ok(())
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes
                    .get("/session-test/shared", shared)
                    .get("/session-test/flash", flash)
                    .get("/session-test/inspect", inspect)
                    .get("/session-test/handler-error", handler_error)
                    .get("/session-test/cancel", cancel)
                    .get("/session-test/unsaved", unsaved)
                    .get("/session-test/presence", presence)
                    .get("/session-test/require", require)
            })?;
            Ok(())
        })
    }
}

async fn shared(
    session: Session,
    info: RequestInfo,
    Inject(other): Inject<Session>,
) -> Result<Json<Value>> {
    session.put("test.shared.one", 1)?;
    let from_info = info.session()?;
    from_info.put("test.shared.two", 2)?;
    other.put("test.shared.three", 3)?;
    Ok(Json(json!({"has_session":info.has_session(),
        "info_reads_one":from_info.get("test.shared.one")? == Some(json!(1)),
        "injected_reads_two":other.get("test.shared.two")? == Some(json!(2)),
        "extractor_reads_three":session.get("test.shared.three")? == Some(json!(3))})))
}
async fn flash(session: Session) -> Result<Json<Value>> {
    session
        .flash("test.notice", "next")?
        .now("test.instant", "current")?;
    Ok(Json(json!({"created":true})))
}
async fn inspect(session: Session) -> Result<Json<Value>> {
    Ok(Json(
        json!({"notice":session.get("test.notice")?, "instant":session.get("test.instant")?, "after_error":session.get("test.after_error")?}),
    ))
}
async fn handler_error(session: Session) -> Result<Json<Value>> {
    session.put("test.after_error", "persisted")?;
    Err(Error::custom_with_status(
        StatusCode::INTERNAL_SERVER_ERROR,
        "session_test_handler_failed",
        "受控业务失败",
    ))
}
async fn cancel(session: Session, Inject(control): Inject<Control>) -> Result<Json<Value>> {
    session.put("test.cancelled", "not persisted")?;
    *control.leaked.lock().map_err(|_| invalid())? = Some(session);
    if let Some(entered) = control.entered.lock().map_err(|_| invalid())?.take() {
        entered.send(()).map_err(|()| invalid())?;
    }
    std::future::pending().await
}
async fn unsaved(session: Session, jar: CookieJar) -> Result<impl IntoResponse> {
    session.put("test.unsaved", "private original body")?;
    jar.queue(
        jar.factory()
            .make("queued_test", "must not leave", 0, CookieOptions::default())?,
    )?;
    let direct =
        jar.factory()
            .make("direct_test", "must not leave", 0, CookieOptions::default())?;
    Json(json!({"original":"must be replaced"})).with_cookie(direct)
}
async fn presence(info: RequestInfo) -> Json<Value> {
    Json(json!({"has_session":info.has_session()}))
}
async fn require(info: RequestInfo) -> Result<Json<Value>> {
    info.session()?;
    Ok(Json(json!({"unreachable":true})))
}

fn invalid() -> Error {
    Error::custom("session_http_test_failed", "Session HTTP 生命周期断言失败")
}
fn check(condition: bool) -> Result<()> {
    if condition { Ok(()) } else { Err(invalid()) }
}

async fn app(enabled: bool, probe: Probe, control: Arc<Control>) -> Result<App> {
    let mut config = FrameworkConfig::default();
    config.api.http_status = HttpStatusMode::Semantic;
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(CookieServiceProvider::plain());
    if enabled {
        builder.provider(
            SessionServiceProvider::with_store(probe).with_cookie_config(SessionConfig {
                driver: Some("array".into()),
                cookie: Some("test_session".into()),
                secure: Some(false),
                ..SessionConfig::default()
            }),
        );
    }
    builder.provider(TestRoutes(control));
    App::from_provider_runtime(
        builder
            .start(figment::Figment::from(
                figment::providers::Serialized::defaults(config),
            ))
            .await?,
    )
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}
async fn send(router: axum::Router, path: &str, cookie: Option<&str>) -> Result<Reply> {
    let mut wire = Wire::get(path).header("host", "localhost");
    if let Some(cookie) = cookie {
        wire = wire.header("cookie", cookie);
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let response = router
            .oneshot(wire.body(Body::empty()).map_err(Error::internal)?)
            .await
            .map_err(Error::internal)?;
        reply(response).await
    })
    .await
    .map_err(|_| {
        Error::custom_with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session_test_timeout",
            "Session HTTP测试超过期限",
        )
    })?
}
async fn reply(response: Response) -> Result<Reply> {
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 8192)
        .await
        .map_err(Error::internal)?;
    let body = serde_json::from_slice(&bytes).map_err(Error::internal)?;
    Ok(Reply {
        status,
        headers,
        body,
    })
}
fn cookie(reply: &Reply) -> Result<String> {
    let values = reply
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|header| header.to_str().ok())
        .filter_map(|header| header.split(';').next())
        .filter(|pair| pair.starts_with("test_session="))
        .collect::<Vec<_>>();
    check(values.len() == 1)?;
    Ok(values[0].to_owned())
}
fn success(reply: &Reply, data: &Value) -> Result<()> {
    check(reply.status == StatusCode::OK)?;
    check(reply.body == json!({"success":true,"code":"ok","message":"ok","data":data}))
}
fn failure(reply: &Reply, code: &str) -> Result<()> {
    check(reply.status == StatusCode::INTERNAL_SERVER_ERROR)?;
    check(reply.body.as_object().is_some_and(|body| body.len() == 5))?;
    check(reply.body["success"] == false && reply.body["code"] == code)?;
    check(reply.body["message"] == "internal server error")?;
    check(
        reply.body["timestamp"]
            .as_str()
            .is_some_and(|value| chrono::DateTime::parse_from_rfc3339(value).is_ok()),
    )?;
    check(reply.body["request_id"].as_str().is_some_and(|value| {
        !value.is_empty()
            && reply
                .headers
                .get("x-request-id")
                .and_then(|header| header.to_str().ok())
                == Some(value)
    }))
}
async fn finish(app: &App, outcome: Result<()>) -> Result<()> {
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await?;
    outcome
}

#[tokio::test]
async fn public_extractors_share_one_handle_and_unchanged_requests_still_age_flash() -> Result<()> {
    let probe = Probe::default();
    let app = app(true, probe.clone(), Arc::new(Control::default())).await?;
    let outcome = async {
        let router = build_router(&app)?;
        let shared = send(router.clone(), "/session-test/shared", None).await?;
        success(&shared, &json!({"has_session":true,"info_reads_one":true,"injected_reads_two":true,"extractor_reads_three":true}))?;
        let cookie = cookie(&shared)?;
        let flash = send(router.clone(), "/session-test/flash", Some(&cookie)).await?;
        success(&flash, &json!({"created":true}))?;
        let before = probe.writes.load(Ordering::SeqCst);
        let second = send(router.clone(), "/session-test/inspect", Some(&cookie)).await?;
        success(&second, &json!({"notice":"next","instant":null,"after_error":null}))?;
        check(probe.writes.load(Ordering::SeqCst) == before + 1)?;
        let third = send(router, "/session-test/inspect", Some(&cookie)).await?;
        success(&third, &json!({"notice":null,"instant":null,"after_error":null}))?;
        check(probe.writes.load(Ordering::SeqCst) == before + 2)
    }.await;
    finish(&app, outcome).await
}

#[tokio::test]
async fn handler_500_still_saves_and_write_failure_replaces_body_and_all_cookies() -> Result<()> {
    let probe = Probe::default();
    let app = app(true, probe.clone(), Arc::new(Control::default())).await?;
    let outcome = async {
        let router = build_router(&app)?;
        let error = send(router.clone(), "/session-test/handler-error", None).await?;
        failure(&error, "session_test_handler_failed")?;
        let cookie = cookie(&error)?;
        check(probe.writes.load(Ordering::SeqCst) == 1)?;
        let restored = send(router.clone(), "/session-test/inspect", Some(&cookie)).await?;
        success(
            &restored,
            &json!({"notice":null,"instant":null,"after_error":"persisted"}),
        )?;
        probe.fail.store(true, Ordering::SeqCst);
        let failed = send(router, "/session-test/unsaved", Some(&cookie)).await?;
        failure(&failed, "session_test_save_failed")?;
        check(failed.body.get("data").is_none())?;
        check(!failed.body.to_string().contains("must be replaced"))?;
        check(!failed.headers.contains_key("set-cookie"))
    }
    .await;
    finish(&app, outcome).await
}

#[tokio::test]
async fn cancelled_handler_never_background_saves_and_closes_a_leaked_handle() -> Result<()> {
    let probe = Probe::default();
    let control = Arc::new(Control::default());
    let (entered, observed) = oneshot::channel();
    *control.entered.lock().map_err(|_| invalid())? = Some(entered);
    let app = app(true, probe.clone(), control.clone()).await?;
    let outcome = async {
        let router = build_router(&app)?;
        let task = tokio::spawn(send(router.clone(), "/session-test/cancel", None));
        let observed = tokio::time::timeout(std::time::Duration::from_secs(2), observed).await;
        if !matches!(observed, Ok(Ok(()))) {
            task.abort();
            let _ = task.await;
            return Err(invalid());
        }
        task.abort();
        check(task.await.err().is_some_and(|error| error.is_cancelled()))?;
        check(probe.writes.load(Ordering::SeqCst) == 0)?;
        let leaked = control
            .leaked
            .lock()
            .map_err(|_| invalid())?
            .take()
            .ok_or_else(invalid)?;
        check(
            leaked
                .put("late", true)
                .err()
                .is_some_and(|error| error.code() == "session_closed"),
        )?;
        check(
            leaked
                .save()
                .await
                .err()
                .is_some_and(|error| error.code() == "session_closed"),
        )?;
        let fresh = send(router, "/session-test/inspect", None).await?;
        success(
            &fresh,
            &json!({"notice":null,"instant":null,"after_error":null}),
        )?;
        check(probe.writes.load(Ordering::SeqCst) == 1)
    }
    .await;
    finish(&app, outcome).await
}

#[tokio::test]
async fn missing_provider_has_no_implicit_memory_and_session_access_fails_safely() -> Result<()> {
    let probe = Probe::default();
    let app = app(false, probe.clone(), Arc::new(Control::default())).await?;
    let outcome = async {
        let router = build_router(&app)?;
        let absent = send(router.clone(), "/session-test/presence", None).await?;
        success(&absent, &json!({"has_session":false}))?;
        check(!absent.headers.contains_key("set-cookie"))?;
        for path in ["/session-test/require", "/session-test/shared"] {
            let required = send(router.clone(), path, None).await?;
            failure(&required, "session_layer_missing")?;
            check(!required.headers.contains_key("set-cookie"))?;
        }
        check(probe.writes.load(Ordering::SeqCst) == 0)
    }
    .await;
    finish(&app, outcome).await
}
