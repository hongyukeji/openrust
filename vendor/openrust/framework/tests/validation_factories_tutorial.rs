use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        http::Kernel as _,
        validation::{Factory as _, ParameterizedRule, Validator as _},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::Router,
    validation::{Data, Factory, Rule, RuleBuilder, RuleSet, rules::*},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
fn specification(name: &str) -> Rule {
    match name {
        "accepted" => Rule::accepted(),
        "declined" => Rule::declined(),
        "exclude" => Rule::exclude(),
        "filled" => Rule::filled(),
        "missing" => Rule::missing(),
        "prohibited" => Rule::prohibited(),
        "hex_color" => Rule::hex_color(),
        "json" => Rule::json(),
        "list" => Rule::list(),
        "mac_address" => Rule::mac_address(),
        "ulid" => Rule::ulid(),
        "accepted_if" => Rule::accepted_if("account_type", ["personal", "business"]).into(),
        "declined_if" => Rule::declined_if("account_type", ["personal", "business"]).into(),
        "missing_if" => Rule::missing_if("other", true).into(),
        "missing_unless" => Rule::missing_unless("other", false).into(),
        "present_if" => Rule::present_if("other", Value::Null).into(),
        "present_unless" => Rule::present_unless("other", "A").into(),
        "exclude_with" => Rule::exclude_with("other").into(),
        "exclude_without" => Rule::exclude_without("other").into(),
        "prohibited_if_accepted" => Rule::prohibited_if_accepted("other").into(),
        "prohibited_if_declined" => Rule::prohibited_if_declined("other").into(),
        "required_if_accepted" => Rule::required_if_accepted("other").into(),
        "required_if_declined" => Rule::required_if_declined("other").into(),
        "in_array" => Rule::in_array("other.*").into(),
        "in_array_keys" => Rule::in_array_keys(["first", "second"]).into(),
        "missing_with" => Rule::missing_with(["first", "second"]).into(),
        "missing_with_all" => Rule::missing_with_all(["first", "second"]).into(),
        "present_with" => Rule::present_with(["first", "second"]).into(),
        "present_with_all" => Rule::present_with_all(["first", "second"]).into(),
        "prohibits" => Rule::prohibits(["first", "second"]).into(),
        "required_array_keys" => Rule::required_array_keys(["first", "second"]).into(),
        "required_with" => Rule::required_with(["first", "second"]).into(),
        "required_with_all" => Rule::required_with_all(["first", "second"]).into(),
        "required_without" => Rule::required_without(["first", "second"]).into(),
        "required_without_all" => Rule::required_without_all(["first", "second"]).into(),
        "array_keys" => Rule::array_keys(["first", "second"]).into(),
        "regex" => Rule::regex(r"/^(a|b),c$/").into(),
        "not_regex" => Rule::not_regex("/^test-/i").into(),
        "contains" => Rule::contains(["a,b", "quoted\"value"]).into(),
        "doesnt_contain" => Rule::doesnt_contain(["a,b", "quoted\"value"]).into(),
        "confirmed" => Rule::confirmed().custom_field("repeat_value"),
        _ => panic!("未声明构造器{name}"),
    }
}
fn submit(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let name = request.header("x-spec")?.unwrap().to_owned();
        let rules = RuleSet::new().field("value", [specification(&name)])?;
        Response::json(StatusCode::OK, request.validate(rules).await?)
    })
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
async fn send(address: SocketAddr, path: &str, body: &str, content: &str) -> (u16, Value) {
    timeout(Duration::from_secs(5),async{
 let mut stream=TcpStream::connect(address).await.unwrap();stream.write_all(format!("POST {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: {content}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();let mut response=String::new();stream.read_to_string(&mut response).await.unwrap();let status=response.split_whitespace().nth(1).unwrap().parse().unwrap();let body=response.split_once("\r\n\r\n").unwrap().1;(status,serde_json::from_str(body).unwrap())
}).await.unwrap()
}
#[tokio::test]
async fn all_named_factories_run_the_original_rules_over_real_http() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().to_owned())
        .with_config(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                json!({"locale":"en","supported_locales":[]}),
            )]))
        })
        .with_routing(Routing::new(|_| {
            let mut r = Router::new();
            r.post("/submit", submit)?;
            r.post("/upload", upload)?;
            Ok(r)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    let fixture: Value =
        serde_json::from_str(include_str!("reference/validation-factories.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let (status, data) = send(
            address,
            "/submit",
            &case["input"].to_string(),
            &format!("application/json\r\nX-Spec: {name}"),
        )
        .await;
        assert_eq!(
            status,
            if case["passes"] == true { 200 } else { 422 },
            "{case}: {data}"
        );
        if status == 200 {
            let mut expected = case["validated"].clone();
            if expected == json!([]) {
                expected = json!({});
            }
            if expected.get("value") == Some(&json!("")) {
                expected["value"] = Value::Null;
            }
            assert_eq!(data, expected, "{case}");
        } else {
            assert_eq!(
                data["errors"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .collect::<Vec<_>>(),
                case["errors"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .collect::<Vec<_>>(),
                "{case}"
            );
        }
    }
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"2\" height=\"2\"></svg>";
    for (spec, status) in [("file", 200), ("image", 422), ("svg", 200)] {
        let body = format!(
            "--form\r\nContent-Disposition: form-data; name=\"value\"; filename=\"input.svg\"\r\nContent-Type: image/svg+xml\r\n\r\n{svg}\r\n--form--\r\n"
        );
        let (actual, data) = send(
            address,
            "/upload",
            &body,
            &format!("multipart/form-data; boundary=form\r\nX-Spec: {spec}"),
        )
        .await;
        assert_eq!(actual, status, "{data}");
    }
    server.abort();
    let _ = server.await;
}
fn upload(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let spec = request.header("x-spec")?.unwrap_or("file").to_owned();
        let rule = match spec.as_str() {
            "image" => Rule::image_file(false).max(2).into(),
            "svg" => ImageFile::new(true)
                .dimensions(Rule::dimensions().max_width(20))
                .extensions(["svg"])
                .encoding("UTF-8")
                .min(0)
                .max(2)
                .between(0, 2)
                .into(),
            _ => Rule::file().into(),
        };
        let input = request
            .validate(RuleSet::new().field("value", [rule])?)
            .await?;
        assert!(input.file("value").is_some());
        Response::json(StatusCode::OK, input)
    })
}
fn parameterized<P: ParameterizedRule + ToString>(rule: P) -> (Value, String) {
    (json!(rule.to_array()), rule.to_string())
}
#[tokio::test]
async fn typed_factories_preserve_parameter_values_and_original_declarations() {
    let fixture: Value =
        serde_json::from_str(include_str!("reference/validation-factories.json")).unwrap();
    for case in fixture["parameters"].as_array().unwrap() {
        let value = case["value"].clone();
        let (params, declaration) = match case["method"].as_str().unwrap() {
            "acceptedIf" => parameterized(AcceptedIf::new("other", value)),
            "declinedIf" => parameterized(DeclinedIf::new("other", value)),
            "missingIf" => parameterized(MissingIf::new("other", value)),
            "missingUnless" => parameterized(MissingUnless::new("other", value)),
            "presentIf" => parameterized(PresentIf::new("other", value)),
            "presentUnless" => parameterized(PresentUnless::new("other", value)),
            _ => unreachable!(),
        };
        assert_eq!(params, case["parameters"], "{case}");
        assert_eq!(declaration, case["declaration"], "{case}");
    }
    let factory = Factory::default();
    for case in fixture["cases"].as_array().unwrap() {
        let mut v = factory
            .make(
                Data::new(case["input"].clone()).unwrap(),
                RuleSet::new()
                    .field("value", [specification(case["name"].as_str().unwrap())])
                    .unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let result = v.validate().await;
        assert_eq!(result.is_ok(), case["passes"].as_bool().unwrap(), "{case}");
        if let Ok(data) = result {
            let expected = if case["validated"] == json!([]) {
                json!({})
            } else {
                case["validated"].clone()
            };
            assert_eq!(data.all(), &expected, "{case}");
        }
    }
    assert_eq!(
        Confirmed::new().custom_field("repeat_value").to_array(),
        vec!["confirmed", "repeat_value"]
    );
    assert_eq!(
        CurrentPassword::new().guard("web").to_array(),
        vec!["current_password", "web"]
    );
    let mut v = factory
        .make(
            Data::new(json!({"value":"https://example.com"})).unwrap(),
            RuleSet::new().field("value", [Rule::active_url()]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    v.fake_dns_lookups(true);
    assert!(v.validate().await.is_ok());
}
