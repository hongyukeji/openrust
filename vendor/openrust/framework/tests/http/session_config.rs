//! 最终配置经正式 Provider/TestApp/Router 到 Session Cookie 的行为回归。
#![cfg(feature = "session")]

use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use openrust::config::{ConfigError, FrameworkConfig, HttpStatusMode, SessionConfig};
use openrust::database::Db;
use openrust::foundation::ApplicationDefinition;
use openrust::foundation::BootContext;
use openrust::foundation::RegisterContext;
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode, header::SET_COOKIE},
};
use openrust::http::{
    Cookie, CookieJar, CookieOptions, CookieServiceProvider, HttpServiceProvider, Json,
    RequestInfo, RequestSecurity, SameSite,
};
use openrust::session::{MemorySessionStore, Session, SessionServiceProvider};
use openrust::support::BoxFuture;
use openrust::support::ServiceProvider;
use openrust::testing::{TestApp, TestResponse};
use openrust::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(5);
#[derive(Default, Deserialize)]
#[serde(default)]
struct Harness {
    session_test: Plan,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
struct Plan {
    memory: bool,
    boot: Option<SessionConfig>,
    late: Option<Value>,
    late_name: Option<String>,
}
impl Default for Plan {
    fn default() -> Self {
        Self {
            memory: true,
            boot: None,
            late: None,
            late_name: None,
        }
    }
}
struct HarnessApp;
impl ApplicationDefinition for HarnessApp {
    type Config = Harness;

