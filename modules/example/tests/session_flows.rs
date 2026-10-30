//! 正式模块 Session 路由的内存 driver 消费与生命周期回归；不替代双库或 TCP 验收。
#![cfg(feature = "session")]

use std::time::Duration;

use figment::{Figment, providers::Serialized};
use openrust::core::{Application, ServiceProvider};
use openrust::http::axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use openrust::http::{Cookie, SameSite};
use openrust::session::{MemorySessionStore, SessionServiceProvider};
use openrust::testing::{TestApp, TestResponse};
use serde_json::{Value, json};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const PATH: &str = "/api/example/session";
const COOKIE_NAME: &str = "example_module_session";
const TIMEOUT: Duration = Duration::from_secs(5);

struct SessionApplication;
impl Application for SessionApplication {
    type Config = <crate::bootstrap::Server as Application>::Config;
    fn config_namespace() -> &'static str {
        crate::bootstrap::Server::config_namespace()
    }
    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = crate::bootstrap::Server::providers(config);
        // 本 fixture 仅从下方固定源构造，没有数据库源；Server 因此不登记认证 Session。
        // 不改变宿主 Cookie Provider 的加密/明文选择，也不替换正式生产 driver。
        providers.push(Box::new(
            SessionServiceProvider::with_store(MemorySessionStore::default()).with_cookie_config(
                openrust::config::SessionConfig {
                    lottery: [1, 1],
                    driver: Some("array".into()),
                    cookie: Some(COOKIE_NAME.into()),
                    secure: Some(false),
                    ..openrust::config::SessionConfig::default()
                },
            ),
        ));
        providers
    }
    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        crate::bootstrap::Server::modules(config)
    }
}

fn sources() -> Figment {
    let mut config = openrust::config::FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    Figment::from(Serialized::defaults(config)).merge(Serialized::defaults(json!({
        "app":{"env":"test"},
        "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
        "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
    })))
}
fn ensure(condition: bool) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err("module Session contract mismatch".into())
    }
}
fn data(preference: Option<&str>, notice: Option<&str>, action: Option<&str>) -> Value {
    let mut result = json!({"preference":preference,"notice":notice,
        "memo":null,"list":null,"count":null,
        "has":{"preference":preference.is_some(),"notice":notice.is_some()},
        "exists":{"preference":preference.is_some(),"notice":notice.is_some()}});
    if let Some(action) = action {
        result["action"] = json!(action);
    }
    result
}
fn helper_data(
    action: Option<&str>,
    memo: Option<&str>,
    list: Option<&[&str]>,
    count: Option<i64>,
    removed: Option<&str>,
) -> Value {
    let mut result = data(None, None, action);
    result["memo"] = json!(memo);
    result["list"] = json!(list);
    result["count"] = json!(count);
    if let Some(removed) = removed {
        result["removed"] = json!(removed);
    }
    result
}
fn success(response: &TestResponse, data: &Value) -> TestResult {
    response
        .assert_status(StatusCode::OK)?
        .assert_json(&json!({"success":true,"code":"ok","message":"ok","data":data}))?;
    Ok(())
}
async fn request<A: Application>(
    app: &TestApp<A>,
    payload: Option<Value>,
    cookie: Option<&str>,
) -> TestResult<TestResponse> {
    let mut builder = if payload.is_some() {
        Request::post(PATH).header(header::CONTENT_TYPE, "application/json")
    } else {
        Request::get(PATH)
    };
    builder = builder.header("host", "localhost");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let body = payload
        .map(|value| serde_json::to_vec(&value))
        .transpose()?
        .unwrap_or_default();
    Ok(app
        .request_with_limits(builder.body(Body::from(body))?, 4096, TIMEOUT)
        .await?)
}
async fn previous_request<A: Application>(
    app: &TestApp<A>,
    path: &str,
    method: Method,
    cookie: Option<&str>,
    attempting: bool,
) -> TestResult<TestResponse> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    if attempting {
        builder = builder.header("precognition", "true");
    }
    Ok(app
        .request_with_limits(builder.body(Body::empty())?, 4096, TIMEOUT)
        .await?)
}

fn previous_data(
    url: Option<&str>,
    route: Option<&str>,
    visits: Option<i64>,
    attempting: bool,
    route_exists: bool,
) -> Value {
    json!({"url":url,"route":route,"has_uri":url.is_some(),"route_exists":route_exists,
        "visits":visits,"attempting":attempting,"confirmed":false})
}

