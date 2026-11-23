use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    validation::{Data, Factory, Rule, RuleSet, rules::Email},
};
use serde_json::{Value, json};

#[tokio::test]
async fn default_email_matches_fixed_reference_and_egulias_cases() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/api-f01/fixtures/email_rule.json"
    ))
    .unwrap();
    let factory = Factory::default();
    let mut mismatches = Vec::new();
    for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let mut validator = factory
            .make(
                Data::new(json!({"email":case["value"]})).unwrap(),
                RuleSet::parse([("email", "required|email")]).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        if validator.validate().await.is_ok() != case["passes"].as_bool().unwrap() {
            mismatches.push(format!("{index}: {case}"));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[tokio::test]
async fn email_rules_preserve_nullable_wildcards_and_error_messages() {
    let factory = Factory::default();
    for rule in [Rule::email(), Email::new().into(), Email::default().into()] {
        let mut validator = factory
            .make(
                Data::new(json!({"users":[{"email":"valid@test"},{"email":"invalid"}]})).unwrap(),
                RuleSet::new().field("users.*.email", [rule]).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert!(validator.validate().await.is_err());
        assert!(
            !validator
                .errors()
                .get("users.1.email", None)
                .into_messages()
                .is_empty()
        );
        assert!(
            validator
                .errors()
                .get("users.0.email", None)
                .into_messages()
                .is_empty()
        );
    }
    for value in [Value::Null, json!(""), json!("a@test")] {
        let mut validator = factory
            .make(
                Data::new(json!({"email":value})).unwrap(),
                RuleSet::parse([("email", "nullable|email:rfc")]).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert!(validator.validate().await.is_ok());
    }
    for mode in ["email:dns", "email:strict", "email:spoof", "email:filter"] {
        assert!(RuleSet::parse([("email", mode)]).is_err());
    }
}

struct EmailForm;
impl openrust::foundation::http::FormRequest for EmailForm {
    fn rules(&self) -> openrust::validation::ValidationResult<RuleSet> {
        RuleSet::parse([("email", "required|email")])
    }
}

fn submit(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    use openrust::foundation::http::FormRequest as _;
    Box::pin(async move {
        let input = EmailForm.validate_resolved(request).await?;
        openrust::http::Response::json(openrust::http::StatusCode::OK, input.into_value())
    })
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::post("/email", submit)?;
    Ok(())
}

#[tokio::test]
async fn email_form_request_runs_over_real_tcp() {
    use axum::handler::Handler as _;
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing, http::Kernel},
    };
    use std::{sync::Arc, time::Duration};
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::{TcpListener, TcpStream},
        time::timeout,
    };
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = std::env::temp_dir().join(format!("openrust-email-{:016x}", fastrand::u64(..)));
    std::fs::create_dir_all(&root).unwrap();
    let app = Application::configure(root.clone())
        .with_routing(Routing::default().api(routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    for (email, status) in [
        (json!("person@example.test"), 200),
        (json!("\"a,b\"@test"), 200),
        (json!("person@例子.测试"), 200),
        (json!("a..b@test"), 422),
        (json!("a\r\n @test"), 422),
        (json!(["person@test"]), 422),
        (Value::Null, 422),
    ] {
        let body = json!({"email":email}).to_string();
        let reply = timeout(Duration::from_secs(5), async {
            let mut socket = TcpStream::connect(address).await.unwrap();
            let wire = format!("POST /api/email HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(wire.as_bytes()).await.unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).await.unwrap();
            response
        }).await.unwrap();
        assert!(reply.starts_with(&format!("HTTP/1.1 {status} ")), "{reply}");
        let (_, body) = reply.split_once("\r\n\r\n").unwrap();
        let body: Value = serde_json::from_str(body).unwrap();
        if status == 422 {
            assert!(body["errors"]["email"].is_array(), "{body}");
        } else {
            assert_eq!(body["email"], email);
        }
    }
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(root).unwrap();
}
