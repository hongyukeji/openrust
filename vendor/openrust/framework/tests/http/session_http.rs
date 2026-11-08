//! 正式 Provider/Router 路径上的进程内 Session HTTP 生命周期回归。
//! 不启动监听端口；Memory store 与受控保存失败不替代双库证据。
#![cfg(feature = "session")]

#[cfg(feature = "encryption")]
mod forgery;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request as Wire, StatusCode},
    response::{IntoResponse, Response},
};
use openrust::Application;
use openrust::Error;
use openrust::Result;
use openrust::ServiceExt;
use openrust::config::{FrameworkConfig, HttpStatusMode, SessionConfig};
use openrust::foundation::BootContext;
use openrust::foundation::RegisterContext;
use openrust::foundation::http::build_router;
use openrust::http::CookieJar;
use openrust::http::CookieOptions;
use openrust::http::CookieResponseExt;
use openrust::http::CookieServiceProvider;
use openrust::http::HttpServiceProvider;
use openrust::http::Inject;
use openrust::http::Json;
use openrust::http::Path;
use openrust::http::RequestInfo;
use openrust::http::Validated;
use openrust::http::handle_precognitive_requests;
use openrust::routing::Route;
use openrust::routing::RouteGroup;
use openrust::session::{
    MemorySessionStore, Session, SessionId, SessionRecord, SessionServiceProvider, SessionStore,
    SessionWriteMode, SessionWriteOutcome,
};
use openrust::support::{BoxFuture, ServiceProvider};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio::sync::{Notify, oneshot};

