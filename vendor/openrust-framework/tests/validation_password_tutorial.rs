use openrust::{
    contracts::validation::{Factory as _, UncompromisedVerifier, Validator as _},
    validation::{
        Data, Factory, Rule, RuleSet, ValidationFuture, ValidationResult, rules::Password,
    },
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
fn fixture() -> Value {
    serde_json::from_str(include_str!("reference/validation-password-tutorial.json")).unwrap()
}
#[derive(Default)]
struct Breaches(Mutex<Vec<Value>>);
impl UncompromisedVerifier for Breaches {
    fn verify<'a>(
        &'a self,
        value: &'a Value,
        threshold: i64,
    ) -> ValidationFuture<'a, ValidationResult<bool>> {
        Box::pin(async move {
            self.0
                .lock()
                .unwrap()
                .push(json!({"value":value,"threshold":threshold}));
            Ok(threshold >= 5)
        })
    }
}
fn policy(case: &Value) -> Password {
    match case["mode"].as_str().unwrap() {
        "min0" => Password::min(0),
        "max0" => Password::min(3).max(0),
        "max8" => Password::min(3).max(8),
        "strong" => Password::min(3).mixed_case().letters().numbers().symbols(),
        "replace_rules" => Password::min(3)
            .rules([Rule::min(20)])
            .rules([Rule::max(8)]),
        "mixed" => Password::min(3).mixed_case(),
        "letters" => Password::min(3).letters(),
        "required" => Password::required(),
        "sometimes" => Password::sometimes(),
        "uncompromised" => Password::min(3).uncompromised(0),
        "threshold" => Password::min(3).uncompromised(5),
        _ => Password::default(),
    }
}
fn applied(rule: &Password) -> Value {
    let a = rule.applied_rules();
    json!({"min":a.min,"max":a.max,"mixedCase":a.mixed_case,"letters":a.letters,"numbers":a.numbers,"symbols":a.symbols,"uncompromised":a.uncompromised,"compromisedThreshold":a.compromised_threshold,"customRules":if a.custom_rules.is_empty(){Vec::<String>::new()}else{vec!["max:8".into()]}})
}
#[tokio::test]
async fn password_policy_and_breach_contract_follow_fixed_reference() {
    let mut mismatches = Vec::new();
    for (index, case) in fixture()["cases"].as_array().unwrap().iter().enumerate() {
        let factory = Factory::default();
        let verifier = Arc::new(Breaches::default());
        factory.set_uncompromised_verifier(verifier.clone());
        let password = policy(case);
        assert_eq!(applied(&password), case["applied"], "{index}");
        assert_eq!(
            password.to_password_rules_string(),
            case["string"],
            "{index}"
        );
        assert_eq!(
            (&password).into_iter().count(),
            case["iterator"].as_array().unwrap().len(),
            "{index}"
        );
        let mut rules = Vec::new();
        if case["nullable"] == true {
            rules.push(Rule::nullable());
        }
        rules.push(password.into());
        let mut validator = factory
            .make(
                Data::new(case["data"].clone()).unwrap(),
                RuleSet::new().field("password", rules).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let pass = validator.validate().await.is_ok();
        let errors = json!(validator.errors().messages());
        let expected = if case["errors"].is_array() {
            json!({})
        } else {
            case["errors"].clone()
        };
        let calls = json!(*verifier.0.lock().unwrap());
        if pass != case["passes"].as_bool().unwrap() || errors != expected || calls != case["calls"]
        {
            mismatches.push(format!(
                "{index}: expected={case}, actual={errors}, calls={calls}"
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
fn submit(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let index: usize = request.route("index").unwrap().parse().unwrap();
        let fixtures = fixture();
        let case = &fixtures["cases"][index];
        let factory = Factory::default();
        factory.set_uncompromised_verifier(Arc::new(Breaches::default()));
        let mut rules = Vec::new();
        if case["nullable"] == true {
            rules.push(Rule::nullable());
        }
        rules.push(policy(case).into());
        let mut validator = factory.make(
            Data::from_request(request).await?,
            RuleSet::new().field("password", rules)?,
            Default::default(),
            Default::default(),
        )?;
        openrust::http::Response::json(openrust::http::StatusCode::OK, validator.validate().await?)
    })
}
fn defaults_response() -> Result<openrust::http::Response, openrust::http::HttpError> {
    let rules = Password::defaults()?;
    openrust::http::Response::json(
        openrust::http::StatusCode::OK,
        json!({"min":rules.applied_rules().min,"string":rules.to_password_rules_string()}),
    )
}
fn routes(
    _: &openrust::foundation::Application,
) -> Result<openrust::routing::Router, openrust::routing::RoutingError> {
    let mut router = openrust::routing::Router::new();
    router.post("/password/{index}", submit)?;
    router.get("/defaults", defaults_response)?;
    Ok(router)
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
async fn send(address: std::net::SocketAddr, path: &str, data: &Value) -> (u16, Value) {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    tokio::time::timeout(std::time::Duration::from_secs(15),async{let body=data.to_string();let mut socket=tokio::net::TcpStream::connect(address).await.unwrap();let wire=format!("POST {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());socket.write_all(wire.as_bytes()).await.unwrap();let mut response=String::new();socket.read_to_string(&mut response).await.unwrap();let status=response.split(' ').nth(1).unwrap().parse().unwrap();let value=serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();(status,value)}).await.unwrap()
}
#[tokio::test]
async fn password_rules_run_through_real_http() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use openrust::foundation::{Application, configuration::Routing, http::Kernel};
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
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    for (index, case) in fixture()["cases"].as_array().unwrap().iter().enumerate() {
        let (status, actual) = send(address, &format!("/password/{index}"), &case["data"]).await;
        assert_eq!(
            status,
            if case["passes"] == true { 200 } else { 422 },
            "{index}: {actual}"
        );
        if status == 422 {
            assert_eq!(actual["errors"], case["errors"], "{index}");
        }
    }
    server.abort();
    let _ = server.await;
}

struct Defaults(usize);
impl openrust::support::ServiceProvider for Defaults {
    fn boot<'a>(
        &'a mut self,
        _: &'a openrust::foundation::Application,
    ) -> openrust::http::HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            let min = self.0;
            Password::set_defaults(move || Password::min(min).letters().numbers())?;
            Ok(())
        })
    }
}
struct CheckDefaults(openrust::console::CommandDefinition);
impl openrust::console::Command for CheckDefaults {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a openrust::foundation::Application,
        _: &'a openrust::console::Input,
        _: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>> {
        Box::pin(async move {
            assert_eq!(Password::defaults().unwrap().applied_rules().min, 12);
            for (rule, expected) in [
                Password::default(),
                Password::defaults().unwrap(),
                Password::required(),
                Password::sometimes(),
            ]
            .iter()
            .zip(fixture()["defaults"].as_array().unwrap())
            {
                assert_eq!(applied(rule), expected["applied"]);
                assert_eq!(rule.to_password_rules_string(), expected["string"]);
                assert_eq!(
                    rule.into_iter().count(),
                    expected["iterator"].as_array().unwrap().len()
                );
            }
            let frozen = Password::required();
            Password::set_defaults(|| Password::min(6)).unwrap();
            assert_eq!(frozen.applied_rules().min, 12);
            assert_eq!(Password::default().applied_rules().min, 6);
            let mut validator = openrust::support::facades::Validator::make(
                Data::new(json!({"password":"abcdef"})).unwrap(),
                RuleSet::new()
                    .field("password", [Password::default().into()])
                    .unwrap(),
            )
            .unwrap();
            assert!(validator.validate().await.is_ok());
            Ok(0)
        })
    }
}
fn commands(
    artisan: &mut openrust::console::Application,
) -> Result<(), openrust::console::ConsoleError> {
    artisan.add_command(Box::new(CheckDefaults(
        openrust::console::CommandDefinition::new(clap::Command::new("password:check")),
    )))
}
#[tokio::test]
async fn configured_defaults_are_app_local_and_run_in_artisan() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use openrust::foundation::{Application, configuration::Routing, http::Kernel};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    assert_eq!(Password::default().applied_rules().min, 8);
    assert!(Password::set_defaults(|| Password::min(2)).is_err());
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let mut servers = Vec::new();
    let mut addresses = Vec::new();
    for (root, min) in roots.iter().zip([8, 12]) {
        let mut kernel = Kernel::new(
            Application::configure(root.path().to_owned())
                .with_providers(vec![Box::new(Defaults(min))])
                .with_routing(Routing::new(routes))
                .create()
                .unwrap(),
        );
        kernel.bootstrap().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        addresses.push((listener.local_addr().unwrap(), min));
        servers.push(tokio::spawn(async move {
            axum::serve(
                listener,
                transport.with_state(Arc::new(kernel)).into_make_service(),
            )
            .await
            .unwrap();
        }));
    }
    let mut tasks = Vec::new();
    for _ in 0..12 {
        for (address, min) in &addresses {
            let (address, min) = (*address, *min);
            tasks.push(tokio::spawn(async move {
                let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
                socket
                    .write_all(
                        format!(
                            "GET /defaults HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
                let mut response = String::new();
                socket.read_to_string(&mut response).await.unwrap();
                assert!(response.starts_with("HTTP/1.1 200"), "{response}");
                let body: Value =
                    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
                assert_eq!(body["min"], min);
                assert_eq!(
                    body["string"],
                    format!("minlength: {min}; required: lower; required: digit;")
                );
            }));
        }
    }
    for task in tasks {
        task.await.unwrap();
    }
    for task in servers {
        task.abort();
        let _ = task.await;
    }
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().to_owned())
        .with_providers(vec![Box::new(Defaults(12))])
        .with_commands(commands)
        .create()
        .unwrap();
    assert_eq!(
        app.handle_command(&["password:check".into()])
            .await
            .unwrap(),
        0
    );
    assert_eq!(Password::default().applied_rules().min, 8);
}
struct Mutating {
    trace: Arc<Mutex<Vec<Value>>>,
}
impl openrust::contracts::validation::DataAwareRule for Mutating {
    fn set_data(&mut self, data: &Data) -> ValidationResult<()> {
        self.trace
            .lock()
            .unwrap()
            .push(json!({"data":data.as_value()}));
        Ok(())
    }
}
impl openrust::contracts::validation::ValidatorAwareRule for Mutating {
    fn set_validator(
        &mut self,
        validator: openrust::validation::ValidatorHandle,
    ) -> ValidationResult<()> {
        let data = validator.with(|validator| {
            let data = validator.get_data().as_value().clone();
            validator.set_value("password", json!("Abc1!"))?;
            Ok(data)
        })?;
        self.trace.lock().unwrap().push(json!({"validator":data}));
        Ok(())
    }
}
impl openrust::contracts::validation::ValidationRule for Mutating {
    fn validate<'a>(
        &'a self,
        _: &'a openrust::validation::RuleContext<'_>,
        _: &'a str,
        value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<openrust::validation::RuleFailure>>> {
        Box::pin(async move {
            self.trace.lock().unwrap().push(json!({"validate":value}));
            Ok(Vec::new())
        })
    }
}
#[tokio::test]
async fn password_additional_aware_rules_use_the_original_factory_and_original_complexity_value() {
    let fixtures = fixture();
    let expected = &fixtures["aware"];
    let trace = Arc::new(Mutex::new(Vec::new()));
    let factory = Factory::default();
    let verifier = Arc::new(Breaches::default());
    factory.set_uncompromised_verifier(verifier.clone());
    let resolutions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = resolutions.clone();
    factory.resolver(move |validator| {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(validator)
    });
    let rule = Password::min(3)
        .mixed_case()
        .uncompromised(0)
        .rules([Rule::aware(Mutating {
            trace: trace.clone(),
        })]);
    let mut validator = factory
        .make(
            Data::new(json!({"password":"abc","sibling":42})).unwrap(),
            RuleSet::new().field("password", [rule.into()]).unwrap(),
            Default::default(),
            std::collections::BTreeMap::from([("password".into(), "chosen".into())]),
        )
        .unwrap();
    assert_eq!(
        validator.validate().await.is_ok(),
        expected["passes"].as_bool().unwrap()
    );
    assert_eq!(json!(validator.errors().messages()), expected["errors"]);
    assert_eq!(json!(*trace.lock().unwrap()), expected["trace"]);
    assert_eq!(validator.get_data().as_value(), &expected["data"]);
    assert_eq!(
        verifier.0.lock().unwrap().len(),
        expected["calls"].as_u64().unwrap() as usize
    );
    assert_eq!(resolutions.load(std::sync::atomic::Ordering::SeqCst), 2);
}
#[tokio::test]
async fn public_password_iterator_executes_the_fixed_reference_rule_list() {
    for case in fixture()["cases"].as_array().unwrap() {
        let actual = policy(case).into_iter().collect::<Vec<_>>();
        let expected = case["iterator"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| Rule::parse(rule.as_str().unwrap()).unwrap())
            .collect::<Vec<_>>();
        let mut a = Factory::default()
            .make(
                Data::new(case["data"].clone()).unwrap(),
                RuleSet::new().field("password", actual).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let mut b = Factory::default()
            .make(
                Data::new(case["data"].clone()).unwrap(),
                RuleSet::new().field("password", expected).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            a.validate().await.is_ok(),
            b.validate().await.is_ok(),
            "{case}"
        );
        assert_eq!(a.errors().messages(), b.errors().messages(), "{case}");
    }
    let conditional = Password::min(3)
        .when(true, Password::numbers)
        .unless(false, Password::symbols)
        .when_else(false, Password::letters, Password::mixed_case);
    let state = conditional.applied_rules();
    assert!(state.numbers && state.symbols && state.mixed_case && !state.letters);
    assert_eq!(
        conditional.to_password_rules_string(),
        "minlength: 3; required: lower; required: upper; required: digit; required: special;"
    );
}
