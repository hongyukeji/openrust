use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::Router,
    support::BackedEnum,
    validation::{
        Rule, RuleSet,
        rules::{Enum, In, NotIn},
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
#[derive(PartialEq)]
enum StateCode {
    Draft,
    Ready,
}
impl BackedEnum for StateCode {
    type Backing = String;
    fn try_from_backing(value: String) -> Option<Self> {
        match value.as_str() {
            "draft" => Some(Self::Draft),
            "ready" => Some(Self::Ready),
            _ => None,
        }
    }
}
#[derive(PartialEq)]
enum Level {
    Zero,
    One,
    Two,
}
impl BackedEnum for Level {
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
fn string_rule(mode: &str) -> Enum<StateCode> {
    let rule = Rule::r#enum::<StateCode>();
    match mode {
        "only" => rule.only([StateCode::Draft]),
        "except" => rule.except([StateCode::Draft]),
        "precedence" => rule.only([StateCode::Draft]).except([StateCode::Draft]),
        "when" => rule.when_else(
            true,
            |r| r.only([StateCode::Ready]),
            |r| r.except([StateCode::Ready]),
        ),
        _ => rule,
    }
}
fn int_rule(mode: &str) -> Enum<Level> {
    let rule = Enum::new();
    match mode {
        "only" => rule.only([Level::One]),
        "except" => rule.except([Level::One]),
        "precedence" => rule.only([Level::One]).except([Level::One]),
        "when" => rule.when_else(true, |r| r.only([Level::Two]), |r| r.except([Level::Two])),
        _ => rule,
    }
}
fn check(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data: Value = serde_json::from_slice(&request.body_bytes().await?)?;
        let mode = data["mode"].as_str().unwrap_or("all");
        let rule = if data["kind"] == "string" {
            string_rule(mode).into()
        } else {
            int_rule(mode).into()
        };
        let rules = RuleSet::new().field("value", [Rule::required(), rule])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::OK, data)
    })
}
fn choices(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new()
            .field(
                "zone",
                [
                    Rule::required(),
                    Rule::r#in(["a,b", "quoted\"value", "ready"]).into(),
                ],
            )?
            .field("topping", [NotIn::new(["cherries", "sprinkles"]).into()])?
            .field("airports", [Rule::array(), In::new(["NYC", "LIT"]).into()])?;
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
async fn enum_and_list_rules_match_fixed_cases_over_real_http() {
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
            r.post("/check", check)?;
            r.post("/choices", choices)?;
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
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("reference/validation-enum.json")).unwrap();
    for case in cases {
        let value = &case["value"];
        let mode = case["mode"].as_str().unwrap();
        let passes = if case["kind"] == "string" {
            string_rule(mode).passes(value)
        } else {
            int_rule(mode).passes(value)
        };
        assert_eq!(passes, case["passes"], "{case}");
        let (status, data) = send(address, "/check", &case.to_string(), "application/json").await;
        assert_eq!(status, if passes { 200 } else { 422 }, "{case}: {data}");
        if passes {
            assert_eq!(data, json!({"value":value}));
        } else {
            assert!(data["errors"]["value"].is_array());
        }
    }
    for (zone, expected) in [
        ("a,b", 200),
        ("quoted\"value", 200),
        ("ready", 200),
        ("other", 422),
    ] {
        let (status, data) = send(
            address,
            "/choices",
            &json!({"zone":zone,"airports":["NYC","LIT"],"topping":"cream"}).to_string(),
            "application/json",
        )
        .await;
        assert_eq!(status, expected, "{data}");
    }
    for body in [
        json!({"zone":"ready","topping":"cherries"}),
        json!({"zone":"ready","airports":["NYC","BAD"]}),
    ] {
        assert_eq!(
            send(address, "/choices", &body.to_string(), "application/json")
                .await
                .0,
            422
        );
    }
    assert!(
        Enum::<StateCode>::new()
            .unless(false, |r| r.only([StateCode::Ready]))
            .passes_case(&StateCode::Ready)
    );
    assert!(
        !Enum::<StateCode>::new()
            .only([StateCode::Draft])
            .passes_case(&StateCode::Ready)
    );
    server.abort();
    let _ = server.await;
}
