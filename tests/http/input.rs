//! 统一输入来源、规范化、严格类型及资源边界的公开行为回归。
#![allow(clippy::expect_used)] // 测试构造失败直接中止当前用例。

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, FromRequest},
    http::{Request as Wire, StatusCode},
    routing::{get, post},
};
use openrust::ServiceExt;
use openrust::http::{InputNormalization, Json, Query, Request};
use serde::Deserialize;
use serde_json::{Value, json};

async fn inspect(method: &str, uri: &str, content_type: &str, body: &str) -> Request {
    Request::from_request(
        Wire::builder()
            .method(method)
            .uri(uri)
            .header("content-type", content_type)
            .body(Body::from(body.to_owned()))
            .expect("wire"),
        &(),
    )
    .await
    .expect("input")
}

#[tokio::test]
async fn json_source_union_null_and_original_body_are_preserved() {
    let raw = r#"{"user":{"name":" Alice "},"nullable":null,"zero":0,"flag":false,"empty":[],"blank":" "}"#;
    let mut request = inspect(
        "GET",
        "/?user[extra]=query&nullable=fallback&q=%20value%20",
        "application/json",
        raw,
    )
    .await;
    assert_eq!(
        request.input("user.name").expect("get"),
        Some(json!("Alice"))
    );
    assert_eq!(request.input("user.extra").expect("get"), None);
    assert_eq!(request.input("nullable").expect("get"), Some(Value::Null));
    assert_eq!(request.query("nullable"), Some(json!("fallback")));
    assert!(request.has(&["nullable", "zero", "flag", "empty"]));
    assert!(request.filled(&["zero", "flag", "empty"]));
    assert!(request.is_not_filled(&["blank", "missing", "nullable"]));
    assert!(request.any_filled(&["missing", "q"]));
    assert!(request.missing(&["q", "absent"]));
    assert_eq!(request.content(), &Bytes::from_static(raw.as_bytes()));
    request
        .merge_if_missing(json!({"nullable":"no","q":"no","new":" untrimmed "}))
        .expect("merge");
    assert_eq!(
        request.input("new").expect("get"),
        Some(json!(" untrimmed "))
    );
    request.replace(json!({"only":1})).expect("replace");
    assert_eq!(request.input("q").expect("get"), Some(json!("value")));
    assert_eq!(request.content(), &Bytes::from_static(raw.as_bytes()));
    assert!(request.post("user").is_none());
}

#[tokio::test]
async fn form_get_head_other_media_and_dynamic_json_scalars_follow_sources() {
    let get = inspect(
        "HEAD",
        "/?name=query",
        "application/x-www-form-urlencoded",
        "name=body",
    )
    .await;
    assert_eq!(get.input("name").expect("get"), Some(json!("query")));
    let form = inspect(
        "PATCH",
        "/?name=query&fallback=yes",
        "application/x-www-form-urlencoded",
        "name=%20body%20&user[age]=42&list[]=a&list[]=b",
    )
    .await;
    assert_eq!(form.post("name"), Some(json!("body")));
    assert!(form.post("user.age").is_none());
    assert_eq!(form.input("user.age").expect("age"), Some(json!("42")));
    assert_eq!(
        form.input("list.*").expect("wildcard"),
        Some(json!(["a", "b"]))
    );
    assert_eq!(form.input("fallback").expect("get"), Some(json!("yes")));
    let unknown = inspect(
        "POST",
        "/?q=yes",
        "multipart/form-data;boundary=x",
        "unparsed raw multipart",
    )
    .await;
    assert_eq!(unknown.all(), json!({"q":"yes"}));
    assert_eq!(
        unknown.content(),
        &Bytes::from_static(b"unparsed raw multipart")
    );
    for (raw, expected) in [
        ("broken", json!([])),
        ("null", json!([])),
        ("false", json!([false])),
        ("12", json!([12])),
        ("\" hi \"", json!(["hi"])),
    ] {
        assert_eq!(
            inspect("POST", "/", "application/json", raw).await.all(),
            expected
        );
    }
}

