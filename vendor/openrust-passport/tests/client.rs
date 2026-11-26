use openrust::{
    contracts::{
        config::Repository, container::Container as _, foundation::Application as _,
        hashing::Hasher,
    },
    database::{
        DatabaseManager, Value,
        eloquent::{Attributes, Model, ModelState},
    },
    foundation::Application,
    hashing::{HashError, HashOptions},
};
use openrust_passport::{Client, ClientModel, PhpArray};
use serde_json::json;

fn hydrated(attributes: impl IntoIterator<Item = (&'static str, Value)>) -> Client {
    let mut state = ModelState::default();
    for (key, value) in attributes {
        state.set_attribute(key, value);
    }
    Client::from_state(state)
}

#[test]
fn client_grant_and_redirect_accessors_preserve_shapes_and_fixed_fallback_order() {
    let mut client = hydrated([
        ("redirect", "https://one.test/cb,https://two.test/cb".into()),
        ("secret", "hash".into()),
        ("user_id", Value::Null),
        ("owner_id", 42_i64.into()),
        ("password_client", true.into()),
        ("personal_access_client", true.into()),
    ]);
    assert!(client.first_party().unwrap());
    assert!(client.confidential());
    assert_eq!(
        client.redirect_uris().unwrap().as_value(),
        &json!(["https://one.test/cb", "https://two.test/cb"])
    );
    assert_eq!(
        client.grant_types().unwrap().as_value(),
        &json!([
            "authorization_code",
            "client_credentials",
            "implicit",
            "password",
            "personal_access",
            "refresh_token",
            "urn:ietf:params:oauth:grant-type:device_code"
        ])
    );
    client.state_mut().set_attribute("redirect_uris", "[]");
    assert!(client.redirect_uris().unwrap().is_empty());
    client.state_mut().set_attribute(
        "grant_types",
        Value::Json(json!({"0":"refresh_token", "1":"authorization_code"})),
    );
    assert_eq!(
        client.grant_types().unwrap().as_value(),
        &json!(["refresh_token", "authorization_code"])
    );
    client.state_mut().set_attribute(
        "grant_types",
        Value::Json(json!({"named":"authorization_code", "2":true})),
    );
    let grants = client.grant_types().unwrap();
    assert!(grants.as_value().is_object());
    assert!(client.has_grant_type("authorization_code").unwrap());
    assert!(
        client.has_grant_type("other").unwrap(),
        "PHP in_array is non-strict"
    );
    assert!(!grants.contains_strict_string("other"));
    client.state_mut().set_attribute("grant_types", "null");
    assert!(client.grant_types().is_err());
    client.state_mut().set_attribute("redirect_uris", "12");
    assert!(client.redirect_uris().is_err());
}

#[test]
fn client_projection_hides_raw_secret_and_uses_model_accessors_and_uuid_strategy() {
    let client = hydrated([
        ("id", "550e8400-e29b-41d4-a716-446655440000".into()),
        ("secret", "never serialize or debug this".into()),
        ("grant_types", Value::Null),
        ("redirect_uris", "[]".into()),
        ("revoked", "0".into()),
    ]);
    assert!(Client::uses_unique_ids());
    assert!(!Client::get_incrementing());
    assert!(!Client::is_valid_unique_id("1"));
    assert!(Client::is_valid_unique_id(
        "550e8400-e29b-41d4-a716-446655440000"
    ));
    let json = client.to_array().unwrap();
    assert!(!json.contains_key("secret"));
    assert_eq!(json["revoked"], false);
    assert_eq!(
        json["grant_types"],
        json!([
            "client_credentials",
            "refresh_token",
            "urn:ietf:params:oauth:grant-type:device_code"
        ])
    );
    assert_eq!(json["redirect_uris"], json!([]));
    assert!(client.plain_secret.is_none());
    assert_eq!(
        client.get_attribute_value("plain_secret").unwrap(),
        Some(Value::Null)
    );
    assert!(!format!("{client:?}").contains("never serialize"));
}

async fn application(rounds: u32) -> (Application, openrust::database::Connection) {
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.container().get::<dyn Repository>("config").unwrap();
    config.set("hashing.bcrypt.rounds", json!(rounds)).unwrap();
    config.set("database.default", json!("test")).unwrap();
    config
        .set(
            "database.connections.test",
            json!({"driver":"mysql", "database":"not_accessed"}),
        )
        .unwrap();
    let connection = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    (app, connection)
}

#[tokio::test]
async fn secrets_use_existing_eloquent_hashing_and_plaintext_remains_instance_local() {
    let (app, db) = application(4).await;
    let hasher = app.container().make::<dyn Hasher>().unwrap();
    let mut client = Client::query_on(&db)
        .make_async(Attributes::from([
            ("name".into(), "native client".into()),
            ("secret".into(), "plain secret".into()),
            (
                "grant_types".into(),
                Value::Json(json!(["authorization_code", "refresh_token"])),
            ),
            (
                "redirect_uris".into(),
                Value::Json(json!(["https://example.test/cb"])),
            ),
        ]))
        .await
        .unwrap();
    assert_eq!(
        client.plain_secret.as_deref().map(|s| s.as_str()),
        Some("plain secret")
    );
    let Some(Value::Text(hash)) = client.get_attribute("secret") else {
        panic!("missing hash")
    };
    assert!(hash.starts_with("$2y$04$"));
    assert!(
        hasher
            .check(b"plain secret", hash, &HashOptions::new())
            .await
            .unwrap()
    );
    let hash = hash.clone();
    assert!(!client.to_json().unwrap().contains("plain secret"));
    assert!(!format!("{client:?}").contains("plain secret"));
    let loaded = Client::from_state(client.state().clone());
    assert!(loaded.plain_secret.is_none());
    assert_eq!(
        loaded.get_attribute("secret"),
        Some(&Value::Text(hash.clone()))
    );
    client.set_attribute_value("secret", hash.clone()).unwrap();
    assert_eq!(client.get_attribute("secret"), Some(&Value::Text(hash)));
    assert!(
        client.plain_secret.is_some(),
        "PHP retains the setter input, even when already hashed"
    );
    client
        .force_fill_async(Attributes::from([("secret".into(), "replacement".into())]))
        .await
        .unwrap();
    assert_eq!(
        client.plain_secret.as_deref().map(|s| s.as_str()),
        Some("replacement")
    );
    let Some(Value::Text(hash)) = client.get_attribute("secret") else {
        panic!("missing hash")
    };
    assert!(
        hasher
            .check(b"replacement", hash, &HashOptions::new())
            .await
            .unwrap()
    );
    client
        .set_attribute_value_async("secret", Value::Null)
        .await
        .unwrap();
    assert!(client.plain_secret.is_none());
    assert!(!client.confidential());
    assert!(
        client
            .state()
            .get_attributes()
            .get("plain_secret")
            .is_none()
    );
}

#[tokio::test]
async fn secret_failure_keeps_the_existing_hash_and_applications_keep_their_hash_configuration() {
    let (_one, first) = application(4).await;
    let (_two, second) = application(5).await;
    let query1 = Client::query_on(&first);
    let query2 = Client::query_on(&second);
    let attrs = || Attributes::from([("secret".into(), "same input".into())]);
    let (a, b) = tokio::join!(query1.make_async(attrs()), query2.make_async(attrs()));
    let mut a = a.unwrap();
    let b = b.unwrap();
    assert!(
        matches!(a.get_attribute("secret"), Some(Value::Text(hash)) if hash.starts_with("$2y$04$"))
    );
    assert!(
        matches!(b.get_attribute("secret"), Some(Value::Text(hash)) if hash.starts_with("$2y$05$"))
    );
    let stored = a.get_attribute("secret").cloned();
    assert!(matches!(
        a.set_attribute_value_async("secret", b.get_attribute("secret").unwrap().clone())
            .await,
        Err(openrust::database::DatabaseError::AttributeHashing(
            HashError::InvalidOptions
        ))
    ));
    assert_eq!(a.get_attribute("secret"), stored.as_ref());
    assert_eq!(
        a.plain_secret.as_deref().map(|s| s.as_str()),
        b.get_attribute("secret").and_then(|v| match v {
            Value::Text(s) => Some(s.as_str()),
            _ => None,
        })
    );
}

#[test]
fn php_arrays_preserve_associative_keys_and_php_scalar_membership() {
    let list = PhpArray::try_from(json!({"0":{"0":"a"},"1":"b"})).unwrap();
    assert_eq!(list.as_value(), &json!([["a"], "b"]));
    let map = PhpArray::try_from(json!({"1":"first", "0":"second"})).unwrap();
    assert!(map.as_value().is_object());
    for (needle, value, expected) in [
        ("1e3", json!(1000), true),
        ("authorization_code", json!(0), false),
        ("anything", json!(true), true),
        ("", json!(null), true),
        ("9223372036854775808", json!("9223372036854775809"), false),
        ("1e9999", json!("2e9999"), false),
    ] {
        let values = PhpArray::try_from(json!([value])).unwrap();
        assert_eq!(values.contains_loose(needle), expected, "{needle}");
    }
}

#[test]
fn scope_exposes_the_fixed_id_and_description_payload() {
    let scope = openrust_passport::Scope::new("orders:read", "读取订单");
    assert_eq!(
        scope.to_array(),
        json!({"id":"orders:read", "description":"读取订单"})
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&scope.to_json().unwrap()).unwrap(),
        scope.to_array()
    );
}

