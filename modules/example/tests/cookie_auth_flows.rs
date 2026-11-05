//! Cookie Session 的Router构建拒绝与账户业务真实TCP回归；准备账户不调用外部邮件。
#![cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]

use figment::{Figment, providers::Serialized};
use openrust::core::{Application, ServiceProvider};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::{Cookie, axum};
use openrust::testing::TestDatabase;
use openrust::{App, auth::AuthFlows};
use serde_json::{Value, json};
use std::{error::Error as StdError, net::SocketAddr, time::Duration};

type TestResult<T = ()> = std::result::Result<T, Box<dyn StdError>>;
const BASE: &str = "/api/example/account";
const COOKIE_NAME: &str = "cookie_account_session";
const EMAIL: &str = "cookie-account@example.test";
const PASSWORD: &str = "correct horse battery staple";
const WAIT: Duration = Duration::from_secs(5);
struct Migrator;
#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = super::selected_test_auth::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}

#[cfg(feature = "encryption")]
#[tokio::test]
async fn cookie_account_login_preferences_and_logout_use_real_tcp_and_isolated_database()
-> TestResult {
    run_case::<0>().await
}
#[tokio::test]
async fn cookie_account_plain_configuration_is_rejected_before_router_ready_even_without_encryption()
-> TestResult {
    run_case::<1>().await
}
#[cfg(feature = "encryption")]
#[tokio::test]
async fn cookie_account_unprotected_id_cookie_is_rejected_before_router_ready() -> TestResult {
    run_case::<2>().await
}
#[cfg(feature = "encryption")]
#[tokio::test]
async fn cookie_account_any_unprotected_data_id_is_rejected() -> TestResult {
    run_case::<3>().await
}

