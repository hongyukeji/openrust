use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{http::Kernel as _, validation::ValidationRule},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::Router,
    support::facades::Validator,
    validation::{
        Data, Rule, RuleContext, RuleFailure, RuleSet, ValidationFuture, ValidationResult,
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
struct Presence<const REJECT: bool>;
impl<const REJECT: bool> ValidationRule for Presence<REJECT> {
    fn is_implicit(&self) -> bool {
        true
    }
    fn validate<'a>(
        &'a self,
        _context: &'a RuleContext<'_>,
        attribute: &'a str,
        value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            Ok(
                if REJECT
                    && (value.is_none_or(Value::is_null)
                        || value
                            .and_then(Value::as_str)
                            .is_some_and(|v| v.trim().is_empty()))
                {
                    vec![RuleFailure::new(format!(
                        "The {attribute} is required by this rule."
                    ))]
                } else {
                    Vec::new()
                },
            )
        })
    }
}
fn validate(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let path = request.uri().path().to_owned();
        // 调度对照使用原始 JSON，与固定 Validator 输入一致，不混入 HTTP 空串转 null。
        let input: Value = serde_json::from_slice(&request.body_bytes().await?)?;
        let rules = if path.contains("implicit") {
            let mut rules = vec![Rule::custom(Presence::<true>)];
            if path == "/implicit-chain" {
                rules.push(Rule::custom(
                    |_: &RuleContext<'_>, _: &str, _: Option<&Value>| {
                        Ok(vec![RuleFailure::new("second")])
                    },
                ));
            }
            if path.contains("nullable") {
                rules.insert(0, Rule::nullable());
            }
            if path.contains("sometimes") {
                rules.insert(0, Rule::sometimes());
            }
            RuleSet::new().field("title", rules)?
        } else if path == "/allow" {
            RuleSet::new().field("title", [Rule::custom(Presence::<false>)])?
        } else if path == "/bail" {
            RuleSet::new().field(
                "title",
                [
                    Rule::bail(),
                    Rule::custom(|_: &RuleContext<'_>, _: &str, _: Option<&Value>| {
                        Ok(vec![RuleFailure::new("first")])
                    }),
                    Rule::custom(|_: &RuleContext<'_>, _: &str, _: Option<&Value>| {
                        panic!("bail后不得继续规则")
                    }),
                ],
            )?
        } else {
            let probe = path == "/probe" || path == "/nullable";
            let field = if path == "/literal" {
                r"author\.name"
            } else {
                "title"
            };
            let callback = Rule::custom(
                move |context: &RuleContext<'_>, attribute: &str, value: Option<&Value>| {
                    if value == Some(&json!("explode")) {
                        return Err(openrust::validation::ValidationError::Execution {
                            source: std::io::Error::other("rule error").into(),
                        });
                    }
                    Ok(
                        if probe
                            || value == Some(&json!("foo"))
                            || context.data.as_value().get("role") == Some(&json!("blocked"))
                        {
                            vec![RuleFailure::new(format!("The {attribute} is invalid."))]
                        } else {
                            Vec::new()
                        },
                    )
                },
            );
            let mut rules = vec![callback];
            if path == "/nullable" {
                rules.insert(0, Rule::nullable());
            }
            RuleSet::new().field(field, rules)?
        };
        let mut validator = Validator::make(Data::new(input)?, rules)?;
        let data = Box::pin(validator.validate_request(request)).await?;
        Response::json(StatusCode::OK, data.into_value())
    })
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
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
    timeout(Duration::from_secs(5),async {
        let mut stream=TcpStream::connect(address).await.unwrap();stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        let mut response=String::new();stream.read_to_string(&mut response).await.unwrap();let status=response.split_whitespace().nth(1).unwrap().parse().unwrap();let data=response.split_once("\r\n\r\n").unwrap().1;
        let value=if data.is_empty(){Value::Null}else{serde_json::from_str(data).unwrap()};(status,value,response)
    }).await.unwrap()
}

