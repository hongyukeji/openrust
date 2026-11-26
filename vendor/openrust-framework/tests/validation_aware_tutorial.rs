use axum::{extract::State, handler::Handler as _};
use openrust::contracts::container::Container as _;
use openrust::{
    contracts::{
        foundation::Application as _,
        http::Kernel as _,
        validation::{
            DataAwareRule, Factory as _, ValidationRule, Validator as _, ValidatorAwareRule,
        },
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::Router,
    validation::{
        Data, Factory, Rule, RuleContext, RuleFailure, RuleSet, ValidationFuture, ValidationResult,
        ValidatorHandle,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
#[derive(Default)]
struct Trace {
    mode: String,
    implicit: bool,
    events: Arc<Mutex<Vec<Value>>>,
    data: Option<Data>,
    parent: Option<ValidatorHandle>,
    retained: Arc<Mutex<Vec<ValidatorHandle>>>,
}
impl DataAwareRule for Trace {
    fn set_data(&mut self, data: &Data) -> ValidationResult<()> {
        self.data = Some(data.clone());
        self.events
            .lock()
            .unwrap()
            .push(json!(["data", data.as_value()]));
        Ok(())
    }
}
impl ValidatorAwareRule for Trace {
    fn set_validator(&mut self, parent: ValidatorHandle) -> ValidationResult<()> {
        let before = parent.with(|v| {
            Ok(json!([
                "validator",
                v.get_data().as_value(),
                v.errors().messages()
            ]))
        })?;
        self.events.lock().unwrap().push(before);
        match self.mode.as_str() {
            "replace_nullable" => parent.with(|v| {
                v.set_value("value", Value::Null)?;
                v.set_rules(RuleSet::parse([("value", "nullable")])?);
                Ok(())
            })?,
            "replace_numeric" => parent.with(|v| {
                v.set_rules(RuleSet::parse([("value", "numeric|min:5")])?);
                Ok(())
            })?,
            "replace_sometimes" => parent.with(|v| {
                v.set_rules(RuleSet::parse([("value", "sometimes")])?);
                Ok(())
            })?,
            _ => {}
        }
        if self.mode == "mutation" {
            parent.with(|v| {
                v.set_value("value", json!(42))?;
                v.set_custom_messages(BTreeMap::from([(
                    "string".to_owned(),
                    "Changed :attribute".to_owned(),
                )]));
                v.errors_mut().add("manual", "Manual error");
                Ok(())
            })?;
        }
        self.retained.lock().unwrap().push(parent.clone());
        self.parent = Some(parent);
        Ok(())
    }
}
impl ValidationRule for Trace {
    fn is_implicit(&self) -> bool {
        self.implicit
    }
    fn validate<'a>(
        &'a self,
        context: &'a RuleContext<'_>,
        attribute: &'a str,
        value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            let parent = self.parent.as_ref().unwrap();
            assert!(parent.is_active());
            let current = parent.with(|v| {
                assert!(std::ptr::eq(context.data, v.get_data()));
                Ok(v.get_data().as_value().clone())
            })?;
            self.events.lock().unwrap().push(json!([
                "validate",
                attribute,
                value,
                self.data.as_ref().unwrap().as_value(),
                current
            ]));
            tokio::task::yield_now().await;
            assert_eq!(context.data.as_value(), &current);
            let child = parent.clone();
            assert!(
                tokio::spawn(async move { child.with(|_| Ok(())) })
                    .await
                    .unwrap()
                    .is_err()
            );
            parent.with(|_| {
                assert!(parent.with(|_| Ok(())).is_err());
                Ok(())
            })?;
            let actual = parent
                .with_async(|v| {
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        Ok(v.get_data().as_value().clone())
                    })
                })
                .await?;
            assert_eq!(actual, current);
            assert!(
                parent
                    .with_async(|v| Box::pin(async move { v.validate().await.map(|_| ()) }))
                    .await
                    .is_err()
            );
            if self.mode == "foreach" {
                parent.with(|validator| validator.set_value(attribute, json!(42)))?;
            }
            Ok(if self.mode == "failure" {
                vec![RuleFailure::new("Custom :attribute")]
            } else {
                Vec::new()
            })
        })
    }
}
fn normalized(mut value: Value) -> Value {
    if value == json!([]) {
        return json!({});
    }
    if let Some(events) = value.as_array_mut() {
        for event in events {
            if let Some(parts) = event.as_array_mut() {
                match parts[0].as_str() {
                    Some("data") => {
                        if parts[1] == json!([]) {
                            parts[1] = json!({});
                        }
                    }
                    Some("validator") => {
                        for i in [1, 2] {
                            if parts[i] == json!([]) {
                                parts[i] = json!({});
                            }
                        }
                    }
                    Some("validate") => {
                        for i in [3, 4] {
                            if parts[i] == json!([]) {
                                parts[i] = json!({});
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    value
}
#[tokio::test]
async fn setter_order_mutation_and_skip_semantics_match_the_fixed_rule_wrapper() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("reference/validation-aware.json")).unwrap();
    let factory = Factory::default();
    for case in cases
        .into_iter()
        .filter(|case| case["mode"] != "manual_bail" && case["mode"] != "live_flags")
    {
        let events = Arc::new(Mutex::new(Vec::new()));
        let retained = Arc::new(Mutex::new(Vec::new()));
        let rule = Rule::aware(Trace {
            mode: case["mode"].as_str().unwrap().into(),
            implicit: case["implicit"] == true,
            events: events.clone(),
            retained: retained.clone(),
            ..Default::default()
        });
        let mut rules = if case["mode"] == "mutation" {
            RuleSet::parse([("previous", "required")]).unwrap()
        } else {
            RuleSet::new()
        };
        let mut field = if case["rules"] == json!(["nullable"]) {
            vec![Rule::nullable()]
        } else {
            Vec::new()
        };
        field.push(rule);
        if case["mode"] == "mutation" || case["mode"] == "foreach" {
            field.push(Rule::string());
        }
        match case["mode"].as_str().unwrap() {
            "replace_nullable" => field.push(Rule::string()),
            "replace_numeric" => field.push(Rule::min(5)),
            "replace_sometimes" => field.push(Rule::required()),
            _ => {}
        }
        rules = rules
            .field(
                if case["mode"] == "foreach" {
                    "rows.*.name"
                } else {
                    "value"
                },
                field,
            )
            .unwrap();
        let mut validator = factory
            .make(
                Data::new(case["input"].clone()).unwrap(),
                rules,
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert!(events.lock().unwrap().is_empty());
        let result = validator.validate().await;
        assert_eq!(result.is_ok(), case["passes"].as_bool().unwrap(), "{case}");
        assert_eq!(
            json!(*events.lock().unwrap()),
            if case["events"] == json!([]) {
                json!([])
            } else {
                normalized(case["events"].clone())
            },
            "{case}"
        );
        assert_eq!(
            json!(validator.errors().messages()),
            normalized(case["errors"].clone()),
            "{case}"
        );
        if let Ok(input) = result {
            assert_eq!(
                input.all(),
                &normalized(case["validated"].clone()),
                "{case}"
            );
        }
        if case.get("data").is_some() {
            assert_eq!(validator.get_data().as_value(), &case["data"]);
        }
        for handle in retained.lock().unwrap().iter() {
            assert!(!handle.is_active());
            assert!(handle.with(|_| Ok(())).is_err());
        }
    }
}
fn trace(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mode = if request.path() == "mutation" {
            "mutation"
        } else {
            "trace"
        };
        let rule = Rule::aware(Trace {
            mode: mode.into(),
            ..Default::default()
        });
        let mut rules = if mode == "mutation" {
            RuleSet::parse([("previous", "required")])?
        } else {
            RuleSet::new()
        };
        rules = rules.field("value", [rule, Rule::string()])?;
        Response::json(StatusCode::OK, request.validate(rules).await?)
    })
}
struct SharedRule(Rule);
fn register(app: &Application) -> Result<Router, openrust::routing::RoutingError> {
    let mut r = Router::new();
    r.post("/trace", trace)?;
    r.post("/upload", upload)?;
    r.post("/upload-rewrite", rewrite_upload)?;
    r.post("/upload-literal", rewrite_upload)?;
    r.post("/mutation", trace)?;
    let shared = app.container().make::<SharedRule>().unwrap();
    r.post("/shared", move || {
        let rule = shared.0.clone();
        async move {
            let data = openrust::support::facades::Request::validate(
                RuleSet::new().field("value", [rule])?,
            )
            .await?;
            Response::json(StatusCode::OK, data)
        }
    })?;
    Ok(r)
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
async fn server(rule: Rule) -> (tempfile::TempDir, SocketAddr, tokio::task::JoinHandle<()>) {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().to_owned())
        .with_config(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                json!({"locale":"en","supported_locales":[]}),
            )]))
        })
        .with_routing(Routing::new(register))
        .create()
        .unwrap();
    app.container()
        .instance(Arc::new(SharedRule(rule)))
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    (root, address, task)
}
#[tokio::test]
async fn aware_rules_share_the_real_request_validator_and_remain_isolated_across_apps() {
    let retained = Arc::new(Mutex::new(Vec::new()));
    let rule = Rule::aware(Trace {
        mode: "trace".into(),
        retained: retained.clone(),
        ..Default::default()
    });
    let (_a, address, a) = server(rule.clone()).await;
    let (_b, other, b) = server(rule).await;
    let (status, data) = send(
        address,
        "/trace",
        r#"{"value":"A","extra":true}"#,
        "application/json",
    )
    .await;
    assert_eq!(status, 200, "{data}");
    assert_eq!(data, json!({"value":"A"}));
    let (status, data) = send(
        address,
        "/mutation",
        r#"{"previous":null,"value":"A"}"#,
        "application/json",
    )
    .await;
    assert_eq!(status, 422, "{data}");
    assert_eq!(
        data["errors"],
        json!({"previous":["The previous field is required."],"manual":["Manual error"],"value":["Changed value"]})
    );
    let mut tasks = Vec::new();
    for i in 0..32 {
        let address = if i % 2 == 0 { address } else { other };
        tasks.push(tokio::spawn(async move {
            let value = format!("request-{i}");
            let (status, data) = send(
                address,
                "/shared",
                &json!({"value":value}).to_string(),
                "application/json",
            )
            .await;
            assert_eq!(status, 200, "{data}");
            assert_eq!(data, json!({"value":value}));
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    for handle in retained.lock().unwrap().iter() {
        assert!(!handle.is_active());
        assert!(handle.with(|_| Ok(())).is_err());
    }
    a.abort();
    b.abort();
    let _ = a.await;
    let _ = b.await;
}

#[derive(Default)]
struct DataOnly {
    data: Option<Data>,
}
impl DataAwareRule for DataOnly {
    fn set_data(&mut self, data: &Data) -> ValidationResult<()> {
        self.data = Some(data.clone());
        Ok(())
    }
}
impl ValidationRule for DataOnly {
    fn validate<'a>(
        &'a self,
        context: &'a RuleContext<'_>,
        attribute: &'a str,
        _: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            let file = self.data.as_ref().unwrap().file(attribute).unwrap();
            assert_eq!(file.get(), "hello");
            assert_eq!(context.data.file(attribute).unwrap().get(), file.get());
            Ok(Vec::new())
        })
    }
}
#[derive(Default)]
struct ParentOnly {
    parent: Option<ValidatorHandle>,
}
impl ValidatorAwareRule for ParentOnly {
    fn set_validator(&mut self, parent: ValidatorHandle) -> ValidationResult<()> {
        self.parent = Some(parent);
        Ok(())
    }
}
impl ValidationRule for ParentOnly {
    fn validate<'a>(
        &'a self,
        context: &'a RuleContext<'_>,
        attribute: &'a str,
        _: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            self.parent.as_ref().unwrap().with(|validator| {
                assert_eq!(validator.get_data().file(attribute).unwrap().get(), "hello");
                assert!(std::ptr::eq(context.data, validator.get_data()));
                Ok(())
            })?;
            Ok(Vec::new())
        })
    }
}
fn upload(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new().field(
            "photo",
            [
                Rule::required(),
                Rule::data_aware(DataOnly::default()),
                Rule::validator_aware(ParentOnly::default()),
            ],
        )?;
        let input = request.validate(rules).await?;
        assert_eq!(input.file("photo").unwrap().get(), "hello");
        Response::json(StatusCode::OK, input)
    })
}
struct Wait {
    parent: Option<ValidatorHandle>,
    pause: Arc<std::sync::atomic::AtomicBool>,
    retained: Arc<Mutex<Vec<ValidatorHandle>>>,
}
impl ValidatorAwareRule for Wait {
    fn set_validator(&mut self, parent: ValidatorHandle) -> ValidationResult<()> {
        parent.with(|validator| validator.set_value("value", json!("changed")))?;
        self.retained.lock().unwrap().push(parent.clone());
        self.parent = Some(parent);
        Ok(())
    }
}
impl ValidationRule for Wait {
    fn validate<'a>(
        &'a self,
        _: &'a RuleContext<'_>,
        _: &'a str,
        _: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            if self.pause.swap(false, std::sync::atomic::Ordering::SeqCst) {
                self.parent
                    .as_ref()
                    .unwrap()
                    .with_async(|_| {
                        Box::pin(async move {
                            tokio::time::sleep(Duration::from_secs(10)).await;
                            Ok(())
                        })
                    })
                    .await?;
            }
            Ok(Vec::new())
        })
    }
}
struct FailOnce(Arc<std::sync::atomic::AtomicBool>);
impl DataAwareRule for FailOnce {
    fn set_data(&mut self, _: &Data) -> ValidationResult<()> {
        if self.0.swap(false, std::sync::atomic::Ordering::SeqCst) {
            Err(openrust::validation::ValidationError::Execution {
                source: std::io::Error::other("测试一次性setter失败").into(),
            })
        } else {
            Ok(())
        }
    }
}
impl ValidationRule for FailOnce {
    fn validate<'a>(
        &'a self,
        _: &'a RuleContext<'_>,
        _: &'a str,
        _: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}
