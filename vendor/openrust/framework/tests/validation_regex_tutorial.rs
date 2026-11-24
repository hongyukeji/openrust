use axum::handler::Handler as _;
use openrust::{
    contracts::{
        http::Kernel as _,
        validation::{Factory as _, Validator as _},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Route,
    validation::{Data, Factory, Rule, RuleSet},
};
use serde_json::Value;
use std::{
    net::SocketAddr,
    sync::{Arc, OnceLock},
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
fn cases() -> &'static Vec<Value> {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("reference/validation-regex-modifiers.json")).unwrap()
    })["cases"]
        .as_array()
        .unwrap()
}
fn rules(case: &Value) -> openrust::validation::ValidationResult<RuleSet> {
    RuleSet::new().field(
        "value",
        [Rule::parse(&format!(
            "{}:{}",
            case["kind"].as_str().unwrap(),
            case["pattern"].as_str().unwrap()
        ))?],
    )
}
fn check(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let index = request.route("index").unwrap().parse::<usize>().unwrap();
        let input = request.validate(rules(&cases()[index])?).await?;
        Response::json(StatusCode::OK, input)
    })
}
#[tokio::test]
async fn regex_modifiers_match_fixed_php_and_reject_unknown_flags() {
    assert_eq!(cases().len(), 480);
    for case in cases() {
        let mut validator = Factory::default()
            .make(
                Data::new(serde_json::json!({"value":case["value"]})).unwrap(),
                rules(case).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            validator.passes().await.unwrap(),
            case["passes"].as_bool().unwrap(),
            "{case}"
        );
        let errors = serde_json::to_value(validator.errors()).unwrap();
        if !case["passes"].as_bool().unwrap() {
            assert_eq!(errors, case["errors"], "{case}");
        }
    }
    for pattern in ["/a/e", "/a/z", "/a/\ti", "/a/\ri", "/a/\0i"] {
        assert!(
            Rule::parse(&format!("regex:{pattern}")).is_err(),
            "{pattern:?}"
        );
    }
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
#[tokio::test]
async fn regex_and_not_regex_modifiers_execute_through_real_http() {
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(
        Application::configure(root.path().into())
            .with_routing(Routing::default().api(|| {
                Route::post("/regex/{index}", check)?;
                Ok(())
            }))
            .create()
            .unwrap(),
    );
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address: SocketAddr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    for (index, case) in cases().iter().enumerate() {
        let body = serde_json::json!({"value":case["value"]}).to_string();
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        socket.write_all(format!("POST /api/regex/{index} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        let expected = if case["wire_passes"].as_bool().unwrap() {
            200
        } else {
            422
        };
        assert!(
            response.starts_with(&format!("HTTP/1.1 {expected}")),
            "{case}: {response}"
        );
        if expected == 422 {
            let data: Value =
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(data["errors"], case["wire_errors"], "{case}");
        }
    }
    server.abort();
    let _ = server.await;
}