#[derive(Clone, Default)]
struct Probe {
    memory: MemorySessionStore,
    writes: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
    loads: Arc<AtomicUsize>,
    collections: Arc<AtomicUsize>,
    pending: Arc<AtomicBool>,
    write_entered: Arc<Notify>,
    blocked_write: Arc<AtomicBool>,
    write_released: Arc<Notify>,
}
impl SessionStore for Probe {
    fn gc(&self, _: i64) -> BoxFuture<'_, Result<u64>> {
        self.collections.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(0) })
    }
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        maximum: usize,
        expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        self.loads.fetch_add(1, Ordering::SeqCst);
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
            if self.pending.load(Ordering::SeqCst) {
                self.write_entered.notify_one();
                return std::future::pending().await;
            }
            if self.blocked_write.load(Ordering::SeqCst) {
                self.write_entered.notify_one();
                self.write_released.notified().await;
            }
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
    jar: Mutex<Option<CookieJar>>,
    business: AtomicUsize,
    resolver_calls: AtomicUsize,
}
struct TestRoutes(Arc<Control>, bool);
impl ServiceProvider for TestRoutes {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(self.0.clone()).await?;
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let enabled = self.1;
            app.exceptions(|handler| {
                handler.api_envelope_when(|request, _| {
                    request.path().starts_with("/history/")
                        || request.path().starts_with("/session-test/")
                });
            })?;
            app.routes(move |routes| {
                let add = |routes: openrust::routing::Router| {
                    routes
                        .get("/session-test/shared", shared)
                        .get("/session-test/flash", flash)
                        .get("/session-test/inspect", inspect)
                        .get("/session-test/handler-error", handler_error)
                        .get("/session-test/cancel", cancel)
                        .get("/session-test/unsaved", unsaved)
                        .get("/session-test/presence", presence)
                        .get("/session-test/require", require)
                        .get("/history/named", history)
                        .name("history.named")
                        .get("/history/anonymous", history)
                        .post("/history/read", history)
                        .get("/history/business-error", handler_error)
                        .name("history.error")
                        .get("/history/resolver-error", resolver_error)
                        .get("/history/no-route", no_route)
                        .get("/history/resolver/{mode}", changing_resolver)
                        .get("/history/invalid-cookie", invalid_cookie)
                        .group(
                            RouteGroup::new().prefix("/history/confirmed").middleware(
                                axum::middleware::from_fn(handle_precognitive_requests),
                            ),
                            |routes| {
                                routes
                                    .get("/read", history)
                                    .post("/validate", history_validated)
                                    .group(
                                        RouteGroup::new()
                                            .middleware(axum::middleware::from_fn(deny)),
                                        |routes| {
                                            routes.get("/denied", history).name("history.denied")
                                        },
                                    )
                            },
                        )
                };
                if enabled {
                    routes.group(
                        RouteGroup::new()
                            .middleware_alias("cookies")
                            .middleware_alias("session"),
                        add,
                    )
                } else {
                    add(routes)
                }
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
    session.put_typed("test.shared.one", 1)?;
    let from_info = info.session()?;
    from_info.put_typed("test.shared.two", 2)?;
    other.put_typed("test.shared.three", 3)?;
    Ok(Json(json!({"has_session":info.has_session(),
        "info_reads_one":from_info.get_json("test.shared.one")? == Some(json!(1)),
        "injected_reads_two":other.get_json("test.shared.two")? == Some(json!(2)),
        "extractor_reads_three":session.get_json("test.shared.three")? == Some(json!(3))})))
}
async fn flash(session: Session) -> Result<Json<Value>> {
    session
        .flash_typed("test.notice", "next")?
        .now_typed("test.instant", "current")?;
    Ok(Json(json!({"created":true})))
}
async fn inspect(session: Session) -> Result<Json<Value>> {
    Ok(Json(
        json!({"notice":session.get_json("test.notice")?, "instant":session.get_json("test.instant")?, "after_error":session.get_json("test.after_error")?}),
    ))
}
async fn handler_error(session: Session) -> Result<Json<Value>> {
    session.put_typed("test.after_error", "persisted")?;
    Err(Error::custom_with_status(
        StatusCode::INTERNAL_SERVER_ERROR,
        "session_test_handler_failed",
        "受控业务失败",
    ))
}
async fn cancel(session: Session, Inject(control): Inject<Control>) -> Result<Json<Value>> {
    session.put_typed("test.cancelled", "not persisted")?;
    *control.leaked.lock().map_err(|_| invalid())? = Some(session);
    if let Some(entered) = control.entered.lock().map_err(|_| invalid())?.take() {
        entered.send(()).map_err(|()| invalid())?;
    }
    std::future::pending().await
}
async fn unsaved(
    session: Session,
    jar: CookieJar,
    Inject(control): Inject<Control>,
) -> Result<impl IntoResponse> {
    session.put_typed("test.unsaved", "private original body")?;
    *control.leaked.lock().map_err(|_| invalid())? = Some(session.clone());
    *control.jar.lock().map_err(|_| invalid())? = Some(jar.clone());
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

async fn history(
    session: Session,
    info: RequestInfo,
    Inject(control): Inject<Control>,
) -> Result<Json<Value>> {
    control.business.fetch_add(1, Ordering::SeqCst);
    Ok(Json(json!({
        "url": session.previous_url()?, "route": session.previous_route()?,
        "route_exists": session.exists(&["_previous.route"])?,
        "attempting": info.is_attempting_precognition()?, "confirmed": info.is_precognitive()
    })))
}

#[derive(serde::Deserialize)]
struct HistoryInput {
    value: String,
}

impl openrust::validation::Validate for HistoryInput {
    fn rules() -> openrust::validation::Rules {
        use openrust::validation::{Rule, Rules};
        Rules::new().field(
            "value",
            [
                Rule::required(),
                Rule::string(),
                Rule::length(Some(3), None),
            ],
        )
    }
}

async fn history_validated(
    Inject(control): Inject<Control>,
    Validated(Json(input)): Validated<Json<HistoryInput>>,
) -> Json<String> {
    control.business.fetch_add(1, Ordering::SeqCst);
    Json(input.value)
}

async fn deny(_: axum::extract::Request, _: axum::middleware::Next) -> Response {
    Error::custom_with_status(StatusCode::FORBIDDEN, "history_denied", "拒绝执行").into_response()
}

async fn resolver_error(info: RequestInfo) -> Result<Json<Value>> {
    info.set_route_resolver(Arc::new(|| {
        Err(Error::custom_with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "history_resolver_failed",
            "private resolver details",
        ))
    }))?;
    Ok(Json(json!({"original":"must be replaced"})))
}

async fn no_route(info: RequestInfo, Inject(control): Inject<Control>) -> Result<Json<Value>> {
    info.set_route_resolver(Arc::new(move || {
        control.resolver_calls.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }))?;
    Ok(Json(json!({"unmatched":true})))
}

async fn changing_resolver(
    info: RequestInfo,
    jar: CookieJar,
    Inject(control): Inject<Control>,
    Path(mode): Path<String>,
) -> Result<Json<Value>> {
    jar.queue(
        jar.factory()
            .make("resolver_receipt", "private", 0, CookieOptions::default())?,
    )?;
    *control.jar.lock().map_err(|_| invalid())? = Some(jar);
    info.set_route_resolver(Arc::new(move || {
        let call = control.resolver_calls.fetch_add(1, Ordering::SeqCst);
        let name = if call == 0 {
            "first.route"
        } else {
            match mode.as_str() {
                "switch" => "second.route",
                "none" => return Ok(None),
                _ => {
                    return Err(Error::custom_with_status(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "history_second_resolver_failed",
                        "private resolver details",
                    ));
                }
            }
        };
        Ok(Some(Route::new(
            Some(name.into()),
            "history/resolver/{mode}",
            None,
            vec!["GET".into(), "HEAD".into()],
            Vec::new(),
        )))
    }))?;
    Ok(Json(json!({"original":"must be replaced on failure"})))
}

async fn invalid_cookie() -> Response {
    let mut response = Json(json!({"original":"must be replaced"})).into_response();
    response.headers_mut().insert(
        "set-cookie",
        axum::http::HeaderValue::from_static("bad name=private"),
    );
    response
}

fn invalid() -> Error {
    Error::custom("session_http_test_failed", "Session HTTP 生命周期断言失败")
}
fn check(condition: bool) -> Result<()> {
    if condition { Ok(()) } else { Err(invalid()) }
}

async fn app(enabled: bool, probe: Probe, control: Arc<Control>) -> Result<Application> {
    app_with_lifetime(enabled, probe, control, SessionConfig::default().lifetime).await
}

async fn app_with_lifetime(
    enabled: bool,
    probe: Probe,
    control: Arc<Control>,
    lifetime: u64,
) -> Result<Application> {
    let mut config = FrameworkConfig::default();
    config.api.http_status = HttpStatusMode::Semantic;
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(CookieServiceProvider::plain());
    if enabled {
        builder.provider(
            SessionServiceProvider::with_store(probe).with_cookie_config(SessionConfig {
                driver: Some("array".into()),
                cookie: Some("test_session".into()),
                lifetime,
                secure: Some(false),
                lottery: [1, 1],
                ..SessionConfig::default()
            }),
        );
    }
    builder.provider(TestRoutes(control, enabled));
    Application::from_provider_runtime(
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
    send_wire(router, Method::GET, path, cookie, &[], "").await
}
async fn send_wire(
    router: axum::Router,
    method: Method,
    path: &str,
    cookie: Option<&str>,
    headers: &[(&str, &str)],
    body: &str,
) -> Result<Reply> {
    let mut wire = Wire::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost");
    if let Some(cookie) = cookie {
        wire = wire.header("cookie", cookie);
    }
    for (name, value) in headers {
        wire = wire.header(*name, *value);
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let response = router
            .oneshot(
                wire.body(Body::from(body.to_owned()))
                    .map_err(Error::internal)?,
            )
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
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).map_err(Error::internal)?
    };
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
async fn finish(app: &Application, outcome: Result<()>) -> Result<()> {
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await?;
    outcome
}

async fn stored_attributes(probe: &Probe, cookie: &str) -> Result<Value> {
    let id = SessionId::parse(cookie.strip_prefix("test_session=").ok_or_else(invalid)?)?;
    let record = probe
        .memory
        .load(
            &id,
            openrust::session::DEFAULT_MAX_BYTES,
            OffsetDateTime::now_utc() + time::Duration::hours(1),
        )
        .await?
        .ok_or_else(invalid)?;
    Ok(record.data)
}

async fn stored_history(probe: &Probe, cookie: &str) -> Result<Value> {
    Ok(stored_attributes(probe, cookie)
        .await?
        .get("_previous")
        .cloned()
        .unwrap_or(Value::Null))
}

#[tokio::test]
async fn previous_history_updates_after_dispatch_and_anonymous_routes_clear_the_old_name()
-> Result<()> {
    let probe = Probe::default();
    let app = app(true, probe.clone(), Arc::new(Control::default())).await?;
    let outcome = async {
        let router = build_router(&app)?;
        let first = send(router.clone(), "/history/named?b=2&a=1", None).await?;
        assert_eq!(first.body["data"]["url"], Value::Null);
        assert_eq!(first.body["data"]["route_exists"], false);
        let cookie = cookie(&first)?;
        let expected =
            json!({"url":"http://localhost/history/named?a=1&b=2","route":"history.named"});
        assert_eq!(stored_history(&probe, &cookie).await?, expected);
        let second = send(router.clone(), "/history/anonymous", Some(&cookie)).await?;
        assert_eq!(second.body["data"]["url"], expected["url"]);
        assert_eq!(second.body["data"]["route"], "history.named");
        assert_eq!(
            stored_history(&probe, &cookie).await?,
            json!({"url":"http://localhost/history/anonymous","route":null})
        );
        let read = send_wire(
            router.clone(),
            Method::POST,
            "/history/read",
            Some(&cookie),
            &[],
            "",
        )
        .await?;
        assert_eq!(read.body["data"]["route"], Value::Null);
        assert_eq!(read.body["data"]["route_exists"], true);
        let isolated = send(router, "/history/named", None).await?;
        assert_eq!(isolated.body["data"]["url"], Value::Null);
        Ok(())
    }
    .await;
    finish(&app, outcome).await
}

#[tokio::test]
async fn business_errors_still_update_previous_but_head_ajax_prefetch_and_internal_misses_only_save()
-> Result<()> {
    let probe = Probe::default();
    let app = app(true, probe.clone(), Arc::new(Control::default())).await?;
    let outcome = async {
        let router = build_router(&app)?;
        let first = send(router.clone(), "/history/named", None).await?;
        let cookie = cookie(&first)?;
        for (path, status, name) in [
            (
                "/history/business-error",
                StatusCode::INTERNAL_SERVER_ERROR,
                "history.error",
            ),
            (
                "/history/confirmed/denied",
                StatusCode::FORBIDDEN,
                "history.denied",
            ),
        ] {
            let response = send(router.clone(), path, Some(&cookie)).await?;
            assert_eq!(response.status, status);
            assert_eq!(
                stored_history(&probe, &cookie).await?,
                json!({"url":format!("http://localhost{path}"),"route":name})
            );
        }
        let expected = stored_history(&probe, &cookie).await?;
        for (method, path, headers, status, session_access) in [
            (Method::HEAD, "/history/named", &[][..], StatusCode::OK, 1),
            (
                Method::POST,
                "/history/named",
                &[][..],
                StatusCode::METHOD_NOT_ALLOWED,
                0,
            ),
            (
                Method::GET,
                "/history/not-registered",
                &[][..],
                StatusCode::NOT_FOUND,
                0,
            ),
            (
                Method::GET,
                "/history/named",
                &[("x-requested-with", "XMLHttpRequest")][..],
                StatusCode::OK,
                1,
            ),
            (
                Method::GET,
                "/history/named",
                &[("purpose", "prefetch")][..],
                StatusCode::OK,
                1,
            ),
        ] {
            let writes = probe.writes.load(Ordering::SeqCst);
            let loads = probe.loads.load(Ordering::SeqCst);
            let collections = probe.collections.load(Ordering::SeqCst);
            let response =
                send_wire(router.clone(), method, path, Some(&cookie), headers, "").await?;
            assert_eq!(response.status, status);
            assert_eq!(probe.loads.load(Ordering::SeqCst), loads + session_access);
            assert_eq!(
                probe.collections.load(Ordering::SeqCst),
                collections + session_access
            );
            assert_eq!(probe.writes.load(Ordering::SeqCst), writes + session_access);
            assert_eq!(stored_history(&probe, &cookie).await?, expected);
        }
        Ok(())
    }
    .await;
    finish(&app, outcome).await
}

#[tokio::test]
async fn attempting_header_without_middleware_does_not_skip_previous_or_save() -> Result<()> {
    let probe = Probe::default();
    let app = app(true, probe.clone(), Arc::new(Control::default())).await?;
    let outcome = async {
        let router = build_router(&app)?;
        let first = send(router.clone(), "/history/named", None).await?;
        let cookie = cookie(&first)?;
        let writes = probe.writes.load(Ordering::SeqCst);
        let response = send_wire(
            router,
            Method::GET,
            "/history/named?attempt=1",
            Some(&cookie),
            &[("precognition", "true")],
            "",
        )
        .await?;
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.body["data"]["attempting"], true);
        assert_eq!(response.body["data"]["confirmed"], false);
        assert_eq!(probe.writes.load(Ordering::SeqCst), writes + 1);
        assert_eq!(
            stored_history(&probe, &cookie).await?,
            json!({"url":"http://localhost/history/named?attempt=1","route":"history.named"})
        );
        Ok(())
    }
    .await;
    finish(&app, outcome).await
}

#[tokio::test]
async fn confirmed_success_validation_failure_and_rejection_keep_start_gc_cookie_but_skip_save()
-> Result<()> {
    let probe = Probe::default();
    let control = Arc::new(Control::default());
    let app = app(true, probe.clone(), control.clone()).await?;
    let outcome = async {
        let router = build_router(&app)?;
        let first = send(router.clone(), "/history/named", None).await?;
        let cookie = cookie(&first)?;
        let previous = stored_history(&probe, &cookie).await?;
        let business = control.business.load(Ordering::SeqCst);
        let writes = probe.writes.load(Ordering::SeqCst);
        for (method, path, body, status, success) in [
            (
                Method::GET,
                "/history/confirmed/read",
                "",
                StatusCode::NO_CONTENT,
                true,
            ),
            (
                Method::POST,
                "/history/confirmed/validate",
                r#"{"value":"x"}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
                false,
            ),
            (
                Method::GET,
                "/history/confirmed/denied",
                "",
                StatusCode::FORBIDDEN,
                false,
            ),
        ] {
            let loads = probe.loads.load(Ordering::SeqCst);
            let collections = probe.collections.load(Ordering::SeqCst);
            let response = send_wire(
                router.clone(),
                method,
                path,
                Some(&cookie),
                &[
                    ("precognition", "true"),
                    ("content-type", "application/json"),
                ],
                body,
            )
            .await?;
            assert_eq!(response.status, status);
            assert_eq!(
                response
                    .headers
                    .get("precognition")
                    .and_then(|value| value.to_str().ok()),
                Some("true")
            );
            assert_eq!(
                response.headers.contains_key("precognition-success"),
                success
            );
            assert!(response.headers.contains_key("set-cookie"));
            assert_eq!(probe.loads.load(Ordering::SeqCst), loads + 1);
            assert_eq!(probe.collections.load(Ordering::SeqCst), collections + 1);
            assert_eq!(probe.writes.load(Ordering::SeqCst), writes);
            assert_eq!(control.business.load(Ordering::SeqCst), business);
            assert_eq!(stored_history(&probe, &cookie).await?, previous);
        }
        Ok(())
    }
    .await;
    finish(&app, outcome).await
}

#[tokio::test]
async fn missing_route_short_circuits_header_checks_after_exactly_one_resolver_call() -> Result<()>
{
    let probe = Probe::default();
    let control = Arc::new(Control::default());
    let app = app(true, probe.clone(), control.clone()).await?;
    let outcome = async {
        let router = build_router(&app)?;
        for (path, status) in [
            ("/history/no-route", StatusCode::OK),
            ("/history/not-registered", StatusCode::NOT_FOUND),
        ] {
            let request = Wire::get(path)
                .header("host", "localhost")
                .header(
                    "x-requested-with",
                    axum::http::HeaderValue::from_bytes(&[0xff]).map_err(Error::internal)?,
                )
                .header(
                    "purpose",
                    axum::http::HeaderValue::from_bytes(&[0xff]).map_err(Error::internal)?,
                )
                .body(Body::empty())
                .map_err(Error::internal)?;
            let response = router
                .clone()
                .oneshot(request)
                .await
                .map_err(Error::internal)?;
            assert_eq!(response.status(), status);
        }
        assert_eq!(control.resolver_calls.load(Ordering::SeqCst), 1);
        // 未注册路径没有进入路由组，因此不触发该组的 Session 生命周期。
        assert_eq!(probe.loads.load(Ordering::SeqCst), 1);
        assert_eq!(probe.collections.load(Ordering::SeqCst), 1);
        assert_eq!(probe.writes.load(Ordering::SeqCst), 1);
        Ok(())
    }
    .await;
    finish(&app, outcome).await
}

#[tokio::test]
async fn previous_route_name_uses_the_second_stateful_resolver_result() -> Result<()> {
    let probe = Probe::default();
    let control = Arc::new(Control::default());
    let app = app(true, probe.clone(), control.clone()).await?;
    let outcome = async {
        let router = build_router(&app)?;
        let first = send(router.clone(), "/history/named", None).await?;
        let cookie = cookie(&first)?;
        let response = send(router, "/history/resolver/switch", Some(&cookie)).await?;
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(control.resolver_calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            stored_history(&probe, &cookie).await?,
            json!({"url":"http://localhost/history/resolver/switch","route":"second.route"})
        );
        Ok(())
    }
    .await;
    finish(&app, outcome).await
}

#[tokio::test]
async fn second_resolver_error_or_missing_route_discards_cookies_and_prevents_save() -> Result<()> {
    let probe = Probe::default();
    let control = Arc::new(Control::default());
    let app = app(true, probe.clone(), control.clone()).await?;
    let outcome = async {
        let router = build_router(&app)?;
        let first = send(router.clone(), "/history/named", None).await?;
        let cookie = cookie(&first)?;
        let before = stored_attributes(&probe, &cookie).await?;
        let writes = probe.writes.load(Ordering::SeqCst);
        for (mode, code) in [
            ("error", "history_second_resolver_failed"),
            ("none", "session_previous_route_missing"),
        ] {
            control.resolver_calls.store(0, Ordering::SeqCst);
            let response = send(
                router.clone(),
                &format!("/history/resolver/{mode}"),
                Some(&cookie),
            )
            .await?;
            failure(&response, code)?;
            assert_eq!(control.resolver_calls.load(Ordering::SeqCst), 2);
            assert!(!response.headers.contains_key("set-cookie"));
            assert_eq!(
                response
                    .headers
                    .get("cache-control")
                    .and_then(|value| value.to_str().ok()),
                Some("no-store")
            );
            assert!(!response.body.to_string().contains("private"));
            assert!(!response.body.to_string().contains("must be replaced"));
            assert_eq!(probe.writes.load(Ordering::SeqCst), writes);
            assert_eq!(stored_attributes(&probe, &cookie).await?, before);
            let jar = control
                .jar
                .lock()
                .map_err(|_| invalid())?
                .take()
                .ok_or_else(invalid)?;
            assert!(jar.queued("resolver_receipt", None)?.is_none());
        }
        Ok(())
    }
    .await;
    finish(&app, outcome).await
}

#[tokio::test]
async fn previous_resolver_url_cookie_and_persistence_failures_keep_original_records_private()
-> Result<()> {
    let probe = Probe::default();
    let app = app(true, probe.clone(), Arc::new(Control::default())).await?;
    let outcome = async {
        let router = build_router(&app)?;
        let first = send(router.clone(), "/history/named", None).await?;
        let cookie = cookie(&first)?;
        let before = stored_attributes(&probe, &cookie).await?;
        let writes = probe.writes.load(Ordering::SeqCst);
        for (path, code) in [
            ("/history/resolver-error", "history_resolver_failed"),
            ("/history/invalid-cookie", "cookie_parameters_invalid"),
        ] {
            let response = send(router.clone(), path, Some(&cookie)).await?;
            failure(&response, code)?;
            assert!(!response.headers.contains_key("set-cookie"));
            assert!(!response.body.to_string().contains("private"));
            assert!(!response.body.to_string().contains("must be replaced"));
            assert_eq!(probe.writes.load(Ordering::SeqCst), writes);
            assert_eq!(stored_attributes(&probe, &cookie).await?, before);
        }
        let excessive = format!("/history/named?{}", "x".repeat(16 * 1024 + 1));
        let response = send(router.clone(), &excessive, Some(&cookie)).await?;
        assert_eq!(response.status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(response.body["code"], "input_limits_exceeded");
        assert!(!response.headers.contains_key("set-cookie"));
        assert_eq!(probe.writes.load(Ordering::SeqCst), writes);
        assert_eq!(stored_attributes(&probe, &cookie).await?, before);
        probe.fail.store(true, Ordering::SeqCst);
        let response = send(router, "/session-test/unsaved", Some(&cookie)).await?;
        failure(&response, "session_test_save_failed")?;
        assert!(!response.headers.contains_key("set-cookie"));
        assert!(!response.body.to_string().contains("must be replaced"));
        assert_eq!(probe.writes.load(Ordering::SeqCst), writes + 1);
        assert_eq!(stored_attributes(&probe, &cookie).await?, before);
        Ok(())
    }
    .await;
    finish(&app, outcome).await
}

// 等生产Cookie使用的真实UTC跨越目标秒；每次醒来重读时钟，不假设固定睡眠已跨秒。
async fn wait_for_real_second(second: i64) -> Result<()> {
    let target = OffsetDateTime::from_unix_timestamp(second).map_err(Error::internal)?;
    loop {
        let now = OffsetDateTime::now_utc();
        if now >= target {
            return Ok(());
        }
        let remaining = std::time::Duration::try_from(target - now).map_err(Error::internal)?;
        tokio::time::sleep(remaining).await;
    }
}

#[tokio::test]
async fn slow_save_does_not_shorten_the_final_response_cookie_lifetime() -> Result<()> {
    let probe = Probe::default();
    probe.blocked_write.store(true, Ordering::SeqCst);
    let app = app_with_lifetime(true, probe.clone(), Arc::new(Control::default()), 7).await?;
    let router = match build_router(&app) {
        Ok(router) => router,
        Err(error) => return finish(&app, Err(error)).await,
    };
    let mut joined = false;
    let mut task = tokio::spawn(async move { send(router, "/session-test/shared", None).await });
    let outcome: Result<()> = async {
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            probe.write_entered.notified(),
        )
        .await
        .map_err(Error::internal)?;
        check(probe.writes.load(Ordering::SeqCst) == 1)?;
        check(!task.is_finished())?;
        let entered = OffsetDateTime::now_utc().unix_timestamp();
        let started = tokio::time::Instant::now();
        // 从已进入write的时刻再跨两个秒刻度，确保保存实际等待至少一秒。
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            wait_for_real_second(entered + 2),
        )
        .await
        .map_err(Error::internal)??;
        check(started.elapsed() >= std::time::Duration::from_secs(1))?;
        check(!task.is_finished())?;
        let released = OffsetDateTime::now_utc().unix_timestamp();
        check(released >= entered + 2)?;
        probe.blocked_write.store(false, Ordering::SeqCst);
        probe.write_released.notify_one();
        let completed = tokio::time::timeout(std::time::Duration::from_secs(2), &mut task)
            .await
            .map_err(Error::internal)?;
        joined = true;
        let response = completed.map_err(Error::internal)??;
        check(response.status == StatusCode::OK)?;
        check(probe.writes.load(Ordering::SeqCst) == 1)?;
        let header = response
            .headers
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with("test_session="))
            .ok_or_else(invalid)?;
        check(header.split(';').any(|part| part.trim() == "Max-Age=420"))?;
        let expires = openrust::http::Cookie::from_header(header)?
            .expires()
            .ok_or_else(invalid)?;
        let received = OffsetDateTime::now_utc().unix_timestamp();
        // 不仅检查相对期限：Expires也必须由保存释放后的时间生成。
        check(expires.unix_timestamp() >= released + 420)?;
        check(expires.unix_timestamp() <= received + 420)?;
        let persisted = stored_attributes(&probe, &cookie(&response)?).await?;
        check(persisted["test"]["shared"] == json!({"one":1,"two":2,"three":3}))
    }
    .await;
    // 任一等待或断言失败均取消并join本次请求，不遗留等待释放的save任务。
    if !joined {
        task.abort();
        let _ = task.await;
    }
    finish(&app, outcome).await
}

