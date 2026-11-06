//! C04-A 请求快照与固定 Laravel/Symfony 内容协商回归。

// 行为测试以明确期望的构造失败直接中止该用例。
#![allow(clippy::expect_used)]

use axum::{
    Router,
    body::Body,
    extract::FromRequestParts,
    http::{HeaderValue, Request, StatusCode},
    routing::post,
};
use openrust::ServiceExt;
use openrust::http::{Json, RequestInfo, axum};
use serde::Deserialize;

fn inspect(request: Request<Body>) -> Result<RequestInfo, openrust::Error> {
    let (mut parts, _) = request.into_parts();
    tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(RequestInfo::from_request_parts(&mut parts, &()))
}

fn request(path: &str, accept: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().uri(path);
    if let Some(accept) = accept {
        builder = builder.header("accept", accept);
    }
    builder.body(Body::empty()).expect("request")
}

#[cfg(feature = "session")]
#[test]
fn old_input_without_session_is_missing_and_uses_default() {
    let info = inspect(request("/", None)).expect("info");
    assert!(matches!(info.old_all().expect("old bag"),
        openrust::support::DataValue::Array(values) if values.is_empty()));
    assert!(info.old("name").expect("old value").is_none());
    assert_eq!(
        info.old_or(
            "name",
            openrust::support::DataValue::String("fallback".into())
        )
        .expect("old default")
        .to_json()
        .expect("json"),
        serde_json::json!("fallback")
    );
    assert_eq!(
        info.old_all_json().expect("json bag"),
        serde_json::json!([])
    );
    assert_eq!(info.old_json("name").expect("json value"), None);
}

#[cfg(feature = "session")]
struct Avatar;

#[cfg(feature = "session")]
impl openrust::support::DataObject for Avatar {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn json_projection(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!("avatar"))
    }
}

#[cfg(feature = "session")]
#[tokio::test]
async fn old_input_retains_same_object_then_reads_explicit_json_after_reload()
-> openrust::Result<()> {
    use openrust::session::{MemorySessionStore, Session};
    use openrust::support::{DataValue, ObjectEncoding};
    use std::sync::Arc;

    let store = Arc::new(MemorySessionStore::default());
    let session = Session::new(None, store.clone(), None)?;
    session.start().await?;
    let object = DataValue::object(Avatar);
    let mut input = DataValue::Array(Vec::new());
    input.set("profile.avatar", object.clone())?;
    input.set("nil", DataValue::Null)?;
    session.flash_input(input)?;
    let mut wire = request("/", None);
    wire.extensions_mut().insert(session.clone());
    let (mut parts, _) = wire.into_parts();
    let info = RequestInfo::from_request_parts(&mut parts, &()).await?;
    let raw = info
        .old("profile.avatar")?
        .ok_or_else(|| openrust::Error::custom("old_input_missing", "旧输入对象缺失"))?;
    assert!(raw.same_object(&object));
    assert!(
        info.old_or("profile.avatar", DataValue::String("fallback".into()))?
            .same_object(&object)
    );
    assert!(
        info.old_all()?
            .get("profile.avatar")?
            .is_some_and(|value| value.same_object(&object))
    );
    assert_eq!(
        info.old_json("profile.avatar")
            .err()
            .map(|error| error.code().to_owned()),
        Some("data_object_not_serializable".into())
    );
    assert_eq!(
        info.old_all_json()
            .err()
            .map(|error| error.code().to_owned()),
        Some("data_object_not_serializable".into())
    );
    assert!(
        info.old_or("nil", DataValue::String("fallback".into()))?
            .is_null()
    );
    assert_eq!(
        info.old_or_json("absent", serde_json::json!("fallback"))?,
        serde_json::json!("fallback")
    );
    assert_eq!(
        info.old_or_json("nil", serde_json::json!("fallback"))?,
        serde_json::Value::Null
    );

    session.set_object_encoding(ObjectEncoding::JsonProjection)?;
    session.save().await?;
    let loaded = Session::new(Some(session.id()?), store, None)?;
    loaded.start().await?;
    let mut wire = request("/", None);
    wire.extensions_mut().insert(loaded);
    let (mut parts, _) = wire.into_parts();
    let info = RequestInfo::from_request_parts(&mut parts, &()).await?;
    assert_eq!(
        info.old_json("profile.avatar")?,
        Some(serde_json::json!("avatar"))
    );
    assert!(
        matches!(info.old("profile.avatar")?, Some(DataValue::String(value)) if value == "avatar")
    );
    Ok(())
}

