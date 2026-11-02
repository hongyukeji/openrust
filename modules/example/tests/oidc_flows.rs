//! 实际受控 Keycloak 网络与生产模块回调；RP 请求为 `TestApp` oneshot，不冒称浏览器/TCP全链路。
#![cfg(all(
    feature = "auth-oidc",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use super::services::oidc::ExampleOidcConfig;
use figment::{Figment, providers::Serialized};
use openrust::auth::{AuthFlows, JwtTokens};
use openrust::db::sea_orm::{ConnectionTrait, Statement};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use openrust::testing::{TestApp, TestDatabase, TestResponse};
use openrust::{Db, Error, Result};
use std::time::Duration;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
pub(super) const BASE: &str = "/api/example/oidc";
pub(super) const LOCAL_PASSWORD: &str = "correct horse battery staple";
pub(super) struct Idp {
    pub(super) config: ExampleOidcConfig,
    username: String,
    password: String,
}
impl Idp {
    pub(super) fn for_client(&self, client_id: &str, redirect_uri: &str) -> Self {
        let mut config = self.config.clone();
        client_id.clone_into(&mut config.client_id);
        redirect_uri.clone_into(&mut config.redirect_uri);
        Self {
            config,
            username: self.username.clone(),
            password: self.password.clone(),
        }
    }

    pub(super) fn environment() -> TestResult<Option<Self>> {
        let keys = [
            "OPENRUST_OIDC_ISSUER",
            "OPENRUST_EXAMPLE_OIDC_CLIENT_ID",
            "OPENRUST_EXAMPLE_OIDC_CLIENT_SECRET",
            "OPENRUST_EXAMPLE_OIDC_REDIRECT_URI",
            "OPENRUST_EXAMPLE_OIDC_USERNAME",
            "OPENRUST_EXAMPLE_OIDC_PASSWORD",
        ];
        let values = keys
            .into_iter()
            .map(std::env::var)
            .collect::<std::result::Result<Vec<_>, _>>();
        let Ok(values) = values else {
            if std::env::var("OPENRUST_REQUIRE_OIDC_IDP").as_deref() == Ok("1") {
                return Err("required example OIDC fixture configuration is missing".into());
            }
            eprintln!(
                "skipping real example OIDC: configure issuer and OPENRUST_EXAMPLE_OIDC client/callback/test-user variables"
            );
            return Ok(None);
        };
        let issuer = url::Url::parse(&values[0]).map_err(|_| "fixture issuer invalid")?;
        if issuer.scheme() != "http"
            || issuer
                .host_str()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                .is_none_or(|ip| !ip.is_loopback())
        {
            return Err("example OIDC acceptance requires controlled loopback Keycloak".into());
        }
        let callback = url::Url::parse(&values[3]).map_err(|_| "fixture callback invalid")?;
        if callback.path() != "/api/example/oidc/callback" {
            return Err("fixture callback must use the production example OIDC route".into());
        }
        Ok(Some(Self {
            config: ExampleOidcConfig {
                issuer: values[0].clone(),
                client_id: values[1].clone(),
                client_secret: Some(values[2].clone()),
                redirect_uri: values[3].clone(),
                allow_loopback: true,
            },
            username: values[4].clone(),
            password: values[5].clone(),
        }))
    }
}

