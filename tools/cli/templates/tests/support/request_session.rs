//! 生成应用经正式 Session Provider 与请求作用域验证跨请求生命周期。
use figment::Figment;
use openrust::config::SessionConfig;
use openrust::core::{Application, BootContext, BoxFuture, ServiceProvider};
use openrust::http::axum::{
    body::Body,
    http::{
        Request, StatusCode,
        header::{COOKIE, SET_COOKIE},
    },
};
use openrust::http::{Json, RequestInfo, Routes};
use openrust::session::{MemorySessionStore, Session, SessionServiceProvider};
use openrust::testing::{TestApp, TestResponse};
use serde_json::{Value, json};
use std::time::Duration;

const PREFIX: &str = "/__framework_contract/request-session";
const NOTICE: &str = "contract.request_session.notice";
const NOW: &str = "contract.request_session.now";
const NULL: &str = "contract.request_session.null";
const SHARED: &str = "contract.request_session.shared";
const PREFERENCE: &str = "contract.request_session.preference";
const BODY_LIMIT: usize = 4096;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

struct SessionRoutes;
impl ServiceProvider for SessionRoutes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(|routes: Routes| {
                routes
                    .get(&format!("{PREFIX}/set"), set)
                    .get(&format!("{PREFIX}/inspect"), inspect)
            })?;
            Ok(())
        })
    }
}

struct SessionApplication<A, const EXISTING: bool>(std::marker::PhantomData<fn() -> A>);
impl<A: Application, const EXISTING: bool> Application for SessionApplication<A, EXISTING> {
    type Config = A::Config;
    fn config_namespace() -> &'static str {
        A::config_namespace()
    }
    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = A::providers(config);
        if !EXISTING {
            // plain 没有正式 Session 时才显式安装内存 driver；Cookie Provider 完整保留。
            providers.push(Box::new(
                SessionServiceProvider::with_store(MemorySessionStore::default())
                    .with_cookie_config(SessionConfig {
                        driver: Some("array".into()),
                        cookie: Some("contract_session".into()),
                        secure: Some(false),
                        ..SessionConfig::default()
                    }),
            ));
        }
        providers.push(Box::new(SessionRoutes));
        providers
    }
    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        A::modules(config)
    }
}

async fn set(session: Session, info: RequestInfo) -> openrust::Result<Json<Value>> {
    session
        .put(PREFERENCE, "preserved")?
        .put(NULL, Value::Null)?
        .flash(NOTICE, "next")?
        .now(NOW, "current")?;
    info.session()?.put(SHARED, "same request")?;
    Ok(Json(snapshot(&session, &info)?))
}
async fn inspect(session: Session, info: RequestInfo) -> openrust::Result<Json<Value>> {
    Ok(Json(snapshot(&session, &info)?))
}
fn snapshot(session: &Session, info: &RequestInfo) -> openrust::Result<Value> {
    Ok(json!({
        "cookie_name": session.name()?,
        "same_handle": info.has_session() && info.session()?.id()? == session.id()?,
        "shared_value": session.get(SHARED)? == Some(json!("same request")),
        "stored_null": session.get_or(NULL, json!("fallback"))? == Value::Null,
        "preference": session.get(PREFERENCE)?,
        "notice": session.get(NOTICE)?,
        "now": session.get(NOW)?,
    }))
}

/// 在正式生成应用内验证 Session；不是网络或存储 driver 的替代验收。
///
/// `existing_session=true` 保留应用原有 Session Provider/数据库；false 仅为没有
/// Session 的 plain 应用追加显式内存 driver。此参数不表示 Cookie 是否加密，
/// 响应不返回 ID/token，ID 仅在处理器内比较句柄；Set-Cookie 首段只逐字传入
/// 后续请求，不解密也不打印。
pub async fn assert_request_session<A: Application>(
    source: Figment,
    existing_session: bool,
) -> openrust::Result<()> {
    if existing_session {
        verify::<A, true>(source).await
    } else {
        verify::<A, false>(source).await
    }
}

async fn verify<A: Application, const EXISTING: bool>(source: Figment) -> openrust::Result<()> {
    let mut app = TestApp::<SessionApplication<A, EXISTING>>::builder(source)
        .build()
        .await?;
    let outcome: openrust::Result<()> = async {
        let first = app
            .request_with_limits(request("set", None)?, BODY_LIMIT, REQUEST_TIMEOUT)
            .await?;
        let name = first.json::<Value>()?["data"]["cookie_name"]
            .as_str()
            .ok_or_else(mismatch)?
            .to_owned();
        ensure(!name.is_empty() && name.len() <= 64 * 1024)?;
        check_response(&first, expected(&name, true, Some("next"), Some("current")))?;
        let first_cookie = cookie_pair(&first, &name)?;
        let second = app
            .request_with_limits(
                request("inspect", Some(&first_cookie))?,
                BODY_LIMIT,
                REQUEST_TIMEOUT,
            )
            .await?;
        check_response(&second, expected(&name, true, Some("next"), None))?;
        // 每次都从真实响应取得下一次 Cookie，不假定保护后的字节保持不变。
        let second_cookie = cookie_pair(&second, &name)?;
        let third = app
            .request_with_limits(
                request("inspect", Some(&second_cookie))?,
                BODY_LIMIT,
                REQUEST_TIMEOUT,
            )
            .await?;
        check_response(&third, expected(&name, true, None, None))?;
        cookie_pair(&third, &name)?;
        let isolated = app
            .request_with_limits(request("inspect", None)?, BODY_LIMIT, REQUEST_TIMEOUT)
            .await?;
        check_response(&isolated, expected(&name, false, None, None))?;
        Ok(())
    }
    .await;
    let shutdown = app
        .shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    outcome?;
    shutdown?;
    Ok(())
}

fn expected(name: &str, persisted: bool, notice: Option<&str>, now: Option<&str>) -> Value {
    json!({"cookie_name":name, "same_handle":true, "shared_value":persisted,
        "stored_null":persisted, "preference":persisted.then_some("preserved"), "notice":notice, "now":now})
}
fn request(action: &str, cookies: Option<&str>) -> openrust::Result<Request<Body>> {
    let mut request = Request::get(format!("{PREFIX}/{action}")).header("host", "localhost");
    if let Some(cookies) = cookies {
        request = request.header(COOKIE, cookies);
    }
    request
        .body(Body::empty())
        .map_err(openrust::Error::internal)
}
fn check_response(response: &TestResponse, data: Value) -> openrust::Result<()> {
    response
        .assert_status(StatusCode::OK)?
        .assert_json(&json!({"success":true,"code":"ok","message":"ok","data":data}))?;
    Ok(())
}
fn cookie_pair(response: &TestResponse, name: &str) -> openrust::Result<String> {
    let mut selected = None;
    for value in response.headers().get_all(SET_COOKIE) {
        let value = value.to_str().map_err(|_| mismatch())?;
        let pair = value.split(';').next().ok_or_else(mismatch)?;
        let (key, value) = pair.split_once('=').ok_or_else(mismatch)?;
        if key == name {
            ensure(selected.is_none() && !value.is_empty())?;
            selected = Some(pair.to_owned());
        }
    }
    selected.ok_or_else(mismatch)
}
fn ensure(condition: bool) -> openrust::Result<()> {
    if condition { Ok(()) } else { Err(mismatch()) }
}
fn mismatch() -> openrust::Error {
    openrust::Error::custom(
        "test_session_contract_mismatch",
        "生成应用 Session 契约不匹配",
    )
}
