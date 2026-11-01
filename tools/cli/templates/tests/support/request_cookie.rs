//! 生成应用经正式 Provider、请求读取与响应中间件验证 Cookie 契约。
use figment::Figment;
use openrust::config::SessionConfig;
use openrust::core::{Application, BootContext, BoxFuture, ServiceProvider};
use openrust::http::axum::{
    Extension,
    body::Body,
    http::{
        HeaderValue, Request, StatusCode,
        header::{COOKIE, SET_COOKIE},
    },
    response::IntoResponse,
};
use openrust::http::{
    Cookie, CookieJar, CookieOptions, CookieResponseExt, Json, RequestInfo, RouteGroup, Routes,
    SameSite,
};
use openrust::session::Session;
use openrust::testing::{TestApp, TestResponse};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PREFIX: &str = "/__framework_contract/request-cookie";
const NAMES: [&str; 3] = ["contract_queued", "contract_direct", "contract_many"];
const VALUES: [&str; 3] = ["queued;plus+value", "direct;plus+value", "many;plus+value"];

struct CookieRoutesProvider;

impl ServiceProvider for CookieRoutesProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            let session = app.is_bound::<openrust::session::SessionStoreHandle>();
            app.routes(move |routes: Routes| {
                let group = RouteGroup::new().middleware_alias("cookies");
                let group = if session {
                    group.middleware_alias("session")
                } else {
                    group
                };
                routes.group(group, |routes| {
                    routes
                        .get(&format!("{PREFIX}/set"), set_cookies)
                        .get(&format!("{PREFIX}/inspect"), inspect_cookies)
                        .get(&format!("{PREFIX}/forget"), forget_cookies)
                })
            })?;
            Ok(())
        })
    }
}

// 透传生成应用的正式服务和模块，仅追加本测试独占的路由 Provider。
struct CookieApplication<A>(std::marker::PhantomData<fn() -> A>);

impl<A: Application> Application for CookieApplication<A> {
    type Config = A::Config;

    fn config() -> openrust::config::ConfigManifest {
        A::config()
    }
    fn default_providers() -> openrust::core::DefaultProviders {
        A::default_providers()
    }
    fn package_providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        A::package_providers(config)
    }
    fn additional_providers() -> Vec<Box<dyn ServiceProvider>> {
        A::additional_providers()
    }
    fn with_bootstrap_providers() -> bool {
        A::with_bootstrap_providers()
    }

    fn config_namespace() -> &'static str {
        A::config_namespace()
    }

    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = A::providers(config);
        providers.push(Box::new(CookieRoutesProvider));
        providers
    }

    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        A::modules(config)
    }
}

fn options() -> CookieOptions {
    CookieOptions {
        path: Some(PREFIX.into()),
        secure: Some(false),
        same_site: Some(SameSite::Strict),
        ..CookieOptions::default()
    }
}

async fn set_cookies(
    jar: CookieJar,
    session: Option<Extension<Session>>,
) -> openrust::Result<impl IntoResponse> {
    let factory = jar.factory();
    jar.queue(factory.make(NAMES[0], VALUES[0], 5, options())?)?;
    let mut response = Json(json!({"queued": true, "direct": true, "many": true}))
        .with_cookie(factory.make(NAMES[1], VALUES[1], 5, options())?)?
        .with_cookies([factory.make(NAMES[2], VALUES[2], 5, options())?])?
        .into_response();
    if let Some(Extension(session)) = session {
        response.headers_mut().insert(
            "x-contract-session-name",
            HeaderValue::from_str(&session.name()?).map_err(openrust::Error::internal)?,
        );
    }
    Ok(response)
}

async fn inspect_cookies(info: RequestInfo) -> Json<Value> {
    // 仅比较固定测试叶；不把原始 Cookie 或原始请求头写入响应。
    Json(json!({
        "queued": info.cookie(NAMES[0]) == Some(json!(VALUES[0])),
        "direct": info.cookie(NAMES[1]) == Some(json!(VALUES[1])),
        "many": info.cookie(NAMES[2]) == Some(json!(VALUES[2])),
        "present": info.has_cookie(NAMES[0]),
        "missing_default": info.cookie_or("contract_missing", json!("fallback")) == json!("fallback")
    }))
}

async fn forget_cookies(jar: CookieJar) -> openrust::Result<impl IntoResponse> {
    jar.expire(NAMES[0], options())?;
    Json(json!({"forgotten": true}))
        .with_cookie(jar.factory().forget(NAMES[1], options())?)?
        .with_cookies([jar.factory().forget(NAMES[2], options())?])
}

