//! 公开 Tower 层与受管回程的交互回归。

use super::{source, wire};
use axum::{
    extract::Request,
    middleware::{Next, from_fn},
    response::{IntoResponse, Response},
};
use http::{StatusCode, header::SET_COOKIE};
use openrust::{
    Application, Error, Result, ServiceExt,
    config::{Environment, SessionConfig},
    encryption::EncryptionServiceProvider,
    foundation::{BootContext, ProviderBuilder, http::build_router},
    http::{CookieOptions, CookieServiceProvider, HttpServiceProvider, PreventRequestForgery},
    routing::RouteGroup,
    session::{
        MemorySessionStore, Session, SessionId, SessionRecord, SessionServiceProvider,
        SessionStore, SessionWriteMode, SessionWriteOutcome,
    },
    support::{BoxFuture, ServiceProvider},
};
use std::{
    convert::Infallible,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
};
use time::OffsetDateTime;
use tokio::sync::Notify;
use tower::{Layer, Service};

type RouteCall = BoxFuture<'static, std::result::Result<Response, Infallible>>;

#[derive(Clone, Copy)]
enum Behavior {
    Early,
    EarlyFound,
    Unpolled,
    Rebuilt,
    StripProof,
    Repeat,
    Token,
    Cancel,
    Pending,
    Late,
    Retired,
    OrdinaryShort,
    ProtocolRejection(StatusCode),
}

#[derive(Default)]
struct Observation {
    writes: AtomicUsize,
    flash_writes: AtomicUsize,
    handlers: AtomicUsize,
    parked: Mutex<Option<RouteCall>>,
    write_entered: Notify,
    release_write: Notify,
}

struct Store {
    data: MemorySessionStore,
    behavior: Behavior,
    observed: Arc<Observation>,
}

impl SessionStore for Store {
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        limit: usize,
        expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        self.data.load(id, limit, expiry)
    }

    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        limit: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async move {
            self.observed.writes.fetch_add(1, Ordering::SeqCst);
            if record
                .data
                .get("notice")
                .and_then(serde_json::Value::as_str)
                == Some("保存 flash")
            {
                self.observed.flash_writes.fetch_add(1, Ordering::SeqCst);
            }
            if matches!(self.behavior, Behavior::Late) {
                self.observed.write_entered.notify_one();
                self.observed.release_write.notified().await;
            }
            self.data.write(record, mode, limit).await
        })
    }

    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        self.data.delete(id)
    }
}

#[derive(Clone)]
struct Probe {
    behavior: Behavior,
    observed: Arc<Observation>,
}

impl Layer<axum::routing::Route> for Probe {
    type Service = ProbeService;

    fn layer(&self, inner: axum::routing::Route) -> Self::Service {
        ProbeService {
            probe: self.clone(),
            inner,
        }
    }
}

#[derive(Clone)]
struct ProbeService {
    probe: Probe,
    inner: axum::routing::Route,
}

impl Service<Request> for ProbeService {
    type Response = Response;
    type Error = Infallible;
    type Future = RouteCall;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        Service::<Request>::poll_ready(&mut self.inner, cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let probe = self.probe.clone();
        let mut inner = self.inner.clone();
        Box::pin(async move {
            request
                .extensions()
                .get::<Session>()
                .expect("Session 已在 probe 前启动")
                .flash_typed("notice", "保存 flash")
                .expect("公开 flash");
            match probe.behavior {
                Behavior::Token => return inner.call(request).await,
                Behavior::Early | Behavior::EarlyFound => {}
                Behavior::Unpolled => drop(inner.call(request)),
                Behavior::Late | Behavior::Retired => {
                    *probe.observed.parked.lock().expect("probe mutex") =
                        Some(Box::pin(inner.call(request)));
                }
                Behavior::Cancel | Behavior::Pending => {
                    let mut call = Box::pin(inner.call(request));
                    assert!(futures_util::poll!(&mut call).is_pending());
                    if matches!(probe.behavior, Behavior::Pending) {
                        *probe.observed.parked.lock().expect("probe mutex") = Some(call);
                    }
                }
                Behavior::Repeat => {
                    let mut stripped = Request::new(axum::body::Body::empty());
                    *stripped.uri_mut() = request.uri().clone();
                    let bad = inner.call(stripped).await.expect("Route 错误不可构造");
                    assert_ne!(bad.status(), StatusCode::OK);
                    let good = inner.call(request).await.expect("Route 错误不可构造");
                    assert_eq!(good.status(), StatusCode::OK);
                }
                Behavior::Rebuilt
                | Behavior::StripProof
                | Behavior::OrdinaryShort
                | Behavior::ProtocolRejection(_) => {
                    let (mut parts, body) = request.into_parts();
                    if matches!(probe.behavior, Behavior::StripProof) {
                        parts.extensions.clear();
                    }
                    let inner = inner.call(Request::from_parts(parts, body));
                    let response = tokio::spawn(inner)
                        .await
                        .expect("跨任务调用完成")
                        .expect("Route 错误不可构造");
                    if matches!(probe.behavior, Behavior::Rebuilt | Behavior::OrdinaryShort) {
                        assert_eq!(response.status(), StatusCode::OK);
                    }
                    if let Behavior::ProtocolRejection(expected) = probe.behavior {
                        assert_eq!(response.status(), expected);
                    }
                }
            }
            // 新响应不继承下游 Extensions；其中伪造的 Cookie 也须受回程限制。
            Ok((
                redirect_status(probe.behavior),
                [("location", "/next"), ("set-cookie", "probe=return")],
            )
                .into_response())
        })
    }
}

