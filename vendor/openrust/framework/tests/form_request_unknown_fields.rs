use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        container::Container as _, foundation::Application as _, http::Kernel as _,
        session::Session as _,
    },
    foundation::{
        Application,
        configuration::Routing,
        http::{FormRequest, FormRequestDefaults, Kernel},
    },
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
    routing::Router,
    validation::{RuleSet, ValidationResult},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
#[derive(Default)]
struct Article<const MODE: u8>;
impl<const MODE: u8> FormRequest for Article<MODE> {
    fn fail_on_unknown_fields(&self) -> Option<bool> {
        match MODE {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        }
    }
    fn error_bag(&self) -> &str {
        "article"
    }
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([
            ("title", "sometimes|string"),
            ("profile.name", "sometimes|string"),
            ("items.*.name", "sometimes|string"),
            ("password", "sometimes|string"),
            ("author\\.name", "sometimes|string"),
        ])
    }
}
fn validate(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = match request.uri().path().rsplit('/').next().unwrap() {
            "strict" => Article::<1>.validate_resolved(request).await?,
            "relaxed" => Article::<2>.validate_resolved(request).await?,
            _ => Article::<0>.validate_resolved(request).await?,
        };
        Response::json(StatusCode::OK, data.into_value())
    })
}
struct ValidateBeforeDispatch;
impl openrust::contracts::http::Middleware for ValidateBeforeDispatch {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        _next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        validate(request)
    }
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}