#[test]
fn accept_order_canonical_params_duplicate_and_zero_quality() {
    let info = inspect(request("/", Some("text/html; q=0, application/json; note=\"a,b;c=d\";q=0.8, text/html;q=0.5, application/xml;q=0.8"))).expect("info");
    assert_eq!(
        info.acceptable_content_types(),
        [
            "application/json;note=\"a,b;c=d\"",
            "application/xml",
            "text/html"
        ]
    );
    assert!(info.accepts(&["text/html"]));
    assert_eq!(info.prefers(&["html", "json"]), Some("json"));
    assert_eq!(info.format("fallback"), "json");
    let zero = inspect(request("/", Some("application/json;q=0"))).expect("info");
    assert!(zero.accepts_json());
    assert!(zero.wants_json());
}

#[test]
fn wildcard_vendor_alias_and_match_direction() {
    let empty = inspect(request("/", None)).expect("info");
    assert!(empty.accepts_any_content_type());
    assert!(empty.accepts(&[]));
    assert_eq!(empty.prefers(&["json"]), None);
    let wildcard = inspect(request("/", Some("*/*"))).expect("info");
    assert_eq!(wildcard.prefers(&["html", "json"]), Some("html"));
    let vendor = inspect(request("/", Some("application/vnd.api+json"))).expect("info");
    assert!(vendor.wants_json());
    assert_eq!(vendor.format("html"), "jsonapi");
    assert!(!vendor.accepts_json());
    assert_eq!(vendor.prefers(&["json"]), Some("json"));
    let plain = inspect(request("/", Some("application/json"))).expect("info");
    assert!(plain.accepts(&["application/vnd.api+json"]));
    assert_eq!(plain.prefers(&["application/vnd.api+json"]), None);
    let markdown = inspect(request("/", Some("text/markdown"))).expect("info");
    assert!(markdown.wants_markdown());
    assert!(markdown.accepts_markdown());
}

#[test]
fn path_headers_bearer_ajax_pjax_and_debug_are_bounded() {
    let mut incoming = Request::builder()
        .method("POST")
        .uri("/nested?secret=URI_SENTINEL")
        .header(
            "authorization",
            "Basic ignored, bEaReR TOKEN_SENTINEL, next",
        )
        .header("x-requested-with", "XMLHttpRequest")
        .header("x-pjax", "0")
        .header("content-type", "Application/JSON")
        .header("host", "Example.test")
        .body(Body::empty())
        .expect("request");
    incoming.extensions_mut().insert(axum::extract::OriginalUri(
        "/a/%E4%B8%AD//?secret=URI_SENTINEL".parse().expect("uri"),
    ));
    let info = inspect(incoming).expect("info");
    assert_eq!(info.path(), "a/%E4%B8%AD");
    assert_eq!(info.uri().query(), Some("secret=URI_SENTINEL"));
    assert!(info.is_method("post"));
    assert_eq!(info.host().expect("host"), "example.test");
    assert_eq!(info.bearer_token().expect("token"), Some("TOKEN_SENTINEL"));
    assert!(info.has_header("AUTHORIZATION"));
    assert_eq!(
        info.header("authorization").expect("header"),
        Some("Basic ignored, bEaReR TOKEN_SENTINEL, next")
    );
    assert!(info.ajax().expect("ajax"));
    assert!(!info.pjax().expect("pjax"));
    assert!(info.expects_json().expect("expects"));
    assert!(!info.is_json().expect("is_json"));
    let debug = format!("{info:?}");
    assert!(!debug.contains("TOKEN_SENTINEL"));
    assert!(!debug.contains("URI_SENTINEL"));
    assert!(!debug.contains("authorization"));
    assert_eq!(inspect(request("////", None)).expect("root").path(), "/");
}

