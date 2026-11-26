use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    localization::{FileLoader, Localizer},
    validation::{Data, Factory, Rule, RuleBuilder, RuleSet},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
fn fixture() -> Value {
    let mut fixture: Value =
        serde_json::from_str(include_str!("reference/validation-message-catalog.json")).unwrap();
    for (text, expected) in [
        (
            include_str!("../../../docs/en/the-basics/validation.md"),
            "Photo 1: description needs 3 characters.",
        ),
        (
            include_str!("../../../docs/zh-Hans/the-basics/validation.md"),
            "第 1 张：描述至少需要 3 个字符。",
        ),
        (
            include_str!("../../../docs/zh-Hant/the-basics/validation.md"),
            "第 1 張：描述至少需要 3 個字元。",
        ),
    ] {
        let text = text.split("{#language-file-messages}").nth(1).unwrap();
        let block = text
            .split("```json\n")
            .nth(1)
            .unwrap()
            .split("```")
            .next()
            .unwrap();
        let catalog: Value = serde_json::from_str(block).unwrap();
        fixture["cases"].as_array_mut().unwrap().push(json!({"rule":{"photos.*.description":"min:3"},"data":{"photos":[{"description":"x"}]},"catalog":catalog,"passes":false,"errors":{"photos.0.description":[expected]}}));
    }
    fixture
}
fn merge(base: &mut Value, extra: &Value) {
    if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            if let Some(old) = base.get_mut(key)
                && old.is_object()
                && value.is_object()
            {
                merge(old, value);
                continue;
            }
            base.insert(key.clone(), value.clone());
        }
    }
}
fn factory(case: &Value) -> (tempfile::TempDir, Factory) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("en")).unwrap();
    let locale = case["locale"].as_str().unwrap_or("en");
    let mut catalog: Value =
        serde_json::from_str(include_str!("../src/localization/lang/en/validation.json")).unwrap();
    merge(
        &mut catalog,
        if locale == "en" {
            &case["catalog"]
        } else {
            &case["fallback_catalog"]
        },
    );
    std::fs::write(root.path().join("en/validation.json"), catalog.to_string()).unwrap();
    if locale != "en" {
        std::fs::create_dir(root.path().join(locale)).unwrap();
        std::fs::write(
            root.path().join(locale).join("validation.json"),
            case["catalog"].to_string(),
        )
        .unwrap();
    }
    let localizer = Arc::new(
        Localizer::new(
            FileLoader::new(root.path().into()),
            locale.into(),
            "en".into(),
        )
        .unwrap(),
    );
    (root, Factory::new(localizer))
}
fn rules(case: &Value) -> RuleSet {
    let mut rules = RuleSet::new();
    for (attribute, rule) in case["rule"].as_object().unwrap() {
        if rule == "nested_email" {
            rules = rules
                .field(attribute, [Rule::email().rules([Rule::min(8)])])
                .unwrap();
        } else {
            rules = rules
                .field(
                    attribute,
                    rule.as_str()
                        .unwrap()
                        .split('|')
                        .map(|rule| Rule::parse(rule).unwrap()),
                )
                .unwrap();
        }
    }
    rules
}
fn map(case: &Value, key: &str) -> BTreeMap<String, String> {
    case.get(key).map_or_else(BTreeMap::new, |value| {
        serde_json::from_value(value.clone()).unwrap()
    })
}
#[tokio::test]
async fn language_catalog_and_inline_attributes_match_fixed_reference() {
    let mut mismatches = Vec::new();
    for (i, case) in fixture()["cases"].as_array().unwrap().iter().enumerate() {
        let (_root, factory) = factory(case);
        let mut validator = factory
            .make(
                Data::new(case["data"].clone()).unwrap(),
                rules(case),
                map(case, "messages"),
                map(case, "attributes"),
            )
            .unwrap();
        let result = validator.validate().await;
        let errors = json!(validator.errors().messages());
        if result.is_ok() != case["passes"].as_bool().unwrap() || errors != case["errors"] {
            mismatches.push(format!("{i}: expected={} actual={errors}", case["errors"]));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
fn submit(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let i: usize = request.route("index").unwrap().parse().unwrap();
        let fixtures = fixture();
        let case = &fixtures["cases"][i];
        let (_root, factory) = factory(case);
        let mut validator = factory.make(
            Data::from_request(request).await?,
            rules(case),
            map(case, "messages"),
            map(case, "attributes"),
        )?;
        openrust::http::Response::json(openrust::http::StatusCode::OK, validator.validate().await?)
    })
}
fn routes(
    _: &openrust::foundation::Application,
) -> Result<openrust::routing::Router, openrust::routing::RoutingError> {
    let mut router = openrust::routing::Router::new();
    router.post("/catalog/{index}", submit)?;
    Ok(router)
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
#[tokio::test]
async fn catalogs_run_through_real_file_loader_and_http() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use openrust::foundation::{Application, configuration::Routing, http::Kernel};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(
        Application::configure(root.path().into())
            .with_routing(Routing::new(routes))
            .create()
            .unwrap(),
    );
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    for (index, case) in fixture()["cases"].as_array().unwrap().iter().enumerate() {
        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        let body = case["data"].to_string();
        let wire = format!(
            "POST /catalog/{index} HTTP/1.1\r\nHost: {addr}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(wire.as_bytes()).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 422"), "{response}");
        let actual: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(actual["errors"], case["errors"], "case {index}");
    }
    server.abort();
    let _ = server.await;
}