#[tokio::test]
async fn module_previous_named_anonymous_and_opt_in_precognition_preserve_request_history()
-> TestResult {
    let mut app = TestApp::<SessionApplication>::builder(sources())
        .build()
        .await?;
    let outcome = verify_module_previous(&app).await;
    let shutdown = app
        .shutdown(openrust::tokio::time::Instant::now() + TIMEOUT)
        .await;
    outcome?;
    shutdown?;
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[tokio::test]
async fn module_database_previous_history_and_precognition_use_the_original_persistent_host()
-> TestResult {
    let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping module previous HTTP test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database =
        openrust::testing::TestDatabase::create::<openrust::db::FrameworkMigrator>(&admin).await?;
    let source = sources().merge(Serialized::defaults(json!({
        "database":{"url":database.url()},
        "session":{"driver":"database","cookie":COOKIE_NAME,"secure":false,"lottery":[1,1]},
        "auth":{"jwt":{"issuer":"http://openrust.test","audience":"module-previous",
            "active_key_id":"test","keys":{"test":"QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI="}}},
        "security":{"signed_links":{"issuer":"http://openrust.test","active_key_id":"test",
            "keys":{"test":"Q0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0M="}}}
    })));
    let outcome: TestResult = async {
        let mut app = TestApp::<crate::bootstrap::Server>::builder(source)
            .build()
            .await?;
        let outcome = verify_module_previous(&app).await;
        let shutdown = app
            .shutdown(openrust::tokio::time::Instant::now() + TIMEOUT)
            .await;
        outcome?;
        shutdown?;
        Ok(())
    }
    .await;
    let cleanup = database.close().await;
    outcome?;
    cleanup?;
    Ok(())
}

async fn verify_module_previous<A: Application>(app: &TestApp<A>) -> TestResult {
    let base = "/api/example/session/previous";
    let named_url = "http://localhost/api/example/session/previous/named";
    let anonymous_url = "http://localhost/api/example/session/previous/anonymous";
    let first = previous_request(app, &format!("{base}/named"), Method::GET, None, false).await?;
    success(&first, &previous_data(None, None, Some(1), false, false))?;
    let mut cookie = cookie_pair(&first)?;
    let second = previous_request(
        app,
        &format!("{base}/anonymous"),
        Method::GET,
        Some(&cookie),
        false,
    )
    .await?;
    success(
        &second,
        &previous_data(
            Some(named_url),
            Some("example.session.previous.named"),
            Some(2),
            false,
            true,
        ),
    )?;
    cookie = cookie_pair(&second)?;
    let read = previous_request(app, base, Method::POST, Some(&cookie), false).await?;
    success(
        &read,
        &previous_data(Some(anonymous_url), None, Some(2), false, true),
    )?;
    cookie = cookie_pair(&read)?;
    let attempted = previous_request(
        app,
        &format!("{base}/named"),
        Method::GET,
        Some(&cookie),
        true,
    )
    .await?;
    success(
        &attempted,
        &previous_data(Some(anonymous_url), None, Some(3), true, true),
    )?;
    cookie = cookie_pair(&attempted)?;
    let confirmed = previous_request(
        app,
        &format!("{base}/preview"),
        Method::GET,
        Some(&cookie),
        true,
    )
    .await?;
    check_previous_preview(&confirmed)?;
    cookie = cookie_pair(&confirmed)?;
    let after = previous_request(app, base, Method::POST, Some(&cookie), false).await?;
    success(
        &after,
        &previous_data(
            Some(named_url),
            Some("example.session.previous.named"),
            Some(3),
            false,
            true,
        ),
    )?;
    let isolated = previous_request(app, base, Method::POST, None, false).await?;
    success(&isolated, &previous_data(None, None, None, false, false))?;
    Ok(())
}

fn check_previous_preview(response: &TestResponse) -> TestResult {
    response.assert_status(StatusCode::NO_CONTENT)?;
    assert_eq!(response.body(), &[] as &[u8]);
    for name in ["precognition", "precognition-success"] {
        assert_eq!(
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok()),
            Some("true")
        );
    }
    Ok(())
}

