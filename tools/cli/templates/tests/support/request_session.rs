//! 生成应用经正式 Session Provider 与请求作用域验证跨请求生命周期。
use figment::Figment;
use openrust::config::SessionConfig;
use openrust::core::{Application, BootContext, BoxFuture, RegisterContext, ServiceProvider};
use openrust::http::axum::{
    body::Body,
    http::{
        Request, StatusCode,
        header::{COOKIE, SET_COOKIE},
    },
};
use openrust::http::{CookieJar, CookieOptions, Json, RequestInfo, Routes};
use openrust::session::{
    DatabaseSessionStore, MemorySessionStore, Session, SessionId, SessionRecord,
    SessionRequestContext, SessionServiceProvider, SessionStore, SessionStoreHandle,
    SessionWriteMode, SessionWriteOutcome,
};
use openrust::testing::{TestApp, TestResponse};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;

const PREFIX: &str = "/__framework_contract/request-session";
const NOTICE: &str = "contract.request_session.notice";
const NOW: &str = "contract.request_session.now";
const NULL: &str = "contract.request_session.null";
const SHARED: &str = "contract.request_session.shared";
const PREFERENCE: &str = "contract.request_session.preference";
const MEMO: &str = "contract.request_session.memo";
const LIST: &str = "contract.request_session.list";
const COUNT: &str = "contract.request_session.count";
const BODY_LIMIT: usize = 4096;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

struct SessionRoutes;
impl ServiceProvider for SessionRoutes {
    fn register(&self, app: &mut RegisterContext<'_>) -> openrust::Result<()> {
        // fixture 已明确选择 plain array 或 auth database；不是装饰任意已有 factory。
        app.singleton::<SessionStoreHandle, _>(|resolver| {
            Box::pin(async move {
                let config = resolver.section::<SessionConfig>("session")?;
                let backend: Arc<dyn SessionStore> = match config.driver.as_deref() {
                    Some("array") => Arc::new(MemorySessionStore::default()),
                    Some("database") => Arc::new(DatabaseSessionStore::new(
                        (*resolver.resolve::<openrust::db::Db>().await?).clone(),
                    )),
                    _ => return Err(mismatch()),
                };
                Ok(Arc::new(SessionStoreHandle::new(ContractRequestDriver {
                    backend,
                    context: None,
                })))
            })
        });
        Ok(())
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(|routes: Routes| {
                routes
                    .get(&format!("{PREFIX}/set"), set)
                    .get(&format!("{PREFIX}/inspect"), inspect)
                    .get(&format!("{PREFIX}/advance"), advance)
                    .get(&format!("{PREFIX}/remove"), remove)
                    .get(&format!("{PREFIX}/seed-one"), seed_one)
                    .get(&format!("{PREFIX}/seed-two"), seed_two)
                    .get(&format!("{PREFIX}/probe"), probe)
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
    let before = session.id()?;
    session.set_id(Some(before.as_str()))?;
    let actual = session.handler()?;
    ensure(Arc::ptr_eq(&actual, &session.set_handler(actual.clone())?))?;
    ensure(session.handler_needs_request()?)?;
    session.set_id(None)?;
    ensure(session.id()? != before)?;
    session
        .put(PREFERENCE, "preserved")?
        .put(NULL, Value::Null)?
        .flash(NOTICE, "next")?
        .now(NOW, "current")?;
    info.session()?.put(SHARED, "same request")?;
    session.put(MEMO, Value::Null)?;
    session.remember(MEMO, || Ok("computed"))?;
    session.push(LIST, "first")?;
    session.increment(COUNT)?;
    Ok(Json(snapshot(&session, &info)?))
}
async fn inspect(session: Session, info: RequestInfo) -> openrust::Result<Json<Value>> {
    Ok(Json(snapshot(&session, &info)?))
}
async fn advance(session: Session, info: RequestInfo) -> openrust::Result<Json<Value>> {
    session.remember(MEMO, || Ok("unexpected"))?;
    session.push(LIST, "second")?;
    session.increment_by(COUNT, 2)?;
    session.decrement(COUNT)?;
    session.decrement_by(COUNT, 2)?;
    Ok(Json(snapshot(&session, &info)?))
}
async fn remove(session: Session, info: RequestInfo) -> openrust::Result<Json<Value>> {
    let removed = session.remove(MEMO)?;
    let mut data = snapshot(&session, &info)?;
    data["removed"] = json!(removed);
    Ok(Json(data))
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
        "memo": session.get(MEMO)?,
        "list": session.get(LIST)?,
        "count": session.get(COUNT)?,
    }))
}

