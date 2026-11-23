use super::*;
use openrust::database::eloquent::relations::Relation as _;
use openrust::database::eloquent::{
    Observer,
    casts::AttributeCast,
    concerns::{ModelEvent, ModelEventResponse},
};

macro_rules! grant_model {
    ($name:ident, $table:literal, $base:ty) => {
        struct $name(ModelState);
        impl Model for $name {
            fn metadata() -> &'static ModelMetadata {
                static META: ModelMetadata = ModelMetadata::new($table)
                    .connection("main")
                    .incrementing(false)
                    .guarded(&[])
                    .hidden(&["secret"])
                    .timestamps(None, None);
                &META
            }
            fn casts() -> &'static [(&'static str, AttributeCast)] {
                <$base>::casts()
            }
            fn get_key_type(&self) -> openrust::database::eloquent::ModelKeyType {
                openrust::database::eloquent::ModelKeyType::String
            }
            fn from_state(s: ModelState) -> Self {
                Self(s)
            }
            fn state(&self) -> &ModelState {
                &self.0
            }
            fn state_mut(&mut self) -> &mut ModelState {
                &mut self.0
            }
        }
    };
}
grant_model!(CustomCode, "custom_codes", openrust_passport::AuthCode);
grant_model!(
    CustomDevice,
    "custom_devices",
    openrust_passport::DeviceCode
);
struct CustomClient(ModelState);
impl Model for CustomClient {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("custom_clients")
            .connection("main")
            .unique_id(openrust::database::eloquent::UniqueId::Uuid)
            .guarded(&[])
            .hidden(&["secret"]);
        &META
    }
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        openrust_passport::Client::casts()
    }
    fn get_key_type(&self) -> openrust::database::eloquent::ModelKeyType {
        openrust::database::eloquent::ModelKeyType::String
    }
    fn from_state(s: ModelState) -> Self {
        Self(s)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}
