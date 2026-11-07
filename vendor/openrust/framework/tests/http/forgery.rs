//! 默认 web 与 api 组的公开装配和 Session CSRF 语义。
#![cfg(all(feature = "session", feature = "encryption"))]
#![allow(
    clippy::expect_used,
    reason = "测试静态请求 fixture 构造失败或必需 cookie 缺失应立即失败"
)]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use openrust::Application;
use openrust::Result;
use openrust::ServiceExt;
use openrust::foundation::BootContext;
use openrust::foundation::http::build_router;
use openrust::http::CookieServiceProvider;
use openrust::http::HttpServiceProvider;
use openrust::http::Json;
use openrust::http::PreventRequestForgery;
use openrust::http::Query;
use openrust::http::RequestInfo;
use openrust::http::Validated;
use openrust::routing::RouteGroup;
use openrust::session::{MemorySessionStore, Session, SessionServiceProvider};
use openrust::support::BoxFuture;
use openrust::support::ServiceProvider;
use tokio::io::AsyncReadExt;

struct WebRoutes(PreventRequestForgery);
impl ServiceProvider for WebRoutes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.prevent_request_forgery(self.0.clone())?;
            app.exceptions(|handler| {
                handler.dont_flash(["profile.api_key", "items.0.token", "literal.dot"]);
            })?;
            app.routes(|routes| {
                routes
                    .group(RouteGroup::new().middleware_group("web"), |web| {
                        web.get("/form", token)
                            .get("/form/empty-token", |session: Session| async move {
                                session.put_typed("_token", "")?;
                                Ok::<_, openrust::Error>("empty")
                            })
                            .post("/form", || async { "saved" })
                            .post("/validation/json", invalid_json)
                            .post("/validation/large", invalid_large_snapshot)
                            .get("/validation/query", invalid_query)
                            .get("/validation/old", old_validation)
                            .post("/upload", upload)
                            .post("/native", |body: axum::body::Bytes| async move {
                                body.len().to_string()
                            })
                    })
                    .group(RouteGroup::new().middleware_group("api"), |api| {
                        api.post(
                            "/api/record/{id}",
                            |openrust::http::Path(id): openrust::http::Path<u64>| async move {
                                id.to_string()
                            },
                        )
                    })
            })?;
            Ok(())
        })
    }
}

async fn token(session: Session) -> Result<String> {
    Ok(session
        .token()?
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default())
}

async fn upload(request: openrust::http::Request) -> Result<String> {
    let file = request
        .file("attachment")?
        .ok_or_else(|| openrust::Error::custom("upload_missing", "缺少上传文件"))?;
    let file = file
        .downcast_ref::<openrust::http::UploadedFile>()
        .ok_or_else(|| openrust::Error::custom("upload_invalid", "上传字段不是文件"))?;
    let mut reader = file.open().await?;
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(openrust::Error::internal)?;
    if bytes != b"hello-upload" || request.content_len() < bytes.len() {
        return Err(openrust::Error::custom(
            "upload_replay_invalid",
            "受管上传未保留",
        ));
    }
    Ok("uploaded".to_owned())
}

#[derive(serde::Deserialize)]
struct ValidationInput {
    value: String,
}

impl openrust::validation::Validate for ValidationInput {
    fn rules() -> openrust::validation::Rules {
        use openrust::validation::{Rule, Rules};
        Rules::new().field("value", [Rule::required(), Rule::length(Some(3), None)])
    }
}

async fn invalid_json(Validated(Json(input)): Validated<Json<ValidationInput>>) -> String {
    input.value
}

async fn invalid_query(Validated(Query(input)): Validated<Query<ValidationInput>>) -> String {
    input.value
}

#[derive(serde::Deserialize)]
struct SnapshotQuery {
    marker: String,
}

#[derive(serde::Deserialize)]
struct SnapshotBody {
    value: u128,
}

async fn invalid_large_snapshot(
    Query(query): Query<SnapshotQuery>,
    Json(body): Json<SnapshotBody>,
) -> Result<()> {
    if query.marker != "query-only" || body.value != 0 {
        return Err(openrust::Error::custom(
            "snapshot_fixture_invalid",
            "异常输入快照测试未获得预期的有效 DTO",
        ));
    }
    let mut errors = openrust::ValidationErrors::default();
    errors.add("value", "invalid");
    Err(openrust::Error::Validation(errors))
}