/// 在正式生成应用内验证 Session；不是网络或存储 driver 的替代验收。
///
/// `existing_session=true` 保留认证 Provider 并显式选择同 Db 委托驱动；false 仅为没有
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

#[allow(clippy::too_many_lines)] // 生成应用的正式 Provider、Cookie 往返和关闭必须留在同一生命周期。
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
        check_response(
            &first,
            expected(
                &name,
                true,
                Some("next"),
                Some("current"),
                Some("computed"),
                Some(&["first"]),
                Some(1),
                None,
            ),
        )?;
        let first_cookie = cookie_pair(&first, &name)?;
        let second = app
            .request_with_limits(
                request("inspect", Some(&first_cookie))?,
                BODY_LIMIT,
                REQUEST_TIMEOUT,
            )
            .await?;
        check_response(
            &second,
            expected(
                &name,
                true,
                Some("next"),
                None,
                Some("computed"),
                Some(&["first"]),
                Some(1),
                None,
            ),
        )?;
        // 每次都从真实响应取得下一次 Cookie，不假定保护后的字节保持不变。
        let second_cookie = cookie_pair(&second, &name)?;
        let third = app
            .request_with_limits(
                request("inspect", Some(&second_cookie))?,
                BODY_LIMIT,
                REQUEST_TIMEOUT,
            )
            .await?;
        check_response(
            &third,
            expected(
                &name,
                true,
                None,
                None,
                Some("computed"),
                Some(&["first"]),
                Some(1),
                None,
            ),
        )?;
        let third_cookie = cookie_pair(&third, &name)?;
        let advanced = app
            .request_with_limits(
                request("advance", Some(&third_cookie))?,
                BODY_LIMIT,
                REQUEST_TIMEOUT,
            )
            .await?;
        check_response(
            &advanced,
            expected(
                &name,
                true,
                None,
                None,
                Some("computed"),
                Some(&["first", "second"]),
                Some(0),
                None,
            ),
        )?;
        let advanced_cookie = cookie_pair(&advanced, &name)?;
        let removed = app
            .request_with_limits(
                request("remove", Some(&advanced_cookie))?,
                BODY_LIMIT,
                REQUEST_TIMEOUT,
            )
            .await?;
        check_response(
            &removed,
            expected(
                &name,
                true,
                None,
                None,
                None,
                Some(&["first", "second"]),
                Some(0),
                Some("computed"),
            ),
        )?;
        let removed_cookie = cookie_pair(&removed, &name)?;
        let after_remove = app
            .request_with_limits(
                request("inspect", Some(&removed_cookie))?,
                BODY_LIMIT,
                REQUEST_TIMEOUT,
            )
            .await?;
        check_response(
            &after_remove,
            expected(
                &name,
                true,
                None,
                None,
                None,
                Some(&["first", "second"]),
                Some(0),
                None,
            ),
        )?;
        let isolated = app
            .request_with_limits(request("inspect", None)?, BODY_LIMIT, REQUEST_TIMEOUT)
            .await?;
        check_response(
            &isolated,
            expected(&name, false, None, None, None, None, None, None),
        )?;
        verify_request_driver(&app, &name).await?;
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