#[test]
fn plain_secret_appending_reads_the_instance_and_respects_visibility() {
    let mut client = hydrated([("secret", "stored hash".into())]);
    client.plain_secret = Some(zeroize::Zeroizing::new("instance-only secret".to_owned()));
    assert!(client.to_array().unwrap().get("plain_secret").is_none());
    client.append(&["plain_secret"]);
    let projected = client.to_array().unwrap();
    assert_eq!(projected["plain_secret"], "instance-only secret");
    assert!(!projected.contains_key("secret"));
    assert!(client.state().get_attribute("plain_secret").is_none());
    client.make_hidden(&["plain_secret"]);
    assert!(!client.to_array().unwrap().contains_key("plain_secret"));
    client.make_visible(&["plain_secret"]);
    assert_eq!(
        client.to_array().unwrap()["plain_secret"],
        "instance-only secret"
    );
    client.set_visible(&["secret"]);
    assert!(!client.to_array().unwrap().contains_key("plain_secret"));
    let mut hydrated_again = hydrated([("secret", "stored hash".into())]);
    hydrated_again.append(&["plain_secret"]);
    assert_eq!(
        hydrated_again.to_array().unwrap()["plain_secret"],
        serde_json::Value::Null
    );
}

#[test]
fn missing_and_null_scope_restrictions_allow_scopes_without_loading_application_state() {
    for client in [Client::default(), hydrated([("scopes", Value::Null)])] {
        for scope in ["", "*", "orders:read", "账:读"] {
            assert!(client.has_scope(scope).unwrap());
        }
    }
}