fn redirect_status(behavior: Behavior) -> StatusCode {
    if matches!(behavior, Behavior::EarlyFound) {
        StatusCode::FOUND
    } else {
        StatusCode::SEE_OTHER
    }
}

struct Routes(Probe);

impl ServiceProvider for Routes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let behavior = self.0.behavior;
            let observed = Arc::clone(&self.0.observed);
            let policy = match behavior {
                Behavior::ProtocolRejection(StatusCode::FORBIDDEN) => {
                    PreventRequestForgery::new().origin_only()
                }
                _ => PreventRequestForgery::new(),
            };
            app.prevent_request_forgery(policy)?;
            app.middleware_priority(["cookies", "session", "probe", "tail", "csrf"])?;
            app.middleware_alias("probe", self.0.clone())?;
            app.middleware_alias(
                "tail",
                from_fn(move |request: Request, next: Next| async move {
                    match behavior {
                        Behavior::Cancel | Behavior::Pending => std::future::pending().await,
                        Behavior::OrdinaryShort => "opaque".into_response(),
                        _ => next.run(request).await,
                    }
                }),
            )?;
            app.routes(move |routes| {
                let observed = Arc::clone(&observed);
                routes.group(
                    RouteGroup::new()
                        .middleware_alias("cookies")
                        .middleware_alias("session")
                        .middleware_alias("probe")
                        .middleware_alias("tail")
                        .middleware_alias("csrf"),
                    |routes| {
                        let handler = move || {
                            let observed = Arc::clone(&observed);
                            async move {
                                observed.handlers.fetch_add(1, Ordering::SeqCst);
                                "handler"
                            }
                        };
                        routes
                            .get("/return", handler.clone())
                            .post("/return", handler)
                            .get("/rotate", |session: Session| async move {
                                let before = super::token(session.clone()).await?;
                                session.regenerate_token()?;
                                let after = super::token(session).await?;
                                assert_ne!(before, after);
                                Ok::<String, Error>(after)
                            })
                            .post("/rotate", || async { "verified" })
                    },
                )
            })?;
            Ok(())
        })
    }
}

async fn fixture(behavior: Behavior) -> Result<(Application, Arc<Observation>)> {
    let observed = Arc::new(Observation::default());
    let mut builder = ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(EncryptionServiceProvider)
        .provider(CookieServiceProvider::contextual())
        .provider(
            SessionServiceProvider::with_store(Store {
                data: MemorySessionStore::default(),
                behavior,
                observed: Arc::clone(&observed),
            })
            .with_cookie_config(SessionConfig {
                driver: Some("array".into()),
                cookie: Some("test_session".into()),
                secure: Some(false),
                ..SessionConfig::default()
            }),
        )
        .provider(Routes(Probe {
            behavior,
            observed: Arc::clone(&observed),
        }));
    let app =
        Application::from_provider_runtime(builder.start(source(Environment::Development)).await?)?;
    Ok((app, observed))
}