#[tokio::test]
async fn configured_oidc_rejects_missing_business_dependencies_before_ready() -> TestResult {
    let Some(idp) = Idp::environment()? else {
        return Ok(());
    };
    let admin = std::env::var("OPENRUST_TEST_DATABASE_URL")?;
    let database = TestDatabase::create::<Migrator>(&admin).await?;
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(openrust::db::DatabaseServiceProvider)
        .provider(super::providers::ExampleServiceProvider::new(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        ))
        .provider(super::services::oidc::ExampleOidcServiceProvider);
    let source = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Serialized::defaults(serde_json::json!({
        "app": {"env": "test"},
        "database": {"url": database.url()},
        "example": {"oidc": {
            "issuer": idp.config.issuer,
            "client_id": idp.config.client_id,
            "client_secret": idp.config.client_secret,
            "redirect_uri": idp.config.redirect_uri,
            "allow_loopback": true
        }}
    })));
    let error = builder
        .start(crate::modules::example::test_sources(&(source))?)
        .await
        .err()
        .ok_or("OIDC without AuthFlows and Encryptor unexpectedly became ready")?;
    assert_eq!(error.code(), "service_not_bound");
    database.close().await?;
    Ok(())
}
struct Migrator;
#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut m = openrust::db::FrameworkMigrator::migrations();
        // RP/logout/backchannel 验证服务器端撤销；在同一原生表上显式安装增强能力。
        m.push(Box::new(openrust::session::GuardedSessionMigration));
        m.push(Box::new(openrust::session::GuardedSessionGcMigration));
        m.extend(super::database::migrations::Migrator::migrations());
        m
    }
}
pub(super) struct Fixture {
    pub(super) app: TestApp<crate::bootstrap::Server>,
    database: TestDatabase,
}
impl Fixture {
    async fn create(idp: &Idp) -> TestResult<Option<Self>> {
        Self::create_at(idp, "http://127.0.0.1:3000").await
    }
    async fn create_at(idp: &Idp, origin: &str) -> TestResult<Option<Self>> {
        Self::create_at_with_http_status(idp, origin, openrust::config::HttpStatusMode::Semantic)
            .await
    }
    async fn create_at_with_http_status(
        idp: &Idp,
        origin: &str,
        http_status: openrust::config::HttpStatusMode,
    ) -> TestResult<Option<Self>> {
        let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("skipping example OIDC DB test: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(None);
        };
        let database = TestDatabase::create::<Migrator>(&admin).await?;
        let connection = database.connection()?;
        let backend = connection.get_database_backend();
        let table = match backend {
            openrust::db::sea_orm::DbBackend::Postgres => "\"example_oidc_bind_intents\"",
            openrust::db::sea_orm::DbBackend::MySql => "`example_oidc_bind_intents`",
            _ => return Err("unsupported browser bind database backend".into()),
        };
        connection
            .query_one_raw(Statement::from_string(
                backend,
                format!("SELECT 1 FROM {table} LIMIT 1"),
            ))
            .await
            .map_err(|_| "OIDC browser bind intent table is unavailable")?;
        drop(connection);
        let mut framework = openrust::config::FrameworkConfig::default();
        framework.api.http_status = http_status;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        // client_secret 的正式配置序列化会省略秘密；测试源显式传入，不输出到日志。
        let source=Figment::from(Serialized::defaults(framework)).merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test","url":origin},"database":{"url":database.url()},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"oidc-example","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}},
            "session":{"driver":"database-guarded","secure":false,"cookie":"example_oidc_session","same_site":"lax"},
            "example":{"oidc":{"issuer":idp.config.issuer,"client_id":idp.config.client_id,"client_secret":idp.config.client_secret,"redirect_uri":idp.config.redirect_uri,"allow_loopback":true}}
        })));
        let app = TestApp::<crate::bootstrap::Server>::builder(
            crate::modules::example::test_sources(&(source))?,
        )
        .build()
        .await?;
        Ok(Some(Self { app, database }))
    }
    async fn close(mut self) -> TestResult {
        self.app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(10))
            .await?;
        self.database.close().await?;
        Ok(())
    }
    async fn authorize(&self) -> TestResult<(String, String)> {
        let response = self
            .app
            .request(request("GET", "/authorize", None, None, None)?)
            .await?;
        if !response.status().is_redirection() {
            let code = serde_json::from_slice::<serde_json::Value>(response.body())
                .ok()
                .and_then(|body| body.get("code")?.as_str().map(str::to_owned));
            return Err(format!(
                "OIDC authorize returned status {} and safe code {}",
                response.status(),
                code.as_deref().unwrap_or("unavailable")
            )
            .into());
        }
        response.assert_header("cache-control", "no-store")?;
        let url = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or("authorization Location missing")?
            .to_owned();
        let cookie = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|v| {
                openrust::http::Cookie::from_header(v).is_ok_and(|cookie| {
                    cookie.http_only()
                        && cookie.secure() == Some(true)
                        && cookie.same_site() == Some(openrust::http::SameSite::Lax)
                })
            })
            .ok_or("secure OIDC binding cookie missing")?;
        Ok((
            url,
            cookie.split(';').next().ok_or("cookie missing")?.to_owned(),
        ))
    }
    pub(super) async fn local_account(&self, email: &str) -> TestResult<String> {
        let flows = self.app.app()?.state().resolve::<AuthFlows>().await?;
        let code = flows
            .register(email, LOCAL_PASSWORD, "local-fixture")
            .await?
            .ok_or("verification code missing")?;
        assert!(
            flows
                .verify_email(email, code.code(), "local-fixture")
                .await?
        );
        let credential = flows.login(email, LOCAL_PASSWORD, "local-fixture").await?;
        Ok(self
            .app
            .app()?
            .state()
            .resolve::<JwtTokens>()
            .await?
            .issue_access_token(credential.user_id())?)
    }
}
fn request(
    method: &str,
    path: &str,
    token: Option<&str>,
    cookie: Option<&str>,
    body: Option<serde_json::Value>,
) -> Result<Request<Body>> {
    let mut builder = Request::builder()
        .method(method)
        .uri(format!("{BASE}{path}"))
        .header("host", "localhost")
        .extension(ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            45678,
        ))))
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    let body = body
        .map(|value| serde_json::to_vec(&value))
        .transpose()
        .map_err(Error::internal)?
        .unwrap_or_default();
    builder.body(Body::from(body)).map_err(Error::internal)
}
pub(super) fn callback_path(state: &str, code: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("state", state)
        .append_pair("code", code)
        .finish();
    format!("/callback?{query}")
}
fn cleared(response: &TestResponse) -> TestResult {
    response.assert_header("cache-control", "no-store")?;
    assert!(
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .any(|v| v.contains("Max-Age=0")
                && openrust::http::Cookie::from_header(v)
                    .is_ok_and(|cookie| cookie.http_only() && cookie.secure() == Some(true)))
    );
    Ok(())
}

