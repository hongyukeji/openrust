use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    validation::{Data, Factory, RuleSet},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[tokio::test]
async fn numeric_indexes_positions_and_input_match_fixed_reference() {
    let fixture: Value =
        serde_json::from_str(include_str!("reference/validation-message-positions.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let attribute = case["attribute"].as_str().unwrap();
        let template = case["message"].as_str().unwrap();
        let messages = ["required", "string", "min"]
            .into_iter()
            .map(|rule| (format!("{attribute}.{rule}"), template.to_owned()))
            .collect();
        let mut validator = Factory::default()
            .make(
                Data::new(case["data"].clone()).unwrap(),
                RuleSet::parse([(attribute, "required|string|min:3")]).unwrap(),
                messages,
                Default::default(),
            )
            .unwrap();
        validator.set_value_names(BTreeMap::from([(
            "rows.01.value".into(),
            BTreeMap::from([("x".into(), "alias".into())]),
        )]));
        assert_eq!(
            validator.validate().await.is_ok(),
            case["passes"].as_bool().unwrap()
        );
        assert_eq!(
            json!(validator.errors().messages()),
            case["errors"],
            "{attribute}"
        );
    }
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("reference/validation-message-positions.json")).unwrap()
}
fn submit(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let index: usize = request.route("index").unwrap().parse().unwrap();
        let fixture = fixture();
        let case = &fixture["cases"][index];
        let attribute = case["attribute"].as_str().unwrap();
        let template = case["message"].as_str().unwrap();
        let messages = ["required", "string", "min"]
            .into_iter()
            .map(|rule| (format!("{attribute}.{rule}"), template.to_owned()))
            .collect();
        let mut validator = Factory::default().make(
            Data::from_request(request).await?,
            RuleSet::parse([(attribute, "required|string|min:3")])?,
            messages,
            Default::default(),
        )?;
        validator.set_value_names(BTreeMap::from([(
            "rows.01.value".into(),
            BTreeMap::from([("x".into(), "alias".into())]),
        )]));
        let result = validator.validate().await?;
        openrust::http::Response::json(openrust::http::StatusCode::OK, result)
    })
}
fn routes(
    _: &openrust::foundation::Application,
) -> Result<openrust::routing::Router, openrust::routing::RoutingError> {
    let mut router = openrust::routing::Router::new();
    router.post("/positions/{index}", submit)?;
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
async fn send(address: std::net::SocketAddr, index: usize, data: &Value) -> Value {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    tokio::time::timeout(std::time::Duration::from_secs(10),async{
 let body=data.to_string();let mut socket=tokio::net::TcpStream::connect(address).await.unwrap();
 let wire=format!("POST /positions/{index} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());socket.write_all(wire.as_bytes()).await.unwrap();let mut response=String::new();socket.read_to_string(&mut response).await.unwrap();
 assert!(response.starts_with("HTTP/1.1 422"),"{response}");serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
 }).await.unwrap()
}
#[tokio::test]
async fn message_placeholders_run_over_real_http_with_default_and_explicit_input_policy() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use openrust::foundation::{Application, configuration::Routing, http::Kernel};
    let fixture = fixture();
    for preserve_empty in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let mut builder =
            Application::configure(root.path().to_owned()).with_routing(Routing::new(routes));
        if preserve_empty {
            builder = builder.with_middleware(|middleware| {
                middleware.convert_empty_strings_to_null(|_| true);
            });
        }
        let mut kernel = Kernel::new(builder.create().unwrap());
        kernel.bootstrap().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let kernel = std::sync::Arc::new(kernel);
        let server = tokio::spawn(async move {
            axum::serve(listener, transport.with_state(kernel).into_make_service())
                .await
                .unwrap();
        });
        if preserve_empty {
            for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
                let actual = send(address, index, &case["data"]).await;
                assert_eq!(actual["errors"], case["errors"], "{index}");
            }
        } else {
            let actual = send(address, 0, &fixture["cases"][0]["data"]).await;
            assert_eq!(actual["errors"], fixture["cases"][6]["errors"]);
        }
        server.abort();
        let _ = server.await;
    }
}