async fn check_return(
    behavior: Behavior,
    expected_writes: usize,
    expected_handlers: usize,
) -> Result<()> {
    let (app, observed) = fixture(behavior).await?;
    let method = if matches!(behavior, Behavior::ProtocolRejection(_)) {
        "POST"
    } else {
        "GET"
    };
    let response = build_router(&app)?
        .oneshot(wire(method, "/return", None, ""))
        .await
        .map_err(Error::internal)?;
    assert_eq!(response.status(), redirect_status(behavior));
    assert_eq!(response.headers()["location"], "/next");
    assert_eq!(observed.writes.load(Ordering::SeqCst), expected_writes);
    assert_eq!(
        observed.flash_writes.load(Ordering::SeqCst),
        expected_writes
    );
    assert_eq!(observed.handlers.load(Ordering::SeqCst), expected_handlers);
    let cookies = response.headers().get_all(SET_COOKIE);
    assert_eq!(
        cookies
            .iter()
            .any(|value| value.as_bytes().starts_with(b"test_session=")),
        expected_writes != 0
    );
    if expected_writes == 0 {
        assert_eq!(cookies.iter().count(), 0);
    }
    drop(observed.parked.lock().expect("probe mutex").take());
    Ok(())
}

#[tokio::test]
async fn real_tower_return_allows_flash_when_no_call_was_admitted() -> Result<()> {
    for behavior in [
        Behavior::Early,
        Behavior::EarlyFound,
        Behavior::Unpolled,
        Behavior::OrdinaryShort,
    ] {
        check_return(behavior, 1, 0).await?;
    }
    Ok(())
}

#[tokio::test]
async fn spawned_rebuilt_request_retains_the_actual_proof() -> Result<()> {
    check_return(Behavior::Rebuilt, 1, 1).await
}

#[tokio::test]
async fn stripped_proof_and_unsettled_or_cancelled_calls_cannot_commit() -> Result<()> {
    for behavior in [Behavior::StripProof, Behavior::Pending, Behavior::Cancel] {
        check_return(behavior, 0, 0).await?;
    }
    Ok(())
}

#[tokio::test]
async fn real_lazy_route_polled_during_save_cannot_enter_a_sealed_subtree() -> Result<()> {
    let (app, observed) = fixture(Behavior::Late).await?;
    let router = build_router(&app)?;
    let late = async {
        observed.write_entered.notified().await;
        let call = observed
            .parked
            .lock()
            .expect("probe mutex")
            .take()
            .expect("probe 保留了真实 RouteFuture");
        let response = call.await.expect("Route 错误不可构造");
        observed.release_write.notify_one();
        response
    };
    let (response, late) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(router.oneshot(wire("GET", "/return", None, "")), late)
    })
    .await
    .map_err(Error::internal)?;
    assert_ne!(late.status(), StatusCode::OK);
    assert_eq!(
        response.map_err(Error::internal)?.status(),
        StatusCode::SEE_OTHER
    );
    assert_eq!(observed.handlers.load(Ordering::SeqCst), 0);
    assert_eq!(observed.writes.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn trusted_protocol_rejections_permit_the_opaque_redirect_to_save() -> Result<()> {
    for status in [
        StatusCode::FORBIDDEN,
        StatusCode::from_u16(419).expect("有效状态"),
    ] {
        check_return(Behavior::ProtocolRejection(status), 1, 0).await?;
    }
    Ok(())
}

#[tokio::test]
async fn retired_route_from_an_earlier_request_cannot_reach_the_handler() -> Result<()> {
    let (app, observed) = fixture(Behavior::Retired).await?;
    let router = build_router(&app)?;
    let response = router
        .clone()
        .oneshot(wire("GET", "/return", None, ""))
        .await
        .map_err(Error::internal)?;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let old = observed
        .parked
        .lock()
        .expect("probe mutex")
        .take()
        .expect("保存旧请求调用");
    let fresh = router
        .oneshot(wire("GET", "/return", None, ""))
        .await
        .map_err(Error::internal)?;
    assert_eq!(fresh.status(), StatusCode::SEE_OTHER);
    let rejected = old.await.expect("Route 错误不可构造");
    assert_ne!(rejected.status(), StatusCode::OK);
    assert_eq!(observed.handlers.load(Ordering::SeqCst), 0);
    assert_eq!(observed.writes.load(Ordering::SeqCst), 2);
    drop(observed.parked.lock().expect("probe mutex").take());
    Ok(())
}

struct OrderingRoutes {
    policy: PreventRequestForgery,
    custom_session: bool,
}

impl ServiceProvider for OrderingRoutes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.prevent_request_forgery(self.policy.clone())?;
            app.middleware_priority(["cookies", "custom-session", "csrf", "session"])?;
            app.middleware_alias(
                "custom-session",
                from_fn(|mut request: Request, next: Next| async move {
                    let session =
                        Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
                    session.start().await?;
                    request.extensions_mut().insert(session);
                    Ok::<Response, Error>(next.run(request).await)
                }),
            )?;
            let custom_session = self.custom_session;
            app.routes(move |routes| {
                let route = RouteGroup::new()
                    .middleware_alias("cookies")
                    .middleware_alias("csrf");
                let route = if custom_session {
                    route.middleware_alias("custom-session")
                } else {
                    route.middleware_alias("session")
                };
                routes.group(route, |routes| {
                    routes.get("/ordered", |session: Session| async move {
                        session.regenerate_token()?;
                        super::token(session).await
                    })
                })
            })?;
            Ok(())
        })
    }
}

