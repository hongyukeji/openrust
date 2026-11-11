use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::access::{AuthorizationError, Response as Authorization},
    contracts::{http::Kernel as _, validation::Validator as _},
    foundation::{
        Application,
        configuration::Routing,
        http::{FormRequest, Kernel},
    },
    http::{HttpError, HttpFuture, Method, Request, Response, StatusCode},
    routing::{Router, RoutingError},
    support::facades::{Lang, Validator},
    validation::{Data, RuleSet, ValidatedInput, ValidationFuture, ValidationResult},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct Submit;

impl FormRequest for Submit {
    fn prepareForValidation(&mut self, data: &mut Data) -> ValidationResult<()> {
        let mut input = data.as_value().clone();
        if let Some(name) = input.get("name").and_then(Value::as_str) {
            input["name"] = name.trim().into();
        }
        data.replace(input)
    }

    fn authorize<'a>(
        &'a self,
        request: &'a mut Request,
        _: &'a Data,
    ) -> ValidationFuture<'a, Result<(), AuthorizationError>> {
        Box::pin(async move {
            if request.headers().contains_key("x-denied") {
                return Err(AuthorizationError::Denied(Authorization::deny(
                    "Submission is forbidden.",
                )));
            }
            Ok(())
        })
    }

    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([
            ("name", "required|string|min:2"),
            ("users", "required|array"),
            ("users.*.email", "required|string"),
            ("users.*.score", "nullable|integer|min:0"),
            ("description", "sometimes|nullable|string"),
        ])
    }

    fn passedValidation(
        &mut self,
        request: &mut Request,
        _: &ValidatedInput,
    ) -> ValidationResult<()> {
        request.parts_mut().extensions.insert("passed");
        Ok(())
    }
}

async fn submit(request: &mut Request) -> Result<Response, HttpError> {
    let input = Submit.validateResolved(request).await?;
    Response::json(
        StatusCode::OK,
        json!({
            "input":input.into_value(),
            "hook":request.parts().extensions.get::<&'static str>()
        }),
    )
}

fn form(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let locale = request.route("locale").unwrap().to_owned();
        Lang::withLocale(&locale, submit(request))
            .await
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?
    })
}

fn manual(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = Data::new(request.input(None).await?.unwrap())?;
        let rules = RuleSet::parse([("name", "required|string")])?;
        let input = Validator::make(data, rules)?.validate().await?;
        Response::json(StatusCode::OK, input.into_value())
    })
}

fn register(router: &mut Router) -> Result<(), RoutingError> {
    router.r#match(&[Method::POST], "/submit/{locale}", form)?;
    router.r#match(&[Method::POST], "/manual", manual)?;
    Ok(())
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(incoming).await
}

async fn start(root: PathBuf) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(root)
        .withRouting(Routing::default().api(register))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    (address, server)
}

async fn send(address: SocketAddr, path: &str, body: Value, denied: bool, status: u16) -> Value {
    timeout(Duration::from_secs(5), async {
        let mut socket = TcpStream::connect(address).await.unwrap();
        let body = body.to_string();
        let denied = if denied { "X-Denied: yes\r\n" } else { "" };
        let wire = format!("POST {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{denied}Connection: close\r\n\r\n{body}",body.len());
        socket.write_all(wire.as_bytes()).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {status} ")), "{response}");
        let (headers,body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.contains("content-type: application/json"));
        serde_json::from_str(body).unwrap()
    }).await.unwrap()
}

#[tokio::test]
async fn form_requests_validate_and_translate_concurrent_real_http_requests() {
    let root = std::env::temp_dir().join(format!("openrust-validation-{:016x}", fastrand::u64(..)));
    std::fs::create_dir_all(root.join("lang/zh_Hans")).unwrap();
    std::fs::write(
        root.join("lang/zh_Hans/validation.json"),
        r#"{"required":"请填写 :attribute。","attributes":{"name":"名称"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("lang/zh_Hans.json"),
        r#"{"(and :count more error)":"（另有 :count 条错误）"}"#,
    )
    .unwrap();
    let (address, server) = start(root.clone()).await;

    let (en, zh, deny) = tokio::join!(
        send(
            address,
            "/api/submit/en",
            json!({"users":[{"score":null}]}),
            false,
            422
        ),
        send(
            address,
            "/api/submit/zh_Hans",
            json!({"users":[{"score":null}]}),
            false,
            422
        ),
        send(address, "/api/submit/en", json!({}), true, 403),
    );
    assert_eq!(
        en,
        json!({
            "message":"The name field is required. (and 1 more error)",
            "errors":{"name":["The name field is required."],"users.0.email":["The users.0.email field is required."]}
        })
    );
    assert_eq!(zh["errors"]["name"], json!(["请填写 名称。"]));
    assert_eq!(zh["message"], "请填写 名称。 （另有 1 条错误）");
    assert_eq!(deny, json!({"message":"Submission is forbidden."}));

    let accepted = send(
        address,
        "/api/submit/en",
        json!({
            "name":"  Ada  ",
            "users":[{"email":"ada@example.test","score":null,"secret":"ignored"}],
            "description":null,"extra":true
        }),
        false,
        200,
    )
    .await;
    assert_eq!(
        accepted,
        json!({
            "input":{"name":"Ada","users":[{"email":"ada@example.test","score":null}],"description":null},
            "hook":"passed"
        })
    );
    let manual = send(
        address,
        "/api/manual",
        json!({"name":"Ada","extra":true}),
        false,
        200,
    )
    .await;
    assert_eq!(manual, json!({"name":"Ada"}));
    server.abort();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn custom_rules_respect_presence_nullable_bail_and_custom_messages() {
    use openrust::{
        contracts::validation::{Factory as _, ValidationRule},
        validation::{Factory, Rule, RuleContext, RuleFailure},
    };
    use std::collections::BTreeMap;

    struct Verify;

    impl ValidationRule for Verify {
        fn validate<'a>(
            &'a self,
            context: &'a RuleContext<'_>,
            attribute: &'a str,
            value: Option<&'a Value>,
        ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
            Box::pin(async move {
                tokio::task::yield_now().await;
                assert_eq!(attribute, "checked");
                assert_eq!(context.data.as_value()["checked"], value.unwrap().clone());
                Ok(vec![RuleFailure::new("The :attribute is already used.")])
            })
        }
    }

    let data = Data::new(json!({"null":null,"checked":"Ada","blocked":42})).unwrap();
    let rules = RuleSet::new()
        .field(
            "absent",
            [Rule::sometimes(), Rule::required(), Rule::custom(Verify)],
        )
        .unwrap()
        .field("null", [Rule::nullable(), Rule::custom(Verify)])
        .unwrap()
        .field(
            "blocked",
            [Rule::bail(), Rule::string(), Rule::custom(Verify)],
        )
        .unwrap()
        .field("checked", [Rule::custom(Verify)])
        .unwrap();
    let messages = BTreeMap::from([("blocked.string".into(), "Supply a text :attribute.".into())]);
    let attributes = BTreeMap::from([
        ("blocked".into(), "description".into()),
        ("checked".into(), "name".into()),
    ]);
    let mut validator = Factory::default()
        .make(data, rules, messages, attributes)
        .unwrap();
    assert!(validator.fails().await.unwrap());
    assert_eq!(
        validator.errors().get("blocked"),
        ["Supply a text description."]
    );
    assert_eq!(
        validator.errors().get("checked"),
        ["The name is already used."]
    );
    assert!(validator.errors().get("absent").is_empty());
    assert!(validator.errors().get("null").is_empty());
}