// 仅复用既有 Keycloak 测试的 HTML 表单驱动方式；OIDC/PKCE/ID Token 验证仍由公开 Provider 完成。
pub(super) struct AuthorizationGrant {
    pub(super) state: String,
    pub(super) code: String,
    pub(super) browser: reqwest::Client,
    pub(super) cookies: String,
}
async fn authorization_code(idp: &Idp, url: &str) -> TestResult<(String, String)> {
    let grant = authorization_grant(idp, url).await?;
    Ok((grant.state, grant.code))
}
#[allow(
    clippy::too_many_lines,
    reason = "真实 IdP 表单、重定向与来源边界须在同一次授权链内检查"
)]
pub(super) async fn authorization_grant(idp: &Idp, url: &str) -> TestResult<AuthorizationGrant> {
    let browser = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let authorize = url::Url::parse(url).map_err(|_| "authorize URL invalid")?;
    let issuer = url::Url::parse(&idp.config.issuer).map_err(|_| "fixture issuer invalid")?;
    if authorize.origin() != issuer.origin() {
        return Err("authorize URL escaped fixture origin".into());
    }
    let page = browser
        .get(authorize.clone())
        .send()
        .await
        .map_err(|_| "controlled IdP login page unavailable")?;
    if !page.status().is_success() {
        return Err("controlled IdP login page rejected".into());
    }
    let cookies = page
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok()?.split(';').next())
        .collect::<Vec<_>>()
        .join("; ");
    if cookies.is_empty() {
        return Err("controlled IdP did not set a login session cookie".into());
    }
    let html = page
        .text()
        .await
        .map_err(|_| "controlled IdP login page unreadable")?;
    let mut action = None;
    for remainder in html.split("<form").skip(1) {
        let tag = remainder.split('>').next().ok_or("login form malformed")?;
        if attribute(tag, "id").as_deref() == Some("kc-form-login") {
            action = attribute(tag, "action");
            break;
        }
    }
    let action = authorize
        .join(
            &action
                .ok_or("controlled login form missing")?
                .replace("&amp;", "&"),
        )
        .map_err(|_| "controlled login action invalid")?;
    if action.origin() != issuer.origin() {
        return Err("login action escaped fixture origin".into());
    }
    let response = browser
        .post(action)
        .header(reqwest::header::COOKIE, &cookies)
        .form(&[
            ("username", idp.username.as_str()),
            ("password", idp.password.as_str()),
            ("credentialId", ""),
        ])
        .send()
        .await
        .map_err(|_| "controlled IdP login failed")?;
    if !response.status().is_redirection() {
        return Err("controlled IdP did not return authorization redirect".into());
    }
    let redirect = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or("controlled IdP redirect missing")?;
    let redirect = url::Url::parse(redirect).map_err(|_| "controlled callback URL invalid")?;
    let expected =
        url::Url::parse(&idp.config.redirect_uri).map_err(|_| "fixture callback invalid")?;
    if redirect.origin() != expected.origin() || redirect.path() != expected.path() {
        return Err("unexpected callback destination".into());
    }
    let state = redirect
        .query_pairs()
        .find_map(|(k, v)| (k == "state").then(|| v.into_owned()))
        .ok_or("state missing")?;
    let code = redirect
        .query_pairs()
        .find_map(|(k, v)| (k == "code").then(|| v.into_owned()))
        .ok_or("authorization code missing")?;
    let mut jar = std::collections::BTreeMap::new();
    for pair in cookies.split("; ").chain(
        response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok()?.split(';').next()),
    ) {
        if let Some((name, value)) = pair.split_once('=') {
            jar.insert(name.to_owned(), value.to_owned());
        }
    }
    let cookies = jar
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    Ok(AuthorizationGrant {
        state,
        code,
        browser,
        cookies,
    })
}
pub(super) fn attribute(tag: &str, name: &str) -> Option<String> {
    let mut remainder = tag;
    while let Some(index) = remainder.find(name) {
        let boundary = index == 0
            || remainder[..index]
                .chars()
                .last()
                .is_some_and(char::is_whitespace);
        remainder = &remainder[index + name.len()..];
        if !boundary {
            continue;
        }
        let value = remainder.trim_start().strip_prefix('=')?.trim_start();
        let quote = value.chars().next()?;
        if !matches!(quote, '\'' | '"') {
            return None;
        }
        let value = &value[quote.len_utf8()..];
        return Some(value[..value.find(quote)?].into());
    }
    None
}
#[test]
fn oidc_configuration_rejects_untrusted_urls_and_does_not_serialize_client_secret() -> Result<()> {
    for issuer in [
        "http://127.0.0.1/realms/test",
        "https://user:SECRET_SENTINEL@example.test/",
        "file:///etc/passwd",
    ] {
        let config = ExampleOidcConfig {
            issuer: issuer.into(),
            client_id: "example".into(),
            client_secret: Some("SECRET_SENTINEL".into()),
            redirect_uri: "https://example.test/api/example/oidc/callback".into(),
            allow_loopback: false,
        };
        let error = config
            .validate()
            .err()
            .ok_or_else(|| Error::BadRequest("untrusted issuer accepted".into()))?;
        assert!(!format!("{error:?}").contains("SECRET_SENTINEL"));
    }
    let config = ExampleOidcConfig {
        issuer: "https://issuer.example.test".into(),
        client_id: "example".into(),
        client_secret: Some("SECRET_SENTINEL".into()),
        redirect_uri: "https://example.test/api/example/oidc/callback".into(),
        allow_loopback: false,
    };
    config.validate()?;
    let mut wrong_callback = config.clone();
    wrong_callback.redirect_uri = "https://example.test/callback".into();
    assert!(wrong_callback.validate().is_err());
    assert!(
        !serde_json::to_string(&config)
            .map_err(Error::internal)?
            .contains("SECRET_SENTINEL")
    );
    Ok(())
}
#[tokio::test]
async fn keycloak_module_login_cookie_state_and_replay_are_validated() -> TestResult {
    let Some(idp) = Idp::environment()? else {
        return Ok(());
    };
    let Some(fixture) = Fixture::create(&idp).await? else {
        return Ok(());
    };
    let (url, cookie) = fixture.authorize().await?;
    let (state, code) = authorization_code(&idp, &url).await?;
    let missing = fixture
        .app
        .request(request(
            "GET",
            &callback_path(&state, &code),
            None,
            None,
            None,
        )?)
        .await?;
    if missing.status() != StatusCode::UNAUTHORIZED {
        let code = serde_json::from_slice::<serde_json::Value>(missing.body())
            .ok()
            .and_then(|body| body["code"].as_str().map(str::to_owned));
        return Err(format!(
            "missing-cookie callback status {}, safe code {}",
            missing.status(),
            code.as_deref().unwrap_or("unavailable")
        )
        .into());
    }
    cleared(&missing)?;
    let (name, _) = cookie
        .split_once('=')
        .ok_or("binding cookie pair malformed")?;
    let invalid = format!("{name}=invalid-duplicate");
    for (multiple_headers, invalid_first) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        let (first, second) = if invalid_first {
            (&invalid, &cookie)
        } else {
            (&cookie, &invalid)
        };
        let mut duplicate = request("GET", &callback_path(&state, &code), None, None, None)?;
        if multiple_headers {
            duplicate.headers_mut().append("cookie", first.parse()?);
            duplicate.headers_mut().append("cookie", second.parse()?);
        } else {
            duplicate
                .headers_mut()
                .insert("cookie", format!("{first}; {second}").parse()?);
        }
        let rejected = fixture.app.request(duplicate).await?;
        rejected.assert_status(StatusCode::UNAUTHORIZED)?;
        cleared(&rejected)?;
    }
    // 缺少或重复浏览器绑定不得消费合法 state；携带原绑定完成同一个真实授权码。
    let response = fixture
        .app
        .request(request(
            "GET",
            &callback_path(&state, &code),
            None,
            Some(&cookie),
            None,
        )?)
        .await?;
    if response.status() != StatusCode::OK {
        let code = serde_json::from_slice::<serde_json::Value>(response.body())
            .ok()
            .and_then(|body| body["code"].as_str().map(str::to_owned));
        return Err(format!(
            "OIDC callback status {}, safe code {}",
            response.status(),
            code.as_deref().unwrap_or("unavailable")
        )
        .into());
    }
    cleared(&response)?;
    let body: serde_json::Value = response.json()?;
    assert_eq!(body["data"]["status"], "authenticated");
    let replay = fixture
        .app
        .request(request(
            "GET",
            &callback_path(&state, &code),
            None,
            Some(&cookie),
            None,
        )?)
        .await?;
    assert!(!replay.status().is_success());
    cleared(&replay)?;
    fixture.close().await
}
#[tokio::test]
async fn keycloak_native_binding_requires_local_password_and_unbind_is_owner_scoped() -> TestResult
{
    let Some(idp) = Idp::environment()? else {
        return Ok(());
    };
    let Some(fixture) = Fixture::create(&idp).await? else {
        return Ok(());
    };
    let email = "oidc-local-account@example.test";
    let token = fixture.local_account(email).await?;
    let (url, cookie) = fixture.authorize().await?;
    let (state, code) = authorization_code(&idp, &url).await?;
    // 原生客户端在 redirect 前截获 code；不先调用会消费 state 的浏览器登录 callback。
    let bad=fixture.app.request(request("POST","/bind",Some(&token),Some(&cookie),Some(serde_json::json!({"email":email,"password":"wrong password","state":state,"code":code})))?).await?;
    assert!(!bad.status().is_success());
    cleared(&bad)?;
    let (url, cookie) = fixture.authorize().await?;
    let (state, code) = authorization_code(&idp, &url).await?;
    let bound=fixture.app.request(request("POST","/bind",Some(&token),Some(&cookie),Some(serde_json::json!({"email":email,"password":LOCAL_PASSWORD,"state":state,"code":code})))?).await?;
    bound.assert_status(StatusCode::OK)?;
    cleared(&bound)?;
    let links = fixture
        .app
        .request(request("GET", "/links", Some(&token), None, None)?)
        .await?;
    let body: serde_json::Value = links.json()?;
    let links = body["data"].as_array().ok_or("links missing")?;
    assert_eq!(links.len(), 1);
    let subject = links[0]["subject"]
        .as_str()
        .ok_or("linked subject missing")?;
    let other = fixture
        .local_account("oidc-other-owner@example.test")
        .await?;
    let forbidden = fixture
        .app
        .request(request(
            "POST",
            "/unbind",
            Some(&other),
            None,
            Some(serde_json::json!({"email":email,"password":LOCAL_PASSWORD,"subject":subject})),
        )?)
        .await?;
    assert!(!forbidden.status().is_success());
    let retained = fixture
        .app
        .request(request("GET", "/links", Some(&token), None, None)?)
        .await?;
    let retained: serde_json::Value = retained.json()?;
    assert_eq!(retained["data"].as_array().ok_or("links missing")?.len(), 1);
    let removed = fixture
        .app
        .request(request(
            "POST",
            "/unbind",
            Some(&token),
            None,
            Some(serde_json::json!({"email":email,"password":LOCAL_PASSWORD,"subject":subject})),
        )?)
        .await?;
    removed.assert_status(StatusCode::OK)?;
    let removed: serde_json::Value = removed.json()?;
    assert_eq!(removed["data"]["removed"], true);
    fixture.close().await
}