async fn ordered_fixture(
    policy: PreventRequestForgery,
    custom_session: bool,
) -> Result<Application> {
    let mut builder = ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(EncryptionServiceProvider)
        .provider(CookieServiceProvider::contextual())
        .provider(
            SessionServiceProvider::with_store(MemorySessionStore::default()).with_cookie_config(
                SessionConfig {
                    driver: Some("array".into()),
                    cookie: Some("test_session".into()),
                    secure: Some(false),
                    ..SessionConfig::default()
                },
            ),
        )
        .provider(OrderingRoutes {
            policy,
            custom_session,
        });
    Application::from_provider_runtime(builder.start(source(Environment::Development)).await?)
}

#[tokio::test]
async fn no_outbound_csrf_ancestor_can_authorize_the_inner_session() -> Result<()> {
    for policy in [
        PreventRequestForgery::new().origin_only(),
        PreventRequestForgery::new().without_xsrf_cookie(),
    ] {
        let app = ordered_fixture(policy, false).await?;
        let response = build_router(&app)?
            .oneshot(wire("GET", "/ordered", None, ""))
            .await
            .map_err(Error::internal)?;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get_all(SET_COOKIE)
                .iter()
                .any(|value| value.as_bytes().starts_with(b"test_session="))
        );
        assert!(
            !response
                .headers()
                .get_all(SET_COOKIE)
                .iter()
                .any(|value| value.as_bytes().starts_with(b"XSRF-TOKEN="))
        );
    }
    Ok(())
}

#[tokio::test]
async fn outbound_csrf_rejects_known_reverse_order_but_accepts_an_actual_custom_session()
-> Result<()> {
    let bad = ordered_fixture(PreventRequestForgery::new(), false).await?;
    let error = build_router(&bad).expect_err("已知内置 Session 位于出站 CSRF 之后");
    assert_eq!(error.code(), "web_cookie_encryption_missing");
    let good = ordered_fixture(PreventRequestForgery::new(), true).await?;
    let response = build_router(&good)?
        .oneshot(wire("GET", "/ordered", None, ""))
        .await
        .map_err(Error::internal)?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .any(|value| value.as_bytes().starts_with(b"XSRF-TOKEN="))
    );
    Ok(())
}

#[tokio::test]
async fn later_success_cannot_overwrite_an_earlier_protection_failure() -> Result<()> {
    check_return(Behavior::Repeat, 0, 1).await
}

