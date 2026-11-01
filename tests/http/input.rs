//! 统一输入来源、规范化、严格类型及资源边界的公开行为回归。
#![allow(clippy::expect_used, clippy::panic)] // 测试构造失败直接中止当前用例。

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, FromRequest},
    http::{Request as Wire, StatusCode},
    routing::{get, post},
};
use openrust::ServiceExt;
use openrust::http::{InputNormalization, InputOutcome, Json, Query, Request, UploadedFile};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

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

fn assert_multipart_input_priority_and_empty_files(request: &Request) {
    assert_eq!(
        request.input("profile.name").expect("name"),
        Some(json!("Ada"))
    );
    assert_eq!(request.input("from").expect("query"), Some(json!("query")));
    assert_eq!(
        request.input("same").expect("input"),
        Some(json!("input wins"))
    );
    assert!(request.has_file("same").expect("shadowed file"));
    assert_eq!(
        request
            .all()
            .get("same")
            .expect("combined")
            .and_then(|value| value.to_json().ok()),
        Some(json!("input wins"))
    );
    let shadowed = request.file("same").expect("file").expect("present");
    let shadowed = shadowed.downcast_ref::<UploadedFile>().expect("uploaded");
    assert_eq!(shadowed.client_original_name(), "client.txt");
    assert_eq!(shadowed.client_original_path(), "folder/client.txt");
    assert!(
        request
            .file("empty")
            .expect("empty")
            .is_some_and(|value| value.is_null())
    );
    assert!(!request.has_file("empty").expect("empty file"));
    assert!(request.file("list.0").expect("list zero").is_none());
    assert!(request.has_file("list.1").expect("list one"));
    assert!(
        request
            .file("named.missing")
            .expect("named empty")
            .is_some_and(|value| value.is_null())
    );
    assert!(request.has_file("profile.avatar").expect("avatar"));
    assert!(!request.has_file("photos").expect("nested isn't direct"));
    assert!(request.has_file("photos.0").expect("direct array"));
}

#[tokio::test]
async fn multipart_file_tree_preserves_identity_input_priority_and_original_body() {
    let raw = concat!(
        "--files\r\nContent-Disposition: form-data; name=\"profile[name]\"\r\n\r\nAda\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"same\"\r\n\r\ninput wins\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"same\"; filename=\"folder/client.txt\"\r\nContent-Type: text/plain\r\n\r\nfile\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"profile[avatar]\"; filename=\"client.png\"\r\nContent-Type: image/png\r\n\r\nhello\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"photos[0][image]\"; filename=\"first.txt\"\r\nContent-Type: text/plain\r\n\r\nfirst\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"photos[1][image]\"; filename=\"second.txt\"\r\nContent-Type: text/plain\r\n\r\nsecond\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"empty\"; filename=\"\"\r\nContent-Type: application/octet-stream\r\n\r\n\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"list[0]\"; filename=\"\"\r\n\r\n\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"list[1]\"; filename=\"kept.txt\"\r\nContent-Type: text/plain\r\n\r\nkept\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"named[missing]\"; filename=\"\"\r\n\r\n\r\n",
        "--files--\r\n"
    );
    let request = inspect(
        "POST",
        "/?from=query",
        "multipart/form-data; boundary=files",
        raw,
    )
    .await;
    assert_eq!(
        request.content_bytes(raw.len()).await.expect("raw body"),
        Bytes::from_static(raw.as_bytes())
    );
    assert_multipart_input_priority_and_empty_files(&request);
    let avatar = request
        .file("profile.avatar")
        .expect("file")
        .expect("present");
    let from_all = request
        .all()
        .get("profile.avatar")
        .expect("path")
        .cloned()
        .expect("all file");
    assert!(avatar.same_object(&from_all));
    assert_eq!(
        request
            .when_has("profile.avatar", |value| Ok(value.same_object(&avatar)))
            .expect("file callback")
            .into_value(),
        Some(true)
    );
    assert!(matches!(
        request
            .when_filled("profile.avatar", |_| Ok(true))
            .expect("file-only is not filled input"),
        InputOutcome::Request(_)
    ));
    assert_eq!(
        request
            .when_has("same", |value| value.to_json())
            .expect("input priority callback")
            .into_value(),
        Some(json!("input wins"))
    );
    assert_eq!(
        request
            .when_filled("same", |value| value.to_json())
            .expect("pure input gate with mixed callback")
            .into_value(),
        Some(json!("input wins"))
    );
    let wildcard = request
        .file("photos.*.image")
        .expect("wildcard")
        .expect("files");
    assert_eq!(wildcard.entries().map(<[_]>::len), Some(2));
    assert!(request.all().to_json().is_err());
    assert_eq!(request.input_all()["profile"]["name"], "Ada");
    assert_eq!(
        request.keys(),
        vec![
            "profile", "same", "from", "same", "profile", "photos", "empty", "list", "named"
        ]
    );
    let file = avatar
        .downcast_ref::<UploadedFile>()
        .expect("uploaded file")
        .clone();
    let mut first = file.open().await.expect("first reader");
    let mut second = file.open().await.expect("second reader");
    drop(request);
    drop(avatar);
    drop(from_all);
    let mut one = [0_u8; 2];
    let mut two = [0_u8; 5];
    first.read_exact(&mut one).await.expect("first bytes");
    second.read_exact(&mut two).await.expect("second bytes");
    assert_eq!(&one, b"he");
    assert_eq!(&two, b"hello");
}

