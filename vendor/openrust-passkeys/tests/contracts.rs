use openrust::database::{
    Value,
    eloquent::{Model, ModelState},
};
use openrust_passkeys::{Passkey, PasskeyAuthenticatable, Passkeys};
use std::sync::Arc;

mod user {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../app/models/user.rs"
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
                user.set_attribute(key, value);
            }
        }
        assert_eq!(
            user.get_passkey_user_handle(input["secret"].as_str().unwrap())
                .unwrap(),
            case["handle"]
        );
        assert_eq!(user.get_passkey_display_name().unwrap(), case["display"]);
        assert_eq!(user.get_passkey_username().unwrap(), case["username"]);
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
    let passkey = openrust_passkeys::PasskeyRecord::new(Passkey::default());
    assert!(
        first
            .allows_login(&mut request, &user, &passkey)
            .await
            .unwrap()
    );
    first.authorize_login_using(Some(Arc::new(|request, _, _| {
        Box::pin(async move {
            assert_eq!(request.method(), &openrust::http::Method::GET);
            Ok(false)
        })
    })));
    assert!(
        !first
            .allows_login(&mut request, &user, &passkey)
            .await
            .unwrap()
    );
    assert!(
        second
            .allows_login(&mut request, &user, &passkey)
            .await
            .unwrap()
    );
    first.authorize_login_using(None);
    assert!(
        first
            .allows_login(&mut request, &user, &passkey)
            .await
            .unwrap()
    );
    first.ignore_routes();
    assert!(!first.should_register_routes());
    assert!(second.should_register_routes());
}

#[test]
fn passkey_metadata_keeps_server_owned_foreign_key_out_of_mass_assignment() {
    let record = Passkey::new_instance(
        [("name".into(), "Key".into()), ("user_id".into(), 99.into())].into(),
    )
    .unwrap();
    assert_eq!(record.get_attribute("name"), Some(&Value::from("Key")));
    assert_eq!(record.get_attribute("user_id"), None);
    assert_eq!(record.get_table(), "passkeys");
}

#[derive(Clone, Default)]
struct SelectedPasskey(Passkey);
impl Model for SelectedPasskey {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static META: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("selected_passkeys").primary_key("pk");
        &META
    }
    fn from_state(state: ModelState) -> Self {
        Self(Passkey::from_state(state))
    }
    fn state(&self) -> &ModelState {
        self.0.state()
    }
    fn state_mut(&mut self) -> &mut ModelState {
        self.0.state_mut()
    }
}
impl openrust_passkeys::PasskeyModel for SelectedPasskey {}

#[tokio::test]
async fn selected_models_are_application_local_and_records_retain_the_actual_instance() {
    let config = Arc::new(openrust::config::Repository::new(Default::default()));
    let first = Passkeys::new(config.clone());
    let second = Passkeys::new(config);
    first.use_passkey_model::<SelectedPasskey>();
    assert_eq!(
        first.passkey_model(),
        std::any::type_name::<SelectedPasskey>()
    );
    assert_eq!(second.passkey_model(), std::any::type_name::<Passkey>());
    let record = openrust_passkeys::PasskeyRecord::new(SelectedPasskey::default());
    let shared = record.clone();
    assert!(record.same_instance(&shared));
    assert!(record.model::<Passkey>().is_none());
    let model = record.model::<SelectedPasskey>().unwrap();
    assert!(Arc::ptr_eq(
        &model,
        &shared.model::<SelectedPasskey>().unwrap()
    ));
    model.lock().await.set_attribute("pk", 42_i64);
    assert_eq!(shared.get_key().await, Some(42.into()));
    assert_eq!(shared.to_array().await.unwrap()["pk"], 42);
    first.use_passkey_model::<Passkey>();
    assert_eq!(first.passkey_model(), second.passkey_model());
    assert_eq!(record.get_key().await, Some(42.into()));
}
