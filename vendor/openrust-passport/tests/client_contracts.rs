use openrust::{
    contracts::{container::Container as _, foundation::Application as _, hashing::Hasher},
    database::{
        Value,
        eloquent::{Attributes, Model},
    },
    foundation::{Application, configuration::Routing},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};
use openrust_passport::{Client, PassportServiceProvider, PhpArray, Scope, ScopeJsonOptions};
use serde_json::{Value as Json, json};
use std::sync::Arc;

fn reference() -> Json {
    serde_json::from_str(include_str!("fixtures/client_contracts.json")).unwrap()
}
fn input(case: &Json) -> Value {
    if case["type"] == "float" {
        Value::Real(f64::from_bits(
            u64::from_str_radix(case["bits"].as_str().unwrap(), 16).unwrap(),
        ))
    } else {
        match &case["value"] {
            Json::Null => Value::Null,
            Json::Bool(v) => Value::Bool(*v),
            Json::String(v) => Value::Text(v.clone()),
            Json::Number(v) => Value::Integer(v.as_i64().unwrap()),
            v => Value::Json(v.clone()),
        }
    }
}
#[test]
fn fixed_php_scope_json_matches_options_unicode_slashes_and_numeric_strings() {
    for case in reference()["json"].as_array().unwrap() {
        let scope = Scope::new(
            case["id"].as_str().unwrap(),
            case["description"].as_str().unwrap(),
        );
        let options =
            ScopeJsonOptions::from_bits(u32::try_from(case["options"].as_u64().unwrap()).unwrap());
        assert_eq!(
            scope.to_json_with_options(options).unwrap(),
            case["expected"].as_str().unwrap(),
            "{case}"
        );
        if case["options"] == 0 {
            assert_eq!(scope.to_json().unwrap(), case["expected"]);
        }
    }
}
#[test]
fn fixed_php_arrays_keep_shapes_and_loose_membership() {
    for case in reference()["arrays"].as_array().unwrap() {
        let array = PhpArray::try_from(case["array"].clone()).unwrap();
        assert_eq!(array.as_value(), &case["array"], "array shape: {case}");
        assert_eq!(
            array.contains_loose(case["needle"].as_str().unwrap()),
            case["expected"].as_bool().unwrap(),
            "{case}"
        );
    }
}
fn routes(_: &Application) -> Result<openrust::routing::Router, openrust::routing::RoutingError> {
    let mut routes = openrust::routing::Router::new();
    routes.get("/contracts", contracts)?;
    Ok(routes)
}
fn contracts(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let container = openrust::container::Container::get_instance();
        let hasher = container.make::<dyn Hasher>().unwrap();
        let cases = reference();
        for case in cases["secrets"].as_array().unwrap() {
            let mut client = Client::default();
            client.state_mut().set_attribute("secret", "stored");
            assert!(client.set_attribute_value("secret", "before").is_err());
            let failed = client.set_attribute_value("secret", input(case)).is_err();
            assert_eq!(
                failed,
                case["expected"]["error"].as_bool().unwrap(),
                "{case}"
            );
            let plain = client.plain_secret.as_ref().map(|s| s.as_str());
            assert_eq!(plain, case["expected"]["plain"].as_str(), "{case}");
            if failed {
                assert_eq!(
                    client.get_attribute("secret"),
                    Some(&Value::Text("stored".into()))
                );
            }
        }
        for case in cases["secrets"].as_array().unwrap().iter().take(13) {
            let mut client = Client::default();
            client
                .set_attribute_value_async("secret", input(case))
                .await
                .unwrap();
            let Some(Value::Text(hash)) = client.get_attribute("secret") else {
                panic!("hash missing")
            };
            assert!(
                hasher
                    .check(
                        case["expected"]["plain"].as_str().unwrap().as_bytes(),
                        hash,
                        &openrust::hashing::HashOptions::new()
                    )
                    .await
                    .unwrap()
            );
        }
        // Mass assignment preserves earlier successful setters when a later setter fails.
        let mut client = Client::default();
        client
            .set_attribute_value_async("secret", "old")
            .await
            .unwrap();
        let old = client.get_attribute("secret").cloned();
        assert!(
            client
                .fill_async(Attributes::from([
                    ("name".into(), "earlier".into()),
                    ("secret".into(), Value::Json(json!([]))),
                ]))
                .await
                .is_err()
        );
        assert_eq!(
            client.get_attribute("name"),
            Some(&Value::Text("earlier".into()))
        );
        assert_eq!(client.get_attribute("secret"), old.as_ref());
        assert_eq!(client.plain_secret.as_ref().unwrap().as_str(), "old");
        assert!(
            client
                .fill_async(Attributes::from([
                    ("created_at".into(), "invalid date".into()),
                    ("secret".into(), "unreached".into()),
                ]))
                .await
                .is_err()
        );
        assert_eq!(client.plain_secret.as_ref().unwrap().as_str(), "old");
        assert!(
            client
                .fill_async(Attributes::from([
                    ("secret".into(), "retained".into()),
                    ("updated_at".into(), "invalid date".into()),
                ]))
                .await
                .is_err()
        );
        assert_eq!(client.plain_secret.as_ref().unwrap().as_str(), "retained");
        assert_ne!(client.get_attribute("secret"), old.as_ref());
        Response::json(
            StatusCode::OK,
            json!({"passed":true,"secrets":538,"hashes":13}),
        )
    })
}
#[tokio::test]
async fn fixed_php_secret_coercion_failure_order_and_hash_input_run_over_http() {
    let mut app = Application::configure(std::env::temp_dir())
        .with_providers(vec![Box::new(PassportServiceProvider)])
        .with_routing(Routing::new(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app.config()
        .unwrap()
        .set("hashing.bcrypt.rounds", json!(4))
        .unwrap();
    app.config()
        .unwrap()
        .set("app.host", json!("127.0.0.1"))
        .unwrap();
    app.config().unwrap().set("app.port", json!(0)).unwrap();
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = Arc::new(kernel);
    let router = axum::Router::new()
        .fallback(move |request: axum::extract::Request| kernel.clone().handle_transport(request));
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let response = reqwest::get(format!("http://{address}/contracts"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    server.abort();
}
