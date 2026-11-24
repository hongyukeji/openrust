use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    validation::{Data, Factory, RuleSet},
};
use serde_json::{Value, json};
#[tokio::test]
async fn sizes_and_message_types_follow_declared_rules_and_php_scalar_conversion() {
    let fixture: Value =
        serde_json::from_str(include_str!("reference/validation-sizes.json")).unwrap();
    let factory = Factory::default();
    let mut failures = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let mut validator = factory
            .make(
                Data::new(case["data"].clone()).unwrap(),
                RuleSet::parse([("value", case["rule"].as_str().unwrap())]).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let result = validator.validate().await;
        let actual = json!(validator.errors().messages());
        let expected = if case["errors"].is_array() {
            json!({})
        } else {
            case["errors"].clone()
        };
        if result.is_ok() != case["passes"].as_bool().unwrap() || actual != expected {
            failures.push(format!("{case}; actual={actual}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("reference/validation-sizes.json")).unwrap()
}
fn submit(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let index: usize = request.route("index").unwrap().parse().unwrap();
        let fixture = fixture();
        let count = fixture["cases"].as_array().unwrap().len();
        let case = if index < count {
            &fixture["cases"][index]
        } else {
            &fixture["file_cases"][index - count]
        };
        let mut validator = Factory::default().make(
            Data::from_request(request).await?,
            RuleSet::parse([("value", case["rule"].as_str().unwrap())])?,
            Default::default(),
            Default::default(),
        )?;
        openrust::http::Response::json(openrust::http::StatusCode::OK, validator.validate().await?)
    })
}
fn routes(
    _: &openrust::foundation::Application,
) -> Result<openrust::routing::Router, openrust::routing::RoutingError> {
    let mut router = openrust::routing::Router::new();
    router.post("/sizes/{index}", submit)?;
    Ok(router)
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<
        std::sync::Arc<openrust::foundation::http::Kernel>,
    >,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
#[tokio::test]
async fn size_rules_run_through_real_http_with_typed_scalar_and_array_inputs() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use openrust::foundation::{Application, configuration::Routing, http::Kernel};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(
        Application::configure(root.path().to_owned())
            .with_routing(Routing::new(routes))
            .with_middleware(|middleware| {
                middleware.skip_trim_strings_when(|_| true);
                middleware.convert_empty_strings_to_null(|_| true);
            })
            .create()
            .unwrap(),
    );
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = std::sync::Arc::new(kernel);
    let server = tokio::spawn(async move {
        axum::serve(listener, transport.with_state(kernel).into_make_service())
            .await
            .unwrap();
    });
    for (index, case) in fixture()["cases"].as_array().unwrap().iter().enumerate() {
        let body = case["data"].to_string();
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let wire = format!(
            "POST /sizes/{index} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(wire.as_bytes()).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        let success = case["passes"].as_bool().unwrap();
        assert!(
            response.starts_with(if success {
                "HTTP/1.1 200"
            } else {
                "HTTP/1.1 422"
            }),
            "{index}: {response}"
        );
        let actual: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        if !success {
            assert_eq!(actual["errors"], case["errors"], "{index}");
        } else {
            assert_eq!(actual, case["wire_validated"], "{index}");
        }
    }
    for (index, case) in fixture()["file_cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let index = index + fixture()["cases"].as_array().unwrap().len();
        let body = format!(
            "--probe\r\nContent-Disposition: form-data; name=\"value\"; filename=\"photo.txt\"\r\nContent-Type: text/plain\r\n\r\n{}\r\n--probe--\r\n",
            "a".repeat(1024)
        );
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let wire = format!(
            "POST /sizes/{index} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: multipart/form-data; boundary=probe\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(wire.as_bytes()).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 422"), "{response}");
        let actual: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(actual["errors"], case["errors"], "file {case}");
    }
    server.abort();
    let _ = server.await;
}