#[tokio::test]
async fn keycloak_existing_email_requires_explicit_link_instead_of_automatic_merge() -> TestResult {
    let Some(idp) = Idp::environment()? else {
        return Ok(());
    };
    let Some(first) = Fixture::create(&idp).await? else {
        return Ok(());
    };
    let (url, cookie) = first.authorize().await?;
    let (state, code) = authorization_code(&idp, &url).await?;
    let response = first
        .app
        .request(request(
            "GET",
            &callback_path(&state, &code),
            None,
            Some(&cookie),
            None,
        )?)
        .await?;
    response.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = response.json()?;
    let user_id = body["data"]["credentials"]["user_id"]
        .as_i64()
        .ok_or("external user missing")?;
    let db = first.app.app()?.state().resolve::<Db>().await?;
    let connection = db.connection()?;
    let backend = connection.get_database_backend();
    let placeholder = if backend == openrust::db::sea_orm::DbBackend::Postgres {
        "$1"
    } else {
        "?"
    };
    let row = connection
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            format!(
                "SELECT email FROM {} WHERE id = {placeholder}",
                db.quoted_table_name("users")?
            ),
            [user_id.into()],
        ))
        .await?
        .ok_or("external user row missing")?;
    let email: String = row.try_get("", "email")?;
    drop(connection);
    drop(db);
    first.close().await?;

    let Some(second) = Fixture::create(&idp).await? else {
        return Ok(());
    };
    let token = second.local_account(&email).await?;
    let (url, cookie) = second.authorize().await?;
    let (state, code) = authorization_code(&idp, &url).await?;
    let refused = second
        .app
        .request(request(
            "GET",
            &callback_path(&state, &code),
            None,
            Some(&cookie),
            None,
        )?)
        .await?;
    assert!(!refused.status().is_success());
    cleared(&refused)?;
    let body: serde_json::Value = refused.json()?;
    assert_eq!(body["code"], "auth_external_identity_link_required");
    let links = second
        .app
        .request(request("GET", "/links", Some(&token), None, None)?)
        .await?;
    let body: serde_json::Value = links.json()?;
    assert_eq!(body["data"].as_array().ok_or("links missing")?.len(), 0);
    second.close().await
}