/// 在生成应用自身的正式 Provider 和中间件链路验证 Cookie；不替代真实 TCP 验收。
pub async fn assert_request_cookie<A: Application>(
    source: Figment,
    expect_session: bool,
) -> openrust::Result<()> {
    let mut app = TestApp::<CookieApplication<A>>::builder(source)
        .build()
        .await?;
    let outcome: openrust::Result<()> = async {
        let response = app.request(request("set", None)?).await?;
        check_response(
            &response,
            json!({"queued": true, "direct": true, "many": true}),
        )?;
        let session_name = session_name(&response, expect_session)?;
        let session_config = if expect_session {
            Some(
                app.app()?
                    .section::<SessionConfig>("session")
                    .map_err(openrust::Error::internal)?,
            )
        } else {
            None
        };
        let mut pairs = check_cookie_headers(
            &response,
            false,
            session_name.as_deref().zip(session_config.as_ref()),
        )?;
        let cookie_header = pairs.join("; ");

        let response = app
            .request(request("inspect", Some(&cookie_header))?)
            .await?;
        check_response(&response, inspection(true))?;
        if let Some(pair) = check_session_only(
            &response,
            session_name.as_deref().zip(session_config.as_ref()),
        )? {
            *pairs.get_mut(NAMES.len()).ok_or_else(mismatch)? = pair;
        }
        let cookie_header = pairs.join("; ");

        // 不携带上次 Cookie 的请求既无读取值，也不能收到上次请求的队列。
        let response = app.request(request("inspect", None)?).await?;
        check_response(&response, inspection(false))?;
        check_session_only(
            &response,
            session_name.as_deref().zip(session_config.as_ref()),
        )?;

        let response = app
            .request(request("forget", Some(&cookie_header))?)
            .await?;
        check_response(&response, json!({"forgotten": true}))?;
        check_cookie_headers(
            &response,
            true,
            session_name.as_deref().zip(session_config.as_ref()),
        )?;

        let response = app.request(request("inspect", None)?).await?;
        check_response(&response, inspection(false))?;
        check_session_only(
            &response,
            session_name.as_deref().zip(session_config.as_ref()),
        )?;
        Ok(())
    }
    .await;
    // 业务检查仅返回 Result，保证失败也进入应用关闭流程。
    let shutdown = app
        .shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    outcome?;
    shutdown?;
    Ok(())
}

fn inspection(present: bool) -> Value {
    json!({"queued": present, "direct": present, "many": present, "present": present, "missing_default": true})
}

fn request(action: &str, cookies: Option<&str>) -> openrust::Result<Request<Body>> {
    let mut builder = Request::builder()
        .uri(format!("{PREFIX}/{action}"))
        .header("host", "localhost");
    if let Some(cookies) = cookies {
        builder = builder.header(COOKIE, cookies);
    }
    builder
        .body(Body::empty())
        .map_err(openrust::Error::internal)
}

fn check_response(response: &TestResponse, data: Value) -> openrust::Result<()> {
    response
        .assert_status(StatusCode::OK)?
        .assert_json(&json!({
            "success": true, "code": "ok", "message": "ok", "data": data
        }))?;
    Ok(())
}

fn session_name(response: &TestResponse, expected: bool) -> openrust::Result<Option<String>> {
    let mut values = response.headers().get_all("x-contract-session-name").iter();
    let name = values
        .next()
        .map(|value| value.to_str().map(str::to_owned))
        .transpose()
        .map_err(|_| mismatch())?;
    ensure(values.next().is_none() && name.is_some() == expected)?;
    Ok(name)
}

