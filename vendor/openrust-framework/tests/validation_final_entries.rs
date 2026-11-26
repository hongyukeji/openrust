use openrust::{
    contracts::{
        support::MessageProvider as _,
        validation::{Factory as FactoryContract, Validator as ValidatorContract},
    },
    support::{MessageBag, MessageSource},
    validation::{Data, Factory, Rule, RuleSet, ValidationResult},
};
use serde_json::json;
#[tokio::test]
async fn factory_validate_passes_and_callback_lists_use_existing_pipeline() {
    let factory = Factory::default();
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let resolutions = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let resolved = resolutions.clone();
    factory.resolver(move |validator| {
        resolved.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(validator)
    });
    let value = factory
        .validate(
            Data::new(json!({"name":"Ada","ignored":42})).unwrap(),
            RuleSet::parse([("name", "required")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(value.into_value(), json!({"name":"Ada"}));
    let mut validator = factory
        .make(
            Data::new(json!({"name":""})).unwrap(),
            RuleSet::parse([("name", "unique:users,name")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(validator.passes().await.unwrap());
    let callbacks: Vec<openrust::validation::AfterValidation> = ["first", "second"]
        .into_iter()
        .map(|name| {
            let calls = calls.clone();
            Box::new(move |validator: &mut openrust::validation::Validator| {
                calls.lock().unwrap().push(name);
                validator.errors_mut().add("name", name);
                Ok(())
            }) as openrust::validation::AfterValidation
        })
        .collect();
    validator.after_many(callbacks);
    assert!(!validator.passes().await.unwrap());
    assert_eq!(*calls.lock().unwrap(), vec!["first", "second"]);
    assert_eq!(validator.messages().all(None), vec!["first", "second"]);
    assert!(std::ptr::eq(
        validator.get_message_bag(),
        validator.errors()
    ));
    let borrowed = MessageSource::from(&validator);
    assert!(std::ptr::eq(borrowed.as_ref(), validator.errors()));
    let contract: &dyn ValidatorContract = &validator;
    assert!(std::ptr::eq(contract.get_message_bag(), validator.errors()));
    assert_eq!(resolutions.load(std::sync::atomic::Ordering::SeqCst), 2);
    let source = MessageSource::from([("name", "one")]);
    assert_eq!(source.as_ref().all(None), vec!["one"]);
    let source = MessageSource::from([("name", ["one", "one", "two"])]);
    assert_eq!(source.as_ref().all(None), vec!["one", "two"]);
    let mut validator = factory
        .make(
            Data::new(json!({"value":0})).unwrap(),
            RuleSet::new().field("value", [Rule::min(3)]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(!validator.passes().await.unwrap());
    validator.set_value("value", json!("long")).unwrap();
    assert!(validator.passes().await.unwrap());
}
struct External(MessageBag);
impl ValidatorContract for External {
    fn validate(
        &mut self,
    ) -> openrust::validation::ValidationFuture<
        '_,
        ValidationResult<openrust::validation::ValidatedInput>,
    > {
        Box::pin(async { Err(openrust::validation::ValidationError::NotValidated) })
    }
    fn fails(&mut self) -> openrust::validation::ValidationFuture<'_, ValidationResult<bool>> {
        Box::pin(async { Ok(true) })
    }
    fn validated(&self) -> ValidationResult<openrust::validation::ValidatedInput> {
        Err(openrust::validation::ValidationError::NotValidated)
    }
    fn errors(&self) -> &MessageBag {
        &self.0
    }
}
#[test]
fn existing_external_validator_implementations_gain_message_provider_without_extra_methods() {
    let external = External(MessageBag::new([("name", ["external"])]));
    let provider: &dyn openrust::contracts::support::MessageProvider = &external;
    assert_eq!(provider.get_message_bag().first(None, None), "external");
    let contract: &dyn ValidatorContract = &external;
    let source = MessageSource::from(contract);
    assert!(std::ptr::eq(source.as_ref(), contract.errors()));
}
struct FieldCheck(&'static str);
impl openrust::validation::AfterValidationRule for FieldCheck {
    fn after(&self, validator: &mut openrust::validation::Validator) -> ValidationResult<()> {
        validator.errors_mut().add("name", self.0);
        Ok(())
    }
}
#[tokio::test]
async fn after_services_and_form_callback_adapter_execute_in_order() {
    use openrust::validation::AfterValidationRule as _;
    let factory = Factory::default();
    let mut validator = factory
        .make(
            Data::new(json!({"name":"Ada"})).unwrap(),
            RuleSet::parse([("name", "required")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    validator.after_rule(FieldCheck("one"));
    let services: Vec<Box<dyn openrust::validation::AfterValidationRule>> =
        vec![Box::new(FieldCheck("two")), Box::new(FieldCheck("three"))];
    validator.after_rules(services);
    validator.after_many([FieldCheck("four").into_callback()]);
    assert!(!validator.passes().await.unwrap());
    assert_eq!(
        validator.errors().all(None),
        vec!["one", "two", "three", "four"]
    );
}
fn direct(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::http::{Response, StatusCode};
        use openrust::support::facades::Validator;
        let mode = request.route("mode").unwrap().to_owned();
        let data = Data::from_request(request).await?;
        if mode == "passes" {
            let mut validator =
                Validator::make(data, RuleSet::parse([("name", "unique:users,name")])?)?;
            return Response::json(StatusCode::OK, validator.passes().await?);
        }
        let rules = if mode == "file" {
            RuleSet::new().field(
                "attachment",
                [
                    Rule::required(),
                    openrust::validation::rules::File::default()
                        .encoding("UTF-8")
                        .into(),
                ],
            )?
        } else {
            RuleSet::parse([("name", "required|string|min:3")])?
        };
        let validated = if mode == "custom" {
            Validator::validate_with(
                data,
                rules,
                std::collections::BTreeMap::from([(
                    "name.required".into(),
                    "Please provide :attribute.".into(),
                )]),
                std::collections::BTreeMap::from([("name".into(), "your name".into())]),
            )
            .await?
        } else {
            Validator::validate(data, rules).await?
        };
        Response::json(StatusCode::OK, validated)
    })
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<
        std::sync::Arc<openrust::foundation::http::Kernel>,
    >,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
#[tokio::test]
async fn direct_facade_validate_custom_options_passes_and_file_encoding_run_over_tcp() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use openrust::foundation::{Application, configuration::Routing, http::Kernel};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(
        Application::configure(root.path().into())
            .with_routing(Routing::new(|_| {
                let mut router = openrust::routing::Router::new();
                router.post("/direct/{mode}", direct)?;
                Ok(router)
            }))
            .with_middleware(|middleware| {
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
            transport
                .with_state(std::sync::Arc::new(kernel))
                .into_make_service(),
        )
        .await
        .unwrap();
    });
    let mut cases = vec![
        (
            "default",
            "application/json",
            br#"{"name":"Ada","ignored":42}"#.to_vec(),
            200,
            json!({"name":"Ada"}),
        ),
        (
            "custom",
            "application/json",
            b"{}".to_vec(),
            422,
            json!({"name":["Please provide your name."]}),
        ),
        (
            "passes",
            "application/json",
            br#"{"name":""}"#.to_vec(),
            200,
            json!(true),
        ),
    ];
    for (bytes, status) in [(b"valid".as_slice(), 200), (b"\xff".as_slice(), 422)] {
        let mut body=b"--probe\r\nContent-Disposition: form-data; name=\"attachment\"; filename=\"notes.txt\"\r\nContent-Type: text/plain\r\n\r\n".to_vec();
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n--probe--\r\n");
        cases.push((
            "file",
            "multipart/form-data; boundary=probe",
            body,
            status,
            if status == 200 {
                json!({"attachment":null})
            } else {
                json!({"attachment":["The attachment field must be encoded in UTF-8."]})
            },
        ));
    }
    for (mode, media, body, status, expected) in cases {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let wire = format!(
            "POST /direct/{mode} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: {media}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket.write_all(wire.as_bytes()).await.unwrap();
        socket.write_all(&body).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        assert!(
            response.starts_with(&format!("HTTP/1.1 {status}")),
            "{response}"
        );
        let actual: serde_json::Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(
            if status == 422 {
                &actual["errors"]
            } else {
                &actual
            },
            &expected,
            "{mode}"
        );
    }
    server.abort();
    let _ = server.await;
}