fn cookie_pair(response: &TestResponse) -> TestResult<String> {
    let mut selected = None;
    for header in response.headers().get_all(header::SET_COOKIE) {
        let text = header.to_str()?;
        let pair = text
            .split(';')
            .next()
            .ok_or("Session Cookie pair missing")?;
        let (name, value) = pair.split_once('=').ok_or("Set-Cookie pair malformed")?;
        if name == COOKIE_NAME {
            ensure(selected.is_none() && !value.is_empty())?;
            let model = Cookie::from_header(text)?;
            ensure(model.path() == "/" && model.http_only() && model.secure() == Some(false))?;
            ensure(model.same_site() == Some(SameSite::Lax))?;
            selected = Some(pair.to_owned());
        }
    }
    // 首段逐字回送，不解密、不打印，也不把显式 plain 模式称为受保护 Cookie。
    selected.ok_or_else(|| "Session Set-Cookie missing".into())
}
async fn step(
    app: &TestApp<SessionApplication>,
    cookie: &mut String,
    action: Option<&str>,
    value: Value,
    expected: Value,
) -> TestResult {
    let payload = action.map(|action| json!({"action":action,"value":value}));
    let response = request(app, payload, Some(cookie)).await?;
    success(&response, &expected)?;
    *cookie = cookie_pair(&response)?;
    Ok(())
}
async fn invalid(
    app: &TestApp<SessionApplication>,
    cookie: &mut String,
    payload: Value,
    code: &str,
) -> TestResult {
    let response = request(app, Some(payload), Some(cookie)).await?;
    response.assert_status(StatusCode::BAD_REQUEST)?;
    let body: Value = response.json()?;
    ensure(body.as_object().is_some_and(|body| body.len() == 5))?;
    ensure(body["success"] == false && body["code"] == code)?;
    let message = match code {
        "example_session_action_invalid" => "无效的示例 Session 操作",
        "example_session_list_full" => "示例 Session 列表已满",
        _ => "示例 Session 值必须是不超过 256 UTF-8 字节的字符串",
    };
    ensure(body["message"] == message)?;
    ensure(
        body["timestamp"]
            .as_str()
            .is_some_and(|value| chrono::DateTime::parse_from_rfc3339(value).is_ok()),
    )?;
    ensure(body["request_id"].as_str().is_some_and(|value| {
        !value.is_empty()
            && response
                .headers()
                .get("x-request-id")
                .and_then(|header| header.to_str().ok())
                == Some(value)
    }))?;
    *cookie = cookie_pair(&response)?;
    Ok(())
}