// 使用真实 RP TCP 和真实 IdP 网络；授权重定向由测试拦截，非自动化浏览器导航。
pub(super) struct BrowserNode {
    pub(super) fixture: Fixture,
    pub(super) origin: String,
    pub(super) client: reqwest::Client,
    running: BrowserServer,
}
struct BrowserServer {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    server: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl BrowserServer {
    async fn close(&mut self) -> TestResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut server) = self.server.take() {
            if let Ok(result) = tokio::time::timeout(Duration::from_secs(10), &mut server).await {
                result??;
            } else {
                server.abort();
                let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
                return Err("RP server shutdown exceeded deadline".into());
            }
        }
        Ok(())
    }
}
impl Drop for BrowserServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = &self.server {
            server.abort();
        }
    }
}
pub(super) struct BrowserSession {
    pub(super) cookie: String,
    pub(super) csrf: String,
    pub(super) email: String,
}
struct BrowserAuthorization {
    state: String,
    code: String,
    binding: String,
}
impl BrowserNode {
    pub(super) async fn start(idp: &Idp) -> TestResult<Option<Self>> {
        Self::start_with_http_status(idp, openrust::config::HttpStatusMode::Semantic).await
    }
    async fn start_with_http_status(
        idp: &Idp,
        http_status: openrust::config::HttpStatusMode,
    ) -> TestResult<Option<Self>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let origin = format!("http://{address}");
        let Some(fixture) = Fixture::create_at_with_http_status(idp, &origin, http_status).await?
        else {
            return Ok(None);
        };
        let router = openrust::http::build_router(fixture.app.app()?)?;
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            openrust::http::axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
        });
        let running = BrowserServer {
            stop: Some(stop),
            server: Some(server),
        };
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Some(Self {
            fixture,
            origin,
            client,
            running,
        }))
    }
    pub(super) async fn close(mut self) -> TestResult {
        let stopped = self.running.close().await;
        let closed = self.fixture.close().await;
        stopped?;
        closed
    }
    async fn post(
        &self,
        path: &str,
        session: &BrowserSession,
        binding: Option<&str>,
        value: &serde_json::Value,
    ) -> TestResult<reqwest::Response> {
        let cookie = binding.map_or_else(
            || session.cookie.clone(),
            |binding| format!("{}; {binding}", session.cookie),
        );
        self.client
            .post(format!("{}{BASE}{path}", self.origin))
            .header("origin", &self.origin)
            .header("x-example-request", "same-origin")
            .header("x-csrf-token", &session.csrf)
            .header("cookie", cookie)
            .json(value)
            .send()
            .await
            .map_err(|_| "RP request failed".into())
    }
    async fn session(&self, email: &str) -> TestResult<BrowserSession> {
        self.fixture.local_account(email).await?;
        self.existing_session(email).await
    }
    pub(super) async fn existing_session(&self, email: &str) -> TestResult<BrowserSession> {
        let response = self
            .client
            .post(format!("{}/api/example/account/session/login", self.origin))
            .header("origin", &self.origin)
            .header("x-example-request", "same-origin")
            .json(&serde_json::json!({"email":email,"password":LOCAL_PASSWORD}))
            .send()
            .await
            .map_err(|_| "session login request failed")?;
        if response.status() != StatusCode::OK {
            let status = response.status();
            let body: serde_json::Value = response.json().await?;
            return Err(format!(
                "browser session login status {status}, safe code {}",
                body["code"]
            )
            .into());
        }
        let cookie = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with("example_oidc_session="))
            .and_then(|value| value.split(';').next())
            .ok_or("session cookie missing")?
            .to_owned();
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|_| "session login response invalid")?;
        let csrf = value["data"]["csrf_token"]
            .as_str()
            .ok_or("session CSRF token missing")?
            .to_owned();
        Ok(BrowserSession {
            cookie,
            csrf,
            email: email.into(),
        })
    }
    async fn authorize_bind(
        &self,
        idp: &Idp,
        session: &BrowserSession,
    ) -> TestResult<BrowserAuthorization> {
        let response = self
            .post(
                "/bind/authorize",
                session,
                None,
                &serde_json::json!({"email":session.email,"password":LOCAL_PASSWORD}),
            )
            .await?;
        if response.status() != StatusCode::OK {
            let status = response.status();
            let body: serde_json::Value = response.json().await?;
            return Err(format!(
                "browser bind authorize status {status}, safe code {}",
                body["code"]
            )
            .into());
        }
        let binding = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| {
                openrust::http::Cookie::from_header(value).is_ok_and(|cookie| {
                    cookie.http_only()
                        && cookie.secure() == Some(true)
                        && cookie.same_site() == Some(openrust::http::SameSite::Lax)
                })
            })
            .and_then(|value| value.split(';').next())
            .ok_or("OIDC binding cookie missing")?
            .to_owned();
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|_| "authorization response invalid")?;
        let url = value["data"]["authorization_url"]
            .as_str()
            .ok_or("authorization URL missing")?;
        let (state, code) = authorization_code(idp, url).await?;
        Ok(BrowserAuthorization {
            state,
            code,
            binding,
        })
    }
    async fn callback(
        &self,
        session: &BrowserSession,
        authorization: &BrowserAuthorization,
    ) -> TestResult<reqwest::Response> {
        self.client
            .get(format!(
                "{}{BASE}{}",
                self.origin,
                callback_path(&authorization.state, &authorization.code)
            ))
            .header(
                "cookie",
                format!("{}; {}", session.cookie, authorization.binding),
            )
            .send()
            .await
            .map_err(|_| "RP callback request failed".into())
    }
    async fn confirm(
        &self,
        session: &BrowserSession,
        authorization: &BrowserAuthorization,
        password: &str,
    ) -> TestResult<reqwest::Response> {
        self.post("/bind/confirm", session, Some(&authorization.binding), &serde_json::json!({
            "state":authorization.state,"code":authorization.code,"email":session.email,"password":password
        })).await
    }
    async fn expire(&self, authorization: &BrowserAuthorization) -> TestResult {
        let db = self.fixture.app.app()?.state().resolve::<Db>().await?;
        let backend = db.backend()?;
        let table = match backend {
            openrust::db::sea_orm::DbBackend::Postgres => "\"example_oidc_bind_intents\"",
            openrust::db::sea_orm::DbBackend::MySql => "`example_oidc_bind_intents`",
            _ => return Err("unsupported browser bind database backend".into()),
        };
        let parameter = if backend == openrust::db::sea_orm::DbBackend::Postgres {
            "$1"
        } else {
            "?"
        };
        db.connection()?
            .execute_raw(Statement::from_sql_and_values(
                backend,
                format!("UPDATE {table} SET expires_at_ms = 0 WHERE state_hash = {parameter}"),
                [super::services::oidc::state_hash(&authorization.state).into()],
            ))
            .await?;
        Ok(())
    }
}
fn response_is_private(response: &reqwest::Response) {
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    assert_eq!(
        response
            .headers()
            .get("referrer-policy")
            .and_then(|value| value.to_str().ok()),
        Some("no-referrer")
    );
}
#[tokio::test]
async fn always_ok_http_mode_keeps_oidc_business_failure_in_error_envelope() -> TestResult {
    let Some(idp) = Idp::environment()? else {
        return Ok(());
    };
    let Some(node) =
        BrowserNode::start_with_http_status(&idp, openrust::config::HttpStatusMode::AlwaysOk)
            .await?
    else {
        return Ok(());
    };
    let owner = node.session("browser-always-ok@example.test").await?;
    let authorization = node.authorize_bind(&idp, &owner).await?;
    let page = node.callback(&owner, &authorization).await?;
    assert_eq!(page.status(), StatusCode::OK);
    let html = page.text().await.map_err(|_| "confirmation HTML invalid")?;
    assert!(html.contains("response.ok&&payload?.success===true&&payload?.data?.status==='bound'"));

    let failure = node
        .confirm(&owner, &authorization, "wrong password")
        .await?;
    assert_eq!(failure.status(), StatusCode::OK);
    let envelope: serde_json::Value = failure
        .json()
        .await
        .map_err(|_| "failure envelope invalid")?;
    assert_eq!(envelope["success"], false);
    assert_ne!(envelope["data"]["status"], "bound");

    let replay = node.confirm(&owner, &authorization, LOCAL_PASSWORD).await?;
    assert_eq!(replay.status(), StatusCode::OK);
    let envelope: serde_json::Value = replay.json().await.map_err(|_| "replay envelope invalid")?;
    assert_eq!(envelope["success"], false);

    let successful = node.authorize_bind(&idp, &owner).await?;
    let callback = node.callback(&owner, &successful).await?;
    assert_eq!(callback.status(), StatusCode::OK);
    let html = callback
        .text()
        .await
        .map_err(|_| "confirmation HTML invalid")?;
    assert!(html.contains("payload?.success===true&&payload?.data?.status==='bound'"));
    let response = node.confirm(&owner, &successful, LOCAL_PASSWORD).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let envelope: serde_json::Value = response
        .json()
        .await
        .map_err(|_| "success envelope invalid")?;
    assert_eq!(envelope["success"], true);
    assert_eq!(envelope["data"]["status"], "bound");
    node.close().await
}

