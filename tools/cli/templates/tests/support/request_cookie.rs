//! 生成应用经正式 Provider、请求读取与响应中间件验证 Cookie 契约。
use figment::Figment;
use openrust::core::{Application, BootContext, BoxFuture, ServiceProvider};
use openrust::http::axum::{
    body::Body,
    http::{
        Request, StatusCode,
        header::{COOKIE, SET_COOKIE},
    },
    response::IntoResponse,
};
use openrust::http::{
    Cookie, CookieJar, CookieOptions, CookieResponseExt, Json, RequestInfo, Routes, SameSite,
};
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
            app.routes(|routes: Routes| {
                routes
                    .get(&format!("{PREFIX}/set"), set_cookies)
                    .get(&format!("{PREFIX}/inspect"), inspect_cookies)
                    .get(&format!("{PREFIX}/forget"), forget_cookies)
            })?;
            Ok(())
        })
    }
}

// 透传生成应用的正式服务和模块，仅追加本测试独占的路由 Provider。
struct CookieApplication<A>(std::marker::PhantomData<fn() -> A>);

impl<A: Application> Application for CookieApplication<A> {
    type Config = A::Config;

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

async fn set_cookies(jar: CookieJar) -> openrust::Result<impl IntoResponse> {
    let factory = jar.factory();
    jar.queue(factory.make(NAMES[0], VALUES[0], 5, options())?)?;
    Json(json!({"queued": true, "direct": true, "many": true}))
        .with_cookie(factory.make(NAMES[1], VALUES[1], 5, options())?)?
        .with_cookies([factory.make(NAMES[2], VALUES[2], 5, options())?])
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
pub async fn assert_request_cookie<A: Application>(source: Figment) -> openrust::Result<()> {
    let mut app = TestApp::<CookieApplication<A>>::builder(source)
        .build()
        .await?;
    let outcome: openrust::Result<()> = async {
        let response = app.request(request("set", None)?).await?;
        check_response(
            &response,
            json!({"queued": true, "direct": true, "many": true}),
        )?;
        let cookie_header = check_cookie_headers(&response, false)?.join("; ");

        let response = app
            .request(request("inspect", Some(&cookie_header))?)
            .await?;
        check_response(&response, inspection(true))?;
        ensure_no_queued_cookie(&response)?;

        // 不携带上次 Cookie 的请求既无读取值，也不能收到上次请求的队列。
        let response = app.request(request("inspect", None)?).await?;
        check_response(&response, inspection(false))?;
        ensure_no_queued_cookie(&response)?;

        let response = app
            .request(request("forget", Some(&cookie_header))?)
            .await?;
        check_response(&response, json!({"forgotten": true}))?;
        check_cookie_headers(&response, true)?;

        let response = app.request(request("inspect", None)?).await?;
        check_response(&response, inspection(false))?;
        ensure_no_queued_cookie(&response)?;
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
    let mut builder = Request::builder().uri(format!("{PREFIX}/{action}"));
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

fn check_cookie_headers(response: &TestResponse, deletion: bool) -> openrust::Result<Vec<String>> {
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
    Ok(pairs)
}

fn ensure_no_queued_cookie(response: &TestResponse) -> openrust::Result<()> {
    for header in response.headers().get_all(SET_COOKIE) {
        let header = header.to_str().map_err(|_| mismatch())?;
        let name = header
            .split(';')
            .next()
            .and_then(|pair| pair.split_once('='))
            .map(|(name, _)| name);
        ensure(name.is_none_or(|name| !NAMES.contains(&name)))?;
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