    fn default_providers() -> openrust::support::DefaultProviders {
        openrust::support::DefaultProviders::empty()
    }
    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let plan = &config.session_test;
        let mut session = if plan.memory {
            SessionServiceProvider::with_store(MemorySessionStore::default())
        } else {
            SessionServiceProvider::new()
        };
        if let Some(config) = &plan.boot {
            session = session.with_cookie_config(config.clone());
        }
        vec![
            Box::new(HttpServiceProvider),
            Box::new(CookieServiceProvider::plain()),
            Box::new(DbTrap),
            Box::new(session),
            Box::new(Late(plan.clone())),
            Box::new(TestRoutes),
        ]
    }
}
struct Late(Plan);
impl ServiceProvider for Late {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if let Some(value) = &self.0.late {
                app.overlay("session", value.clone())?;
            }
            if let Some(name) = &self.0.late_name {
                app.overlay("app", json!({"name":name}))?;
            }
            Ok(())
        })
    }
}
#[derive(Default)]
struct DbResolutions(AtomicUsize);
struct DbTrap;
impl ServiceProvider for DbTrap {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let count = Arc::new(DbResolutions::default());
            app.instance(count.clone()).await?;
            app.singleton::<Db, _>(move |_| {
                count.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async {
                    Err::<Arc<Db>, _>(Error::custom_with_status(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "session_config_test_db_resolved",
                        "禁用 Session 不应解析 Db",
                    ))
                })
            })
            .await?;
            Ok(())
        })
    }
}
struct TestRoutes;
impl ServiceProvider for TestRoutes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let enabled = app
                .config_repository()
                .section::<SessionConfig>("session")?
                .driver
                .is_some();
            app.routes(move |routes| {
                let add = |routes: openrust::routing::Router| {
                    routes
                        .get("/config/session", inspect)
                        .get("/config/presence", presence)
                };
                if enabled {
                    routes.group(
                        openrust::routing::RouteGroup::new()
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
async fn inspect(session: Session, jar: CookieJar) -> Result<Json<Value>> {
    let cookie = jar
        .factory()
        .make("factory_probe", "value", 0, CookieOptions::default())?;
    let site = match cookie.same_site() {
        Some(SameSite::Lax) => Some("lax"),
        Some(SameSite::Strict) => Some("strict"),
        Some(SameSite::None) => Some("none"),
        None => None,
    };
    let data = json!({"name":session.name()?,"factory":{"path":cookie.path(),"domain":cookie.domain(),"secure":cookie.secure(),"same_site":site,"http_only":cookie.http_only()}});
    jar.queue(cookie)?;
    Ok(Json(data))
}
async fn presence(info: RequestInfo) -> Json<Value> {
    Json(json!({"session":info.has_session()}))
}
fn invalid() -> Error {
    Error::custom("session_config_test_mismatch", "Session 配置行为不匹配")
}
fn ensure(condition: bool) -> Result<()> {
    if condition { Ok(()) } else { Err(invalid()) }
}
fn sources(plan: &Plan) -> Figment {
    let mut config = FrameworkConfig::default();
    config.api.http_status = HttpStatusMode::Semantic;
    Figment::from(Serialized::defaults(config))
        .merge(Serialized::defaults(json!({"session_test":plan})))
}
async fn request(app: &TestApp<HarnessApp>, path: &str, secure: bool) -> Result<TestResponse> {
    let mut request = Request::get(path)
        .header("host", "localhost")
        .body(Body::empty())
        .map_err(Error::internal)?;
    request
        .extensions_mut()
        .insert(RequestSecurity::new(secure));
    app.request_with_limits(request, 4096, TIMEOUT).await
}
fn cookie(response: &TestResponse, name: &str) -> Result<(Cookie, String)> {
    let mut selected = None;
    for value in response.headers().get_all(SET_COOKIE) {
        let text = value.to_str().map_err(|_| invalid())?;
        if text
            .split(';')
            .next()
            .and_then(|pair| pair.split_once('='))
            .map(|(key, _)| key)
            == Some(name)
        {
            ensure(selected.is_none())?;
            selected = Some((Cookie::from_header(text)?, text.to_owned()));
        }
    }
    selected.ok_or_else(invalid)
}
fn success(
    response: &TestResponse,
    name: &str,
    path: &str,
    domain: Option<&str>,
    secure: Option<bool>,
    site: Option<&str>,
) -> Result<()> {
    response.assert_status(StatusCode::OK)?.assert_json(&json!({"success":true,"code":"ok","message":"ok","data":{"name":name,"factory":{"path":path,"domain":domain,"secure":secure,"same_site":site,"http_only":true}}}))?;
    Ok(())
}
async fn finish(app: &mut TestApp<HarnessApp>, outcome: Result<()>) -> Result<()> {
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome?;
    shutdown
}

#[test]
fn defaults_and_explicit_null_options_follow_the_public_config() -> Result<()> {
    let config = SessionConfig::default();
    ensure(
        config.driver.as_deref() == Some("database")
            && config.cookie.is_none()
            && config.secure.is_none()
            && config.lifetime == 120,
    )?;
    ensure(
        config.http_only
            && config.same_site.as_deref() == Some("lax")
            && !config.expire_on_close
            && !config.partitioned,
    )?;
    let nullable: SessionConfig =
        serde_json::from_value(json!({"same_site":null,"secure":null,"cookie":null}))
            .map_err(Error::internal)?;
    ensure(nullable.same_site.is_none() && nullable.secure.is_none() && nullable.cookie.is_none())
}

#[tokio::test]
async fn file_boot_and_late_overlay_share_final_cookie_factory_and_session_settings() -> Result<()>
{
    let mut file = tempfile::NamedTempFile::new().map_err(Error::internal)?;
    file.write_all(b"[app]\nname='File Name'\n[session]\npath='/file'\ndomain='file.test'\nsecure=true\nsame_site='lax'\nlifetime=3\nhttp_only=true\n").map_err(Error::internal)?;
    file.flush().map_err(Error::internal)?;
    let plan = Plan {
        boot: Some(SessionConfig {
            path: "/boot".into(),
            domain: Some("boot.test".into()),
            secure: Some(true),
            same_site: Some("none".into()),
            lifetime: 7,
            http_only: false,
            ..SessionConfig::default()
        }),
        late: Some(
            json!({"path":"/late","domain":"late.test","secure":false,"same_site":"strict","lifetime":9}),
        ),
        late_name: Some("Café Final App".into()),
        ..Plan::default()
    };
    let source = sources(&plan).merge(Toml::file(file.path()));
    ensure(
        source
            .extract_inner::<String>("session.path")
            .map_err(Error::internal)?
            == "/file",
    )?;
    ensure(
        source
            .extract_inner::<bool>("session.http_only")
            .map_err(Error::internal)?,
    )?;
    let mut app = TestApp::<HarnessApp>::builder(source).build().await?;
    let outcome = async {
        let response = request(&app, "/config/session", true).await?;
        success(
            &response,
            "cafe_final_app_session",
            "/late",
            Some("late.test"),
            Some(false),
            Some("strict"),
        )?;
        for name in ["cafe_final_app_session", "factory_probe"] {
            let (cookie, _) = cookie(&response, name)?;
            ensure(
                cookie.path() == "/late"
                    && cookie.domain() == Some("late.test")
                    && cookie.secure() == Some(false)
                    && cookie.same_site() == Some(SameSite::Strict),
            )?;
        }
        let (session, wire) = cookie(&response, "cafe_final_app_session")?;
        ensure(session.expires().is_some() && !session.http_only())?;
        ensure(cookie(&response, "factory_probe")?.0.http_only())?;
        let age = max_age(&wire)?.ok_or_else(invalid)?;
        ensure((535..=540).contains(&age))
    }
    .await;
    finish(&mut app, outcome).await
}

fn max_age(header: &str) -> Result<Option<i64>> {
    header
        .split(';')
        .skip(1)
        .find_map(|part| {
            part.trim()
                .split_once('=')
                .filter(|(key, _)| key.eq_ignore_ascii_case("max-age"))
                .map(|(_, value)| value)
        })
        .map(|value| value.parse::<i64>().map_err(|_| invalid()))
        .transpose()
}

#[tokio::test]
async fn explicit_name_and_typed_transport_respect_secure_none_and_overrides() -> Result<()> {
    for secure in [None, Some(false), Some(true)] {
        let plan = Plan {
            boot: Some(SessionConfig {
                cookie: Some("explicit_session".into()),
                secure,
                ..SessionConfig::default()
            }),
            late_name: Some("Must Not Replace Explicit".into()),
            ..Plan::default()
        };
        let mut app = TestApp::<HarnessApp>::builder(sources(&plan))
            .build()
            .await?;
        let outcome = async {
            for transport in [false, true] {
                let response = request(&app, "/config/session", transport).await?;
                success(
                    &response,
                    "explicit_session",
                    "/",
                    None,
                    secure,
                    Some("lax"),
                )?;
                for name in ["explicit_session", "factory_probe"] {
                    ensure(
                        cookie(&response, name)?.0.secure() == Some(secure.unwrap_or(transport)),
                    )?;
                }
            }
            Ok(())
        }
        .await;
        finish(&mut app, outcome).await?;
    }
    Ok(())
}

#[tokio::test]
async fn session_only_cookie_attributes_preserve_nullable_same_site_and_factory_defaults()
-> Result<()> {
    let plan = Plan {
        boot: Some(SessionConfig {
            cookie: Some("attribute_session".into()),
            secure: Some(true),
            same_site: None,
            http_only: false,
            expire_on_close: true,
            partitioned: true,
            ..SessionConfig::default()
        }),
        ..Plan::default()
    };
    let mut app = TestApp::<HarnessApp>::builder(sources(&plan))
        .build()
        .await?;
    let outcome = async {
        let response = request(&app, "/config/session", true).await?;
        success(&response, "attribute_session", "/", None, Some(true), None)?;
        let (session, wire) = cookie(&response, "attribute_session")?;
        ensure(
            !session.http_only()
                && session.same_site().is_none()
                && session.expires().is_none()
                && max_age(&wire)?.is_none(),
        )?;
        ensure(
            wire.split(';')
                .skip(1)
                .any(|part| part.trim().eq_ignore_ascii_case("partitioned")),
        )?;
        let (factory, wire) = cookie(&response, "factory_probe")?;
        ensure(factory.http_only() && factory.same_site().is_none())?;
        ensure(
            !wire
                .split(';')
                .skip(1)
                .any(|part| part.trim().eq_ignore_ascii_case("partitioned")),
        )
    }
    .await;
    finish(&mut app, outcome).await
}

#[tokio::test]
async fn disabled_driver_does_not_resolve_database_or_create_a_session() -> Result<()> {
    let plan = Plan {
        memory: false,
        boot: Some(SessionConfig {
            driver: None,
            ..SessionConfig::default()
        }),
        ..Plan::default()
    };
    let mut app = TestApp::<HarnessApp>::builder(sources(&plan))
        .build()
        .await?;
    let outcome = async {
        let response = request(&app, "/config/presence", false).await?;
        response.assert_status(StatusCode::OK)?.assert_json(
            &json!({"success":true,"code":"ok","message":"ok","data":{"session":false}}),
        )?;
        ensure(!response.headers().contains_key(SET_COOKIE))?;
        let count = app
            .app()?
            .state()
            .container()
            .resolve::<DbResolutions>()
            .await?;
        ensure(count.0.load(Ordering::SeqCst) == 0)
    }
    .await;
    finish(&mut app, outcome).await
}

#[tokio::test]
async fn invalid_boot_config_fails_with_the_exact_configuration_field() -> Result<()> {
    for (field, change) in [
        ("lifetime", json!(0)),
        ("cookie", json!("invalid=name")),
        ("path", json!("/bad;path")),
        ("domain", json!("bad;domain")),
        ("same_site", json!("invalid")),
        ("max_bytes", json!(0)),
        ("serializer", json!("unknown")),
    ] {
        let mut config = serde_json::to_value(SessionConfig::default()).map_err(Error::internal)?;
        config[field] = change;
        let config: SessionConfig = serde_json::from_value(config).map_err(Error::internal)?;
        let plan = Plan {
            boot: Some(config),
            ..Plan::default()
        };
        expect_invalid(sources(&plan), field).await?;
    }
    Ok(())
}

async fn expect_invalid(source: Figment, field: &str) -> Result<()> {
    let error = match TestApp::<HarnessApp>::builder(source).build().await {
        Ok(mut app) => {
            let _ = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
            return Err(invalid());
        }
        Err(error) => error,
    };
    ensure(
        error.code() == "configuration_error"
            && error.status() == StatusCode::INTERNAL_SERVER_ERROR,
    )?;
    let Error::Config(ConfigError::Invalid(detail)) = error else {
        return Err(invalid());
    };
    ensure(detail.starts_with(&format!("session.{field}:")))
}

#[tokio::test]
async fn invalid_file_and_final_overlay_are_checked_after_all_valid_overrides() -> Result<()> {
    for (field, toml) in [
        ("lifetime", "[session]\nlifetime=0\n"),
        ("cookie", "[session]\ncookie='bad=name'\n"),
        ("same_site", "[session]\nsame_site='invalid'\n"),
        ("path", "[session]\npath='/bad;path'\n"),
        ("serializer", "[session]\nserializer='unknown'\n"),
    ] {
        let mut file = tempfile::NamedTempFile::new().map_err(Error::internal)?;
        file.write_all(toml.as_bytes()).map_err(Error::internal)?;
        file.flush().map_err(Error::internal)?;
        let source = sources(&Plan::default()).merge(Toml::file(file.path()));
        expect_invalid(source, field).await?;
    }
    for (field, value) in [
        ("lifetime", json!(0)),
        ("same_site", json!("invalid")),
        ("max_bytes", json!(0)),
        ("serializer", json!("unknown")),
    ] {
        let mut late = json!({});
        late[field] = value;
        let plan = Plan {
            boot: Some(SessionConfig::default()),
            late: Some(late),
            ..Plan::default()
        };
        expect_invalid(sources(&plan), field).await?;
    }
    Ok(())
}

async fn repaired_response(app: &TestApp<HarnessApp>, config: &SessionConfig) -> Result<()> {
    let response = request(app, "/config/session", false).await?;
    let name = config.cookie.as_deref().ok_or_else(invalid)?;
    success(
        &response,
        name,
        &config.path,
        config.domain.as_deref(),
        config.secure,
        config.same_site.as_deref(),
    )?;
    let site = match config.same_site.as_deref() {
        Some("strict") => Some(SameSite::Strict),
        Some("lax") => Some(SameSite::Lax),
        Some("none") => Some(SameSite::None),
        None => None,
        _ => return Err(invalid()),
    };
    for cookie_name in [name, "factory_probe"] {
        let (model, wire) = cookie(&response, cookie_name)?;
        ensure(
            model.path() == config.path
                && model.domain() == config.domain.as_deref()
                && model.secure() == config.secure
                && model.same_site() == site,
        )?;
        if cookie_name == name {
            ensure(model.http_only() == config.http_only && model.expires().is_some())?;
            let maximum = i64::try_from(config.lifetime)
                .map_err(Error::internal)?
                .checked_mul(60)
                .ok_or_else(invalid)?;
            let age = max_age(&wire)?.ok_or_else(invalid)?;
            ensure(((maximum - 5)..=maximum).contains(&age))?;
            ensure(
                wire.split(';')
                    .skip(1)
                    .any(|part| part.trim().eq_ignore_ascii_case("partitioned"))
                    == config.partitioned,
            )?;
        } else {
            ensure(model.http_only() && model.expires().is_none() && max_age(&wire)?.is_none())?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn semantic_errors_in_file_or_boot_can_be_repaired_by_later_valid_config() -> Result<()> {
    let boot = SessionConfig {
        cookie: Some("boot_repaired_session".into()),
        path: "/boot-repaired".into(),
        domain: Some("boot.example.test".into()),
        secure: Some(true),
        http_only: false,
        same_site: Some("strict".into()),
        lifetime: 11,
        partitioned: true,
        ..SessionConfig::default()
    };
    let late = SessionConfig {
        cookie: Some("late_repaired_session".into()),
        path: "/late-repaired".into(),
        domain: None,
        secure: Some(false),
        same_site: None,
        lifetime: 13,
        ..SessionConfig::default()
    };
    let bad_boot = SessionConfig {
        cookie: Some("bad=name".into()),
        path: "/bad;path".into(),
        domain: Some("bad;domain".into()),
        lifetime: 0,
        same_site: Some("invalid".into()),
        max_bytes: 0,
        ..SessionConfig::default()
    };
    let late_value = serde_json::to_value(&late).map_err(Error::internal)?;
    for (bad_file, plan, expected) in [
        (
            true,
            Plan {
                boot: Some(boot.clone()),
                ..Plan::default()
            },
            boot,
        ),
        (
            true,
            Plan {
                late: Some(late_value.clone()),
                ..Plan::default()
            },
            late.clone(),
        ),
        (
            false,
            Plan {
                boot: Some(bad_boot),
                late: Some(late_value),
                ..Plan::default()
            },
            late,
        ),
    ] {
        let mut file = tempfile::NamedTempFile::new().map_err(Error::internal)?;
        let content = if bad_file {
            "[session]\ncookie='bad=name'\npath='/bad;path'\ndomain='bad;domain'\nlifetime=0\nsame_site='invalid'\nmax_bytes=0\n"
        } else {
            "[app]\nname='Valid File'\n"
        };
        // 类型正确的中间值可以被后续配置修复；类型或未知字段并未放宽。
        file.write_all(content.as_bytes())
            .map_err(Error::internal)?;
        file.flush().map_err(Error::internal)?;
        let source = sources(&plan).merge(Toml::file(file.path()));
        let mut app = TestApp::<HarnessApp>::builder(source).build().await?;
        let outcome = repaired_response(&app, &expected).await;
        finish(&mut app, outcome).await?;
    }
    Ok(())
}
