use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    validation::{Data, Factory, Rule, RuleSet},
};
use serde_json::{Value, json};
fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "reference/validation-conditional-messages.json"
    ))
    .unwrap()
}
#[tokio::test]
async fn sometimes_multiple_fields_and_false_conditions_preserve_context_and_primary_names() {
    for case in fixture()["sometimes"].as_array().unwrap() {
        let mut validator = Factory::default()
            .make(
                Data::new(case["data"].clone()).unwrap(),
                RuleSet::parse([("rows.0.account_id", "required")]).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let trace = std::sync::Mutex::new(Vec::new());
        let attributes: Vec<String> = serde_json::from_value(case["attributes"].clone()).unwrap();
        validator
            .sometimes(
                attributes,
                vec![Rule::required(), Rule::string()],
                |data, item| {
                    trace
                        .lock()
                        .unwrap()
                        .push(json!({"input":data.as_value(),"item":item}));
                    case["condition"] == true
                },
            )
            .unwrap();
        assert_eq!(
            validator.passes().await.unwrap(),
            case["passes"].as_bool().unwrap()
        );
        assert_eq!(
            json!(validator.errors().messages()),
            case["errors"],
            "{case}"
        );
        assert_eq!(json!(*trace.lock().unwrap()), case["trace"], "{case}");
    }
}
#[tokio::test]
async fn inline_message_priority_follows_ordered_source_keys_and_each_search_tier() {
    for case in fixture()["messages"].as_array().unwrap() {
        let mut validator = Factory::default()
            .make(
                Data::new(case["data"].clone()).unwrap(),
                RuleSet::parse([("rows.*.name", "required")]).unwrap(),
                serde_json::from_value(case["messages"].clone()).unwrap(),
                Default::default(),
            )
            .unwrap();
        assert!(!validator.passes().await.unwrap());
        assert_eq!(
            json!(validator.errors().messages()),
            case["errors"],
            "{case}"
        );
    }
}
fn collector_condition(input: &Data) -> bool {
    let games = &input.as_value()["games"];
    openrust::support::php_integer_less_than_value(100, games)
        || openrust::support::php_loose_string_value_equal("100", games)
}
fn check(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let fixtures = fixture();
        let kind = request.route("kind").unwrap().to_owned();
        let index: usize = request.route("index").unwrap().parse().unwrap();
        let case = &fixtures[&kind][index];
        let data = Data::from_request(request).await?;
        let rules = match kind.as_str() {
            "sometimes" => RuleSet::parse([("rows.0.account_id", "required")])?,
            "collector" => RuleSet::parse([("games", "required|integer|min:0")])?,
            _ => RuleSet::parse([("rows.*.name", "required")])?,
        };
        let messages = case
            .get("messages")
            .map_or_else(Default::default, |messages| {
                serde_json::from_value(messages.clone()).unwrap()
            });
        let mut validator = openrust::support::facades::Validator::make_with(
            data,
            rules,
            messages,
            Default::default(),
        )?;
        if kind == "sometimes" {
            let attributes: Vec<String> =
                serde_json::from_value(case["attributes"].clone()).unwrap();
            validator.sometimes(
                attributes,
                vec![Rule::required(), Rule::string()],
                |_, _| case["condition"] == true,
            )?;
        }
        if kind == "collector" {
            validator.sometimes(["reason", "cost"], vec![Rule::required()], |input, _| {
                collector_condition(input)
            })?;
        }
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            validator.validate_request(request).await?,
        )
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
async fn conditional_fields_and_inline_messages_run_through_real_http() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use openrust::foundation::{Application, configuration::Routing, http::Kernel};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(
        Application::configure(root.path().into())
            .with_routing(Routing::new(|_| {
                let mut router = openrust::routing::Router::new();
                router.post("/condition/{kind}/{index}", check)?;
                Ok(router)
            }))
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
    for kind in ["sometimes", "messages", "collector"] {
        for (index, case) in fixture()[kind].as_array().unwrap().iter().enumerate() {
            let body = case["data"].to_string();
            let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
            socket.write_all(format!("POST /condition/{kind}/{index} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).await.unwrap();
            let passes = case["passes"] == true;
            assert!(
                response.starts_with(if passes {
                    "HTTP/1.1 200"
                } else {
                    "HTTP/1.1 422"
                }),
                "{kind}/{index}: {response}"
            );
            let actual: Value =
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
            if passes {
                assert_eq!(actual, case["validated"], "{kind}/{index}");
            } else {
                assert_eq!(actual["errors"], case["errors"], "{kind}/{index}");
            }
        }
    }
    server.abort();
    let _ = server.await;
}
#[test]
fn supported_field_inputs_keep_single_field_borrowing_and_accept_owned_and_borrowed_lists() {
    use openrust::validation::ValidationAttributes;
    let field = "value";
    let single = ValidationAttributes::from(field);
    let fields: Vec<_> = single.into_iter().collect();
    assert!(matches!(fields[0], std::borrow::Cow::Borrowed(_)));
    assert!(std::ptr::eq(fields[0].as_ptr(), field.as_ptr()));
    let owned = String::from("value");
    let sources = [
        ValidationAttributes::from(&owned),
        ValidationAttributes::from(owned.clone()),
        ValidationAttributes::from(std::borrow::Cow::Borrowed("value")),
        ValidationAttributes::from(vec!["value"]),
        ValidationAttributes::from(["value"]),
        ValidationAttributes::from(["value"].as_slice()),
    ];
    for source in sources {
        assert_eq!(source.into_iter().collect::<Vec<_>>(), vec!["value"]);
    }
}
