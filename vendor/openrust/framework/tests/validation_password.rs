use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    validation::{Data, Factory, Rule, RuleSet, rules::Password},
};
use serde_json::{Value, json};

#[tokio::test]
async fn password_and_confirmed_match_fixed_reference_validator() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/api-f01/fixtures/password_rule.json"
    ))
    .unwrap();
    let factory = Factory::default();
    let mut mismatches = Vec::new();
    for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let rules = if case["kind"] == "password" {
            let password = match case["mode"].as_str().unwrap() {
                "required" => Password::required(),
                "sometimes" => Password::sometimes(),
                "min1" => Password::min(0),
                "max8" => Password::min(3).max(8),
                "strong" => Password::min(3).mixedCase().letters().numbers().symbols(),
                _ => Password::default(),
            };
            let mut rules = Vec::new();
            if case["nullable"] == true {
                rules.push(Rule::nullable());
            }
            rules.push(password.into());
            RuleSet::new().field("password", rules).unwrap()
        } else {
            RuleSet::parse([(
                case["field"].as_str().unwrap_or("password"),
                case["rule"].as_str().unwrap_or("confirmed"),
            )])
            .unwrap()
        };
        let mut validator = factory
            .make(
                Data::new(case["data"].clone()).unwrap(),
                rules,
                case.get("custom_messages")
                    .map_or_else(Default::default, |value| {
                        serde_json::from_value(value.clone()).unwrap()
                    }),
                case.get("attributes")
                    .map_or_else(Default::default, |value| {
                        serde_json::from_value(value.clone()).unwrap()
                    }),
            )
            .unwrap();
        let passes = validator.validate().await.is_ok();
        let expected = case["passes"].as_bool().unwrap();
        if passes != expected {
            mismatches.push(format!("{index}: {case}; errors: {:?}", validator.errors()));
        }
        let expected_messages: std::collections::BTreeMap<String, Vec<String>> =
            if case["messages"].is_array() {
                Default::default()
            } else {
                serde_json::from_value(case["messages"].clone()).unwrap()
            };
        if serde_json::to_value(validator.errors().messages()).unwrap()
            != serde_json::to_value(expected_messages).unwrap()
        {
            mismatches.push(format!(
                "{index}: messages; {case}; errors: {:?}",
                validator.errors()
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[tokio::test]
async fn fortify_policy_validates_confirmation_and_localized_password_failures() {
    let factory = Factory::default();
    for (input, valid) in [
        (json!({}), false),
        (
            json!({"password":"short", "password_confirmation":"short"}),
            false,
        ),
        (
            json!({"password":"longpassword", "password_confirmation":"mismatch"}),
            false,
        ),
        (
            json!({"password":"longpassword", "password_confirmation":"longpassword"}),
            true,
        ),
        (
            json!({"password":12345678, "password_confirmation":12345678}),
            false,
        ),
    ] {
        let mut validator = factory
            .make(
                Data::new(input).unwrap(),
                RuleSet::new()
                    .field(
                        "password",
                        [
                            Rule::required(),
                            Rule::string(),
                            Password::default().into(),
                            Rule::confirmed(),
                        ],
                    )
                    .unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(validator.validate().await.is_ok(), valid);
    }
    let mut validator = factory
        .make(
            Data::new(json!({"password":"abcdefgh"})).unwrap(),
            RuleSet::new()
                .field(
                    "password",
                    [Password::min(8).mixedCase().numbers().symbols().into()],
                )
                .unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(validator.validate().await.is_err());
    assert_eq!(
        validator.errors().get("password", None).intoMessages(),
        [
            "The password field must contain at least one uppercase and one lowercase letter.",
            "The password field must contain at least one symbol.",
            "The password field must contain at least one number.",
        ]
    );
}

struct PasswordForm;
impl openrust::foundation::http::FormRequest for PasswordForm {
    fn rules(&self) -> openrust::validation::ValidationResult<RuleSet> {
        RuleSet::new().field(
            "password",
            [
                Rule::required(),
                Rule::string(),
                Password::default().into(),
                Rule::confirmed(),
            ],
        )
    }
}

fn submit(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    use openrust::foundation::http::FormRequest as _;
    Box::pin(async move {
        let input = PasswordForm.validateResolved(request).await?;
        openrust::http::Response::json(openrust::http::StatusCode::OK, input.into_value())
    })
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::post("/password", submit)?;
    Ok(())
}

#[tokio::test]
async fn password_form_request_runs_over_real_tcp() {
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
        kernel.handleTransport(request).await
    }
    let root = std::env::temp_dir().join(format!("openrust-password-{:016x}", fastrand::u64(..)));
    std::fs::create_dir_all(&root).unwrap();
    let app = Application::configure(root.clone())
        .withRouting(Routing::default().api(routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    for (password, confirmation, status) in [
        (json!("abcdefgh"), json!("abcdefgh"), 200),
        (json!("éééééééé"), json!("éééééééé"), 200),
        (json!("abc"), json!("abc"), 422),
        (json!("abcdefgh"), json!("mismatch"), 422),
        (json!(12345678), json!(12345678), 422),
        (Value::Null, Value::Null, 422),
    ] {
        let body = json!({"password":password,"password_confirmation":confirmation}).to_string();
        let reply = timeout(Duration::from_secs(5), async {
            let mut socket = TcpStream::connect(address).await.unwrap();
            let wire = format!("POST /api/password HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(wire.as_bytes()).await.unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).await.unwrap();
            response
        }).await.unwrap();
        assert!(reply.starts_with(&format!("HTTP/1.1 {status} ")), "{reply}");
        let (_, body) = reply.split_once("\r\n\r\n").unwrap();
        let body: Value = serde_json::from_str(body).unwrap();
        if status == 422 {
            assert!(body["errors"]["password"].is_array(), "{body}");
        } else {
            assert_eq!(body["password"], password);
            assert!(body.get("password_confirmation").is_none());
        }
    }
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(root).unwrap();
}