#[tokio::test]
async fn module_session_preferences_flash_keep_and_request_isolation() -> TestResult {
    let mut app = TestApp::<SessionApplication>::builder(sources())
        .build()
        .await?;
    let outcome: TestResult = async {
        let initial = request(&app, None, None).await?;
        success(&initial, &data(None, None, None))?;
        let mut cookie = cookie_pair(&initial)?;
        // 正式 Request 会 trim；这是输入规范化，不是 Session bag 自行改写字符串。
        let pref = Some("blue+green;warm");
        for (action, value, preference, notice) in [
            (Some("put"), json!("  blue+green;warm  "), pref, None),
            (None, Value::Null, pref, None),
            (Some("flash"), json!("once"), pref, Some("once")),
            (None, Value::Null, pref, Some("once")),
            (None, Value::Null, pref, None),
            (Some("flash"), json!("next"), pref, Some("next")),
            (Some("keep"), Value::Null, pref, Some("next")),
            (None, Value::Null, pref, Some("next")),
            (None, Value::Null, pref, None),
            (Some("now"), json!("current"), pref, Some("current")),
            (None, Value::Null, pref, None),
            (Some("forget"), Value::Null, None, None),
            (None, Value::Null, None, None),
        ] {
            step(
                &app,
                &mut cookie,
                action,
                value,
                data(preference, notice, action),
            )
            .await?;
        }
        let isolated = request(&app, None, None).await?;
        success(&isolated, &data(None, None, None))?;
        cookie_pair(&isolated)?;
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome?;
    shutdown?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // 固定业务键的跨请求状态序列保持在同一 Cookie 与 fixture 中。
async fn module_session_store_helpers_use_fixed_business_keys_across_requests() -> TestResult {
    let mut app = TestApp::<SessionApplication>::builder(sources())
        .build()
        .await?;
    let outcome: TestResult = async {
        let initial = request(&app, None, None).await?;
        success(&initial, &helper_data(None, None, None, None, None))?;
        let mut cookie = cookie_pair(&initial)?;
        for (action, value, memo, list, count, removed) in [
            ("remember", Value::Null, Some("computed"), None, None, None),
            ("memo_null", Value::Null, None, None, None, None),
            ("remember", Value::Null, Some("computed"), None, None, None),
            (
                "memo_seed",
                json!("seeded"),
                Some("seeded"),
                None,
                None,
                None,
            ),
            ("remember", Value::Null, Some("seeded"), None, None, None),
            (
                "push",
                json!("alpha"),
                Some("seeded"),
                Some(&["alpha"][..]),
                None,
                None,
            ),
            (
                "push",
                json!("beta"),
                Some("seeded"),
                Some(&["alpha", "beta"][..]),
                None,
                None,
            ),
            (
                "increment",
                Value::Null,
                Some("seeded"),
                Some(&["alpha", "beta"][..]),
                Some(1),
                None,
            ),
            (
                "increment_by",
                Value::Null,
                Some("seeded"),
                Some(&["alpha", "beta"][..]),
                Some(3),
                None,
            ),
            (
                "decrement",
                Value::Null,
                Some("seeded"),
                Some(&["alpha", "beta"][..]),
                Some(2),
                None,
            ),
            (
                "decrement_by",
                Value::Null,
                Some("seeded"),
                Some(&["alpha", "beta"][..]),
                Some(0),
                None,
            ),
            (
                "remove",
                Value::Null,
                None,
                Some(&["alpha", "beta"][..]),
                Some(0),
                Some("seeded"),
            ),
        ] {
            step(
                &app,
                &mut cookie,
                Some(action),
                value,
                helper_data(Some(action), memo, list, count, removed),
            )
            .await?;
        }
        step(
            &app,
            &mut cookie,
            None,
            Value::Null,
            helper_data(None, None, Some(&["alpha", "beta"]), Some(0), None),
        )
        .await?;
        let isolated = request(&app, None, None).await?;
        success(&isolated, &helper_data(None, None, None, None, None))?;
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome?;
    shutdown?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // 同一 Cookie 验证业务字节/列表上限及失败后全快照不变。
async fn module_session_utf8_budget_invalid_values_and_namespace_are_bounded() -> TestResult {
    let mut app = TestApp::<SessionApplication>::builder(sources())
        .build()
        .await?;
    let outcome: TestResult = async {
        let initial = request(&app, None, None).await?;
        success(&initial, &data(None, None, None))?;
        let mut cookie = cookie_pair(&initial)?;
        let exact = format!("{}a", "界".repeat(85));
        ensure(exact.len() == 256)?;
        step(
            &app,
            &mut cookie,
            Some("put"),
            json!(exact),
            data(Some(&exact), None, Some("put")),
        )
        .await?;
        for payload in [
            json!({"action":"put","value":format!("{exact}b")}),
            json!({"action":"put","value":"   "}),
            json!({"action":"put","value":17}),
            json!({"action":"flash","value":false}),
        ] {
            invalid(&app, &mut cookie, payload, "example_session_value_invalid").await?;
            step(
                &app,
                &mut cookie,
                None,
                Value::Null,
                data(Some(&exact), None, None),
            )
            .await?;
        }
        invalid(
            &app,
            &mut cookie,
            json!({"action":"reflash","value":"not echoed"}),
            "example_session_action_invalid",
        )
        .await?;
        invalid(
            &app,
            &mut cookie,
            json!({"action":"openrust.auth.user_id","value":"not echoed"}),
            "example_session_action_invalid",
        )
        .await?;
        // 正文里的 key 不能将固定偏好操作转成任意 Session 写入。
        let written = request(
            &app,
            Some(json!({"action":"put","key":"openrust.auth.user_id","value":"own preference"})),
            Some(&cookie),
        )
        .await?;
        success(&written, &data(Some("own preference"), None, Some("put")))?;
        cookie = cookie_pair(&written)?;
        let isolated = request(&app, None, None).await?;
        success(&isolated, &data(None, None, None))?;
        step(
            &app,
            &mut cookie,
            None,
            Value::Null,
            data(Some("own preference"), None, None),
        )
        .await?;
        let mut expected = data(Some("own preference"), None, Some("memo_seed"));
        expected["memo"] = json!(exact);
        step(
            &app,
            &mut cookie,
            Some("memo_seed"),
            json!(exact),
            expected.clone(),
        )
        .await?;
        expected
            .as_object_mut()
            .ok_or("Session 响应必须是对象")?
            .remove("action");
        for action in ["memo_seed", "push"] {
            invalid(
                &app,
                &mut cookie,
                json!({"action":action,"value":format!("{exact}b")}),
                "example_session_value_invalid",
            )
            .await?;
            step(&app, &mut cookie, None, Value::Null, expected.clone()).await?;
        }
        // 八项限制属于示例业务；每一项仍通过同一 256 字节输入边界。
        let mut items = Vec::new();
        for _ in 0..8 {
            items.push(exact.clone());
            expected["list"] = json!(items);
            expected["action"] = json!("push");
            step(
                &app,
                &mut cookie,
                Some("push"),
                json!(exact),
                expected.clone(),
            )
            .await?;
        }
        expected
            .as_object_mut()
            .ok_or("Session 响应必须是对象")?
            .remove("action");
        invalid(
            &app,
            &mut cookie,
            json!({"action":"push","value":"ninth"}),
            "example_session_list_full",
        )
        .await?;
        step(&app, &mut cookie, None, Value::Null, expected).await?;
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome?;
    shutdown?;
    Ok(())
}

struct RequestDriverApplication;
impl Application for RequestDriverApplication {
    type Config = <SessionApplication as Application>::Config;
    fn config_namespace() -> &'static str {
        SessionApplication::config_namespace()
    }
    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = SessionApplication::providers(config);
        providers.push(Box::new(
            crate::modules::example::providers::session::ExampleSessionDriverProvider::memory(),
        ));
        providers
    }
    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        SessionApplication::modules(config)
    }
}

fn merge_probe_cookies(response: &TestResponse, cookies: &mut Vec<(String, String)>) -> TestResult {
    for header in response.headers().get_all(header::SET_COOKIE) {
        let pair = header
            .to_str()?
            .split(';')
            .next()
            .ok_or("Cookie 首段缺失")?;
        let (name, _) = pair.split_once('=').ok_or("Cookie 首段无效")?;
        if ![
            COOKIE_NAME,
            "example_session_probe",
            "example_session_probe_reply",
            "example_session_gc",
        ]
        .contains(&name)
        {
            continue;
        }
        if let Some(existing) = cookies.iter_mut().find(|(key, _)| key == name) {
            pair.clone_into(&mut existing.1);
        } else {
            cookies.push((name.to_owned(), pair.to_owned()));
        }
    }
    Ok(())
}

#[tokio::test]
async fn explicit_request_driver_consumes_cookie_and_keeps_two_request_queues_isolated()
-> TestResult {
    let mut app = TestApp::<RequestDriverApplication>::builder(sources())
        .build()
        .await?;
    let outcome: TestResult = async {
        let mut first = Vec::new();
        let mut second = Vec::new();
        for (cookies, marker, initial) in [(&mut first, "one", 1), (&mut second, "two", 2)] {
            let memo = format!("memo-{marker}");
            for (action, value, expected_memo, count) in [
                ("put", json!(marker), None, None),
                ("memo_seed", json!(memo), Some(memo.as_str()), None),
                (
                    if initial == 1 {
                        "increment"
                    } else {
                        "increment_by"
                    },
                    Value::Null,
                    Some(memo.as_str()),
                    Some(initial),
                ),
            ] {
                let incoming = probe_cookie_header(cookies);
                let response = request(
                    &app,
                    Some(json!({"action":action,"value":value})),
                    Some(&incoming),
                )
                .await?;
                success(
                    &response,
                    &probe_business_data(marker, expected_memo, count, Some(action)),
                )?;
                merge_probe_cookies(&response, cookies)?;
            }
        }
        for round in 0..3 {
            for (cookies, marker, count) in [(&mut first, "one", 1), (&mut second, "two", 2)] {
                let incoming = probe_cookie_header(cookies);
                let response = request(
                    &app,
                    Some(json!({"action":"gc_probe","value":marker})),
                    Some(&incoming),
                )
                .await?;
                let mut expected = probe_business_data(
                    marker,
                    Some(&format!("memo-{marker}")),
                    Some(count),
                    Some("gc_probe"),
                );
                expected["extension"] =
                    json!({"needs_request":true,"reply":(round == 2).then_some(marker),"gc":"7200:0"});
                success(&response, &expected)?;
                merge_probe_cookies(&response, cookies)?;
            }
        }
        for (cookies, marker, count) in [(&mut first, "one", 2), (&mut second, "two", 3)] {
            let incoming = probe_cookie_header(cookies);
            let response =
                request(&app, Some(json!({"action":"increment"})), Some(&incoming)).await?;
            success(
                &response,
                &probe_business_data(
                    marker,
                    Some(&format!("memo-{marker}")),
                    Some(count),
                    Some("increment"),
                ),
            )?;
            merge_probe_cookies(&response, cookies)?;
        }
        for (cookies, marker, count) in [(&first, "one", 2), (&second, "two", 3)] {
            let response = request(&app, None, Some(&probe_cookie_header(cookies))).await?;
            success(
                &response,
                &probe_business_data(marker, Some(&format!("memo-{marker}")), Some(count), None),
            )?;
        }
        let response = request(
            &app,
            Some(json!({"action":"extensions"})),
            Some(&probe_cookie_header(&first)),
        )
        .await?;
        let mut expected =
            probe_business_data("one", Some("memo-one"), Some(2), Some("extensions"));
        expected["extension"] = json!({"id_changed":true,"same_handler":true,"needs_request":true});
        success(&response, &expected)?;
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome?;
    shutdown?;
    Ok(())
}

fn probe_cookie_header(cookies: &[(String, String)]) -> String {
    cookies
        .iter()
        .map(|(_, pair)| pair.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}
fn probe_business_data(
    marker: &str,
    memo: Option<&str>,
    count: Option<i64>,
    action: Option<&str>,
) -> Value {
    let mut expected = data(Some(marker), None, action);
    expected["memo"] = json!(memo);
    expected["count"] = json!(count);
    expected
}

struct CookieSessionApplication;
impl Application for CookieSessionApplication {
    type Config = <SessionApplication as Application>::Config;
    fn config_namespace() -> &'static str {
        SessionApplication::config_namespace()
    }
    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = crate::bootstrap::Server::providers(config);
        providers.push(Box::new(
            crate::modules::example::providers::session::cookie_session_provider(),
        ));
        providers
    }
    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        SessionApplication::modules(config)
    }
}
fn merge_cookie_driver_cookies(
    response: &TestResponse,
    cookies: &mut Vec<(String, String)>,
) -> TestResult {
    for header in response.headers().get_all(header::SET_COOKIE) {
        let pair = header
            .to_str()?
            .split(';')
            .next()
            .ok_or("Cookie 首段缺失")?;
        let (key, value) = pair.split_once('=').ok_or("Cookie 首段无效")?;
        if key != COOKIE_NAME && openrust::session::SessionId::parse(key).is_err() {
            continue;
        }
        if value.is_empty() {
            cookies.retain(|(name, _)| name != key);
        } else if let Some(existing) = cookies.iter_mut().find(|(name, _)| name == key) {
            pair.clone_into(&mut existing.1);
        } else {
            cookies.push((key.to_owned(), pair.to_owned()));
        }
    }
    Ok(())
}
#[tokio::test]
async fn builtin_cookie_session_is_explicit_and_preserves_two_clients_business_state() -> TestResult
{
    let mut app = TestApp::<CookieSessionApplication>::builder(sources())
        .build()
        .await?;
    let outcome: TestResult = async {
        let mut first = Vec::new();
        let mut second = Vec::new();
        for (cookies, marker, count) in [(&mut first, "one", 1), (&mut second, "two", 2)] {
            let memo = format!("memo-{marker}");
            for (action, value, current_memo, current_count) in [
                ("put", json!(marker), None, None),
                ("memo_seed", json!(memo), Some(memo.as_str()), None),
                (
                    if count == 1 {
                        "increment"
                    } else {
                        "increment_by"
                    },
                    Value::Null,
                    Some(memo.as_str()),
                    Some(count),
                ),
            ] {
                let response = request(
                    &app,
                    Some(json!({"action":action,"value":value})),
                    Some(&probe_cookie_header(cookies)),
                )
                .await?;
                success(
                    &response,
                    &probe_business_data(marker, current_memo, current_count, Some(action)),
                )?;
                merge_cookie_driver_cookies(&response, cookies)?;
            }
        }
        for (cookies, marker, count) in [(&first, "one", 1), (&second, "two", 2)] {
            let response = request(&app, None, Some(&probe_cookie_header(cookies))).await?;
            success(
                &response,
                &probe_business_data(marker, Some(&format!("memo-{marker}")), Some(count), None),
            )?;
        }
        // 旧 Cookie 快照在受信任服务端显式换 ID 后仍可重放；不称为远端撤销。
        let replay = probe_cookie_header(&first);
        let moved = request(&app, Some(json!({"action":"extensions"})), Some(&replay)).await?;
        let mut expected =
            probe_business_data("one", Some("memo-one"), Some(1), Some("extensions"));
        expected["extension"] = json!({"id_changed":true,"same_handler":true,"needs_request":true});
        success(&moved, &expected)?;
        let replayed = request(&app, None, Some(&replay)).await?;
        success(
            &replayed,
            &probe_business_data("one", Some("memo-one"), Some(1), None),
        )?;
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome?;
    shutdown?;
    Ok(())
}