struct CookieAuthApplication<const MODE: u8>;
impl<const MODE: u8> Application for CookieAuthApplication<MODE> {
    type Config = <crate::bootstrap::Server as Application>::Config;
    fn config_namespace() -> &'static str {
        crate::bootstrap::Server::config_namespace()
    }
    fn default_providers() -> openrust::core::DefaultProviders {
        let defaults = crate::bootstrap::Server::default_providers();
        if MODE == 0 {
            return defaults;
        }
        let cookie = match MODE {
            1 => openrust::http::CookieServiceProvider::plain(),
            2 => openrust::http::CookieServiceProvider::new().except([COOKIE_NAME]),
            _ => openrust::http::CookieServiceProvider::new()
                .except(["ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"]),
        };
        defaults.replace(
            openrust::core::ProviderId::of::<openrust::http::CookieServiceProvider>(),
            Box::new(cookie),
        )
    }
    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = crate::bootstrap::Server::providers(config);
        if MODE == 3 {
            providers.push(Box::new(ControlledIdRoute));
        }
        providers
    }
    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        crate::bootstrap::Server::modules(config)
    }
}
// 只在 MODE3 fixture 注册；固定服务端 ID 经真实 Cookie 层保护后用于数据 Cookie 攻击。
struct ControlledIdRoute;
impl ServiceProvider for ControlledIdRoute {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes.group(
                    openrust::http::RouteGroup::new()
                        .middleware_alias("cookies")
                        .middleware_alias("session"),
                    |routes| routes.get("/__cookie-auth/controlled-id", controlled_id),
                )
            })?;
            Ok(())
        })
    }
}
async fn controlled_id(
    session: openrust::session::Session,
) -> openrust::Result<openrust::http::Json<Value>> {
    session.set_id(Some("ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"))?;
    session.put_typed("example.cookie_probe", "ordinary")?;
    Ok(openrust::http::Json(json!({"ready":true})))
}
async fn run_case<const MODE: u8>() -> TestResult {
    let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping Cookie account TCP test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let database = TestDatabase::create::<Migrator>(&admin).await?;
    let source = cookie_auth_source(database.url(), &origin);
    let app_result =
        async {
            eprintln!("C04D4_PHASE phase=config_extract");
            let config =
                source.extract::<<CookieAuthApplication<MODE> as Application>::Config>()?;
            eprintln!("C04D4_PHASE phase=provider_start");
            let builder = openrust::core::ProviderBuilder::for_application::<
                CookieAuthApplication<MODE>,
            >(&config);
            Ok::<_, Box<dyn StdError>>(App::from_provider_runtime(
                builder
                    .start(crate::modules::example::test_sources(&(source))?)
                    .await?,
            )?)
        }
        .await;
    let app = match app_result {
        Ok(app) => app,
        Err(error) => {
            database.close().await?;
            return Err(error);
        }
    };
    eprintln!("C04D4_PHASE phase=fixture_start");
    let outcome = run_fixture::<MODE>(&app, listener).await;
    eprintln!("C04D4_PHASE phase=app_shutdown");
    let app_stopped = app.shutdown(tokio::time::Instant::now() + WAIT).await;
    drop(app);
    let database_stopped = database.close().await;
    outcome?;
    app_stopped?;
    database_stopped?;
    Ok(())
}
fn cookie_auth_source(database_url: &str, origin: &str) -> Figment {
    let mut framework = openrust::config::FrameworkConfig::default();
    framework.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
    Figment::from(Serialized::defaults(framework)).merge(Serialized::defaults(json!({
        "app":{"env":"test","url":origin},"database":{"url":database_url},
        "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
        "auth":{"defaults":{"guard":"web","passwords":"users","provider":null},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"app-users","table":"users"}},"jwt":{"issuer":origin,"audience":"cookie-account","active_key_id":"test","keys":{"test":key}}},
        "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":origin,"active_key_id":"test","keys":{"test":key}}},
        "session":{"driver":"cookie","secure":false,"cookie":COOKIE_NAME,"same_site":"strict"}
    })))
}
async fn run_fixture<const MODE: u8>(app: &App, listener: tokio::net::TcpListener) -> TestResult {
    eprintln!("C04D4_PHASE phase=auth_resolve");
    let flows = app.state().resolve::<AuthFlows>().await?;
    eprintln!("C04D4_PHASE phase=auth_register");
    let code = flows
        .register(EMAIL, PASSWORD, "cookie-fixture")
        .await?
        .ok_or("缺少账户验证准备码")?;
    eprintln!("C04D4_PHASE phase=auth_verify");
    if !flows
        .verify_email(EMAIL, code.code(), "cookie-fixture")
        .await?
    {
        return Err("账户验证准备失败".into());
    }
    eprintln!("C04D4_PHASE phase=auth_login_prepare");
    let user_id = flows
        .login(EMAIL, PASSWORD, "cookie-fixture-user")
        .await?
        .user_id();
    let address = listener.local_addr()?;
    eprintln!("C04D4_PHASE phase=router_build");
    let router = match openrust::http::build_router(app) {
        Err(error) if matches!(MODE, 1 | 2) && error.code() == "web_cookie_encryption_missing" => {
            // 标准CSRF组在服务监听前拒绝不可信ID Cookie；不把未执行的旧TCP断言计为通过。
            eprintln!("C04D4_PHASE phase=router_rejected_unprotected_web_cookie");
            return Ok(());
        }
        Err(error) => return Err(error.into()),
        Ok(_) if matches!(MODE, 1 | 2) => {
            return Err("不可信Cookie配置不得构建标准Web路由".into());
        }
        Ok(router) => router,
    };
    let (stop, signal) = tokio::sync::oneshot::channel();
    let mut server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = signal.await;
        })
        .await
    });
    let outcome = if MODE == 0 {
        business(app, address).await
    } else {
        unprotected_data_cookie_business(address, user_id).await
    };
    let _ = stop.send(());
    let stopped: TestResult = match tokio::time::timeout(WAIT, &mut server).await {
        Ok(Ok(completion)) => completion.map_err(Into::into),
        Ok(Err(error)) => Err(error.into()),
        Err(error) => {
            server.abort();
            let _ = server.await;
            Err(error.into())
        }
    };
    outcome?;
    stopped
}