#[tokio::test]
async fn normalize_unicode_password_full_key_and_policy_overrides() {
    let request=inspect("POST","/","application/json",r#"{"name":"\u200b\u00ad X \ufeff","password":" secret ","current_password":"","user":{"password":" nested "},"rows":[{"password":" row "}]}"#).await;
    assert_eq!(request.input("name").expect("get"), Some(json!("X")));
    assert_eq!(
        request.input("password").expect("get"),
        Some(json!(" secret "))
    );
    assert_eq!(
        request.input("current_password").expect("get"),
        Some(Value::Null)
    );
    assert_eq!(
        request.input("user.password").expect("get"),
        Some(json!("nested"))
    );
    let empty_key = inspect(
        "POST",
        "/",
        "application/json",
        r#"{"":{"password":" nested "}}"#,
    )
    .await;
    assert_eq!(
        empty_key.input(".password").expect("get"),
        Some(json!("nested"))
    );
    let app = Router::new()
        .route(
            "/",
            post(|request: Request| async move { Json(request.all()) }),
        )
        .layer(InputNormalization::default().except_trim(["rows.*.password"]));
    let response = app
        .oneshot(
            Wire::post("/")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"rows":[{"password":" row "}]}"#))
                .expect("wire"),
        )
        .await
        .expect("response");
    let data: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(data["data"]["rows"][0]["password"], " row ");
    let mut wire = Wire::post("/")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"empty":"","text":" hi "}"#))
        .expect("wire");
    wire.extensions_mut().insert(InputNormalization::disabled());
    let request = Request::from_request(wire, &()).await.expect("input");
    assert_eq!(request.all(), json!({"empty":"","text":" hi "}));
    let debug = format!("{request:?}");
    assert!(!debug.contains("hi"));
    assert!(!debug.contains("empty"));
}

