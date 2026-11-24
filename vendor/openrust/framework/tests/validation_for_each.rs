use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        http::Kernel as _,
        validation::{CompilableRules, Factory as _, Validator as _},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::Router,
    support::facades::Validator as Facade,
    validation::{
        Data, Factory, NestedRuleSet, NestedRules, Rule, RuleSet, ValidationError,
        ValidationResult, Value,
    },
};
use serde_json::{Map, json};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
fn cases() -> &'static Value {
    static CASES: OnceLock<Value> = OnceLock::new();
    CASES.get_or_init(|| {
        serde_json::from_str(include_str!("reference/validation-for-each.json")).unwrap()
    })
}
fn nested(mode: &str, label: &str, log: Arc<Mutex<Vec<Value>>>) -> NestedRules {
    let mode = mode.to_owned();
    let label = label.to_owned();
    NestedRules::new(move |value, attribute, flat, context| {
        log.lock().unwrap().push(json!({"label":label,"attribute":attribute,"value":value,"flat":flat,"context":context}));
        if label == "inner" || mode == "value" {
            Ok(NestedRuleSet::Value(vec![
                Rule::bail(),
                Rule::required(),
                Rule::integer(),
                Rule::min(2),
            ]))
        } else if mode == "empty" {
            Ok(NestedRuleSet::Value(Vec::new()))
        } else if mode == "fields" {
            Ok(NestedRuleSet::Fields(RuleSet::parse([
                ("id", "required|integer"),
                ("tag", "sometimes|string"),
            ])?))
        } else {
            Ok(NestedRuleSet::Fields(RuleSet::new().field(
                "children.*.id",
                [nested(&mode, "inner", log.clone()).into()],
            )?))
        }
    })
}
fn rules(case: &Value, log: Arc<Mutex<Vec<Value>>>) -> ValidationResult<RuleSet> {
    let attribute = case["attribute"].as_str().unwrap();
    let mut rules = RuleSet::new();
    if attribute.starts_with("items.") {
        rules = rules.field("items", [Rule::array()])?;
    }
    rules.field(
        attribute,
        [nested(case["mode"].as_str().unwrap(), "outer", log).into()],
    )
}
fn validate(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let index = request
            .uri()
            .path()
            .rsplit('/')
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let case = &cases()[index];
        let log = Arc::new(Mutex::new(Vec::new()));
        let input = serde_json::from_slice(&request.body_bytes().await?)?;
        let mut validator = Facade::make_with(
            Data::new(input)?,
            rules(case, log.clone())?,
            BTreeMap::from([
                ("items.*.id.required".into(), "Missing :attribute".into()),
                ("items.*.id.min".into(), "Small :attribute".into()),
            ]),
            BTreeMap::from([("items.*.id".into(), "identifier".into())]),
        )?;
        let created = log.lock().unwrap().clone();
        let compiled: Vec<_> = validator
            .get_rules()?
            .into_iter()
            .map(|(field, _)| field)
            .collect();
        let passed = !validator.fails().await?;
        let validated = if passed {
            Some(validator.validated()?.into_value())
        } else {
            None
        };
        let after = log.lock().unwrap().clone();
        Response::json(
            if passed {
                StatusCode::OK
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            },
            json!({"calls":created,"calls_after":after,"rules":compiled,"passes":passed,"validated":validated,"errors":validator.errors().messages()}),
        )
    })
}
fn preflight(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new().field("companies", [Rule::array()])?.field(
            "companies.*.id",
            [Rule::for_each(|_, _| Ok(vec![Rule::required(), Rule::integer()])).into()],
        )?;
        let value = request.validate(rules).await?;
        Response::json(StatusCode::OK, value.into_value())
    })
}
struct ValidatePreflight;
impl openrust::contracts::http::Middleware for ValidatePreflight {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: openrust::http::Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            preflight(request).await?;
            Ok(next.run(request).await)
        })
    }
}
fn compile_failure(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new().field(
            "ids.*",
            [Rule::for_each(|value, _| {
                if value.and_then(Value::as_str) == Some("error") {
                    return Err(ValidationError::InvalidRule);
                }
                Ok(vec![Rule::integer()])
            })
            .into()],
        )?;
        let value = request.validate(rules).await?;
        Response::json(StatusCode::OK, value.into_value())
    })
}
fn replace_file(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = Data::from_request(request).await?;
        let mut validator = Facade::make(
            data,
            RuleSet::new().field(
                r"payload.part\.id",
                [Rule::for_each(|_, _| Ok(vec![Rule::integer()])).into()],
            )?,
        )?;
        validator.set_value(r"payload.part\.id", json!(42))?;
        let data = validator.validate_request(request).await?;
        assert!(data.file(r"payload.part\.id").is_none());
        Response::json(StatusCode::OK, data.into_value())
    })
}
fn typed_files(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let input = Data::from_request(request).await?;
        let file_rules = |context: &openrust::validation::NestedRuleContext<'_>| {
            let file = context.file().expect("编译上下文应保留文件");
            assert_eq!(file.get_size(), 5);
            Ok(vec![Rule::parse("file")?, Rule::parse("max:1")?])
        };
        let rules = RuleSet::new()
            .field("uploads.*", [Rule::for_each_context(file_rules).into()])?
            .field(
                "rows.*",
                [Rule::for_each(|_, _| {
                    RuleSet::new().field(
                        "document",
                        [Rule::compilable(NestedRules::with_context(|context| {
                            assert_eq!(context.file().expect("嵌套编译应保留文件").get_size(), 5);
                            Ok(vec![Rule::parse("file")?])
                        }))],
                    )
                })
                .into()],
            )?;
        let mut validator = Facade::make(input, rules)?;
        let result = validator.validate_request(request).await?;
        Response::json(
            StatusCode::OK,
            json!({"files":result.file("uploads.0").is_some()&&result.file("rows.0.document").is_some()}),
        )
    })
}
fn database(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new().field("companies", [Rule::array()])?.field(
            "companies.*.id",
            [Rule::for_each(|_, _| {
                Ok(vec![
                    Rule::required(),
                    Rule::exists("companies", None).into(),
                ])
            })
            .into()],
        )?;
        let data = request.validate(rules).await?;
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

#[tokio::test]
async fn foreach_compiles_before_validation_and_matches_fixed_php_over_real_http() {
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
            r.post("/case/{index}", validate)?;
            r.post("/error", compile_failure)?;
            r.post("/replace", replace_file)?;
            r.post("/files", typed_files)?;
            r.post("/precognitive", || Response::empty(StatusCode::OK))?
                .middleware_name("precognitive")
                .middleware(Arc::new(ValidatePreflight));
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
    for (index, case) in cases().as_array().unwrap().iter().enumerate() {
        let (status, data, _) = send(
            address,
            "POST",
            &format!("/case/{index}"),
            &case["input"].to_string(),
            "",
        )
        .await;
        assert_eq!(
            status,
            if case["passes"] == true { 200 } else { 422 },
            "case {index} {data}"
        );
        for field in [
            "calls",
            "calls_after",
            "rules",
            "passes",
            "validated",
            "errors",
        ] {
            let mut expected = case[field].clone();
            if matches!(field, "calls" | "calls_after") {
                for call in expected.as_array_mut().unwrap() {
                    if call["flat"].as_array().is_some_and(Vec::is_empty) {
                        call["flat"] = json!({});
                    }
                }
            }
            let expected = &expected;
            let actual = &data[field];
            if (expected.as_array().is_some_and(Vec::is_empty)
                || expected.as_object().is_some_and(Map::is_empty))
                && (actual.as_array().is_some_and(Vec::is_empty)
                    || actual.as_object().is_some_and(Map::is_empty))
            {
                continue;
            }
            assert_eq!(actual, expected, "case {index}: {field}");
        }
    }
    assert_eq!(
        send(address, "POST", "/error", r#"{"ids":["error"]}"#, "")
            .await
            .0,
        500
    );
    assert_eq!(
        send(address, "POST", "/error", r#"{"ids":[1]}"#, "")
            .await
            .0,
        200
    );
    let headers = "Precognition: true\r\nPrecognition-Validate-Only: companies.0.id\r\n";
    assert_eq!(
        send(
            address,
            "POST",
            "/precognitive",
            r#"{"companies":[{"id":1},{"id":"bad"}]}"#,
            headers
        )
        .await
        .0,
        204
    );
    let (status, data, _) = send(
        address,
        "POST",
        "/precognitive",
        r#"{"companies":[{"id":"bad"},{"id":2}]}"#,
        headers,
    )
    .await;
    assert_eq!(status, 422, "{data}");
    assert!(data["errors"]["companies.0.id"].is_array());
    assert!(data["errors"]["companies.1.id"].is_null());
    let body = "--forms\r\nContent-Disposition: form-data; name=\"payload[part.id]\"; filename=\"test.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--forms--\r\n";
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream.write_all(format!("POST /replace HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: multipart/form-data; boundary=forms\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let value: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(value, json!({"payload":{"part.id":42}}));
    let body=["uploads[0]","rows[0][document]"].iter().map(|name|format!("--forms\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"test.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n")).collect::<String>()+"--forms--\r\n";
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream.write_all(format!("POST /files HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: multipart/form-data; boundary=forms\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let value: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(value, json!({"files":true}));
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn foreach_public_entry_points_recompile_setters_and_recover_from_errors() {
    let factory = Factory::default();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = calls.clone();
    let rule = Rule::for_each(move |value, attribute| {
        observed.lock().unwrap().push(attribute.to_owned());
        if value == Some(&json!("error")) {
            return Err(ValidationError::InvalidRule);
        }
        Ok(vec![Rule::required(), Rule::integer()])
    });
    let mut validator = factory
        .make(
            Data::new(json!({"ids":[1,2]})).unwrap(),
            RuleSet::new().field("ids.*", [rule.into()]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(calls.lock().unwrap().len(), 2);
    assert!(!validator.fails().await.unwrap());
    assert!(!validator.fails().await.unwrap());
    assert_eq!(calls.lock().unwrap().len(), 2);
    validator.set_data(Data::new(json!({"ids":[3]})).unwrap());
    assert_eq!(calls.lock().unwrap().len(), 3);
    assert!(validator.validate().await.is_ok());
    validator.set_data(Data::new(json!({"ids":["error"]})).unwrap());
    assert!(matches!(
        validator.get_rules(),
        Err(ValidationError::InvalidRule)
    ));
    assert!(matches!(
        validator.validate().await,
        Err(ValidationError::InvalidRule)
    ));
    assert!(matches!(
        validator.validate().await,
        Err(ValidationError::InvalidRule)
    ));
    validator.set_data(Data::new(json!({"ids":[4]})).unwrap());
    assert!(validator.validate().await.is_ok());
    let input = json!({"values":[2]});
    let flat = openrust::support::Arr::dot(&input).unwrap();
    let callback = NestedRules::new(|_, _, _, _| Ok(vec![Rule::required(), Rule::integer()]));
    let compiled = callback
        .compile("values.0", Some(&json!(2)), &flat, None)
        .unwrap();
    let mut validator = factory
        .make(
            Data::new(input).unwrap(),
            compiled,
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(validator.validate().await.is_ok());
    struct Generated;
    impl CompilableRules for Generated {
        fn compile(
            &self,
            attribute: &str,
            _: Option<&Value>,
            data: &Map<String, Value>,
            _: Option<&Value>,
        ) -> ValidationResult<RuleSet> {
            Rule::compile(attribute, vec![Rule::required(), Rule::integer()], data)
        }
    }
    let mut validator = factory
        .make(
            Data::new(json!({"values":[2]})).unwrap(),
            RuleSet::new()
                .field("values.*", [Rule::compilable(Generated)])
                .unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(validator.validate().await.is_ok());
}

#[tokio::test]
async fn dynamic_additions_contexts_literal_paths_and_expansion_limits_are_preserved() {
    let factory = Factory::default();
    let cases: Value =
        serde_json::from_str(include_str!("reference/validation-sometimes-contexts.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let mut validator = factory
            .make(
                Data::new(case["input"].clone()).unwrap(),
                RuleSet::new(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let mut contexts = Vec::new();
        // 条件回调不需要共享状态；只借用本次调用的数据。
        let contexts = std::sync::Mutex::new(&mut contexts);
        validator
            .sometimes(
                case["attribute"].as_str().unwrap(),
                vec![Rule::string()],
                |_, item| {
                    contexts
                        .lock()
                        .unwrap()
                        .push(item.cloned().unwrap_or(Value::Null));
                    false
                },
            )
            .unwrap();
        assert_eq!(json!(*contexts.lock().unwrap()), case["contexts"]);
    }
    let rules = RuleSet::new()
        .field("rows", [Rule::array()])
        .unwrap()
        .field(
            "rows.*",
            [Rule::for_each(|_, _| {
                RuleSet::parse([("a", "nullable"), ("b", "nullable"), ("c", "nullable")])
            })
            .into()],
        )
        .unwrap();
    let mut validator = factory
        .make(
            Data::new(json!({"rows":vec![json!({});400]})).unwrap(),
            rules,
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(validator.get_rules().unwrap().len(), 1201);
    assert!(validator.validate().await.is_ok());
    let observed = Arc::new(Mutex::new(Vec::new()));
    let seen = observed.clone();
    let rule = Rule::for_each_with(move |value, attribute, flat, _| {
        assert_eq!(value, Some(&json!(1)));
        assert_eq!(attribute, "rows.0.part.id");
        assert_eq!(flat.get(r"rows.0.part\.id"), Some(&json!(1)));
        assert_eq!(flat.get("rows.0.part.id"), Some(&json!(2)));
        seen.lock().unwrap().push(attribute.to_owned());
        Ok(vec![Rule::integer()])
    });
    let mut validator = factory
        .make(
            Data::new(json!({"rows":[{"part.id":1,"part":{"id":2}}]})).unwrap(),
            RuleSet::new()
                .field(r"rows.*.part\.id", [rule.into()])
                .unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(validator.validate().await.is_ok());
    assert_eq!(observed.lock().unwrap().len(), 1);
    validator.set_value(r"rows.0.part\.id", json!(3)).unwrap();
    assert_eq!(
        validator.get_value(r"rows.0.part\.id").unwrap(),
        Some(&json!(3))
    );
    assert_eq!(
        validator.get_value("rows.0.part.id").unwrap(),
        Some(&json!(2))
    );
    let mut validator = factory
        .make(
            Data::new(json!({"ids":[1]})).unwrap(),
            RuleSet::new(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    validator
        .sometimes(
            "ids.*",
            vec![Rule::for_each(|_, _| Ok(vec![Rule::min(2), Rule::integer()])).into()],
            |_, _| true,
        )
        .unwrap();
    assert!(validator.validate().await.is_err());
    validator.set_data(Data::new(json!({"ids":[1]})).unwrap());
    assert!(validator.validate().await.is_ok());
}

fn database_config(url: &str, prefix: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("app".into(), json!({"locale":"en","supported_locales":[]})),
        (
            "database".into(),
            json!({"default":"directory","connections":{"directory":{"url":url,"prefix":prefix,"read":{"url":"mysql://invalid:invalid@127.0.0.1:1/invalid","port":1}}}}),
        ),
    ])
}
#[tokio::test]
async fn foreach_database_rules_use_the_original_factory_and_inferred_wildcard_column() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let prefix = format!("foreach_{:016x}_", fastrand::u64(..));
    let config = database_config(&url, &prefix);
    let connection = openrust::database::DatabaseManager::new(
        Arc::new(openrust::config::Repository::new(config)),
        ".",
    )
    .connection(None)
    .unwrap();
    let table = format!("{prefix}companies");
    connection
        .statement(&format!("CREATE TABLE {table} (id INT PRIMARY KEY)"), &[])
        .await
        .unwrap();
    connection
        .statement(&format!("INSERT INTO {table} VALUES (1)"), &[])
        .await
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join(&prefix);
    std::fs::create_dir(&base).unwrap();
    let app = Application::configure(base)
        .with_config(|base, _| {
            Ok(database_config(
                &std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap(),
                base.file_name().unwrap().to_str().unwrap(),
            ))
        })
        .with_routing(Routing::new(|_| {
            let mut r = Router::new();
            r.post("/database", database)?;
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
    assert_eq!(
        send(
            address,
            "POST",
            "/database",
            r#"{"companies":[{"id":1}]}"#,
            ""
        )
        .await
        .0,
        200
    );
    let (status, data, _) = send(
        address,
        "POST",
        "/database",
        r#"{"companies":[{"id":1},{"id":999}]}"#,
        "",
    )
    .await;
    assert_eq!(status, 422, "{data}");
    assert!(data["errors"]["companies.1.id"].is_array());
    server.abort();
    let _ = server.await;
    connection
        .statement(&format!("DROP TABLE {table}"), &[])
        .await
        .unwrap();
}