#[tokio::test]
async fn cancellation_and_setter_errors_restore_the_same_validator_and_release_rule_locks() {
    let retained = Arc::new(Mutex::new(Vec::new()));
    let rule = Rule::validator_aware(Wait {
        parent: None,
        pause: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        retained: retained.clone(),
    });
    let factory = Factory::default();
    let mut validator = factory
        .make(
            Data::new(json!({"value":"A"})).unwrap(),
            RuleSet::new()
                .field("value", [rule, Rule::string()])
                .unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(
        timeout(Duration::from_millis(30), validator.validate())
            .await
            .is_err()
    );
    assert_eq!(retained.lock().unwrap().len(), 1);
    assert_eq!(
        validator.get_value("value").unwrap(),
        Some(&json!("changed"))
    );
    assert!(
        retained
            .lock()
            .unwrap()
            .iter()
            .all(|handle| !handle.is_active())
    );
    assert!(
        timeout(Duration::from_secs(2), validator.validate())
            .await
            .unwrap()
            .is_ok()
    );
    assert!(
        retained
            .lock()
            .unwrap()
            .iter()
            .all(|handle| handle.with(|_| Ok(())).is_err())
    );
    let rule = Rule::data_aware(FailOnce(Arc::new(std::sync::atomic::AtomicBool::new(true))));
    let (_root, address, server) = server(rule).await;
    let (status, data) = send(address, "/shared", r#"{"value":"A"}"#, "application/json").await;
    assert_eq!(status, 500, "{data}");
    let (status, data) = send(address, "/shared", r#"{"value":"B"}"#, "application/json").await;
    assert_eq!(status, 200, "{data}");
    assert_eq!(data, json!({"value":"B"}));
    let body = "--form\r\nContent-Disposition: form-data; name=\"photo\"; filename=\"input.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--form--\r\n";
    let (status, data) = send(
        address,
        "/upload",
        body,
        "multipart/form-data; boundary=form",
    )
    .await;
    assert_eq!(status, 200, "{data}");
    assert_eq!(data, json!({"photo":null}));
    server.abort();
    let _ = server.await;
}

struct ManualField {
    parent: Option<ValidatorHandle>,
    clear: bool,
}
impl ValidatorAwareRule for ManualField {
    fn set_validator(&mut self, parent: ValidatorHandle) -> ValidationResult<()> {
        self.parent = Some(parent);
        Ok(())
    }
}
impl ValidationRule for ManualField {
    fn validate<'a>(
        &'a self,
        _: &'a RuleContext<'_>,
        attribute: &'a str,
        _: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            self.parent.as_ref().unwrap().with(|validator| {
                if self.clear {
                    validator.errors_mut().forget(attribute);
                } else {
                    validator.errors_mut().add(attribute, "Manual field");
                }
                Ok(())
            })?;
            Ok(Vec::new())
        })
    }
}
#[tokio::test]
async fn live_error_bags_control_bail_presence_skipping_and_error_recovery() {
    use openrust::validation::ValidationError;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let factory = Factory::default();
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("reference/validation-aware.json")).unwrap();
    for case in cases
        .into_iter()
        .filter(|case| case["mode"] == "manual_bail")
    {
        let calls = Arc::new(AtomicUsize::new(0));
        let capture = calls.clone();
        let mut field = vec![
            Rule::validator_aware(ManualField {
                parent: None,
                clear: false,
            }),
            Rule::custom(move |_: &RuleContext<'_>, _: &str, _: Option<&Value>| {
                capture.fetch_add(1, Ordering::SeqCst);
                Ok(Vec::new())
            }),
        ];
        if case["bail"] == true {
            field.insert(0, Rule::bail());
        }
        let mut validator = factory
            .make(
                Data::new(json!({"value":"A"})).unwrap(),
                RuleSet::new().field("value", field).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert!(matches!(
            validator.validate().await,
            Err(ValidationError::Invalid(_))
        ));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            case["calls"].as_u64().unwrap() as usize
        );
        assert_eq!(json!(validator.errors().messages()), case["errors"]);
    }
    let mut v = factory
        .make(
            Data::new(json!({"value":"A"})).unwrap(),
            RuleSet::new()
                .field(
                    "value",
                    [
                        Rule::validator_aware(ManualField {
                            parent: None,
                            clear: false,
                        }),
                        Rule::exists("no_presence_connection", None).into(),
                    ],
                )
                .unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(matches!(
        v.validate().await,
        Err(ValidationError::Invalid(_))
    ));
    let failure = Rule::custom(|_: &RuleContext<'_>, _: &str, _: Option<&Value>| {
        Ok(vec![RuleFailure::new("First error")])
    });
    let mut v = factory
        .make(
            Data::new(json!({"value":"A"})).unwrap(),
            RuleSet::new()
                .field(
                    "value",
                    [
                        failure,
                        Rule::validator_aware(ManualField {
                            parent: None,
                            clear: true,
                        }),
                        Rule::exists("no_presence_connection", None).into(),
                    ],
                )
                .unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(matches!(
        v.validate().await,
        Err(ValidationError::Execution { .. })
    ));
    let rule = Rule::data_aware(FailOnce(Arc::new(std::sync::atomic::AtomicBool::new(true))));
    let mut v = factory
        .make(
            Data::new(json!({"value":"A"})).unwrap(),
            RuleSet::parse([("previous", "required")])
                .unwrap()
                .field("value", [rule])
                .unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(matches!(
        v.validate().await,
        Err(ValidationError::Execution { .. })
    ));
    assert_eq!(
        v.errors().first(Some("previous"), None),
        "The previous field is required."
    );
    v.set_data(Data::new(json!({"previous":"OK","value":"B"})).unwrap());
    assert!(v.validate().await.is_ok());
}

struct RewritePhoto {
    field: &'static str,
}
impl ValidatorAwareRule for RewritePhoto {
    fn set_validator(&mut self, parent: ValidatorHandle) -> ValidationResult<()> {
        parent.with(|validator| validator.set_value(self.field, json!("changed")))
    }
}
impl ValidationRule for RewritePhoto {
    fn validate<'a>(
        &'a self,
        context: &'a RuleContext<'_>,
        attribute: &'a str,
        value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            assert_eq!(value, Some(&Value::Null));
            assert_eq!(context.file().unwrap().get(), "hello");
            assert!(context.data.file(self.field).is_none());
            assert_eq!(attribute, self.field.replace(r"\.", "."));
            Ok(Vec::new())
        })
    }
}
fn rewrite_upload(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let field = if request.path() == "upload-literal" {
            r"literal.photo\.name"
        } else {
            "photo"
        };
        let rule = Rule::validator_aware(RewritePhoto { field });
        let data = request
            .validate(RuleSet::new().field(field, [Rule::required(), rule])?)
            .await?;
        assert!(data.file(field).is_none());
        Response::json(StatusCode::OK, data)
    })
}
#[tokio::test]
async fn original_uploaded_value_survives_setter_mutation_and_canonical_attribute_conversion() {
    let (_root, address, server) = server(Rule::string()).await;
    for (path, name, key) in [
        ("/upload-rewrite", "photo", "photo"),
        ("/upload-literal", "literal[photo.name]", "literal"),
    ] {
        let body = format!(
            "--form\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"input.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--form--\r\n"
        );
        let (status, data) = send(address, path, &body, "multipart/form-data; boundary=form").await;
        assert_eq!(status, 200, "{data}");
        if key == "literal" {
            assert_eq!(data[key]["photo.name"], "changed");
        } else {
            assert_eq!(data[key], "changed");
        }
    }
    server.abort();
    let _ = server.await;
}