async fn start() -> (tempfile::TempDir, SocketAddr, tokio::task::JoinHandle<()>) {
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
            for path in [
                "/callback",
                "/probe",
                "/nullable",
                "/implicit",
                "/implicit-chain",
                "/implicit-nullable",
                "/implicit-sometimes",
                "/allow",
                "/literal",
                "/bail",
            ] {
                r.post(path, validate)?;
            }
            Ok(r)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    (root, address, server)
}
#[tokio::test]
async fn callback_rules_use_real_attribute_and_data_without_application_registration() {
    let (_root, address, server) = start().await;
    for (path, body, field, message) in [
        (
            "/callback",
            json!({"title":"foo"}),
            "title",
            "The title is invalid.",
        ),
        (
            "/literal",
            json!({"author.name":"foo"}),
            "author.name",
            "The author.name is invalid.",
        ),
        (
            "/callback",
            json!({"title":"ok","role":"blocked"}),
            "title",
            "The title is invalid.",
        ),
        ("/bail", json!({"title":"foo"}), "title", "first"),
    ] {
        let (status, value, _) = send(address, "POST", path, &body.to_string(), "").await;
        assert_eq!(status, 422, "{value}");
        assert_eq!(value["errors"][field][0], message);
    }
    assert_eq!(
        send(address, "POST", "/callback", r#"{"title":"explode"}"#, "")
            .await
            .0,
        500
    );
    let (status, value, _) = send(
        address,
        "POST",
        "/callback",
        r#"{"title":"ok","admin":true}"#,
        "",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(value, json!({"title":"ok"}));
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn implicit_rule_objects_distinguish_skipping_from_requiredness() {
    let (_root, address, server) = start().await;
    let cases: Value =
        serde_json::from_str(include_str!("reference/validation-rule-scheduling.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let path = match (
            case["kind"].as_str().unwrap(),
            case["nullable"].as_bool().unwrap(),
            case["sometimes"].as_bool().unwrap(),
        ) {
            ("ordinary", true, _) => "/nullable",
            ("ordinary", false, _) => "/probe",
            ("allow", _, _) => "/allow",
            (_, _, true) => "/implicit-sometimes",
            (_, true, _) => "/implicit-nullable",
            _ => "/implicit",
        };
        let (status, data, _) = send(address, "POST", path, &case["input"].to_string(), "").await;
        assert_eq!(
            status,
            if case["passes"].as_bool().unwrap() {
                200
            } else {
                422
            },
            "{case}: {data}"
        );
        if status == 422 {
            assert_eq!(data["errors"], case["errors"]);
        }
    }
    for body in ["{}", r#"{"title":""}"#, r#"{"title":"  "}"#] {
        assert_eq!(send(address, "POST", "/probe", body, "").await.0, 200);
    }
    assert_eq!(
        send(address, "POST", "/probe", r#"{"title":null}"#, "")
            .await
            .0,
        422
    );
    assert_eq!(
        send(address, "POST", "/nullable", r#"{"title":null}"#, "")
            .await
            .0,
        200
    );
    for path in ["/implicit", "/implicit-nullable"] {
        for body in ["{}", r#"{"title":null}"#, r#"{"title":""}"#] {
            assert_eq!(send(address, "POST", path, body, "").await.0, 422);
        }
    }
    assert_eq!(
        send(address, "POST", "/implicit-sometimes", "{}", "")
            .await
            .0,
        200
    );
    assert_eq!(send(address, "POST", "/allow", "{}", "").await.0, 200);
    let (status, value, _) =
        send(address, "POST", "/implicit-chain", r#"{"title":null}"#, "").await;
    assert_eq!(status, 422);
    assert_eq!(
        value["errors"]["title"],
        json!(["The title is required by this rule.", "second"])
    );
    server.abort();
    let _ = server.await;
}