#[test]
fn invalid_text_and_resource_limits_are_stable_errors() {
    let mut binary = request("/", None);
    binary
        .headers_mut()
        .insert("x-bin", HeaderValue::from_bytes(b"\xff").expect("header"));
    let info = inspect(binary).expect("info");
    assert!(info.headers().contains_key("x-bin"));
    assert_eq!(
        info.header("x-bin").expect_err("binary").code(),
        "http_request_header_invalid"
    );
    let mut binary_accept = request("/", None);
    binary_accept
        .headers_mut()
        .insert("accept", HeaderValue::from_bytes(b"\xff").expect("header"));
    assert_eq!(
        inspect(binary_accept).expect_err("binary accept").code(),
        "http_request_header_invalid"
    );
    let huge = request("/", Some(&"x".repeat(16 * 1024 + 1)));
    assert_eq!(
        inspect(huge).expect_err("large accept").code(),
        "http_request_headers_too_large"
    );
    let many = request("/", Some(&vec!["text/plain"; 129].join(",")));
    assert_eq!(
        inspect(many).expect_err("many accept").code(),
        "http_request_headers_too_large"
    );
    let mut many_headers = request("/", None);
    for index in 0..129 {
        many_headers.headers_mut().insert(
            format!("x-{index}")
                .parse::<axum::http::HeaderName>()
                .expect("name"),
            HeaderValue::from_static("value"),
        );
    }
    assert_eq!(
        inspect(many_headers).expect_err("many headers").code(),
        "http_request_headers_too_large"
    );
}

#[test]
fn host_reuses_domain_authority_and_ignores_forwarding() {
    let valid = Request::builder()
        .uri("http://EXAMPLE.test:8080/path")
        .header("host", "example.test:8080")
        .header("x-forwarded-host", "evil.test")
        .body(Body::empty())
        .expect("request");
    assert_eq!(
        inspect(valid).expect("info").host().expect("host"),
        "example.test"
    );
    let conflicting = Request::builder()
        .uri("http://example.test/path")
        .header("host", "evil.test")
        .body(Body::empty())
        .expect("request");
    assert_eq!(
        inspect(conflicting)
            .expect("info")
            .host()
            .expect_err("conflict")
            .code(),
        "http_domain_authority_invalid"
    );
    let missing = inspect(request("/", None)).expect("info");
    assert_eq!(
        missing.host().expect_err("missing").code(),
        "http_domain_authority_invalid"
    );
    let mut duplicate = request("/", None);
    duplicate
        .headers_mut()
        .append("host", HeaderValue::from_static("a.test"));
    duplicate
        .headers_mut()
        .append("host", HeaderValue::from_static("a.test"));
    assert_eq!(
        inspect(duplicate)
            .expect("info")
            .host()
            .expect_err("duplicate")
            .code(),
        "http_domain_authority_invalid"
    );
}

#[derive(Deserialize)]
struct Payload {
    value: String,
}

#[tokio::test]
async fn nested_original_uri_and_json_body_coexist() {
    let router = Router::new().nest(
        "/api",
        Router::new().route(
            "/body",
            post(
                |info: RequestInfo, Json(payload): Json<Payload>| async move {
                    format!("{}:{}", info.path(), payload.value)
                },
            ),
        ),
    );
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/body")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"value":"retained"}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .expect("body");
    assert_eq!(&body[..], b"api/body:retained");
}

#[test]
fn symfony_flag_numeric_prefix_and_regex_suffix_are_preserved() {
    let info = inspect(request(
        "/",
        Some(
            "text/plain;flowed;q=.1,text/plain;flowed=1;q=.9,text/html;q=.8junk,application/json;q",
        ),
    ))
    .expect("info");
    assert_eq!(
        info.acceptable_content_types(),
        ["application/json", "text/plain;flowed=1", "text/html"]
    );
    let repeated = inspect(request(
        "/",
        Some("application/json;q=.5,text/html;q=.5,application/json;q=.5"),
    ))
    .expect("info");
    assert_eq!(
        repeated.acceptable_content_types(),
        ["text/html", "application/json"]
    );
    let json = inspect(request("/", Some("application/json"))).expect("info");
    assert!(!json.accepts(&["application/+json"]));
    assert!(json.accepts(&["application/+json+json", "application/vnd+jsonjunk"]));
    let quoted = inspect(request(
        "/",
        Some("\"text/html\";q=.8,application/json;q=.7"),
    ))
    .expect("info");
    assert_eq!(
        quoted.acceptable_content_types(),
        ["text/html", "application/json"]
    );
}