fn check_cookie_headers(
    response: &TestResponse,
    deletion: bool,
    session: Option<(&str, &SessionConfig)>,
) -> openrust::Result<Vec<String>> {
    let mut pairs = Vec::new();
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| mismatch())?
            .as_secs(),
    )
    .map_err(|_| mismatch())?;
    for name in NAMES {
        let mut matching = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|header| {
                let header = header.to_str().ok()?;
                let pair = header.split(';').next()?;
                (pair.split_once('=').map(|(key, _)| key) == Some(name)).then_some((header, pair))
            });
        let (header, pair) = matching.next().ok_or_else(mismatch)?;
        ensure(matching.next().is_none())?;
        let model = Cookie::from_header(header)?;
        ensure(model.name() == name && model.path() == PREFIX)?;
        ensure(model.http_only() && model.secure() == Some(false))?;
        ensure(model.same_site() == Some(SameSite::Strict))?;
        let expires = model.expires().ok_or_else(mismatch)?.unix_timestamp();
        let max_age = header
            .split(';')
            .skip(1)
            .find_map(|attribute| {
                let (key, value) = attribute.trim().split_once('=')?;
                key.eq_ignore_ascii_case("max-age").then_some(value)
            })
            .ok_or_else(mismatch)?
            .parse::<i64>()
            .map_err(|_| mismatch())?;
        if deletion {
            ensure(expires < now && max_age == 0)?;
        } else {
            ensure(expires > now && max_age > 0 && max_age <= 300)?;
        }
        // 第一段逐字保留，既不解密 or1，也不假设当前应用采用明文保护模式。
        pairs.push(pair.to_owned());
    }
    let mut total = 0;
    let mut session_count = 0;
    for value in response.headers().get_all(SET_COOKIE) {
        total += 1;
        let header = value.to_str().map_err(|_| mismatch())?;
        let pair = header.split(';').next().ok_or_else(mismatch)?;
        let (name, value) = pair.split_once('=').ok_or_else(mismatch)?;
        if NAMES.contains(&name) {
            continue;
        }
        let (expected_name, config) = session.ok_or_else(mismatch)?;
        ensure(name == expected_name && !value.is_empty())?;
        check_session_cookie(header, expected_name, config)?;
        pairs.push(pair.to_owned());
        session_count += 1;
    }
    ensure(session_count == usize::from(session.is_some()))?;
    ensure(total == NAMES.len() + session_count)?;
    Ok(pairs)
}

fn check_session_only(
    response: &TestResponse,
    session: Option<(&str, &SessionConfig)>,
) -> openrust::Result<Option<String>> {
    let mut count = 0;
    let mut pair = None;
    for header in response.headers().get_all(SET_COOKIE) {
        let header = header.to_str().map_err(|_| mismatch())?;
        let first = header.split(';').next().ok_or_else(mismatch)?;
        let (name, value) = first.split_once('=').ok_or_else(mismatch)?;
        let (expected_name, config) = session.ok_or_else(mismatch)?;
        ensure(name == expected_name && !value.is_empty())?;
        check_session_cookie(header, expected_name, config)?;
        pair = Some(first.to_owned());
        count += 1;
    }
    ensure(count == usize::from(session.is_some()))?;
    Ok(pair)
}

fn check_session_cookie(header: &str, name: &str, config: &SessionConfig) -> openrust::Result<()> {
    let model = Cookie::from_header(header)?;
    ensure(model.name() == name && model.path() == config.path)?;
    if let Some(explicit) = config.cookie.as_deref() {
        ensure(name == explicit)?;
    }
    let domain = config
        .domain
        .as_deref()
        .filter(|value| !value.is_empty() && *value != "0");
    ensure(model.domain() == domain)?;
    // TestApp 发送普通本机 HTTP；未设置 Secure 时可信传输默认值为 false。
    ensure(model.secure() == Some(config.secure.unwrap_or(false)))?;
    ensure(model.http_only() == config.http_only)?;
    let same_site = match config
        .same_site
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        None => None,
        Some("lax") => Some(SameSite::Lax),
        Some("strict") => Some(SameSite::Strict),
        Some("none") => Some(SameSite::None),
        _ => return Err(mismatch()),
    };
    ensure(model.same_site() == same_site)?;
    let attributes = header.to_ascii_lowercase();
    ensure(
        attributes
            .split(';')
            .any(|part| part.trim() == "partitioned")
            == config.partitioned,
    )?;
    let max_age = header.split(';').skip(1).find_map(|attribute| {
        let (key, value) = attribute.trim().split_once('=')?;
        key.eq_ignore_ascii_case("max-age").then_some(value)
    });
    if config.expire_on_close {
        ensure(model.expires().is_none() && max_age.is_none())?;
    } else {
        let age = max_age
            .ok_or_else(mismatch)?
            .parse::<i64>()
            .map_err(|_| mismatch())?;
        let lifetime = i64::try_from(config.lifetime).map_err(|_| mismatch())?;
        ensure(model.expires().is_some() && age > 0 && age <= lifetime.saturating_mul(60))?;
    }
    Ok(())
}

fn ensure(condition: bool) -> openrust::Result<()> {
    if condition { Ok(()) } else { Err(mismatch()) }
}

fn mismatch() -> openrust::Error {
    openrust::Error::custom(
        "test_cookie_contract_mismatch",
        "生成应用 Cookie 契约不匹配",
    )
}