#[tokio::test]
async fn cancelled_previous_save_closes_retained_state_and_clears_cookie_queue_without_background_io()
-> Result<()> {
    let probe = Probe::default();
    let control = Arc::new(Control::default());
    let app = app(true, probe.clone(), control.clone()).await?;
    let outcome = async {
        let router = build_router(&app)?;
        let first = send(router.clone(), "/history/named", None).await?;
        let cookie = cookie(&first)?;
        let before = stored_attributes(&probe, &cookie).await?;
        let writes = probe.writes.load(Ordering::SeqCst);
        probe.pending.store(true, Ordering::SeqCst);
        let task =
            tokio::spawn(async move { send(router, "/session-test/unsaved", Some(&cookie)).await });
        if tokio::time::timeout(
            std::time::Duration::from_secs(2),
            probe.write_entered.notified(),
        )
        .await
        .is_err()
        {
            task.abort();
            let _ = task.await;
            return Err(invalid());
        }
        task.abort();
        assert!(task.await.err().is_some_and(|error| error.is_cancelled()));
        let session = control
            .leaked
            .lock()
            .map_err(|_| invalid())?
            .take()
            .ok_or_else(invalid)?;
        assert_eq!(
            session
                .set_previous_url("/late")
                .err()
                .map(|error| error.code().to_owned()),
            Some("session_closed".into())
        );
        let jar = control
            .jar
            .lock()
            .map_err(|_| invalid())?
            .take()
            .ok_or_else(invalid)?;
        assert!(jar.queued("queued_test", None)?.is_none());
        tokio::task::yield_now().await;
        assert_eq!(probe.writes.load(Ordering::SeqCst), writes + 1);
        let id = session.id()?;
        let record = probe
            .memory
            .load(
                &id,
                openrust::session::DEFAULT_MAX_BYTES,
                OffsetDateTime::now_utc() + time::Duration::hours(1),
            )
            .await?
            .ok_or_else(invalid)?;
        assert_eq!(record.data, before);
        Ok(())
    }
    .await;
    finish(&app, outcome).await
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
                .put_typed("late", true)
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

struct RenameSessionRoutes;
impl ServiceProvider for RenameSessionRoutes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes.group(
                    RouteGroup::new()
                        .middleware_alias("cookies")
                        .middleware_alias("session"),
                    |routes| routes.get("/session-d2/rename", rename_session),
                )
            })?;
            Ok(())
        })
    }
}
async fn rename_session(session: Session, Inject(control): Inject<Control>) -> Result<Json<Value>> {
    *control.leaked.lock().map_err(|_| invalid())? = Some(session.clone());
    let previous = session
        .get_json("rename_count")?
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    session
        .set_name("renamed_session")?
        .put_typed("rename_count", previous + 1)?;
    Ok(Json(json!({"previous":previous,"name":session.name()?})))
}