async fn old_validation(
    info: RequestInfo,
    session: Session,
) -> Result<axum::Json<serde_json::Value>> {
    Ok(axum::Json(serde_json::json!({
        "old": info.old_all_json()?,
        "errors": session.read_errors()?.has_bag("default")
    })))
}

fn source(env: openrust::config::Environment) -> figment::Figment {
    let mut config = openrust::config::FrameworkConfig::default();
    config.app.env = env;
    figment::Figment::from(figment::providers::Serialized::defaults(config))
        .merge(("security", serde_json::json!({"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}})))
}

async fn fixture_with_environment(
    policy: PreventRequestForgery,
    env: openrust::config::Environment,
) -> Result<Application> {
    fixture_with_cookie_provider(policy, source(env), CookieServiceProvider::contextual()).await
}

async fn fixture_with_cookie_provider(
    policy: PreventRequestForgery,
    config: figment::Figment,
    cookies: CookieServiceProvider,
) -> Result<Application> {
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(openrust::encryption::EncryptionServiceProvider)
        .provider(cookies)
        .provider(
            SessionServiceProvider::with_store(MemorySessionStore::default()).with_cookie_config(
                openrust::config::SessionConfig {
                    driver: Some("array".into()),
                    cookie: Some("test_session".into()),
                    secure: Some(false),
                    ..Default::default()
                },
            ),
        )
        .provider(WebRoutes(policy));
    Application::from_provider_runtime(builder.start(config).await?)
}

async fn fixture(policy: PreventRequestForgery) -> Result<Application> {
    fixture_with_environment(policy, openrust::config::Environment::Development).await
}

#[tokio::test]
async fn web_group_rejects_missing_runtime_instead_of_running_unprotected() -> Result<()> {
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(WebRoutes(PreventRequestForgery::new()));
    let app = Application::from_provider_runtime(
        builder
            .start(source(openrust::config::Environment::Development))
            .await?,
    )?;
    assert!(build_router(&app).is_err());
    Ok(())
}

fn wire(method: &str, path: &str, cookie: Option<&str>, body: &str) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost")
        .header("content-type", "application/x-www-form-urlencoded");
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    request
        .body(Body::from(body.to_owned()))
        .expect("static fixture")
}

#[tokio::test]
async fn anonymous_unsafe_web_requires_token_while_api_has_no_session_cookie() -> Result<()> {
    let app = fixture(PreventRequestForgery::new()).await?;
    let router = build_router(&app)?;
    let api = router
        .clone()
        .oneshot(wire("POST", "/api/record/42", None, ""))
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(api.status(), StatusCode::OK);
    assert!(!api.headers().contains_key("set-cookie"));
    assert_eq!(
        to_bytes(api.into_body(), 4096)
            .await
            .map_err(openrust::Error::internal)?
            .as_ref(),
        b"42"
    );
    let missing = router
        .clone()
        .oneshot(wire("POST", "/form", None, ""))
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(missing.status().as_u16(), 419);
    let form = router
        .clone()
        .oneshot(wire("GET", "/form", None, ""))
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(form.status(), StatusCode::OK);
    let cookies = form
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(cookies.contains("test_session="));
    assert!(cookies.contains("XSRF-TOKEN="));
    let token = to_bytes(form.into_body(), 4096)
        .await
        .map_err(openrust::Error::internal)?;
    let token = std::str::from_utf8(&token).map_err(openrust::Error::internal)?;
    let valid = router
        .clone()
        .oneshot(wire(
            "POST",
            "/form",
            Some(&cookies),
            &format!("_token={token}"),
        ))
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(valid.status(), StatusCode::OK);
    let conflict = Request::builder()
        .method("POST")
        .uri("/form")
        .header("host", "localhost")
        .header("cookie", cookies.clone())
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-csrf-token", token)
        .body(Body::from("_token=incorrect"))
        .expect("static fixture");
    let denied = router
        .clone()
        .oneshot(conflict)
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(denied.status().as_u16(), 419);
    let xsrf = cookies
        .split("; ")
        .find_map(|pair| pair.strip_prefix("XSRF-TOKEN="))
        .expect("XSRF cookie present");
    let decoded = url::form_urlencoded::parse(format!("v={xsrf}").as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .expect("decoded cookie");
    let header = Request::builder()
        .method("POST")
        .uri("/form")
        .header("host", "localhost")
        .header("cookie", cookies)
        .header("x-xsrf-token", decoded)
        .body(Body::empty())
        .expect("static fixture");
    let accepted = router
        .oneshot(header)
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(accepted.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn laravel_xsrf_header_requires_the_session_token_after_authentication() -> Result<()> {
    use openrust::encryption::LaravelEncrypter;

    let app = fixture_with_cookie_provider(
        PreventRequestForgery::new(),
        source(openrust::config::Environment::Development).merge(("app.key", "A".repeat(32))),
        CookieServiceProvider::new(),
    )
    .await?;
    let encrypter = app.state().resolve::<LaravelEncrypter>().await?;
    let router = build_router(&app)?;
    let form = router
        .clone()
        .oneshot(wire("GET", "/form", None, ""))
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(form.status(), StatusCode::OK);
    let pairs = form
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .collect::<Vec<_>>();
    let session = pairs
        .iter()
        .find(|pair| pair.starts_with("test_session="))
        .expect("Session cookie present")
        .to_string();
    let xsrf = pairs
        .iter()
        .find_map(|pair| pair.strip_prefix("XSRF-TOKEN="))
        .expect("XSRF cookie present");
    let emitted_header = url::form_urlencoded::parse(format!("v={xsrf}").as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .expect("decoded XSRF cookie");
    let token = to_bytes(form.into_body(), 4096)
        .await
        .map_err(openrust::Error::internal)?;
    assert!(!token.is_empty());

    // 任意 41 字节的已认证前缀只由 XSRF 请求头跳过。
    let mut alternate_prefix = vec![b'!'; 41];
    alternate_prefix.extend_from_slice(&token);
    let mut incorrect_token = vec![b'!'; 41];
    incorrect_token.extend_from_slice(b"wrong-token");
    for (header, expected) in [
        (emitted_header, 200),
        (encrypter.encrypt_string(&alternate_prefix)?, 200),
        (encrypter.encrypt_string(&incorrect_token)?, 419),
        (encrypter.encrypt_string(b"short")?, 419),
        ("invalid-ciphertext".to_owned(), 419),
    ] {
        let mut request = wire("POST", "/form", Some(&session), "");
        request.headers_mut().insert(
            "x-xsrf-token",
            header.parse().map_err(openrust::Error::internal)?,
        );
        let response = router
            .clone()
            .oneshot(request)
            .await
            .map_err(openrust::Error::internal)?;
        assert_eq!(response.status().as_u16(), expected);
    }
    let xsrf = encrypter.encrypt_string(&alternate_prefix)?;
    let csrf = std::str::from_utf8(&token).map_err(openrust::Error::internal)?;
    assert_laravel_token_source_precedence(&router, &session, csrf, &xsrf).await?;
    assert_empty_session_token_is_rejected(&router, &session, &encrypter).await?;
    Ok(())
}

async fn assert_laravel_token_source_precedence(
    router: &axum::Router,
    session: &str,
    csrf: &str,
    xsrf: &str,
) -> Result<()> {
    for (content_type, body, csrf_header, expected) in [
        ("application/x-www-form-urlencoded", "_token=0", csrf, 200),
        ("application/x-www-form-urlencoded", "_token=0", "0", 200),
        (
            "application/x-www-form-urlencoded",
            "_token=incorrect",
            csrf,
            419,
        ),
        ("application/json", r#"{"_token":1}"#, csrf, 419),
        ("application/json", r#"{"_token":0}"#, csrf, 200),
        ("application/json", r#"{"_token":[]}"#, csrf, 200),
        ("application/json", r#"{"_token":["value"]}"#, csrf, 419),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri("/form")
            .header("host", "localhost")
            .header("cookie", session)
            .header("content-type", content_type)
            .header("x-csrf-token", csrf_header)
            .header("x-xsrf-token", xsrf)
            .body(Body::from(body))
            .expect("static fixture");
        let response = router
            .clone()
            .oneshot(request)
            .await
            .map_err(openrust::Error::internal)?;
        assert_eq!(response.status().as_u16(), expected, "{body}");
    }
    for (csrf_header, xsrf_header, expected) in [
        (b"wrong".as_slice(), xsrf, 419),
        (csrf.as_bytes(), "invalid-ciphertext", 200),
        (&[0xff], xsrf, 419),
    ] {
        let mut request = wire("POST", "/form", Some(session), "");
        request.headers_mut().insert(
            "x-csrf-token",
            axum::http::HeaderValue::from_bytes(csrf_header).map_err(openrust::Error::internal)?,
        );
        request.headers_mut().insert(
            "x-xsrf-token",
            xsrf_header.parse().map_err(openrust::Error::internal)?,
        );
        let response = router
            .clone()
            .oneshot(request)
            .await
            .map_err(openrust::Error::internal)?;
        assert_eq!(response.status().as_u16(), expected);
    }
    Ok(())
}

async fn assert_empty_session_token_is_rejected(
    router: &axum::Router,
    session: &str,
    encrypter: &openrust::encryption::LaravelEncrypter,
) -> Result<()> {
    let empty = router
        .clone()
        .oneshot(wire("GET", "/form/empty-token", Some(session), ""))
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(empty.status(), StatusCode::OK);
    let empty_session = empty
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .find(|pair| pair.starts_with("test_session="))
        .expect("updated Session cookie present");
    let stored = router
        .clone()
        .oneshot(wire("GET", "/form", Some(empty_session), ""))
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(stored.status(), StatusCode::OK);
    assert!(
        to_bytes(stored.into_body(), 4096)
            .await
            .map_err(openrust::Error::internal)?
            .is_empty()
    );
    let empty_xsrf = empty
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .find_map(|pair| pair.strip_prefix("XSRF-TOKEN="))
        .expect("issued empty XSRF cookie present");
    let empty_xsrf = url::form_urlencoded::parse(format!("v={empty_xsrf}").as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .expect("decoded empty XSRF cookie");
    for header in [
        empty_xsrf,
        encrypter.encrypt_string(b"short")?,
        encrypter.encrypt_string(&[b'!'; 41])?,
    ] {
        let mut request = wire("POST", "/form", Some(empty_session), "");
        request.headers_mut().insert(
            "x-xsrf-token",
            header.parse().map_err(openrust::Error::internal)?,
        );
        let response = router
            .clone()
            .oneshot(request)
            .await
            .map_err(openrust::Error::internal)?;
        assert_eq!(response.status().as_u16(), 419);
    }
    Ok(())
}

#[tokio::test]
async fn laravel_falsey_xsrf_headers_do_not_require_an_excluded_cookie() -> Result<()> {
    let app = fixture_with_cookie_provider(
        PreventRequestForgery::new().without_xsrf_cookie(),
        source(openrust::config::Environment::Development).merge(("app.key", "A".repeat(32))),
        CookieServiceProvider::laravel().except(["XSRF-TOKEN"]),
    )
    .await?;
    let router = build_router(&app)?;
    for (header, status) in [("", 419), ("0", 419), ("not-protected", 400)] {
        let mut request = wire("POST", "/form", None, "");
        request.headers_mut().insert(
            "x-xsrf-token",
            header.parse().map_err(openrust::Error::internal)?,
        );
        let response = router
            .clone()
            .oneshot(request)
            .await
            .map_err(openrust::Error::internal)?;
        assert_eq!(response.status().as_u16(), status);
    }
    Ok(())
}

#[tokio::test]
async fn origin_only_rejects_missing_origin_even_with_token() -> Result<()> {
    let app = fixture(PreventRequestForgery::new().origin_only()).await?;
    let router = build_router(&app)?;
    let response = router
        .oneshot(wire("POST", "/form", None, "_token=anything"))
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let accepted = build_router(&app)?
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/form")
                .header("host", "localhost")
                .header("sec-fetch-site", "same-origin")
                .body(Body::empty())
                .expect("static fixture"),
        )
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(accepted.status(), StatusCode::OK);
    assert!(
        !accepted
            .headers()
            .get_all("set-cookie")
            .iter()
            .any(|value| value
                .to_str()
                .is_ok_and(|value| value.starts_with("XSRF-TOKEN=")))
    );
    Ok(())
}

#[tokio::test]
async fn same_site_and_except_are_explicit_per_application() -> Result<()> {
    let default = fixture(PreventRequestForgery::new()).await?;
    let request = || {
        Request::builder()
            .method("POST")
            .uri("/form")
            .header("host", "localhost")
            .header("sec-fetch-site", "same-site")
            .body(Body::empty())
            .expect("static fixture")
    };
    let denied = build_router(&default)?
        .oneshot(request())
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(denied.status().as_u16(), 419);
    let allowed = fixture(PreventRequestForgery::new().allow_same_site()).await?;
    let accepted = build_router(&allowed)?
        .oneshot(request())
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(accepted.status(), StatusCode::OK);
    let except = fixture(PreventRequestForgery::new().except(["form"])).await?;
    let accepted = build_router(&except)?
        .oneshot(wire("POST", "/form", None, ""))
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(accepted.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn multipart_body_token_and_file_share_the_managed_request() -> Result<()> {
    let app = fixture(PreventRequestForgery::new()).await?;
    let router = build_router(&app)?;
    let form = router
        .clone()
        .oneshot(wire("GET", "/form", None, ""))
        .await
        .map_err(openrust::Error::internal)?;
    let cookies = form
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .collect::<Vec<_>>()
        .join("; ");
    let token = to_bytes(form.into_body(), 4096)
        .await
        .map_err(openrust::Error::internal)?;
    let token = std::str::from_utf8(&token).map_err(openrust::Error::internal)?;
    let body = format!(
        "--b\r\nContent-Disposition: form-data; name=\"_token\"\r\n\r\n{token}\r\n--b\r\nContent-Disposition: form-data; name=\"attachment\"; filename=\"hello.txt\"\r\nContent-Type: text/plain\r\n\r\nhello-upload\r\n--b--\r\n"
    );
    let send = |path| {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("host", "localhost")
            .header("cookie", cookies.clone())
            .header("content-type", "multipart/form-data; boundary=b")
            .body(Body::from(body.clone()))
            .expect("static fixture")
    };
    let response = router
        .clone()
        .oneshot(send("/upload"))
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(response.status(), StatusCode::OK);
    let native = router
        .oneshot(send("/native"))
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(native.status(), StatusCode::OK);
    let size = to_bytes(native.into_body(), 4096)
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(size.as_ref(), body.len().to_string().as_bytes());
    let incorrect = body.replacen(token, "wrong-token", 1);
    let denied = build_router(&app)?
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .header("host", "localhost")
                .header("cookie", cookies)
                .header("content-type", "multipart/form-data; boundary=b")
                .header("x-csrf-token", token)
                .body(Body::from(incorrect))
                .expect("static fixture"),
        )
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(denied.status().as_u16(), 419);
    Ok(())
}

struct TestingHarness;
impl openrust::foundation::ApplicationDefinition for TestingHarness {
    type Config = openrust::config::NoConfig;

    fn default_providers() -> openrust::support::DefaultProviders {
        openrust::support::DefaultProviders::empty()
    }

    fn providers(_: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        vec![
            Box::new(HttpServiceProvider),
            Box::new(openrust::encryption::EncryptionServiceProvider),
            Box::new(CookieServiceProvider::contextual()),
            Box::new(
                SessionServiceProvider::with_store(MemorySessionStore::default())
                    .with_cookie_config(openrust::config::SessionConfig {
                        driver: Some("array".into()),
                        secure: Some(false),
                        ..Default::default()
                    }),
            ),
            Box::new(WebRoutes(PreventRequestForgery::new())),
        ]
    }
}

#[tokio::test]
async fn testing_exemption_requires_trusted_test_app_boot_and_exact_environment() -> Result<()> {
    let manual = fixture_with_environment(
        PreventRequestForgery::new(),
        openrust::config::Environment::Testing,
    )
    .await?;
    let denied = build_router(&manual)?
        .oneshot(wire("POST", "/form", None, ""))
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(denied.status().as_u16(), 419);
    let test = openrust::testing::TestApp::<TestingHarness>::builder(source(
        openrust::config::Environment::Testing,
    ))
    .build()
    .await?;
    let accepted = test.raw_response(wire("POST", "/form", None, "")).await?;
    assert_eq!(accepted.status(), StatusCode::OK);
    let local = openrust::testing::TestApp::<TestingHarness>::builder(source(
        openrust::config::Environment::Development,
    ))
    .build()
    .await?;
    let denied = local.raw_response(wire("POST", "/form", None, "")).await?;
    assert_eq!(denied.status().as_u16(), 419);
    Ok(())
}

#[tokio::test]
async fn web_validated_json_and_query_flash_normalized_input_after_csrf_fast_paths() -> Result<()> {
    let app = fixture(PreventRequestForgery::new()).await?;
    let router = build_router(&app)?;
    let form = router
        .clone()
        .oneshot(wire("GET", "/form", None, ""))
        .await
        .map_err(openrust::Error::internal)?;
    let cookies = form
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .collect::<Vec<_>>()
        .join("; ");
    let json_failure = router.clone().oneshot(
        Request::post("/validation/json")
            .header("host", "localhost")
            .header("cookie", cookies.as_str())
            .header("content-type", "application/json")
            .header("accept", "text/html")
            .header("sec-fetch-site", "same-origin")
            .body(Body::from(r#"{"value":"x","profile":{"api_key":"hidden","name":"demo"},"items":[{"token":"hidden","name":"first"}],"literal.dot":"hidden"}"#))
            .map_err(openrust::Error::internal)?,
    ).await.map_err(openrust::Error::internal)?;
    assert_eq!(json_failure.status(), StatusCode::FOUND);
    let json_old = router
        .clone()
        .oneshot(
            Request::get("/validation/old")
                .header("host", "localhost")
                .header("cookie", cookies.as_str())
                .body(Body::empty())
                .map_err(openrust::Error::internal)?,
        )
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(json_old.status(), StatusCode::OK);
    let json_body: serde_json::Value = serde_json::from_slice(
        &to_bytes(json_old.into_body(), 16 * 1024)
            .await
            .map_err(openrust::Error::internal)?,
    )
    .map_err(openrust::Error::internal)?;
    assert_eq!(json_body["errors"], true);
    assert_eq!(
        json_body["old"],
        serde_json::json!({
            "value": "x", "profile": {"name": "demo"}, "items": [{"name": "first"}]
        })
    );
    let query_failure = router
        .clone()
        .oneshot(
            Request::get("/validation/query?value=x&note=hello")
                .header("host", "localhost")
                .header("cookie", cookies.as_str())
                .header("accept", "text/html")
                .body(Body::empty())
                .map_err(openrust::Error::internal)?,
        )
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(query_failure.status(), StatusCode::FOUND);
    let query_old = router
        .oneshot(
            Request::get("/validation/old")
                .header("host", "localhost")
                .header("cookie", cookies.as_str())
                .body(Body::empty())
                .map_err(openrust::Error::internal)?,
        )
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(query_old.status(), StatusCode::OK);
    let query_body: serde_json::Value = serde_json::from_slice(
        &to_bytes(query_old.into_body(), 16 * 1024)
            .await
            .map_err(openrust::Error::internal)?,
    )
    .map_err(openrust::Error::internal)?;
    assert_eq!(query_body["errors"], true);
    assert_eq!(
        query_body["old"],
        serde_json::json!({"value":"x","note":"hello"})
    );
    Ok(())
}

#[tokio::test]
async fn unrepresentable_json_snapshot_clears_partial_query_before_web_redirect() -> Result<()> {
    let app = fixture(PreventRequestForgery::new()).await?;
    let router = build_router(&app)?;
    let form = router
        .clone()
        .oneshot(wire("GET", "/form", None, ""))
        .await
        .map_err(openrust::Error::internal)?;
    let cookies = form
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .collect::<Vec<_>>()
        .join("; ");
    let failure = router
        .clone()
        .oneshot(
            Request::post("/validation/large?marker=query-only")
                .header("host", "localhost")
                .header("cookie", cookies.as_str())
                .header("content-type", "application/json")
                .header("accept", "text/html")
                .header("sec-fetch-site", "same-origin")
                .body(Body::from(r#"{"value":0,"unknown":1e400}"#))
                .map_err(openrust::Error::internal)?,
        )
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(failure.status(), StatusCode::FOUND);
    let old = router
        .oneshot(
            Request::get("/validation/old")
                .header("host", "localhost")
                .header("cookie", cookies.as_str())
                .body(Body::empty())
                .map_err(openrust::Error::internal)?,
        )
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(old.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &to_bytes(old.into_body(), 16 * 1024)
            .await
            .map_err(openrust::Error::internal)?,
    )
    .map_err(openrust::Error::internal)?;
    assert_eq!(body["errors"], true);
    assert_eq!(body["old"], serde_json::json!([]));
    Ok(())
}

#[path = "return_protection_cases.rs"]
mod return_protection_cases;