#[tokio::test]
async fn browser_binding_uses_same_session_and_confirmation_is_once_only_over_tcp() -> TestResult {
    let Some(idp) = Idp::environment()? else {
        return Ok(());
    };
    let Some(node) = BrowserNode::start(&idp).await? else {
        return Ok(());
    };
    let owner = node.session("browser-owner@example.test").await?;
    let other_browser = node.existing_session(&owner.email).await?;
    let other_owner = node.session("browser-other-owner@example.test").await?;
    let authorization = node.authorize_bind(&idp, &owner).await?;
    // 拥有同一账户 Session 也不能替代发起绑定的浏览器。
    let wrong_browser = node.callback(&other_browser, &authorization).await?;
    assert!(!wrong_browser.status().is_success());
    let page = node.callback(&owner, &authorization).await?;
    assert_eq!(page.status(), StatusCode::OK);
    response_is_private(&page);
    assert!(page.headers().get("content-security-policy").is_some());
    let html = page.text().await.map_err(|_| "confirmation HTML invalid")?;
    assert!(html.contains("/api/example/oidc/bind/confirm"));
    assert!(!html.contains(LOCAL_PASSWORD));
    let repeated = node.callback(&owner, &authorization).await?;
    assert!(!repeated.status().is_success());
    let foreign = node
        .confirm(&other_owner, &authorization, LOCAL_PASSWORD)
        .await?;
    assert!(!foreign.status().is_success());
    let (first, second) = tokio::join!(
        node.confirm(&owner, &authorization, LOCAL_PASSWORD),
        node.confirm(&owner, &authorization, LOCAL_PASSWORD)
    );
    let first = first?;
    let second = second?;
    assert_ne!(first.status().is_success(), second.status().is_success());
    for response in [&first, &second] {
        response_is_private(response);
    }
    let replay = node.confirm(&owner, &authorization, LOCAL_PASSWORD).await?;
    assert!(!replay.status().is_success());
    // 意图的终态仍在，旧回调不能成为“新登录”。
    let replay = node.callback(&owner, &authorization).await?;
    assert!(!replay.status().is_success());
    node.close().await
}
#[tokio::test]
async fn browser_binding_password_origin_csrf_expiry_and_error_are_fail_closed() -> TestResult {
    let Some(idp) = Idp::environment()? else {
        return Ok(());
    };
    let Some(node) = BrowserNode::start(&idp).await? else {
        return Ok(());
    };
    let owner = node.session("browser-boundaries@example.test").await?;
    let authorization = node.authorize_bind(&idp, &owner).await?;
    let page = node.callback(&owner, &authorization).await?;
    assert_eq!(page.status(), StatusCode::OK);
    // 框架 CSRF 与业务 Origin 均在任何确认消费前拒绝。
    let body = serde_json::json!({"state":authorization.state,"code":authorization.code,"email":owner.email,"password":LOCAL_PASSWORD});
    let wrong_origin = node
        .client
        .post(format!("{}{BASE}/bind/confirm", node.origin))
        .header("origin", "https://untrusted.example.test")
        .header("x-example-request", "same-origin")
        .header("x-csrf-token", &owner.csrf)
        .header(
            "cookie",
            format!("{}; {}", owner.cookie, authorization.binding),
        )
        .json(&body)
        .send()
        .await
        .map_err(|_| "origin test request failed")?;
    assert!(!wrong_origin.status().is_success());
    let missing_csrf = node
        .client
        .post(format!("{}{BASE}/bind/confirm", node.origin))
        .header("origin", &node.origin)
        .header("x-example-request", "same-origin")
        .header(
            "cookie",
            format!("{}; {}", owner.cookie, authorization.binding),
        )
        .json(&body)
        .send()
        .await
        .map_err(|_| "CSRF test request failed")?;
    assert!(!missing_csrf.status().is_success());
    let wrong_password = node
        .confirm(&owner, &authorization, "wrong password")
        .await?;
    assert!(!wrong_password.status().is_success());
    response_is_private(&wrong_password);
    let retry = node.confirm(&owner, &authorization, LOCAL_PASSWORD).await?;
    assert!(!retry.status().is_success());
    // 通过数据库时间边界注入过期，不用等待十分钟。
    let expired = node.authorize_bind(&idp, &owner).await?;
    node.expire(&expired).await?;
    assert!(!node.callback(&owner, &expired).await?.status().is_success());
    assert!(
        !node
            .confirm(&owner, &expired, LOCAL_PASSWORD)
            .await?
            .status()
            .is_success()
    );
    let cancelled = node.authorize_bind(&idp, &owner).await?;
    let error_query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("state", &cancelled.state)
        .append_pair("error", "access_denied")
        .finish();
    let cancelled_response = node
        .client
        .get(format!("{}{BASE}/callback?{error_query}", node.origin))
        .header("cookie", format!("{}; {}", owner.cookie, cancelled.binding))
        .send()
        .await
        .map_err(|_| "cancel callback request failed")?;
    assert!(!cancelled_response.status().is_success());
    assert!(
        !node
            .callback(&owner, &cancelled)
            .await?
            .status()
            .is_success()
    );
    node.close().await
}