#[test]
fn utf8_first_header_and_exact_resource_boundaries() {
    let mut incoming = request("/", None);
    incoming.headers_mut().append(
        "x-text",
        HeaderValue::from_bytes("Café".as_bytes()).expect("utf8"),
    );
    incoming
        .headers_mut()
        .append("x-text", HeaderValue::from_static("second"));
    incoming.headers_mut().append(
        "authorization",
        HeaderValue::from_static("Bearer first Bearer last, ignored"),
    );
    incoming
        .headers_mut()
        .append("authorization", HeaderValue::from_static("Bearer second"));
    let info = inspect(incoming).expect("info");
    assert_eq!(info.header("x-text").expect("text"), Some("Café"));
    assert_eq!(info.bearer_token().expect("token"), Some("last"));
    let mut headers = request("/", None);
    for _ in 0..128 {
        headers
            .headers_mut()
            .append("x", HeaderValue::from_static("v"));
    }
    assert!(inspect(headers).is_ok());
    let mut bytes = request("/", None);
    bytes.headers_mut().insert(
        "x",
        HeaderValue::from_str(&"a".repeat(65535)).expect("header"),
    );
    assert!(inspect(bytes).is_ok());
    let mut bytes = request("/", None);
    bytes.headers_mut().insert(
        "x",
        HeaderValue::from_str(&"a".repeat(65536)).expect("header"),
    );
    assert_eq!(
        inspect(bytes).expect_err("size").status(),
        StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
    );
    assert!(inspect(request("/", Some(&vec!["text/plain"; 128].join(",")))).is_ok());
    assert!(inspect(request("/", Some(&"x".repeat(16384)))).is_ok());
}

#[test]
fn content_negotiation_formats_ajax_pjax_and_first_accept() {
    for (mime, format) in [
        ("application/problem+json", "problem"),
        ("application/custom+json", "json"),
        ("application/custom+wbxml", "xml"),
        ("text/plain", "txt"),
        ("text/markdown", "fallback"),
    ] {
        let info = inspect(request("/", Some(mime))).expect("info");
        assert_eq!(info.format("fallback"), format);
    }
    let subtype = inspect(request("/", Some("application/*"))).expect("info");
    assert!(subtype.accepts_json());
    assert_eq!(subtype.prefers(&["json", "html"]), Some("json"));
    assert_eq!(subtype.prefers(&[]), None);
    for (pjax, expected) in [("", true), ("0", true), ("1", false)] {
        let mut incoming = request("/", Some("*/*"));
        incoming.headers_mut().insert(
            "x-requested-with",
            HeaderValue::from_static("XMLHttpRequest"),
        );
        incoming
            .headers_mut()
            .insert("x-pjax", HeaderValue::from_str(pjax).expect("pjax"));
        assert_eq!(
            inspect(incoming)
                .expect("info")
                .expects_json()
                .expect("expects"),
            expected
        );
    }
    let mut incoming = request("/", Some("text/html"));
    incoming
        .headers_mut()
        .append("accept", HeaderValue::from_static("application/json"));
    let info = inspect(incoming).expect("info");
    assert!(info.accepts_html());
    assert!(!info.wants_json());
    assert!(!info.accepts_json());
}

#[test]
fn negative_and_positive_zero_quality_keep_declaration_order() {
    let info = inspect(request("/", Some("text/html;q=-0,application/json;q=0"))).expect("info");
    assert_eq!(
        info.acceptable_content_types(),
        ["text/html", "application/json"]
    );
    assert_eq!(info.prefers(&["html", "json"]), Some("html"));
}