#[allow(clippy::too_many_arguments)] // 逐项列出固定响应字段，避免测试遗漏原有 Session 与新增 Store 值。
fn expected(
    name: &str,
    persisted: bool,
    notice: Option<&str>,
    now: Option<&str>,
    memo: Option<&str>,
    list: Option<&[&str]>,
    count: Option<i64>,
    removed: Option<&str>,
) -> Value {
    let mut value = json!({"cookie_name":name, "same_handle":true, "shared_value":persisted,
        "stored_null":persisted, "preference":persisted.then_some("preserved"), "notice":notice, "now":now,
        "memo":memo,"list":list,"count":count});
    if let Some(removed) = removed {
        value["removed"] = json!(removed);
    }
    value
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

const PROBE: &str = "contract_session_probe";
const REPLY: &str = "contract_session_reply";
struct ContractRequestDriver {
    backend: Arc<dyn SessionStore>,
    context: Option<SessionRequestContext>,
}
impl SessionStore for ContractRequestDriver {
    fn needs_request(&self) -> bool {
        true
    }
    fn bind_request(
        &self,
        context: SessionRequestContext,
    ) -> openrust::Result<Option<Arc<dyn SessionStore>>> {
        Ok(Some(Arc::new(Self {
            backend: self.backend.clone(),
            context: Some(context),
        })))
    }
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max: usize,
        expiry: OffsetDateTime,
    ) -> BoxFuture<'a, openrust::Result<Option<SessionRecord>>> {
        Box::pin(async move {
            let value = self.backend.load(id, max, expiry).await?;
            self.acknowledge()?;
            Ok(value)
        })
    }
    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max: usize,
    ) -> BoxFuture<'a, openrust::Result<SessionWriteOutcome>> {
        Box::pin(async move {
            let value = self.backend.write(record, mode, max).await?;
            self.acknowledge()?;
            Ok(value)
        })
    }
    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, openrust::Result<()>> {
        self.backend.delete(id)
    }
}
impl ContractRequestDriver {
    fn acknowledge(&self) -> openrust::Result<()> {
        let Some(context) = &self.context else {
            return Ok(());
        };
        let value = match context.cookie(PROBE) {
            Some(Value::String(value)) if matches!(value.as_str(), "one" | "two") => value,
            _ => return Ok(()),
        };
        let jar = context.cookie_jar();
        jar.queue(
            jar.factory()
                .make(REPLY, value, 0, CookieOptions::default())?,
        )
    }
}
async fn seed_one(session: Session, jar: CookieJar) -> openrust::Result<Json<Value>> {
    seed(&session, &jar, "one")
}
async fn seed_two(session: Session, jar: CookieJar) -> openrust::Result<Json<Value>> {
    seed(&session, &jar, "two")
}
fn seed(session: &Session, jar: &CookieJar, value: &str) -> openrust::Result<Json<Value>> {
    ensure(session.handler_needs_request()?)?;
    jar.queue(
        jar.factory()
            .make(PROBE, value, 0, CookieOptions::default())?,
    )?;
    Ok(Json(json!({"seeded":true})))
}
async fn probe(session: Session, info: RequestInfo) -> openrust::Result<Json<Value>> {
    Ok(Json(
        json!({"needs_request":session.handler_needs_request()?, "reply":info.cookie(REPLY)}),
    ))
}
async fn verify_request_driver<A: Application, const EXISTING: bool>(
    app: &TestApp<SessionApplication<A, EXISTING>>,
    name: &str,
) -> openrust::Result<()> {
    let mut first = Vec::<(String, String)>::new();
    let mut second = Vec::<(String, String)>::new();
    for (cookies, action) in [(&mut first, "seed-one"), (&mut second, "seed-two")] {
        let response = app
            .request_with_limits(request(action, None)?, BODY_LIMIT, REQUEST_TIMEOUT)
            .await?;
        check_response(&response, json!({"seeded":true}))?;
        merge_probe_cookies(&response, name, cookies)?;
    }
    for round in 0..2 {
        for (cookies, value) in [(&mut first, "one"), (&mut second, "two")] {
            let incoming = cookies
                .iter()
                .map(|(_, pair)| pair.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            let response = app
                .request_with_limits(
                    request("probe", Some(&incoming))?,
                    BODY_LIMIT,
                    REQUEST_TIMEOUT,
                )
                .await?;
            check_response(
                &response,
                json!({"needs_request":true,"reply":(round == 1).then_some(value)}),
            )?;
            merge_probe_cookies(&response, name, cookies)?;
        }
    }
    Ok(())
}
fn merge_probe_cookies(
    response: &TestResponse,
    name: &str,
    cookies: &mut Vec<(String, String)>,
) -> openrust::Result<()> {
    for header in response.headers().get_all(SET_COOKIE) {
        let pair = header
            .to_str()
            .map_err(|_| mismatch())?
            .split(';')
            .next()
            .ok_or_else(mismatch)?;
        let (key, _) = pair.split_once('=').ok_or_else(mismatch)?;
        if ![name, PROBE, REPLY].contains(&key) {
            continue;
        }
        if let Some(existing) = cookies.iter_mut().find(|(name, _)| name == key) {
            existing.1 = pair.to_owned();
        } else {
            cookies.push((key.to_owned(), pair.to_owned()));
        }
    }
    Ok(())
}