async fn start(reject: bool) -> (tempfile::TempDir, SocketAddr, tokio::task::JoinHandle<()>) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("resources/views")).unwrap();
    std::fs::write(
        root.path().join("resources/views/errors-probe.blade.html"),
        "@error('admin', 'article')<span>{{ $message }}</span>@else<span>clean</span>@enderror",
    )
    .unwrap();
    let mut app = Application::configure(root.path().to_owned())
        .with_config(|_, _| {
            Ok(BTreeMap::from([
                (
                    "app".into(),
                    json!({"locale":"en","supported_locales":[],"key":"a".repeat(32)}),
                ),
                (
                    "session".into(),
                    json!({"driver":"array","cookie":"unknown_session","lottery":[0,100]}),
                ),
            ]))
        })
        .with_middleware(|m| {
            m.prevent_request_forgery(["web/*"], false, false);
        })
        .with_routing(Routing::new(|_| {
            let mut r = Router::new();
            for path in ["/strict", "/default", "/relaxed"] {
                r.post(path, validate)?;
            }
            for path in [
                "/preflight/strict",
                "/preflight/default",
                "/preflight/relaxed",
            ] {
                r.post(path, || Response::empty(StatusCode::OK))?
                    .middleware_name("precognitive")
                    .middleware(Arc::new(ValidateBeforeDispatch));
            }
            r.post("/web/strict", validate)?.middleware_name("web");
            r.get("/errors", errors)?.middleware_name("web");
            r.get("/error-page", || openrust::view::view("errors-probe"))?
                .middleware_name("web");
            Ok(r)
        }))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app.container()
        .make::<FormRequestDefaults>()
        .unwrap()
        .fail_on_unknown_fields(reject);
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    (root, addr, server)
}
async fn send(
    address: SocketAddr,
    method: &str,
    target: &str,
    body: &str,
    headers: &str,
) -> (u16, Value, String) {
    let path = if target.starts_with("http") {
        let url = url::Url::parse(target).unwrap();
        format!(
            "{}{}",
            url.path(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        )
    } else {
        target.to_owned()
    };
    let accept = if headers.contains("Accept:") {
        ""
    } else {
        "Accept: application/json\r\n"
    };
    let content_type = if headers.contains("Content-Type:") {
        ""
    } else {
        "Content-Type: application/json\r\n"
    };
    timeout(Duration::from_secs(5),async {
        let mut stream=TcpStream::connect(address).await.unwrap();stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\n{accept}{content_type}Content-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        let mut response=String::new();stream.read_to_string(&mut response).await.unwrap();let status=response.split_whitespace().nth(1).unwrap().parse().unwrap();let data=response.split_once("\r\n\r\n").unwrap().1;
        let value=if data.is_empty(){Value::Null}else{serde_json::from_str(data).unwrap_or_else(|_|Value::String(data.to_owned()))};(status,value,response)
    }).await.unwrap()
}

#[tokio::test]
async fn unknown_fields_match_fixed_reference_for_nested_wildcard_and_literal_keys() {
    let (_root, address, server) = start(false).await;
    let cases: Value =
        serde_json::from_str(include_str!("reference/form-request-unknown-fields.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let (status, data, _) =
            send(address, "POST", "/strict", &case["input"].to_string(), "").await;
        let has_errors = case["errors"].is_object();
        assert_eq!(status, if has_errors { 422 } else { 200 }, "{case}: {data}");
        if has_errors {
            assert_eq!(data["errors"], case["errors"], "{case}");
        }
    }
    let (status, data, _) = send(
        address,
        "POST",
        "/strict?admin=1",
        r#"{"title":"Hello"}"#,
        "",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(data, json!({"title":"Hello"}));
    let key = "x".repeat(1100);
    let mut body = serde_json::Map::new();
    body.insert(key.clone(), json!(true));
    let (status, data, _) = send(
        address,
        "POST",
        "/strict",
        &Value::Object(body).to_string(),
        "",
    )
    .await;
    assert_eq!(status, 422);
    assert!(data["errors"].get(&key).is_some());
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn defaults_are_application_scoped_and_forms_can_explicitly_override_both_ways() {
    let (_a, one, server_one) = start(true).await;
    let (_b, two, server_two) = start(false).await;
    let body = r#"{"title":"Hello","admin":true}"#;
    for (address, path, status) in [
        (one, "/default", 422),
        (two, "/default", 200),
        (one, "/relaxed", 200),
        (two, "/strict", 422),
    ] {
        assert_eq!(send(address, "POST", path, body, "").await.0, status);
    }
    server_one.abort();
    server_two.abort();
    let _ = server_one.await;
    let _ = server_two.await;
}
#[tokio::test]
async fn unknown_field_checks_run_after_validation_and_before_precognition_success() {
    let (_root, address, server) = start(true).await;
    let headers = "Precognition: true\r\nPrecognition-Validate-Only: title\r\n";
    for path in ["/preflight/strict", "/preflight/default"] {
        let (status, _, response) =
            send(address, "POST", path, r#"{"title":"Hello"}"#, headers).await;
        assert_eq!(status, 204, "{response}");
        let (status, data, _) = send(
            address,
            "POST",
            path,
            r#"{"title":"Hello","admin":true}"#,
            headers,
        )
        .await;
        assert_eq!(status, 422);
        assert!(data["errors"]["admin"].is_array());
    }
    assert_eq!(
        send(
            address,
            "POST",
            "/preflight/relaxed",
            r#"{"admin":true}"#,
            headers
        )
        .await
        .0,
        204
    );
    server.abort();
    let _ = server.await;
}

fn errors(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move { Response::json(StatusCode::OK, request.session()?.get("errors")) })
}
#[tokio::test]
async fn form_body_upload_exclusion_and_web_error_bags_use_the_same_policy() {
    let (_root, address, server) = start(false).await;
    let (status, html, _) = send(address, "GET", "/error-page", "", "Accept: text/html\r\n").await;
    assert_eq!(status, 200);
    assert_eq!(html, "<span>clean</span>");
    let (status, data, _) = send(
        address,
        "POST",
        "/strict?not_a_body_field=1",
        "title=Hello&profile%5Bname%5D=Ada&unknown_field=1",
        "Content-Type: application/x-www-form-urlencoded\r\n",
    )
    .await;
    assert_eq!(status, 422);
    assert_eq!(
        data["errors"],
        json!({"unknown_field":["The unknown field field is prohibited."]})
    );
    let body = "--fields\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nHello\r\n--fields\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--fields--\r\n";
    let (status, data, _) = send(
        address,
        "POST",
        "/strict",
        body,
        "Content-Type: multipart/form-data; boundary=fields\r\n",
    )
    .await;
    assert_eq!(status, 200, "{data}");
    assert_eq!(data, json!({"title":"Hello"}));
    let (status, _, response) = send(
        address,
        "POST",
        "/web/strict",
        r#"{"title":"Hello","admin":true}"#,
        "Accept: text/html\r\nReferer: /errors\r\n",
    )
    .await;
    assert_eq!(status, 302, "{response}");
    let cookie = response
        .lines()
        .find_map(|line| line.strip_prefix("set-cookie: unknown_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let (status, data, _) = send(
        address,
        "GET",
        "/errors",
        "",
        &format!("Cookie: unknown_session={cookie}\r\n"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        data,
        json!({"article":{"admin":["The admin field is prohibited."]}})
    );
    let (_, _, response) = send(
        address,
        "POST",
        "/web/strict",
        r#"{"admin":true}"#,
        "Accept: text/html\r\nReferer: /error-page\r\n",
    )
    .await;
    let cookie = response
        .lines()
        .find_map(|line| line.strip_prefix("set-cookie: unknown_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let (status, html, _) = send(
        address,
        "GET",
        "/error-page",
        "",
        &format!("Cookie: unknown_session={cookie}\r\nAccept: text/html\r\n"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(html, "<span>The admin field is prohibited.</span>");
    server.abort();
    let _ = server.await;
}