#[tokio::test]
async fn file_wildcards_keep_missing_positions_and_collapse_nested_arrays() {
    let raw = concat!(
        "--files\r\nContent-Disposition: form-data; name=\"photos[0][image]\"; filename=\"a.txt\"\r\n\r\na\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"photos[1][other]\"; filename=\"b.txt\"\r\n\r\nb\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"groups[0][photos][0][image]\"; filename=\"c.txt\"\r\n\r\nc\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"groups[0][photos][1][other]\"; filename=\"d.txt\"\r\n\r\nd\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"groups[1][other]\"; filename=\"e.txt\"\r\n\r\ne\r\n",
        "--files--\r\n"
    );
    let request = inspect("POST", "/", "multipart/form-data; boundary=files", raw).await;
    let photos = request
        .file("photos.*.image")
        .expect("photos")
        .expect("array");
    let photos = photos.entries().expect("array entries");
    assert_eq!(photos.len(), 2);
    assert!(photos[0].1.downcast_ref::<UploadedFile>().is_some());
    assert!(photos[1].1.is_null());
    let groups = request
        .file("groups.*.photos.*.image")
        .expect("groups")
        .expect("collapsed array");
    let groups = groups.entries().expect("collapsed entries");
    assert_eq!(groups.len(), 2);
    assert!(groups[0].1.downcast_ref::<UploadedFile>().is_some());
    assert!(groups[1].1.is_null());
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
    assert_eq!(
        request.content_bytes(raw.len()).await.expect("raw body"),
        Bytes::from_static(raw.as_bytes())
    );
    request
        .merge_if_missing(json!({"nullable":"no","q":"no","new":" untrimmed "}))
        .expect("merge");
    assert_eq!(
        request.input("new").expect("get"),
        Some(json!(" untrimmed "))
    );
    request.replace(json!({"only":1})).expect("replace");
    assert_eq!(request.input("q").expect("get"), Some(json!("value")));
    assert_eq!(
        request.content_bytes(raw.len()).await.expect("raw body"),
        Bytes::from_static(raw.as_bytes())
    );
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
    let malformed = Wire::post("/?q=yes")
        .header("content-type", "multipart/form-data;boundary=x")
        .body(Body::from("unparsed raw multipart"))
        .expect("wire");
    assert_eq!(
        Request::from_request(malformed, &())
            .await
            .expect_err("畸形 multipart 必须失败")
            .code(),
        "multipart_invalid"
    );
    for (raw, expected) in [
        ("broken", json!([])),
        ("null", json!([])),
        ("false", json!([false])),
        ("12", json!([12])),
        ("\" hi \"", json!(["hi"])),
    ] {
        assert_eq!(
            inspect("POST", "/", "application/json", raw)
                .await
                .input_all(),
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
            post(|request: Request| async move { Json(request.input_all()) }),
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
    assert_eq!(request.input_all(), json!({"empty":"","text":" hi "}));
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
        request
            .only(&["users.*.name", "absent"])
            .expect("only")
            .to_json()
            .expect("pure input"),
        json!({"users":{"*":{"name":["a","b"]}}})
    );
    assert_eq!(
        request
            .except(&["users.0"])
            .expect("except")
            .to_json()
            .expect("pure input")["users"],
        json!({"1":{"name":"b"}})
    );
    request
        .merge(json!({"users.*.name":"literal"}))
        .expect("merge");
    assert_eq!(
        request.input(r"users.\*.name").expect("literal"),
        Some(json!("literal"))
    );
    let old = request.input_all();
    let huge = Value::Object(
        (0..1025)
            .map(|index| (format!("k{index}"), Value::Null))
            .collect(),
    );
    assert!(request.merge(huge).is_err());
    assert_eq!(request.input_all(), old);
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
            post(|request: Request| async move { Json(request.content_len()) }),
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
        assert_eq!(plain.content_len(), raw.len());
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
        post(|request: Request| async move { Json(request.content_len()) }),
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

/// 检出便利转换误走严格 serde、显式 null 被默认值覆盖或别名不一致。
#[tokio::test]
async fn typed_scalar_helpers_preserve_php_casts_defaults_and_aliases() {
    let request=inspect("POST","/","application/json",r#"{"mixed":"1ab","null":null,"empty":[],"nonempty":[1],"yes":" YES ","number":"1e3","password":"  secret  "}"#).await;
    assert_eq!(request.integer("mixed").expect("cast"), 1);
    assert_eq!(request.integer_or("missing", 37).expect("default"), 37);
    assert_eq!(request.integer_or("null", 37).expect("null"), 0);
    assert_eq!(request.integer("empty").expect("empty"), 0);
    assert_eq!(request.integer("nonempty").expect("array"), 1);
    assert_eq!(request.float("number").expect("float"), 1000.0);
    assert!(request.boolean("yes").expect("boolean"));
    assert!(request.boolean_or("missing", true).expect("default"));
    assert!(!request.boolean_or("null", true).expect("null"));
    assert_eq!(
        request.string_or("null", "fallback").expect("null").value(),
        ""
    );
    assert_eq!(
        request.str_or("missing", 123).expect("default").value(),
        "123"
    );
    assert_eq!(
        request.string("password").expect("string").trim().value(),
        "secret"
    );
    assert!(request.exists(&["null", "mixed"]));
    assert_eq!(
        request
            .integer(&"x.".repeat(65))
            .expect_err("path limit")
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

/// 检出 keyed/all/only 三种数组读取混淆，以及集合过滤暗中重排稀疏键。
#[tokio::test]
async fn typed_arrays_and_collections_preserve_php_keys() {
    let request = inspect(
        "POST",
        "/",
        "application/json",
        r#"{"roles":["reader","discard","writer"],"email":"one@example.test","null":null}"#,
    )
    .await;
    assert_eq!(
        request.array("email").expect("array"),
        json!(["one@example.test"])
    );
    assert_eq!(request.array("null").expect("null"), json!([]));
    assert_eq!(request.array("missing").expect("missing"), json!([]));
    assert_eq!(
        request.array_only(&["email", "missing"]).expect("subset"),
        json!({"email":"one@example.test"})
    );
    assert_eq!(request.array_all(), request.input_all());
    let filtered = request
        .collect("roles")
        .expect("collect")
        .filter(|value, _| value != "discard")
        .expect("filter");
    assert_eq!(filtered.get(2_i64), Some(&json!("writer")));
    assert_eq!(
        filtered.to_value().expect("value"),
        json!({"0":"reader","2":"writer"})
    );
    assert_eq!(
        filtered
            .values()
            .expect("values")
            .to_value()
            .expect("value"),
        json!(["reader", "writer"])
    );
    assert!(request.collect("missing").expect("collect").is_empty());
    assert_eq!(
        request
            .collect_only(&["email"])
            .expect("subset")
            .to_value()
            .expect("value"),
        json!({"email":"one@example.test"})
    );
}

/// 检出 Fluent 对标量误包装、null 默认以及同一读取内核的 dot/直接键差异。
#[tokio::test]
async fn fluent_snapshots_use_same_typed_kernel_without_mutating_request() {
    let request=inspect("POST","/","application/json",r#"{"profile":{"age":"2_000","name":"Alice"},"scalar":"text","null":null,"profile.age":"direct"}"#).await;
    assert!(request.fluent("scalar").expect("scalar").is_empty());
    assert!(request.fluent("null").expect("null").is_empty());
    assert_eq!(
        request
            .fluent_or("null", json!({"fallback":1}))
            .expect("default")
            .integer("fallback")
            .expect("read"),
        1
    );
    let mut profile = request.fluent("profile").expect("profile");
    assert_eq!(profile.integer("age").expect("integer"), 2);
    profile.set("age", json!(9)).expect("set");
    assert_eq!(
        request.input("profile.age").expect("input"),
        Some(json!("2_000"))
    );
    let all = request.fluent_all().expect("all");
    assert_eq!(all.get("profile.age").expect("dot"), Some(json!("2_000")));
    assert_eq!(all.value("profile.age"), Some(&json!("direct")));
    assert!(!format!("{all:?}").contains("Alice"));
}

/// 检出集合映射绕过有界数组引擎或跨类型映射丢失键。
#[test]
fn support_collection_maps_remain_bounded() {
    let collection =
        openrust::support::Collection::from_value(json!({"left":1,"2":2})).expect("collection");
    let mapped = collection
        .clone()
        .map(|value, _| value.as_i64().expect("integer") + 1)
        .expect("map");
    assert_eq!(mapped.to_value().expect("value"), json!({"left":2,"2":3}));
    let error = collection
        .map(|_, _| vec![Value::Null; 1025])
        .expect_err("map bounds");
    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn conditional_callbacks_preserve_php_fallback_and_lazy_default() {
    use openrust::http::{InputOutcome, InputTruthy};
    use openrust::support::{Collection, Fluent, Stringable};
    use std::cell::Cell;
    let request = inspect(
        "POST",
        "/",
        "application/json",
        r#"{"zero":0,"no":false,"null":null,"empty":[],"text":"yes"}"#,
    )
    .await;
    for value in [
        json!(false),
        json!(0),
        json!(0.0),
        json!(""),
        json!("0"),
        json!([]),
        Value::Null,
    ] {
        assert!(matches!(
            request.when_has("zero", |_| Ok(value)).expect("callback"),
            InputOutcome::Request(_)
        ));
    }
    assert_eq!(
        request
            .when_has("null", |value| {
                assert!(value.is_null());
                Ok(7)
            })
            .expect("null present")
            .into_value(),
        Some(7)
    );
    assert_eq!(
        request
            .when_filled("no", |_| Ok(9))
            .expect("bool filled")
            .into_value(),
        Some(9)
    );
    assert_eq!(
        request
            .when_filled("empty", |_| Ok(9))
            .expect("array filled")
            .into_value(),
        Some(9)
    );
    let called = Cell::new(false);
    let result = request
        .when_has_or_else(
            "absent",
            |_| Ok(1),
            || {
                called.set(true);
                Ok(false)
            },
        )
        .expect("default");
    assert_eq!(result.into_default(), Some(false));
    assert!(called.get());
    let result = request
        .when_has_or_else(
            "text",
            |_| Ok("0"),
            || -> openrust::Result<bool> { panic!("default must stay lazy") },
        )
        .expect("lazy");
    assert!(result.into_request().is_some());
    assert_eq!(
        request
            .when_missing("absent", |value| {
                assert!(value.is_null());
                Ok("missing")
            })
            .expect("missing")
            .into_value(),
        Some("missing")
    );
    let chained = request
        .when_has("absent", |_| Ok(2))
        .expect("continue")
        .and_then(|same| same.when_has("zero", |_| Ok(3)))
        .expect("chain");
    assert_eq!(chained.into_value(), Some(3));
    assert!(Stringable::from("").is_truthy());
    assert!(Collection::<Value>::default().is_truthy());
    assert!(Fluent::default().is_truthy());
    let error = request
        .when_has("text", |_| {
            Err::<i32, _>(openrust::Error::custom("callback_failed", "失败"))
        })
        .err()
        .expect("error");
    assert_eq!(error.code(), "callback_failed");
}

#[tokio::test]
async fn conditional_mut_callbacks_modify_same_bag_and_short_circuit() {
    use openrust::http::InputOutcomeMut;
    let mut request = inspect("POST", "/", "application/json", r#"{"count":1}"#).await;
    let result = request
        .when_has_mut("count", |value, same| {
            assert_eq!(value.to_json()?, json!(1));
            same.merge(json!({"count":2}))?;
            Ok(false)
        })
        .expect("mut callback");
    let same = result.into_request().expect("false continues same request");
    assert_eq!(same.integer("count").expect("integer"), 2);
    let result = same
        .when_missing_mut_or_else(
            "count",
            |_, _| Ok(3),
            |same| {
                same.merge(json!({"default_called":true}))?;
                Ok(None::<i32>)
            },
        )
        .expect("mut default");
    assert!(matches!(result, InputOutcomeMut::Default(None)));
    assert!(request.boolean("default_called").expect("default mutation"));
    let result = request
        .when_has_mut("count", |_, _| Ok(5))
        .expect("value")
        .and_then(|_| panic!("value must short circuit"))
        .expect("short circuit");
    assert_eq!(result.into_value(), Some(5));
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
enum NumberChoice {
    Zero,
    One,
    Two,
}
impl openrust::support::BackedEnum for NumberChoice {
    type Backing = i64;
    fn try_from_backing(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Zero),
            1 => Some(Self::One),
            2 => Some(Self::Two),
            _ => None,
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
enum TextChoice {
    One,
    Empty,
}
impl openrust::support::BackedEnum for TextChoice {
    type Backing = String;
    fn try_from_backing(value: String) -> Option<Self> {
        match value.as_str() {
            "1" => Some(Self::One),
            "" => Some(Self::Empty),
            _ => None,
        }
    }
}
#[tokio::test]
async fn backed_enums_use_weak_union_rules_and_preserve_filtered_keys() {
    use NumberChoice::{One, Two, Zero};
    use openrust::http::InputOutcome;
    let mut request = inspect("POST", "/", "application/json", r#"{"float":1.5,"no":false,"null":null,"prefix":"1ab","decimal":"1.5","blank":" ","items":[1,9,2,1],"array":[]}"#).await;
    assert_eq!(
        request.r#enum::<NumberChoice>("float").expect("enum"),
        Some(One)
    );
    assert_eq!(
        request
            .enum_value::<TextChoice>("float")
            .expect("enum alias"),
        Some(TextChoice::One)
    );
    assert_eq!(
        request.r#enum::<NumberChoice>("no").expect("bool"),
        Some(Zero)
    );
    assert_eq!(
        request.r#enum::<TextChoice>("no").expect("union bool"),
        None
    );
    assert_eq!(
        request.r#enum::<NumberChoice>("null").expect("unfilled"),
        None
    );
    assert_eq!(request.enum_or("blank", Two).expect("default"), Two);
    assert_eq!(
        request
            .r#enum::<NumberChoice>("decimal")
            .expect("numeric full"),
        Some(One)
    );
    assert_eq!(
        request
            .r#enum::<NumberChoice>("prefix")
            .expect_err("type error")
            .code(),
        "input_enum_type_invalid"
    );
    assert!(request.r#enum::<NumberChoice>("array").is_err());
    let cases = request
        .enums::<NumberChoice>("items")
        .expect("enum collection");
    assert_eq!(
        cases
            .all()
            .iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect::<Vec<_>>(),
        vec![
            ("0".to_owned(), &One),
            ("2".to_owned(), &Two),
            ("3".to_owned(), &One)
        ]
    );
    assert!(matches!(
        request
            .when_enum("float", |value: NumberChoice| {
                assert_eq!(value, One);
                Ok(0)
            })
            .expect("false callback"),
        InputOutcome::Request(_)
    ));
    assert_eq!(
        request
            .when_enum_or_else("blank", |_: NumberChoice| Ok(1), || Ok(false))
            .expect("default")
            .into_default(),
        Some(false)
    );
    request
        .when_enum_mut("float", |value: NumberChoice, same| {
            assert_eq!(value, One);
            same.merge(json!({"changed":true}))?;
            Ok(())
        })
        .expect("mut enum")
        .into_request()
        .expect("continue");
    assert!(request.boolean("changed").expect("same bag"));
}
#[tokio::test]
async fn clamp_uses_full_numeric_checks_and_parameter_type_order() {
    use openrust::support::InputNumber::{Float, Integer};
    let request = inspect(
        "POST",
        "/",
        "application/json",
        r#"{"inside":1.0,"high":3.0,"prefix":"2ab","numeric":" 2e0 ","no":true}"#,
    )
    .await;
    assert_eq!(
        request
            .clamp("inside", Integer(1), Integer(3))
            .expect("clamp"),
        Float(1.0)
    );
    assert_eq!(
        request
            .clamp("high", Integer(1), Integer(3))
            .expect("clamp"),
        Integer(3)
    );
    assert_eq!(
        request
            .clamp_or("prefix", Integer(0), Integer(3), Integer(2))
            .expect("full numeric"),
        Integer(2)
    );
    assert_eq!(
        request
            .clamp("numeric", Integer(0), Integer(3))
            .expect("exponent"),
        Float(2.0)
    );
    assert_eq!(
        request
            .clamp("no", Integer(0), Integer(3))
            .expect("bool not numeric"),
        Integer(0)
    );
    assert_eq!(
        request
            .clamp("inside", Integer(3), Integer(1))
            .expect("reversed"),
        Integer(1)
    );
    assert_eq!(
        Float(f64::NAN).clamp(Float(f64::NEG_INFINITY), Float(f64::INFINITY)),
        Float(f64::NEG_INFINITY)
    );
}

/// 检出外部日期字符串在小数进位或中间 i128 极值处溢出 panic，而非安全返回错误。
#[tokio::test]
async fn date_timestamp_carry_extremes_return_safe_errors_through_public_readers() {
    let cases = [
        "@170141183460469231731687303715884105727.9999995",
        "@-170141183460469231731687303715884105727.9999995",
        "@-170141183460469231731687303715884105728.9999995",
        "170141183460469231731687303715884105727.9999995",
        "-170141183460469231731687303715884105727.9999995",
        "-170141183460469231731687303715884105728.9999995",
    ];
    for source in cases {
        let raw = serde_json::to_string(&json!({"when": source})).expect("fixture body");
        let request = inspect("POST", "/", "application/json", &raw).await;
        let fluent = request.fluent_all().expect("same bounded bag");
        for result in [request.date("when"), fluent.date("when")] {
            let error = result.expect_err("unrepresentable epoch must fail without panicking");
            assert_eq!(error.code(), "input_date_invalid");
            assert_eq!(error.status(), StatusCode::BAD_REQUEST);
            assert!(!error.to_string().contains(source));
        }
        assert_eq!(
            request
                .content_bytes(raw.len())
                .await
                .expect("raw body")
                .as_ref(),
            raw.as_bytes()
        );
        assert_eq!(
            request.input("when").expect("unchanged input"),
            Some(json!(source))
        );
    }
}

#[tokio::test]
async fn dates_read_clock_per_call_and_keep_explicit_timezone() {
    use openrust::support::{
        CalendarClock, CalendarContext, CalendarInstant, DateOptions, ResolvedTimezone,
        TimezoneInput,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    };
    struct CountingClock(AtomicI64);
    impl CalendarClock for CountingClock {
        fn is_test_clock(&self) -> bool {
            true
        }
        fn now(&self) -> openrust::Result<CalendarInstant> {
            CalendarInstant::new(self.0.fetch_add(1, Ordering::SeqCst), 0)
        }
    }
    let clock = Arc::new(CountingClock(AtomicI64::new(946_684_800)));
    let mut wire = Wire::post("/")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"current":"now","absolute":"2000-01-01 00:00:00"}"#,
        ))
        .expect("wire");
    wire.extensions_mut()
        .insert(CalendarContext::new(ResolvedTimezone::utc(), clock.clone()));
    let request = Request::from_request(wire, &()).await.expect("request");
    assert_eq!(clock.0.load(Ordering::SeqCst), 946_684_800);
    assert_eq!(
        request
            .date("current")
            .expect("first")
            .expect("date")
            .timestamp(),
        946_684_800
    );
    assert_eq!(
        request
            .date("current")
            .expect("second")
            .expect("date")
            .timestamp(),
        946_684_801
    );
    assert_eq!(clock.0.load(Ordering::SeqCst), 946_684_802);
    let fluent = request.fluent_all().expect("fluent context");
    assert_eq!(
        fluent
            .date("current")
            .expect("fluent date")
            .expect("date")
            .timestamp(),
        946_684_802
    );
    assert_eq!(clock.0.load(Ordering::SeqCst), 946_684_803);
    let date = request
        .date_with(
            "absolute",
            DateOptions {
                format: None,
                timezone: Some(TimezoneInput::Name("Asia/Shanghai")),
            },
        )
        .expect("explicit zone")
        .expect("date");
    assert_eq!(date.timestamp(), 946_656_000);
    assert_eq!(date.timezone().name(), "Asia/Shanghai");
}

struct CalendarRoutes {
    override_clock: bool,
}
impl openrust::core::ServiceProvider for CalendarRoutes {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            // 后继 Provider 覆盖，证明默认 context 读取最终配置。
            app.overlay("app", json!({"timezone":"Europe/Paris"}))?;
            app.routes(|routes| routes.get("/calendar", calendar_handler))?;
            if self.override_clock {
                let context = openrust::support::CalendarContext::new(
                    openrust::support::ResolvedTimezone::utc(),
                    std::sync::Arc::new(openrust::support::FixedClock::new(
                        openrust::support::CalendarInstant::new(946_684_800, 0)?,
                    )),
                );
                app.http_layer(axum::Extension(context))?;
            }
            Ok(())
        })
    }
}
async fn calendar_handler(request: Request) -> openrust::Result<Json<Value>> {
    let date = request.date("when")?.expect("fixture date");
    Ok(Json(
        json!({"timestamp":date.timestamp(),"zone":date.timezone().name()}),
    ))
}
#[tokio::test]
async fn calendar_router_uses_final_timezone_and_respects_application_extension()
-> Result<(), Box<dyn std::error::Error>> {
    for override_clock in [false, true] {
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            openrust::config::FrameworkConfig::default(),
        ));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(openrust::http::HttpServiceProvider)
            .provider(CalendarRoutes { override_clock });
        let app = openrust::App::from_provider_runtime(builder.start(sources).await?)?;
        let response = openrust::http::build_router(&app)?
            .oneshot(
                Wire::get("/calendar?when=2000-01-01%2000%3A00%3A00")
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 4096).await?)?;
        let data = &body["data"];
        assert_eq!(
            data["zone"],
            if override_clock {
                "UTC"
            } else {
                "Europe/Paris"
            }
        );
        assert_eq!(
            data["timestamp"],
            if override_clock {
                946_684_800
            } else {
                946_681_200
            }
        );
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
    }
    Ok(())
}
#[tokio::test]
async fn invalid_app_timezone_fails_startup_without_exposing_original_value() {
    let mut config = openrust::config::FrameworkConfig::default();
    assert_eq!(config.app.timezone, "UTC");
    config.app.timezone = "private-invalid-zone-sentinel".to_owned();
    let mut builder = openrust::core::ProviderBuilder::new();
    builder.provider(openrust::http::HttpServiceProvider);
    let result = builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(config),
        ))
        .await;
    let error = result.err().expect("startup must reject invalid zone");
    assert!(error.to_string().contains("app.timezone"));
    assert!(!error.to_string().contains("sentinel"));
}

#[tokio::test]
async fn scalar_helpers_match_actual_php_control_character_cases_without_normalization() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/php85/scalar-control-cases.json"))
            .expect("synthetic cases");
    let golden: Vec<Value> = include_str!("fixtures/php85/scalar-control-golden.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).expect("golden"))
        .collect();
    assert_eq!(cases.len(), 61);
    assert_eq!(cases.len(), golden.len());
    let values = cases
        .iter()
        .enumerate()
        .map(|(i, case)| {
            (
                format!("case_{i}"),
                serde_json::from_str::<Value>(case["raw"].as_str().expect("raw JSON string"))
                    .expect("actual control string"),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let mut wire = Wire::post("/")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&values).expect("body")))
        .expect("wire");
    wire.extensions_mut().insert(InputNormalization::disabled());
    let request = Request::from_request(wire, &()).await.expect("request");
    for (i, (case, expected)) in cases.iter().zip(&golden).enumerate() {
        let key = format!("case_{i}");
        assert_eq!(case["id"], expected["id"]);
        assert_eq!(
            STANDARD.encode(request.string(&key).expect("string").value()),
            expected["string"]["base64"],
            "{}",
            case["id"]
        );
        assert_eq!(
            request.integer(&key).expect("integer").to_string(),
            expected["integer"]["decimal"],
            "{}",
            case["id"]
        );
        assert_eq!(
            format!("{:016x}", request.float(&key).expect("float").to_bits()),
            expected["float"]["bits"],
            "{}",
            case["id"]
        );
        assert_eq!(
            request.boolean(&key).expect("boolean"),
            expected["boolean"]["value"],
            "{}",
            case["id"]
        );
    }
}

#[tokio::test]
async fn intervals_keep_components_and_distinguish_absence_invalid_units_and_work_limits() {
    use openrust::support::IntervalUnit;
    let request = inspect(
        "POST",
        "/",
        "application/json",
        r#"{"duration":"90 minutes","number":90,"numeric_text":"90","fraction":1.5,"blank":" "}"#,
    )
    .await;
    let interval = request
        .interval("duration")
        .expect("interval")
        .expect("duration");
    assert_eq!(
        (interval.hours, interval.minutes, interval.total_days()),
        (0, 90, None)
    );
    assert_eq!(request.interval("number").expect("nonstring make"), None);
    assert_eq!(
        request
            .interval("numeric_text")
            .expect_err("numeric string invalid")
            .code(),
        "input_interval_invalid"
    );
    assert_eq!(
        request
            .interval_with_unit("missing", "invalid")
            .expect("absence before unit"),
        None
    );
    assert_eq!(
        request
            .interval_with_unit("blank", "invalid")
            .expect("empty before unit"),
        None
    );
    assert_eq!(
        request
            .interval_with_unit("fraction", IntervalUnit::Hour)
            .expect("enum unit")
            .expect("interval")
            .minutes,
        30
    );
    assert_eq!(
        request
            .interval_with_unit("number", "m")
            .expect_err("human alias is not Unit")
            .code(),
        "input_interval_unit_invalid"
    );
    assert_eq!(
        request
            .fluent_all()
            .expect("fluent")
            .interval("duration")
            .expect("same parser"),
        Some(interval)
    );
    let mut budget_request = inspect("POST", "/", "application/json", "{}").await;
    budget_request
        .merge(json!({"large":"1".repeat(20 * 1024 * 1024 - "large".len())}))
        .expect("value reaches DataValue budget without exceeding it");
    let error = budget_request
        .interval_with_unit("large", "minute")
        .expect_err("independent source plus unit budget");
    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(error.code(), "input_limits_exceeded");
}

#[tokio::test]
async fn interval_of_grammar_and_shared_term_budget_reject_without_mutating_bags() {
    let mut request = inspect(
        "POST",
        "/",
        "application/json",
        r#"{"valid":"second Tuesday of next month"}"#,
    )
    .await;
    for text in ["of 2 days", "of of 2 days", "1 day of 2 days"] {
        request.merge(json!({"value":text})).expect("fixture");
        let previous = request.input_all();
        assert_eq!(
            request.interval("value").expect_err("invalid of").code(),
            "input_interval_invalid"
        );
        assert_eq!(
            request
                .fluent_all()
                .expect("fluent")
                .interval("value")
                .expect_err("same grammar")
                .code(),
            "input_interval_invalid"
        );
        assert_eq!(request.input_all(), previous);
    }
    assert_eq!(
        request
            .interval("valid")
            .expect("ordinal weekday of")
            .expect("interval")
            .days,
        7
    );
    for term in ["0d", "0 days ", "tomorrow "] {
        request
            .merge(json!({"value":term.repeat(1024)}))
            .expect("within source budget");
        assert!(request.interval("value").is_ok());
        request
            .merge(json!({"value":term.repeat(1025)}))
            .expect("source valid but parser terms bounded");
        let previous = request.input_all();
        let error = request.interval("value").expect_err("term budget");
        assert_eq!(error.code(), "input_limits_exceeded");
        assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(request.input_all(), previous);
    }
}

/// 检出原文 spill 后两个 reader 共用文件游标，或 Request 释放时提前删除暂存。
#[tokio::test]
async fn spilled_raw_readers_keep_independent_offsets_and_ownership() {
    // 超过暂存阈值，保持在默认 2 MiB 的 Axum 正文限制内。
    let raw = format!("start{}end", "x".repeat(1024 * 1024));
    let request = inspect("POST", "/", "text/plain", &raw).await;
    assert_eq!(request.content_len(), raw.len());
    assert_eq!(
        request
            .content_bytes(raw.len() - 1)
            .await
            .expect_err("explicit bound must reject")
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let mut first = request.content().await.expect("first reader");
    let mut second = request.content().await.expect("second reader");
    let clone = request.clone();
    drop(request);
    let mut prefix = [0_u8; 5];
    first.read_exact(&mut prefix).await.expect("first prefix");
    assert_eq!(&prefix, b"start");
    let mut independent = [0_u8; 5];
    second
        .read_exact(&mut independent)
        .await
        .expect("second prefix");
    assert_eq!(&independent, b"start");
    drop(clone);
    let mut suffix = Vec::new();
    second
        .read_to_end(&mut suffix)
        .await
        .expect("reader outlives request");
    assert_eq!(suffix, raw.as_bytes()[5..]);
    let mut next = [0_u8; 1];
    first.read_exact(&mut next).await.expect("first offset");
    assert_eq!(&next, b"x");
}

/// 检出关闭 Axum 默认 2 MiB 限制后误被 Request 内部硬阈值拒绝。
#[tokio::test]
async fn disabled_default_body_limit_accepts_large_raw_with_explicit_projection_bound() {
    let raw = "x".repeat(2 * 1024 * 1024 + 1);
    let router = Router::new()
        .route(
            "/",
            post(|request: Request| async move {
                let length = request.content_len();
                let below_limit = request.content_bytes(length - 1).await.is_err();
                let exact = request.content_bytes(length).await?.len();
                Ok::<_, openrust::Error>(Json(json!({
                    "length": length,
                    "below_limit": below_limit,
                    "exact": exact,
                })))
            }),
        )
        .layer(DefaultBodyLimit::disable());
    let response = router
        .oneshot(Wire::post("/").body(Body::from(raw.clone())).expect("wire"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .expect("response body");
    let envelope = serde_json::from_slice::<Value>(&body).expect("JSON");
    assert_eq!(envelope["success"], true);
    assert_eq!(envelope["code"], "ok");
    assert_eq!(envelope["message"], "ok");
    assert_eq!(
        envelope["data"],
        json!({"length": raw.len(), "below_limit": true, "exact": raw.len()})
    );
}
