use openrust::database::{
    Value,
    eloquent::{Model, ModelState},
};
use openrust_passkeys::{Passkey, PasskeyAuthenticatable, Passkeys};
use std::sync::Arc;

mod user {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../app/models/user.rs"
    ));
}
impl PasskeyAuthenticatable for user::User {}

#[test]
fn user_defaults_match_48_pinned_php_trait_vectors() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/user-contract.json")).unwrap();
    assert_eq!(cases.as_array().unwrap().len(), 48);
    for case in cases.as_array().unwrap() {
        let input = &case["input"];
        let mut user = user::User::from_state(ModelState::default());
        for key in ["id", "name", "email"] {
            if let Some(value) = input.get(key) {
                let value = if let Some(v) = value.as_str() {
                    Value::Text(v.into())
                } else {
                    Value::Integer(value.as_i64().unwrap())
                };
                user.setAttribute(key, value);
            }
        }
        assert_eq!(
            user.getPasskeyUserHandle(input["secret"].as_str().unwrap())
                .unwrap(),
            case["handle"]
        );
        assert_eq!(user.getPasskeyDisplayName().unwrap(), case["display"]);
        assert_eq!(user.getPasskeyUsername().unwrap(), case["username"]);
    }
}

#[tokio::test]
async fn login_authorization_and_route_configuration_are_application_local() {
    let config = Arc::new(openrust::config::Repository::new(Default::default()));
    let first = Passkeys::new(config.clone());
    let second = Passkeys::new(config);
    let (parts, body) = http::Request::new(openrust::http::Body::empty()).into_parts();
    let mut request = openrust::http::Request::from_parts(parts, body);
    let user = user::User::from_state(ModelState::default());
    let passkey = Passkey::default();
    assert!(
        first
            .allowsLogin(&mut request, &user, &passkey)
            .await
            .unwrap()
    );
    first.authorizeLoginUsing(Some(Arc::new(|request, _, _| {
        Box::pin(async move {
            assert_eq!(request.method(), &openrust::http::Method::GET);
            Ok(false)
        })
    })));
    assert!(
        !first
            .allowsLogin(&mut request, &user, &passkey)
            .await
            .unwrap()
    );
    assert!(
        second
            .allowsLogin(&mut request, &user, &passkey)
            .await
            .unwrap()
    );
    first.authorizeLoginUsing(None);
    assert!(
        first
            .allowsLogin(&mut request, &user, &passkey)
            .await
            .unwrap()
    );
    first.ignoreRoutes();
    assert!(!first.shouldRegisterRoutes());
    assert!(second.shouldRegisterRoutes());
}

#[test]
fn passkey_metadata_keeps_server_owned_foreign_key_out_of_mass_assignment() {
    let record =
        Passkey::newInstance([("name".into(), "Key".into()), ("user_id".into(), 99.into())].into())
            .unwrap();
    assert_eq!(record.getAttribute("name"), Some(&Value::from("Key")));
    assert_eq!(record.getAttribute("user_id"), None);
    assert_eq!(record.getTable(), "passkeys");
}