#[test]
fn existing_eloquent_append_accessors_keep_raw_state_visibility_and_errors() {
    use openrust::database::{
        DatabaseResult,
        eloquent::{ModelMetadata, casts::Attribute},
    };
    struct DefaultAccessor(ModelState);
    impl Model for DefaultAccessor {
        fn metadata() -> &'static ModelMetadata {
            static META: ModelMetadata = ModelMetadata::new("projection")
                .hidden(&["label"])
                .appends(&["summary"]);
            &META
        }
        fn from_state(state: ModelState) -> Self {
            Self(state)
        }
        fn state(&self) -> &ModelState {
            &self.0
        }
        fn state_mut(&mut self) -> &mut ModelState {
            &mut self.0
        }
        fn attribute(name: &str) -> Option<Attribute> {
            (name == "summary").then_some(Attribute::get(|_, raw| -> DatabaseResult<Value> {
                Ok(raw.get("label").cloned().unwrap_or(Value::Null))
            }))
        }
    }
    let mut state = ModelState::default();
    state.set_attribute("label", "current value");
    let mut model = DefaultAccessor::from_state(state);
    assert_eq!(
        model.to_array().unwrap(),
        serde_json::json!({"summary":"current value"})
            .as_object()
            .unwrap()
            .clone()
    );
    model.state_mut().set_attribute("label", "changed value");
    assert_eq!(model.to_array().unwrap()["summary"], "changed value");
    model.make_hidden(&["summary"]);
    assert!(model.to_array().unwrap().is_empty());
    model.append(&["missing"]);
    assert!(model.to_array().is_err());
}