#[derive(Deserialize)]
struct QueryPayload {
    count: u32,
    enabled: bool,
    optional: Option<u32>,
    items: Vec<u32>,
    user: User,
}
#[derive(Deserialize)]
struct User {
    age: u32,
}
#[tokio::test]
async fn query_typed_nested_numbers_booleans_and_nullable_options_survive_normalization() {
    let router = Router::new().route(
        "/",
        get(|Query(value): Query<QueryPayload>| async move {
            Json(json!([
                value.count,
                u32::from(value.enabled),
                value.optional.unwrap_or_default(),
                value.items[1],
                value.user.age
            ]))
        }),
    );
    let response = router
        .oneshot(
            Wire::get("/?count=%2042%20&enabled=true&optional=&items[]=3&items[]=4&user[age]=9")
                .body(Body::empty())
                .expect("wire"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(value["data"], json!([42, 1, 0, 4, 9]));
}
#[derive(Deserialize)]
struct Optional {
    text: Option<String>,
}
#[derive(Deserialize)]
struct Strict {
    user: User,
}
#[tokio::test]
async fn strict_json_preserves_400_415_422_413_and_field_paths() {
    let router = Router::new()
        .route(
            "/",
            post(|Json(value): Json<Strict>| async move { Json(value.user.age) }),
        )
        .layer(DefaultBodyLimit::max(128));
    for (mime, body, status) in [
        ("application/json", "broken", 400),
        ("text/plain", r#"{"user":{"age":2}}"#, 415),
        ("application/json", r#"{"user":{"age":"2"}}"#, 422),
        ("application/json", r#"{"user":{"age":2}}"#, 200),
    ] {
        let response = router
            .clone()
            .oneshot(
                Wire::post("/")
                    .header("content-type", mime)
                    .body(Body::from(body))
                    .expect("wire"),
            )
            .await
            .expect("response");
        assert_eq!(response.status().as_u16(), status);
        if status == 422 {
            let value: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), 4096)
                    .await
                    .expect("body"),
            )
            .expect("json");
            assert!(value["errors"]["user.age"].is_array());
        }
    }
    let response = router
        .oneshot(
            Wire::post("/")
                .header("content-type", "application/json")
                .body(Body::from(" ".repeat(129)))
                .expect("wire"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn filtering_mutations_are_atomic_and_bounded_without_rewriting_raw() {
    let mut request = inspect(
        "POST",
        "/?fallback=query",
        "application/json",
        r#"{"users":[{"name":"a"},{"name":"b"}],"present":null}"#,
    )
    .await;
    assert_eq!(
        request.only(&["users.*.name", "absent"]).expect("only"),
        json!({"users":{"*":{"name":["a","b"]}}})
    );
    assert_eq!(
        request.except(&["users.0"]).expect("except")["users"],
        json!({"1":{"name":"b"}})
    );
    request
        .merge(json!({"users.*.name":"literal"}))
        .expect("merge");
    assert_eq!(
        request.input(r"users.\*.name").expect("literal"),
        Some(json!("literal"))
    );
    let old = request.all();
    let huge = Value::Object(
        (0..1025)
            .map(|index| (format!("k{index}"), Value::Null))
            .collect(),
    );
    assert!(request.merge(huge).is_err());
    assert_eq!(request.all(), old);
    assert_eq!(
        request
            .input(&vec!["x"; 65].join("."))
            .expect_err("path limit")
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn query_encoding_pair_and_body_limits_are_rejections_not_truncation() {
    for uri in [
        "/?name=%FF".to_owned(),
        format!("/?{}", "x".repeat(16385)),
        format!("/?{}", vec!["x=1"; 1025].join("&")),
    ] {
        let error = Request::from_request(Wire::get(uri).body(Body::empty()).expect("wire"), &())
            .await
            .expect_err("rejection");
        assert!(matches!(
            error.status(),
            StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE
        ));
    }
    let router = Router::new()
        .route(
            "/",
            post(|request: Request| async move { Json(request.content().len()) }),
        )
        .layer(DefaultBodyLimit::max(8));
    let response = router
        .oneshot(Wire::post("/").body(Body::from("123456789")).expect("wire"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn json_depth_guard_precedes_decoder_and_non_json_raw_stays_available() {
    for depth in [65, 129, 4096] {
        let raw = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        let error = Request::from_request(
            Wire::post("/")
                .header("content-type", "application/json")
                .body(Body::from(raw.clone()))
                .expect("wire"),
            &(),
        )
        .await
        .expect_err("limit");
        assert_eq!(error.code(), "input_limits_exceeded");
        assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let plain = inspect("POST", "/?q=yes", "text/plain", &raw).await;
        assert_eq!(plain.input("q").expect("query"), Some(json!("yes")));
        assert_eq!(plain.content().len(), raw.len());
        assert_eq!(
            plain.json("0").expect_err("lazy JSON limit").status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }
    let text = format!(
        "{{\"text\":{}}}",
        serde_json::to_string(&format!("{}\\\"{}", "[".repeat(200), "]".repeat(200)))
            .expect("encode")
    );
    assert!(
        inspect("POST", "/", "application/json", &text)
            .await
            .input("text")
            .expect("get")
            .is_some()
    );
}

#[tokio::test]
async fn typed_json_recursion_is_a_limit_and_raw_bytes_have_explicit_limits() {
    let json_router = Router::new().route(
        "/",
        post(|Json(value): Json<Value>| async move { Json(value) }),
    );
    for depth in [65, 129, 4096] {
        let raw = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        let response = json_router
            .clone()
            .oneshot(
                Wire::post("/")
                    .header("content-type", "application/json")
                    .body(Body::from(raw))
                    .expect("wire"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
    for (mime, raw, status) in [
        (
            "application/json",
            r#""recursion limit exceeded at line 1""#.to_owned(),
            StatusCode::OK,
        ),
        (
            "application/json",
            r#"{"recursion limit exceeded at line ":}"#.to_owned(),
            StatusCode::BAD_REQUEST,
        ),
        (
            "text/plain",
            format!("{}0{}", "[".repeat(129), "]".repeat(129)),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
    ] {
        let response = json_router
            .clone()
            .oneshot(
                Wire::post("/")
                    .header("content-type", mime)
                    .body(Body::from(raw))
                    .expect("wire"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), status);
    }
    let plain_router = Router::new().route(
        "/",
        post(|request: Request| async move { Json(request.content().len()) }),
    );
    let raw = "x".repeat(2 * 1024 * 1024 + 1);
    let wire = || Wire::post("/").body(Body::from(raw.clone())).expect("wire");
    assert_eq!(
        plain_router
            .clone()
            .oneshot(wire())
            .await
            .expect("response")
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        plain_router
            .layer(DefaultBodyLimit::max(raw.len()))
            .oneshot(wire())
            .await
            .expect("response")
            .status(),
        StatusCode::OK
    );
    let raw = r#"{"text":""}"#;
    let app = Router::new()
        .route(
            "/",
            post(|Json(value): Json<Optional>| async move { Json(value.text) }),
        )
        .layer(DefaultBodyLimit::max(raw.len()));
    assert_eq!(
        app.oneshot(
            Wire::post("/")
                .header("content-type", "application/json")
                .body(Body::from(raw))
                .expect("wire")
        )
        .await
        .expect("response")
        .status(),
        StatusCode::OK
    );
}

struct PendingBody(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl futures_util::Stream for PendingBody {
    type Item = Result<Bytes, std::io::Error>;
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Pending
    }
}
impl Drop for PendingBody {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
#[tokio::test]
async fn cancelling_input_body_read_releases_the_original_stream() {
    let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let body = Body::from_stream(PendingBody(std::sync::Arc::clone(&dropped)));
    let wire = Wire::post("/").body(body).expect("wire");
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(10),
            Request::from_request(wire, &())
        )
        .await
        .is_err()
    );
    assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
}

#[derive(Deserialize, Debug, PartialEq)]
enum Filter {
    Range { page: u32 },
    Tuple(u32, bool),
    One(u32),
    Unit,
}
#[derive(Deserialize)]
struct FilterQuery {
    filter: Filter,
}
#[tokio::test]
async fn typed_query_enum_variants_keep_numeric_conversion() {
    for (uri, expected) in [
        ("/?filter[Range][page]=2", Filter::Range { page: 2 }),
        (
            "/?filter[Tuple][]=3&filter[Tuple][]=true",
            Filter::Tuple(3, true),
        ),
        ("/?filter[One]=4", Filter::One(4)),
        ("/?filter=Unit", Filter::Unit),
    ] {
        let request = inspect("GET", uri, "text/plain", "").await;
        let parsed: FilterQuery = request.deserialize_query().expect("enum numeric fields");
        assert_eq!(parsed.filter, expected);
    }
}

#[derive(Deserialize)]
struct UnsignedWide {
    value: u128,
}
#[derive(Deserialize)]
struct SignedWide {
    value: i128,
}
#[tokio::test]
async fn managed_json_preserves_native_axum_128_bit_integer_precision() {
    let app = Router::new()
        .route(
            "/native-u",
            post(|axum::Json(value): axum::Json<UnsignedWide>| async move {
                assert_eq!(value.value, u128::MAX);
                StatusCode::OK
            }),
        )
        .route(
            "/managed-u",
            post(|Json(value): Json<UnsignedWide>| async move {
                assert_eq!(value.value, u128::MAX);
                StatusCode::OK
            }),
        )
        .route(
            "/native-i",
            post(|axum::Json(value): axum::Json<SignedWide>| async move {
                assert_eq!(value.value, i128::MIN);
                StatusCode::OK
            }),
        )
        .route(
            "/managed-i",
            post(|Json(value): Json<SignedWide>| async move {
                assert_eq!(value.value, i128::MIN);
                StatusCode::OK
            }),
        );
    for (path, value) in [
        ("/native-u", u128::MAX.to_string()),
        ("/native-i", i128::MIN.to_string()),
        ("/managed-u", u128::MAX.to_string()),
        ("/managed-i", i128::MIN.to_string()),
    ] {
        let response = app
            .clone()
            .oneshot(
                Wire::post(path)
                    .header("content-type", "application/json")
                    .body(Body::from(format!("{{\"value\":{value}}}")))
                    .expect("wire"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
}

#[tokio::test]
async fn empty_key_sets_are_vacuously_present() {
    let request = inspect("GET", "/", "text/plain", "").await;
    assert!(request.has(&[]));
    assert!(!request.missing(&[]));
}

struct CustomWide(u128);
impl<'de> Deserialize<'de> for CustomWide {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        u128::deserialize(deserializer).map(Self)
    }
}
#[derive(Deserialize)]
enum WideChoice {
    U(u128),
    I(i128),
}
#[derive(Deserialize)]
struct NestedWide {
    choices: Vec<WideChoice>,
    custom: CustomWide,
}
#[tokio::test]
async fn large_numbers_in_nested_enum_and_custom_dto_remain_exact() {
    let app = Router::new().route(
        "/",
        post(|Json(value): Json<NestedWide>| async move {
            assert!(matches!(value.choices[0],WideChoice::U(v) if v==u128::MAX));
            assert!(matches!(value.choices[1],WideChoice::I(v) if v==i128::MIN));
            assert!(matches!(value.choices[2],WideChoice::I(v) if v==i128::MAX));
            assert_eq!(value.custom.0, u128::MAX - 1);
            StatusCode::OK
        }),
    );
    let raw = format!(
        "{{\"choices\":[{{\"U\":{}}},{{\"I\":{}}},{{\"I\":{}}}],\"custom\":{}}}",
        u128::MAX,
        i128::MIN,
        i128::MAX,
        u128::MAX - 1
    );
    let response = app
        .oneshot(
            Wire::post("/")
                .header("content-type", "application/json")
                .body(Body::from(raw.clone()))
                .expect("wire"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn native_axum_extractors_do_not_apply_managed_normalization() {
    let app = Router::new()
        .route(
            "/",
            post(|axum::Json(value): axum::Json<Value>| async move { Json(value) }),
        )
        .layer(InputNormalization::default());
    let response = app
        .oneshot(
            Wire::post("/")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":" title ","empty":""}"#))
                .expect("wire"),
        )
        .await
        .expect("response");
    let value: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(value["data"], json!({"title":" title ","empty":""}));
}

#[derive(Deserialize)]
struct DuplicateField {
    text: String,
}
struct Occurrences(Vec<(String, String)>);
impl<'de> Deserialize<'de> for Occurrences {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries;
        impl<'de> serde::de::Visitor<'de> for Entries {
            type Value = Occurrences;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("object entries in original order")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, String>()? {
                    entries.push(entry);
                }
                Ok(Occurrences(entries))
            }
        }
        deserializer.deserialize_map(Entries)
    }
}
#[tokio::test]
async fn native_duplicate_field_errors_and_custom_map_order_are_preserved() {
    let app = Router::new()
        .route(
            "/native",
            post(|axum::Json(value): axum::Json<DuplicateField>| async move { Json(value.text) }),
        )
        .route(
            "/managed",
            post(|Json(value): Json<DuplicateField>| async move { Json(value.text) }),
        )
        .route(
            "/entries",
            post(|Json(value): Json<Occurrences>| async move { Json(value.0) }),
        );
    let raw = r#"{"text":" first ","text":" last "}"#;
    for path in ["/native", "/managed"] {
        let response = app
            .clone()
            .oneshot(
                Wire::post(path)
                    .header("content-type", "application/json")
                    .body(Body::from(raw))
                    .expect("wire"),
            )
            .await
            .expect("response");
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{path}"
        );
    }
    let response = app
        .oneshot(
            Wire::post("/entries")
                .header("content-type", "application/json")
                .body(Body::from(raw))
                .expect("wire"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(value["data"], json!([["text", "first"], ["text", "last"]]));
}

#[tokio::test]
async fn ignored_out_of_range_numbers_match_native_json_behavior() {
    let app = Router::new()
        .route(
            "/native",
            post(|axum::Json(value): axum::Json<UnsignedWide>| async move {
                assert_eq!(value.value, 0);
                StatusCode::OK
            }),
        )
        .route(
            "/managed",
            post(|Json(value): Json<UnsignedWide>| async move {
                assert_eq!(value.value, 0);
                StatusCode::OK
            }),
        );
    let mut statuses = Vec::new();
    for path in ["/native", "/managed"] {
        statuses.push(
            app.clone()
                .oneshot(
                    Wire::post(path)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"value":0,"unknown":1e400}"#))
                        .expect("wire"),
                )
                .await
                .expect("response")
                .status(),
        );
    }
    assert_eq!(
        statuses[0], statuses[1],
        "native versus managed ignored number"
    );
}

#[derive(Deserialize)]
struct Floating {
    value: f64,
}
#[tokio::test]
async fn actual_out_of_range_float_targets_keep_native_rejection() {
    let app = Router::new()
        .route(
            "/native",
            post(|axum::Json(value): axum::Json<Floating>| async move {
                assert!(value.value.is_finite());
                StatusCode::OK
            }),
        )
        .route(
            "/managed",
            post(|Json(value): Json<Floating>| async move {
                assert!(value.value.is_finite());
                StatusCode::OK
            }),
        );
    for path in ["/native", "/managed"] {
        let response = app
            .clone()
            .oneshot(
                Wire::post(path)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"value":1e400}"#))
                    .expect("wire"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
    }
}

#[tokio::test]
async fn typed_json_lexical_resources_reject_large_entry_and_expanded_path_counts() {
    let app = Router::new().route(
        "/",
        post(|Json(value): Json<Value>| async move { Json(value) }),
    );
    for (body, status) in [
        (
            format!(
                "{{{}}}",
                (0..1024)
                    .map(|i| format!(r#""k{i}":null"#))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            StatusCode::OK,
        ),
        (
            format!("{{{}}}", vec![r#""same":null"#; 1025].join(",")),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            serde_json::to_string(&json!({"r".repeat(21*1024): vec![Value::Null;1000]}))
                .expect("json"),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Wire::post("/")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .expect("wire"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), status);
    }
}