struct Reconfigure {
    configuration: String,
}
impl ValidatorAwareRule for Reconfigure {
    fn set_validator(&mut self, parent: ValidatorHandle) -> ValidationResult<()> {
        parent.with(|validator| {
            if self.configuration == "invalid" {
                let rules = RuleSet::new().field(
                    "value",
                    [Rule::for_each(|_, _| {
                        Err::<Vec<Rule>, _>(openrust::validation::ValidationError::InvalidRule)
                    })
                    .into()],
                )?;
                validator.set_rules(rules);
            } else {
                validator.set_rules(RuleSet::parse([(
                    "value",
                    if self.configuration == "bail" {
                        "bail"
                    } else {
                        "string"
                    },
                )])?);
            }
            if self.configuration == "bail" {
                validator.errors_mut().add("value", "Manual field");
            }
            Ok(())
        })
    }
}
impl ValidationRule for Reconfigure {
    fn is_implicit(&self) -> bool {
        true
    }
    fn validate<'a>(
        &'a self,
        _: &'a RuleContext<'_>,
        _: &'a str,
        _: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}
struct ImplicitCounter(Arc<std::sync::atomic::AtomicUsize>);
impl ValidationRule for ImplicitCounter {
    fn is_implicit(&self) -> bool {
        true
    }
    fn validate<'a>(
        &'a self,
        _: &'a RuleContext<'_>,
        _: &'a str,
        _: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Vec::new())
        })
    }
}
#[tokio::test]
async fn live_rule_reconfiguration_changes_bail_implicit_numeric_and_optional_scheduling() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let factory = Factory::default();
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("reference/validation-aware.json")).unwrap();
    for case in cases
        .into_iter()
        .filter(|case| case["mode"] == "live_flags")
    {
        let configuration = case["configuration"].as_str().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut field = vec![Rule::validator_aware(Reconfigure {
            configuration: configuration.into(),
        })];
        if configuration == "remove_implicit" {
            field.push(Rule::required());
        }
        field.push(Rule::custom(ImplicitCounter(calls.clone())));
        let mut v = factory
            .make(
                Data::new(if configuration == "bail" {
                    json!({"value":"A"})
                } else {
                    json!({})
                })
                .unwrap(),
                RuleSet::new().field("value", field).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert!(v.validate().await.is_err());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            case["calls"].as_u64().unwrap() as usize
        );
        assert_eq!(json!(v.errors().messages()), case["errors"]);
    }
    let mut v = factory
        .make(
            Data::new(json!({"value":"A"})).unwrap(),
            RuleSet::parse([("previous", "required")])
                .unwrap()
                .field(
                    "value",
                    [Rule::validator_aware(Reconfigure {
                        configuration: "invalid".into(),
                    })],
                )
                .unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(matches!(
        v.validate().await,
        Err(openrust::validation::ValidationError::InvalidRule)
    ));
    assert!(v.errors().has("previous"));
    v.set_rules(RuleSet::parse([("value", "string")]).unwrap());
    assert!(v.validate().await.is_ok());
}