#[tokio::test]
async fn rotated_token_is_emitted_and_accepted_as_the_real_encrypted_xsrf_header() -> Result<()> {
    let (app, _) = fixture(Behavior::Token).await?;
    let router = build_router(&app)?;
    let initial = router
        .clone()
        .oneshot(wire("GET", "/rotate", None, ""))
        .await
        .map_err(Error::internal)?;
    assert_eq!(initial.status(), StatusCode::OK);
    let cookies = initial
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .map(|value| {
            value
                .to_str()
                .expect("Cookie header")
                .split(';')
                .next()
                .expect("Cookie pair")
        })
        .collect::<Vec<_>>()
        .join("; ");
    let xsrf = cookies
        .split("; ")
        .find_map(|pair| pair.strip_prefix("XSRF-TOKEN="))
        .expect("真实出站 XSRF Cookie");
    let decoded = url::form_urlencoded::parse(format!("v={xsrf}").as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .expect("Cookie 解码");
    let mut post = wire("POST", "/rotate", Some(&cookies), "");
    post.headers_mut()
        .insert("x-xsrf-token", decoded.parse().expect("ASCII 密文"));
    let accepted = router.oneshot(post).await.map_err(Error::internal)?;
    assert_eq!(accepted.status(), StatusCode::OK);
    assert_eq!(
        axum::body::to_bytes(accepted.into_body(), 4096)
            .await
            .map_err(Error::internal)?
            .as_ref(),
        b"verified"
    );
    Ok(())
}

const EXCLUDED_SESSION_NAME: &str = "excluded_session";
const PROTECTED_SESSION_NAME: &str = "renamed_session";
type RenameWitness = Arc<Mutex<Option<tokio::sync::oneshot::Sender<Session>>>>;

struct RenameRoutes {
    name: &'static str,
    web: bool,
    witness: RenameWitness,
}

impl ServiceProvider for RenameRoutes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let name = self.name;
            let web = self.web;
            let witness = Arc::clone(&self.witness);
            app.exceptions(|handler| {
                handler.api_envelope_when(|request, _| request.path() == "/rename-session");
            })?;
            app.routes(move |routes| {
                let group = if web {
                    RouteGroup::new().middleware_group("web")
                } else {
                    RouteGroup::new()
                        .middleware_alias("cookies")
                        .middleware_alias("session")
                };
                let witness = Arc::clone(&witness);
                routes
                    .group(group, |routes| {
                        routes.get(
                            "/rename-session",
                            move |session: Session, jar: openrust::http::CookieJar| {
                                let witness = Arc::clone(&witness);
                                async move {
                                    session.set_name(name)?;
                                    jar.expire("queued_probe", CookieOptions::default())?;
                                    if let Some(sender) =
                                        witness.lock().expect("witness mutex").take()
                                    {
                                        assert!(sender.send(session.clone()).is_ok());
                                    }
                                    Ok::<_, Error>((
                                        [("set-cookie", "raw_probe=present")],
                                        session.id()?.to_string(),
                                    ))
                                }
                            },
                        )
                    })
                    .group(RouteGroup::new().middleware_alias("cookies"), |routes| {
                        routes.get(
                            "/read-renamed-session",
                            move |request: openrust::http::Request| async move {
                                request
                                    .cookie(name)
                                    .and_then(|value| value.as_str().map(str::to_owned))
                                    .ok_or_else(|| {
                                        Error::custom("fixture_missing_cookie", "缺少往返 Cookie")
                                    })
                            },
                        )
                    })
            })?;
            Ok(())
        })
    }
}

async fn rename_fixture(
    name: &'static str,
    initial: &'static str,
    web: bool,
    witness: Option<tokio::sync::oneshot::Sender<Session>>,
) -> Result<(Application, Arc<Observation>)> {
    let observed = Arc::new(Observation::default());
    let behavior = if witness.is_some() {
        Behavior::Late
    } else {
        Behavior::Early
    };
    let mut builder = ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(EncryptionServiceProvider)
        .provider(CookieServiceProvider::contextual().except([EXCLUDED_SESSION_NAME]))
        .provider(
            SessionServiceProvider::with_store(Store {
                data: MemorySessionStore::default(),
                behavior,
                observed: Arc::clone(&observed),
            })
            .with_cookie_config(SessionConfig {
                driver: Some("array".into()),
                cookie: Some(initial.into()),
                secure: Some(false),
                ..SessionConfig::default()
            }),
        )
        .provider(RenameRoutes {
            name,
            web,
            witness: Arc::new(Mutex::new(witness)),
        });
    let app =
        Application::from_provider_runtime(builder.start(source(Environment::Development)).await?)?;
    Ok((app, observed))
}

fn response_cookie(response: &Response, name: &str) -> String {
    response
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .find_map(|header| {
            let pair = header.to_str().ok()?.split(';').next()?;
            let (key, _) = pair.split_once('=')?;
            (key == name).then(|| pair.to_owned())
        })
        .expect("实际出站 Session Cookie")
}

