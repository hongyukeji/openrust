use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{http::Kernel as _, session::Session as _},
    foundation::{
        Application,
        configuration::Routing,
        http::{FormRequest, Kernel},
    },
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
    routing::Router,
    support::facades::Validator,
    validation::{Data, RuleSet, ValidationResult},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
fn messages() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("title.required".into(), "Please provide :attribute.".into()),
        (
            "title.min".into(),
            ":attribute needs :min characters.".into(),
        ),
    ])
}
fn attributes() -> BTreeMap<String, String> {
    BTreeMap::from([("title".into(), "article title".into())])
}
fn rules() -> ValidationResult<RuleSet> {
    RuleSet::parse([("title", "required|string|min:3")])
}
#[derive(Default)]
struct Article;
impl FormRequest for Article {
    fn rules(&self) -> ValidationResult<RuleSet> {
        rules()
    }
    fn messages(&self) -> BTreeMap<String, String> {
        messages()
    }
    fn attributes(&self) -> BTreeMap<String, String> {
        attributes()
    }
    fn error_bag(&self) -> &str {
        "article"
    }
}
fn validate(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let route = request.uri().path().rsplit('/').next().unwrap().to_owned();
        let data = match route.as_str() {
            "form" => Article.validate_resolved(request).await?,
            "facade" => {
                request
                    .with_container(async {
                        openrust::support::facades::Request::validate_with(
                            rules()?,
                            messages(),
                            attributes(),
                        )
                        .await
                    })
                    .await?
            }
            "facade-named" => {
                request
                    .with_container(async {
                        openrust::support::facades::Request::validate_with_bag_and_options(
                            "article",
                            rules()?,
                            messages(),
                            attributes(),
                        )
                        .await
                    })
                    .await?
            }
            "named" => {
                request
                    .validate_with_bag_and_options("article", rules()?, messages(), attributes())
                    .await?
            }
            "manual" => {
                let data = Data::from_request(request).await?;
                let mut validator = Validator::make_with(data, rules()?, messages(), attributes())?;
                Box::pin(validator.validate_request(request)).await?
            }
            _ => {
                request
                    .validate_with(rules()?, messages(), attributes())
                    .await?
            }
        };
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}

async fn start() -> (tempfile::TempDir, SocketAddr, tokio::task::JoinHandle<()>) {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().to_owned())
        .with_config(|_, _| {
            Ok(BTreeMap::from([
                (
                    "app".into(),
                    json!({"locale":"en","supported_locales":[],"key":"a".repeat(32)}),
                ),
                (
                    "session".into(),
                    json!({"driver":"array","cookie":"options_session","lottery":[0,100]}),
                ),
            ]))
        })
        .with_middleware(|m| {
            m.prevent_request_forgery(["web/*"], false, false);
        })
        .with_routing(Routing::new(|_| {
            let mut r = Router::new();
            for path in [
                "/instance",
                "/facade",
                "/facade-named",
                "/named",
                "/manual",
                "/form",
            ] {
                r.post(path, validate)?;
            }
            for path in [
                "/preflight/instance",
                "/preflight/facade",
                "/preflight/facade-named",
                "/preflight/named",
                "/preflight/form",
            ] {
                r.post(path, || Response::empty(StatusCode::OK))?
                    .middleware_name("precognitive")
                    .middleware(Arc::new(ValidateBeforeDispatch));
            }
            for path in ["/web/named", "/web/facade-named", "/web/form"] {
                r.post(path, validate)?.middleware_name("web");
            }
            r.get("/errors", errors)?.middleware_name("web");
            Ok(r)
        }))
        .create()
        .unwrap();
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
    timeout(Duration::from_secs(5),async {
        let mut stream=TcpStream::connect(address).await.unwrap();stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\n{accept}Content-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        let mut response=String::new();stream.read_to_string(&mut response).await.unwrap();let status=response.split_whitespace().nth(1).unwrap().parse().unwrap();let data=response.split_once("\r\n\r\n").unwrap().1;
        let value=if data.is_empty(){Value::Null}else{serde_json::from_str(data).unwrap()};(status,value,response)
    }).await.unwrap()
}

#[tokio::test]
async fn custom_messages_and_attributes_work_through_all_public_validation_entries() {
    let (_root, address, server) = start().await;
    for path in [
        "/instance",
        "/facade",
        "/facade-named",
        "/named",
        "/manual",
        "/form",
    ] {
        for (body, message) in [
            ("{}", "Please provide article title."),
            (r#"{"title":"x"}"#, "article title needs 3 characters."),
        ] {
            let (status, data, _) = send(address, "POST", path, body, "").await;
            assert_eq!(status, 422, "{path}: {data}");
            assert_eq!(data["errors"]["title"][0], message, "{path}");
        }
        let (status, data, _) = send(
            address,
            "POST",
            path,
            r#"{"title":"Hello","admin":true}"#,
            "",
        )
        .await;
        assert_eq!(status, 201, "{path}: {data}");
        assert_eq!(data, json!({"title":"Hello"}));
    }
    server.abort();
    let _ = server.await;
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
#[tokio::test]
async fn customized_validation_preserves_precognition_response_and_messages() {
    let (_root, address, server) = start().await;
    for path in [
        "/preflight/instance",
        "/preflight/facade",
        "/preflight/facade-named",
        "/preflight/named",
        "/preflight/form",
    ] {
        let headers = "Precognition: true\r\nPrecognition-Validate-Only: title\r\n";
        let (status, _, response) =
            send(address, "POST", path, r#"{"title":"Hello"}"#, headers).await;
        assert_eq!(status, 204, "{path}: {response}");
        assert!(response.contains("precognition-success: true"));
        let (status, data, _) = send(address, "POST", path, r#"{"title":"x"}"#, headers).await;
        assert_eq!(status, 422);
        assert_eq!(
            data["errors"]["title"][0],
            "article title needs 3 characters."
        );
    }
    server.abort();
    let _ = server.await;
}

fn errors(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move { Response::json(StatusCode::OK, request.session()?.get("errors")) })
}
#[tokio::test]
async fn customized_named_bags_survive_web_redirect_and_session_flash() {
    let (_root, address, server) = start().await;
    for path in ["/web/named", "/web/facade-named", "/web/form"] {
        let (status, _, response) = send(
            address,
            "POST",
            path,
            "{}",
            "Accept: text/html\r\nReferer: /errors\r\n",
        )
        .await;
        assert_eq!(status, 302, "{path}: {response}");
        let cookie = response
            .lines()
            .find_map(|line| line.strip_prefix("set-cookie: options_session="))
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let (status, value, _) = send(
            address,
            "GET",
            "/errors",
            "",
            &format!("Cookie: options_session={cookie}\r\n"),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(
            value,
            json!({"article":{"title":["Please provide article title."]}})
        );
    }
    server.abort();
    let _ = server.await;
}