#[tokio::test]
async fn set_name_changes_real_response_cookie_but_inbound_still_uses_configuration() -> Result<()>
{
    let mut config = FrameworkConfig::default();
    config.api.http_status = HttpStatusMode::Semantic;
    let control = Arc::new(Control::default());
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(CookieServiceProvider::plain())
        .provider(
            SessionServiceProvider::with_store(Probe::default()).with_cookie_config(
                SessionConfig {
                    driver: Some("array".into()),
                    cookie: Some("original_session".into()),
                    secure: Some(false),
                    ..SessionConfig::default()
                },
            ),
        )
        .provider(TestRoutes(control.clone(), true))
        .provider(RenameSessionRoutes);
    let app = Application::from_provider_runtime(
        builder
            .start(figment::Figment::from(
                figment::providers::Serialized::defaults(config),
            ))
            .await?,
    )?;
    let outcome = async {
        let router = build_router(&app)?;
        let first = send(router.clone(), "/session-d2/rename", None).await?;
        success(&first, &json!({"previous":0,"name":"renamed_session"}))?;
        {
            let guard = control.leaked.lock().map_err(|_| invalid())?;
            let leaked = guard.as_ref().ok_or_else(invalid)?;
            let called = AtomicUsize::new(0);
            check(
                leaked
                    .remember_typed("closed_missing", || {
                        called.fetch_add(1, Ordering::SeqCst);
                        Ok(1)
                    })
                    .is_err(),
            )?;
            check(called.load(Ordering::SeqCst) == 0)?;
            check(leaked.increment("counter").is_err())?;
            check(leaked.push_typed("list", 1).is_err())?;
            check(leaked.remove("counter").is_err())?;
            check(leaked.set_name("closed_name").is_err())?;
        }
        let headers = first
            .headers
            .get_all("set-cookie")
            .iter()
            .map(|value| value.to_str().map_err(Error::internal))
            .collect::<Result<Vec<_>>>()?;
        check(headers.len() == 1)?;
        check(headers[0].starts_with("renamed_session="))?;
        let pair = headers[0].split(';').next().ok_or_else(invalid)?;
        let id = pair.strip_prefix("renamed_session=").ok_or_else(invalid)?;
        let configured = format!("original_session={id}");
        let second = send(router.clone(), "/session-d2/rename", Some(&configured)).await?;
        success(&second, &json!({"previous":1,"name":"renamed_session"}))?;
        check(second.headers.get_all("set-cookie").iter().all(|header| {
            header.to_str().is_ok_and(|value| {
                value.starts_with("renamed_session=") && !value.contains("Max-Age=0")
            })
        }))?;
        let renamed_only = send(router, "/session-d2/rename", Some(pair)).await?;
        success(
            &renamed_only,
            &json!({"previous":0,"name":"renamed_session"}),
        )?;
        Ok(())
    }
    .await;
    finish(&app, outcome).await
}