#[tokio::test]
async fn protected_session_cannot_be_renamed_to_an_encryption_exclusion() -> Result<()> {
    let (app, observed) = rename_fixture(EXCLUDED_SESSION_NAME, "test_session", true, None).await?;
    let mut request = wire("GET", "/rename-session", None, "");
    request.headers_mut().insert(
        http::header::ACCEPT,
        http::HeaderValue::from_static("application/json"),
    );
    let response = build_router(&app)?
        .oneshot(request)
        .await
        .map_err(Error::internal)?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.headers().get_all(SET_COOKIE).iter().count(), 0);
    assert_eq!(observed.writes.load(Ordering::SeqCst), 0);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .map_err(Error::internal)?;
    let body: serde_json::Value = serde_json::from_slice(&bytes).map_err(Error::internal)?;
    assert_eq!(body["code"], "web_cookie_encryption_missing");
    Ok(())
}

#[tokio::test]
async fn protected_session_rename_saves_and_round_trips_the_encrypted_id() -> Result<()> {
    let (app, observed) =
        rename_fixture(PROTECTED_SESSION_NAME, "test_session", true, None).await?;
    let router = build_router(&app)?;
    let response = router
        .clone()
        .oneshot(wire("GET", "/rename-session", None, ""))
        .await
        .map_err(Error::internal)?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(observed.writes.load(Ordering::SeqCst), 1);
    let cookie = response_cookie(&response, PROTECTED_SESSION_NAME);
    let id = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .map_err(Error::internal)?;
    let id = std::str::from_utf8(&id).map_err(Error::internal)?;
    SessionId::parse(id)?;
    assert!(!cookie.contains(id));
    let roundtrip = router
        .oneshot(wire("GET", "/read-renamed-session", Some(&cookie), ""))
        .await
        .map_err(Error::internal)?;
    assert_eq!(roundtrip.status(), StatusCode::OK);
    let decoded = axum::body::to_bytes(roundtrip.into_body(), 4096)
        .await
        .map_err(Error::internal)?;
    assert_eq!(decoded.as_ref(), id.as_bytes());
    Ok(())
}

#[tokio::test]
async fn explicitly_excluded_initial_session_without_csrf_keeps_its_plain_cookie() -> Result<()> {
    let (app, observed) =
        rename_fixture(EXCLUDED_SESSION_NAME, EXCLUDED_SESSION_NAME, false, None).await?;
    let response = build_router(&app)?
        .oneshot(wire("GET", "/rename-session", None, ""))
        .await
        .map_err(Error::internal)?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(observed.writes.load(Ordering::SeqCst), 1);
    let cookie = response_cookie(&response, EXCLUDED_SESSION_NAME);
    let id = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .map_err(Error::internal)?;
    let id = std::str::from_utf8(&id).map_err(Error::internal)?;
    SessionId::parse(id)?;
    assert_eq!(cookie, format!("{EXCLUDED_SESSION_NAME}={id}"));
    Ok(())
}

#[tokio::test]
async fn session_write_io_rejects_concurrent_rename_and_uses_the_approved_name() -> Result<()> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let (app, observed) =
        rename_fixture(PROTECTED_SESSION_NAME, "test_session", true, Some(sender)).await?;
    let router = build_router(&app)?;
    let rename = async {
        let session = receiver.await.map_err(Error::internal)?;
        observed.write_entered.notified().await;
        let result = session.set_name(EXCLUDED_SESSION_NAME).map(|_| ());
        observed.release_write.notify_one();
        let error = result.expect_err("持有实际 Session I/O 门时不能更名");
        assert_eq!(error.code(), "session_operation_in_progress");
        Ok::<(), Error>(())
    };
    let (response, rename) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            router.oneshot(wire("GET", "/rename-session", None, "")),
            rename
        )
    })
    .await
    .map_err(Error::internal)?;
    rename?;
    let response = response.map_err(Error::internal)?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(observed.writes.load(Ordering::SeqCst), 1);
    assert!(response_cookie(&response, PROTECTED_SESSION_NAME).starts_with("renamed_session="));
    assert!(
        !response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .any(|value| { value.as_bytes().starts_with(b"excluded_session=") })
    );
    Ok(())
}