impl openrust_passport::ClientModel for CustomClient {}
struct Gate(Arc<std::sync::atomic::AtomicBool>);
impl<M: Model> Observer<M> for Gate {
    const EVENTS: &'static [ModelEvent] = &[ModelEvent::Saving];
    fn saving<'a>(
        &'a self,
        _: &'a mut M,
        _: &'a Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<ModelEventResponse>,
    > {
        Box::pin(async move {
            Ok(if self.0.load(Ordering::Relaxed) {
                ModelEventResponse::Cancel
            } else {
                ModelEventResponse::Continue
            })
        })
    }
}
const TABLES: &[(&str, &str)] = &[
    ("oauth_clients", "custom_clients"),
    ("oauth_auth_codes", "custom_codes"),
    ("oauth_device_codes", "custom_devices"),
    ("oauth_access_tokens", "custom_access"),
    ("oauth_refresh_tokens", "custom_refresh"),
];
async fn select(f: &Fixture) {
    for (old, new) in TABLES {
        f.db.get_schema_builder().rename(old, new).await.unwrap();
    }
    f.passport.useClientModel::<CustomClient>();
    f.passport.useTokenModel::<AlternateAccess>();
    f.passport.useRefreshTokenModel::<AlternateRefresh>();
    f.passport.useAuthCodeModel::<CustomCode>();
    f.passport.useDeviceCodeModel::<CustomDevice>();
}
async fn restore(f: &Fixture) {
    for (old, new) in TABLES.iter().rev() {
        f.db.get_schema_builder().rename(new, old).await.unwrap();
    }
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_all_five_selected_models_issue_refresh_revoke_purge_and_stay_application_local() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let other = Fixture::new().await;
    let device = f.device_client(false).await;
    select(&f).await;
    assert!(f.passport.authCodeModel().ends_with("CustomCode"));
    assert!(f.passport.deviceCodeModel().ends_with("CustomDevice"));
    assert_eq!(
        other.passport.authCodeModel(),
        std::any::type_name::<openrust_passport::AuthCode>()
    );
    assert_eq!(
        other.passport.deviceCodeModel(),
        std::any::type_name::<openrust_passport::DeviceCode>()
    );
    f.kernel
        .get_application()
        .call(
            openrust::container::Callback::new(&[], |_| async {
                let clients = openrust::container::Container::get_instance()
                    .make::<Passport>()
                    .unwrap()
                    .clientRepository();
                let client = clients
                    .createAuthorizationCodeGrantClient(
                        "Compiled custom client",
                        &[CALLBACK.to_owned()],
                        false,
                        None,
                        false,
                    )
                    .await
                    .unwrap_or_else(|error| match error {
                        openrust::database::DatabaseError::Driver { source, .. } => {
                            panic!("Custom public client SQL: {source}")
                        }
                        error => panic!("{error}"),
                    });
                assert!(client.model::<CustomClient>().is_some());
                assert!(client.get_key().await.is_some());
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(f.db.table("custom_clients").count("*").await.unwrap(), 4);
    let data = f.issued().await;
    use openrust_passport::ClientModel as _;
    let public = f.public.clone();
    f.kernel
        .get_application()
        .call(
            openrust::container::Callback::new(&[], move |_| async move {
                let client = CustomClient::query()
                    .unwrap()
                    .find(public)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    client
                        .authCodesFor::<CustomCode>()
                        .get_results(&client)
                        .await
                        .unwrap()
                        .len(),
                    1
                );
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();

    assert_eq!(
        f.resource_token("/resource", data["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    assert_eq!(
        other
            .resource_token("/resource", data["access_token"].as_str().unwrap())
            .await
            .status(),
        401
    );
    let refreshed = f
        .refresh(data["refresh_token"].as_str().unwrap(), None)
        .await;
    assert_eq!(refreshed.status(), 200);
    let refreshed = json_body(refreshed).await;
    assert_eq!(
        f.resource_token("/resource", data["access_token"].as_str().unwrap())
            .await
            .status(),
        401
    );
    let codes = f.device_code(&device, "read").await;
    let b = f.login("alice@test").await;
    let view = f
        .device_view(&b, codes["user_code"].as_str().unwrap())
        .await;
    assert_eq!(f.device_decide(&b, &view, true).await.status(), 302);
    let token = f
        .device_poll(&device, codes["device_code"].as_str().unwrap())
        .await;
    assert_eq!(token.status(), 200);
    let token = json_body(token).await;
    assert_eq!(
        f.resource_token("/resource", token["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    assert_eq!(
        f.device_poll(&device, codes["device_code"].as_str().unwrap())
            .await
            .status(),
        400
    );
    assert_eq!(f.purge(&["--revoked"]).await.0, 0);
    assert_eq!(f.db.table("custom_codes").count("*").await.unwrap(), 0);
    assert_eq!(f.db.table("custom_devices").count("*").await.unwrap(), 0);
    assert_eq!(f.db.table("custom_access").count("*").await.unwrap(), 2);
    assert_eq!(
        f.resource_token("/resource", refreshed["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    let independent = other.issued().await;
    assert_eq!(
        other
            .resource_token("/resource", independent["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    restore(&f).await;
    other.cleanup().await;
}
#[tokio::test]
async fn mysql_selected_auth_and_device_model_events_cancel_real_transactions_and_retry() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let device = f.device_client(false).await;
    select(&f).await;
    let gate = Arc::new(std::sync::atomic::AtomicBool::new(false));
    CustomCode::observe(f.kernel.get_application(), Gate(gate.clone())).unwrap();
    let b = f.login("alice@test").await;
    let view = f.authorize(&b, &f.public, true).await;
    gate.store(true, Ordering::Relaxed);
    let denied = f
        .wire(
            reqwest::Method::POST,
            "/oauth/authorize",
            &b.cookie,
            Some(json!({"auth_token":view["auth_token"],"_token":b.csrf})),
        )
        .await;
    assert_eq!(denied.status(), 500);
    assert_eq!(f.db.table("custom_codes").count("*").await.unwrap(), 0);
    gate.store(false, Ordering::Relaxed);
    let view = f.authorize(&b, &f.public, true).await;
    let code = f.approve(&b, &view).await;
    gate.store(true, Ordering::Relaxed);
    assert_eq!(
        f.token(&f.public, &code, Some(VERIFIER), None)
            .await
            .status(),
        500
    );
    assert_eq!(f.db.table("custom_access").count("*").await.unwrap(), 0);
    assert_eq!(f.db.table("custom_refresh").count("*").await.unwrap(), 0);
    gate.store(false, Ordering::Relaxed);
    assert_eq!(
        f.token(&f.public, &code, Some(VERIFIER), None)
            .await
            .status(),
        200
    );
    CustomDevice::observe(f.kernel.get_application(), Gate(gate.clone())).unwrap();
    gate.store(true, Ordering::Relaxed);
    assert_eq!(
        f.wire(
            reqwest::Method::POST,
            "/oauth/device/code",
            "",
            Some(json!({"client_id":device.id,"scope":"read"}))
        )
        .await
        .status(),
        500
    );
    assert_eq!(f.db.table("custom_devices").count("*").await.unwrap(), 0);
    gate.store(false, Ordering::Relaxed);
    let codes = f.device_code(&device, "read").await;
    let view = f
        .device_view(&b, codes["user_code"].as_str().unwrap())
        .await;
    assert_eq!(f.device_decide(&b, &view, true).await.status(), 302);
    gate.store(true, Ordering::Relaxed);
    assert_eq!(
        f.device_poll(&device, codes["device_code"].as_str().unwrap())
            .await
            .status(),
        500
    );
    assert_eq!(f.db.table("custom_access").count("*").await.unwrap(), 1);
    gate.store(false, Ordering::Relaxed);
    assert_eq!(
        f.device_poll(&device, codes["device_code"].as_str().unwrap())
            .await
            .status(),
        200
    );
    restore(&f).await;
}

#[tokio::test]
async fn mysql_custom_token_route_and_provider_keep_default_routes_suppressed() {
    if !required() {
        return;
    }
    let f = Fixture::with_device_routes(true, Some("disabled/oauth"), false).await;
    f.passport.enablePasswordGrant();
    let id = f
        .kernel
        .get_application()
        .call(
            openrust::container::Callback::new(&[], |_| async {
                let clients = openrust::container::Container::get_instance()
                    .make::<ClientRepository>()
                    .unwrap();
                let client = clients
                    .createPasswordGrantClient("Custom route", Some("users"), false)
                    .await
                    .unwrap();
                Ok(AuthIdentifier::try_from(&client.get_key().await.unwrap())
                    .unwrap()
                    .to_string())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    let body = json!({"grant_type":"password","client_id":id,"username":"alice@test","password":"password","scope":"read"});
    assert_eq!(
        f.wire(
            reqwest::Method::POST,
            "/oauth/token",
            "",
            Some(body.clone())
        )
        .await
        .status(),
        404
    );
    assert_eq!(
        f.wire(
            reqwest::Method::POST,
            "/disabled/oauth/token",
            "",
            Some(body.clone())
        )
        .await
        .status(),
        404
    );
    let response = f
        .wire(reqwest::Method::POST, "/custom-token", "", Some(body))
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let response = json_body(response).await;
    let bearer = response["access_token"].as_str().unwrap();
    assert_eq!(f.resource_token("/resource", bearer).await.status(), 200);
    assert_eq!(
        f.resource_token("/resource-other", bearer).await.status(),
        401
    );
    let refresh=f.wire(reqwest::Method::POST,"/custom-token","",Some(json!({"grant_type":"refresh_token","client_id":id,"refresh_token":response["refresh_token"]}))).await;
    assert_eq!(refresh.status(), 200);
    assert_eq!(f.resource_token("/resource", bearer).await.status(), 401);
    f.cleanup().await;
}

struct ForeignCode(ModelState);
impl Model for ForeignCode {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("custom_codes")
            .connection("foreign")
            .incrementing(false)
            .guarded(&[])
            .timestamps(None, None);
        &META
    }
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        openrust_passport::AuthCode::casts()
    }
    fn from_state(s: ModelState) -> Self {
        Self(s)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}
#[tokio::test]
async fn mysql_custom_grant_storage_failure_and_cross_connection_transaction_are_explicit_and_retryable()
 {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let device = f.device_client(false).await;
    f.passport.useAuthCodeModel::<CustomCode>();
    f.passport.useDeviceCodeModel::<CustomDevice>();
    let b = f.login("alice@test").await;
    let view = f.authorize(&b, &f.public, true).await;
    assert_eq!(
        f.wire(
            reqwest::Method::POST,
            "/oauth/authorize",
            &b.cookie,
            Some(json!({"auth_token":view["auth_token"],"_token":b.csrf}))
        )
        .await
        .status(),
        500
    );
    assert_eq!(
        f.wire(
            reqwest::Method::POST,
            "/oauth/device/code",
            "",
            Some(json!({"client_id":device.id}))
        )
        .await
        .status(),
        500
    );
    let app = f.kernel.get_application();
    app.container().get::<dyn openrust::contracts::config::Repository>("config").unwrap().set("database.connections.foreign",json!({"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap(),"prefix":format!("p09_foreign_{}_",std::process::id())})).unwrap();
    let foreign = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(Some("foreign"))
        .unwrap();
    foreign
        .get_schema_builder()
        .create("custom_codes", |t| {
            t.string("id", 100).primary();
            t.bigInteger("user_id");
            t.string("client_id", 36);
            t.text("scopes");
            t.boolean("revoked");
            t.date_time("expires_at", 0);
        })
        .await
        .unwrap();
    f.passport.useAuthCodeModel::<ForeignCode>();
    let view = f.authorize(&b, &f.public, true).await;
    assert_eq!(
        f.wire(
            reqwest::Method::POST,
            "/oauth/authorize",
            &b.cookie,
            Some(json!({"auth_token":view["auth_token"],"_token":b.csrf}))
        )
        .await
        .status(),
        500
    );
    assert_eq!(foreign.table("custom_codes").count("*").await.unwrap(), 0);
    assert_eq!(f.db.table("oauth_auth_codes").count("*").await.unwrap(), 0);
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        0
    );
    f.passport.useAuthCodeModel::<openrust_passport::AuthCode>();
    f.passport
        .useDeviceCodeModel::<openrust_passport::DeviceCode>();
    assert!(f.issued().await["access_token"].is_string());
    assert!(f.device_code(&device, "read").await["device_code"].is_string());
    foreign
        .get_schema_builder()
        .drop_if_exists("custom_codes")
        .await
        .unwrap();
    f.cleanup().await;
}