struct Reply {
    status: reqwest::StatusCode,
    body: Value,
    set_cookie: Vec<String>,
}
#[derive(Clone, Default)]
struct Browser(Vec<(String, String)>);
impl Browser {
    fn header(&self) -> String {
        self.0
            .iter()
            .map(|(_, pair)| pair.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    }
    fn merge(&mut self, reply: &Reply) -> TestResult {
        for text in &reply.set_cookie {
            let model = Cookie::from_header(text)?;
            let pair = text.split(';').next().ok_or("缺少 Cookie 首段")?;
            let (name, _) = pair.split_once('=').ok_or("Cookie 首段无效")?;
            if name != COOKIE_NAME && openrust::session::SessionId::parse(name).is_err() {
                continue;
            }
            if model
                .expires()
                .is_some_and(|expires| expires < time::OffsetDateTime::now_utc())
            {
                self.0.retain(|(key, _)| key != name);
            } else if let Some(entry) = self.0.iter_mut().find(|(key, _)| key == name) {
                pair.clone_into(&mut entry.1);
            } else {
                self.0.push((name.to_owned(), pair.to_owned()));
            }
        }
        Ok(())
    }
    fn data_name(&self) -> TestResult<&str> {
        self.0
            .iter()
            .find(|(name, _)| openrust::session::SessionId::parse(name).is_ok())
            .map(|(name, _)| name.as_str())
            .ok_or_else(|| "缺少 Session 数据 Cookie".into())
    }
}
async fn request(
    client: &reqwest::Client,
    origin: &str,
    path: &str,
    payload: Option<Value>,
    browser: &Browser,
    csrf: Option<&str>,
) -> TestResult<Reply> {
    let phase = match (path, payload.is_some()) {
        ("/api/example/session", true) => "ordinary_put",
        ("/api/example/session", false) => "ordinary_read",
        ("/api/example/account/session/login", _) => "login_denied",
        ("/api/example/account/session/preference", false) => "forged_read",
        ("/api/example/account/session/preference", true) => "forged_write",
        _ => "account_other",
    };
    eprintln!("C04D4_PHASE phase={phase}");
    let request = if let Some(payload) = payload {
        client.post(format!("{origin}{path}")).json(&payload)
    } else {
        client.get(format!("{origin}{path}"))
    };
    let mut request = request
        .header("origin", origin)
        .header("x-example-request", "same-origin")
        .header("cookie", browser.header());
    if let Some(csrf) = csrf {
        request = request.header("x-csrf-token", csrf);
    }
    let mut response = request.send().await?;
    let status = response.status();
    let no_store = response
        .headers()
        .get("cache-control")
        .and_then(|value| value.to_str().ok())
        == Some("no-store");
    let set_cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|header| header.to_str().map(str::to_owned))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 4096 {
            return Err("账户响应超过 4 KiB".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: Value = serde_json::from_slice(&bytes)?;
    let code = match body["code"].as_str() {
        Some("ok") => "ok",
        Some("session_cookie_protection_required") => "session_cookie_protection_required",
        Some("unauthorized") => "unauthorized",
        Some("forbidden") => "forbidden",
        _ => "other",
    };
    let success = body["success"] == true;
    let message_redacted = body["message"] == "internal server error";
    eprintln!(
        "C04D4_AUTH phase={phase} status={} code={code} success={success} no_store={no_store} message_redacted={message_redacted}",
        status.as_u16()
    );
    if path.starts_with(BASE) && !no_store {
        return Err("账户响应缺少 no-store".into());
    }
    Ok(Reply {
        status,
        body,
        set_cookie,
    })
}
fn successful(reply: &Reply) -> TestResult {
    if reply.status != reqwest::StatusCode::OK
        || reply.body["success"] != true
        || reply.body["code"] != "ok"
    {
        return Err("Cookie 账户业务未成功".into());
    }
    Ok(())
}
fn deleted(reply: &Reply, name: &str) -> TestResult {
    let text = reply
        .set_cookie
        .iter()
        .find(|text| {
            text.split(';')
                .next()
                .and_then(|pair| pair.split_once('='))
                .is_some_and(|(key, _)| key == name)
        })
        .ok_or("缺少旧数据 Cookie 删除")?;
    let model = Cookie::from_header(text)?;
    if !text.to_ascii_lowercase().contains("; max-age=0")
        || model
            .expires()
            .is_none_or(|expiry| expiry >= time::OffsetDateTime::now_utc())
    {
        return Err("旧数据 Cookie 未严格过期".into());
    }
    Ok(())
}
async fn business(app: &App, address: SocketAddr) -> TestResult {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(WAIT)
        .build()?;
    let origin = format!("http://{address}");
    let mut browser = Browser::default();
    let csrf = login_browser(&client, &origin, &mut browser).await?;
    let read = request(
        &client,
        &origin,
        &format!("{BASE}/session/preference"),
        None,
        &browser,
        None,
    )
    .await?;
    successful(&read)?;
    browser.merge(&read)?;
    let set = request(
        &client,
        &origin,
        &format!("{BASE}/session/preference"),
        Some(json!({"density":"compact"})),
        &browser,
        Some(&csrf),
    )
    .await?;
    successful(&set)?;
    browser.merge(&set)?;
    let replay = browser.clone();
    let logged_in_data = browser.data_name()?.to_owned();
    let logout = request(
        &client,
        &origin,
        &format!("{BASE}/session/logout"),
        Some(json!({})),
        &browser,
        Some(&csrf),
    )
    .await?;
    successful(&logout)?;
    deleted(&logout, &logged_in_data)?;
    browser.merge(&logout)?;
    let denied = request(
        &client,
        &origin,
        &format!("{BASE}/session/preference"),
        None,
        &browser,
        None,
    )
    .await?;
    if denied.status != reqwest::StatusCode::UNAUTHORIZED
        || denied.body["success"] != false
        || denied.body["code"] != "unauthorized"
    {
        return Err("当前浏览器注销后仍可访问账户".into());
    }
    // 已复制的旧有效 Cookie 仍可重放；客户端快照不提供跨请求服务器撤销。
    let copied = request(
        &client,
        &origin,
        &format!("{BASE}/session/preference"),
        None,
        &replay,
        None,
    )
    .await?;
    successful(&copied)?;
    if copied.body["data"]["density"] != "compact" {
        return Err("旧快照重放边界未按契约保留".into());
    }
    reject_disabled_cookie(app, &client, &origin, &replay, &csrf).await
}

async fn reject_disabled_cookie(
    app: &App,
    client: &reqwest::Client,
    origin: &str,
    browser: &Browser,
    csrf: &str,
) -> TestResult {
    // Cookie 快照自身不能撤销，但每个新请求仍须经真实 app-users 检查数据库账户。
    let db = app.state().resolve::<openrust::db::Db>().await?;
    let affected = db
        .table("users")?
        .where_eq("email", EMAIL)?
        .update(vec![("disabled_at", chrono::Utc::now().naive_utc().into())])
        .await?;
    if affected != 1 {
        return Err("未实际禁用持有 Cookie 快照的账户".into());
    }
    for payload in [None, Some(json!({"density":"compact"}))] {
        let denied = request(
            client,
            origin,
            &format!("{BASE}/session/preference"),
            payload,
            browser,
            Some(csrf),
        )
        .await?;
        if denied.status != reqwest::StatusCode::UNAUTHORIZED
            || denied.body["success"] != false
            || denied.body["code"] != "unauthorized"
        {
            return Err("数据库账户禁用后旧 Cookie 快照仍可读写账户偏好".into());
        }
    }
    Ok(())
}

async fn login_browser(
    client: &reqwest::Client,
    origin: &str,
    browser: &mut Browser,
) -> TestResult<String> {
    let initial = request(client, origin, "/api/example/session", None, browser, None).await?;
    successful(&initial)?;
    assert_protected_cookies(&initial).await?;
    browser.merge(&initial)?;
    let anonymous_data = browser.data_name()?.to_owned();
    let login = request(
        client,
        origin,
        &format!("{BASE}/session/login"),
        Some(json!({"email":EMAIL,"password":PASSWORD})),
        browser,
        None,
    )
    .await?;
    successful(&login)?;
    assert_protected_cookies(&login).await?;
    deleted(&login, &anonymous_data)?;
    browser.merge(&login)?;
    if browser.data_name()? == anonymous_data {
        return Err("登录未轮换数据 Cookie 身份".into());
    }
    let csrf = login.body["data"]["csrf_token"]
        .as_str()
        .ok_or("缺少 CSRF token")?
        .to_owned();
    Ok(csrf)
}

async fn assert_protected_cookies(reply: &Reply) -> TestResult {
    use axum::extract::FromRequestParts;
    let mut browser = Browser::default();
    browser.merge(reply)?;
    let pairs = browser.header();
    let (mut parts, _) = axum::http::Request::get("/")
        .header("host", "localhost")
        .header("cookie", pairs)
        .body(axum::body::Body::empty())?
        .into_parts();
    // 只执行公开入站百分解码，不调用 Encryptor 或解出任何凭据。
    let info = openrust::http::RequestInfo::from_request_parts(&mut parts, &()).await?;
    let data_name = browser.data_name()?;
    let id = info
        .cookie(COOKIE_NAME)
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or("缺少 ID Cookie")?;
    if id == data_name || openrust::session::SessionId::parse(&id).is_ok() {
        return Err("ID Cookie 未实际保护".into());
    }
    let data = info
        .cookie(data_name)
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or("缺少数据 Cookie wire值")?;
    if serde_json::from_str::<Value>(&data)
        .is_ok_and(|value| value["data"].is_string() && value["expires"].is_i64())
    {
        return Err("数据 Cookie 暴露明文 Session envelope".into());
    }
    Ok(())
}
fn protection_denied(reply: &Reply) -> TestResult {
    if reply.status != reqwest::StatusCode::INTERNAL_SERVER_ERROR
        || reply.body["success"] != false
        || reply.body["code"] != "session_cookie_protection_required"
        || reply.body["message"] != "internal server error"
    {
        return Err("未保护 Cookie 认证未按安全边界拒绝".into());
    }
    Ok(())
}
fn forged_browser(user_id: i64, csrf: &str) -> TestResult<Browser> {
    let id = "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ";
    // 攻击者同时伪造当前 web Guard 身份和标准 token，不能靠旧键不被识别而通过负例。
    let attributes =
        json!({"login_web_ff9a88a790ea81b2b89656e56873ed257a77b366":user_id,"_token":csrf});
    let payload = json!({"data":attributes.to_string(),"expires":time::OffsetDateTime::now_utc().unix_timestamp()+600});
    let factory = openrust::http::CookieFactory::default();
    let mut browser = Browser::default();
    for (name, value) in [(COOKIE_NAME, id.to_owned()), (id, payload.to_string())] {
        let cookie = factory.make(name, value, 0, openrust::http::CookieOptions::default())?;
        let header = cookie.to_header(false)?;
        let pair = header
            .to_str()?
            .split(';')
            .next()
            .ok_or("伪造探针 Cookie 首段无效")?;
        browser.0.push((name.to_owned(), pair.to_owned()));
    }
    Ok(browser)
}
async fn unprotected_data_cookie_business(address: SocketAddr, user_id: i64) -> TestResult {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(WAIT)
        .build()?;
    let origin = format!("http://{address}");
    let mut browser = Browser::default();
    let ordinary = request(
        &client,
        &origin,
        "/api/example/session",
        Some(json!({"action":"put","value":"ordinary"})),
        &browser,
        None,
    )
    .await?;
    successful(&ordinary)?;
    browser.merge(&ordinary)?;
    let loaded = request(
        &client,
        &origin,
        "/api/example/session",
        None,
        &browser,
        None,
    )
    .await?;
    successful(&loaded)?;
    if loaded.body["data"]["preference"] != "ordinary" {
        return Err("匿名 Cookie Session 业务被全局关闭".into());
    }
    browser.merge(&loaded)?;
    let login = request(
        &client,
        &origin,
        &format!("{BASE}/session/login"),
        Some(json!({"email":EMAIL,"password":PASSWORD})),
        &browser,
        None,
    )
    .await?;
    protection_denied(&login)?;
    let csrf = "synthetic-csrf";
    let mut forged = forged_browser(user_id, csrf)?;
    preserve_protected_id(&client, &origin, &mut forged).await?;
    let read = request(
        &client,
        &origin,
        &format!("{BASE}/session/preference"),
        None,
        &forged,
        None,
    )
    .await?;
    protection_denied(&read)?;
    let write = request(
        &client,
        &origin,
        &format!("{BASE}/session/preference"),
        Some(json!({"density":"compact"})),
        &forged,
        Some(csrf),
    )
    .await?;
    protection_denied(&write)?;
    Ok(())
}

async fn preserve_protected_id(
    client: &reqwest::Client,
    origin: &str,
    forged: &mut Browser,
) -> TestResult {
    let prepared = request(
        client,
        origin,
        "/__cookie-auth/controlled-id",
        None,
        &Browser::default(),
        None,
    )
    .await?;
    successful(&prepared)?;
    let mut protected = Browser::default();
    protected.merge(&prepared)?;
    if protected.data_name()? != "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ" {
        return Err("受控 ID 未经真实保存链输出".into());
    }
    let id_pair = protected
        .0
        .into_iter()
        .find(|(name, _)| name == COOKIE_NAME)
        .ok_or("缺少受保护 ID Cookie")?;
    let entry = forged
        .0
        .iter_mut()
        .find(|(name, _)| name == COOKIE_NAME)
        .ok_or("缺少攻击 ID 槽")?;
    *entry = id_pair;
    Ok(())
}
