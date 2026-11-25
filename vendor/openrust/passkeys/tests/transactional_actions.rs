#[path = "support/authenticator.rs"]
mod authenticator;
use openrust::{
    contracts::{
        auth::Authenticatable as _,
        events::{Dispatcher as _, Listener},
    },
    database::{
        DatabaseFuture, DatabaseResult, Value,
        eloquent::{
            Model, ModelMetadata, ModelState, UniqueId,
            casts::{Attribute, AttributeCast},
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
        },
    },
    events::{EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::Application,
    support::facades::{App, Facade as _},
};
use openrust_passkeys::{
    Passkey, PasskeyAuthenticatable, PasskeyModel, Passkeys,
    actions::{
        DeletePasskey, GenerateRegistrationOptions, GenerateVerificationOptions, StorePasskey,
        VerifyPasskey,
    },
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
#[derive(Clone)]
struct User(ModelState);
impl Model for User {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("tx_users")
            .primary_key("key")
            .incrementing(false)
            .timestamps(None, None);
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
}
impl openrust::contracts::auth::Authenticatable for User {
    fn get_auth_identifier_name(&self) -> &str {
        self.get_key_name()
    }
    fn get_auth_identifier(&self) -> openrust::auth::AuthIdentifier {
        self.get_key().unwrap().try_into().unwrap()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        None
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
}
impl PasskeyAuthenticatable for User {}
#[derive(Clone)]
struct Key(ModelState);
impl Model for Key {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("tx_passkeys")
            .primary_key("key")
            .unique_id(UniqueId::Ulid)
            .fillable(&["name", "credential_id", "credential"]);
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
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        Passkey::casts()
    }
    fn attribute(name: &str) -> Option<Attribute> {
        if name == "name" {
            Some(Attribute::set(|value, _| {
                let Value::Text(value) = value else {
                    return Err(openrust::database::DatabaseError::InvalidQuery(
                        "测试通行密钥名称",
                    ));
                };
                Ok([("name".into(), value.to_uppercase().into())].into())
            }))
        } else {
            None
        }
    }
}
impl PasskeyModel for Key {}
#[derive(Clone)]
struct NoOpSave(ModelState);
impl Model for NoOpSave {
    fn metadata() -> &'static ModelMetadata {
        Key::metadata()
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
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        Passkey::casts()
    }
    async fn save_in_transaction<'a>(
        &'a mut self,
        _: &'a mut openrust::database::Transaction,
    ) -> DatabaseResult<bool> {
        Ok(true)
    }
}
impl PasskeyModel for NoOpSave {}
#[derive(Clone)]
struct NoOpDelete(ModelState);
impl Model for NoOpDelete {
    fn metadata() -> &'static ModelMetadata {
        Key::metadata()
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
    async fn delete_in_transaction<'a>(
        &'a mut self,
        _: &'a mut openrust::database::Transaction,
    ) -> DatabaseResult<bool> {
        Ok(true)
    }
}
impl PasskeyModel for NoOpDelete {}
#[derive(Clone, Default)]
struct Observe {
    cancel_save: Arc<AtomicBool>,
    cancel_delete: Arc<AtomicBool>,
    events: Arc<Mutex<Vec<&'static str>>>,
}
impl ModelEventListener<Key> for Observe {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        _: &'a mut Key,
        _: &'a openrust::database::Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            Ok(
                if (event == ModelEvent::Saving && self.cancel_save.load(Ordering::SeqCst))
                    || (event == ModelEvent::Deleting && self.cancel_delete.load(Ordering::SeqCst))
                {
                    ModelEventResponse::Cancel
                } else {
                    ModelEventResponse::Continue
                },
            )
        })
    }
}
impl Listener for Observe {
    fn handle<'a>(
        &'a self,
        event: &'a dyn openrust::contracts::events::Event,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            macro_rules! check {
                ($kind:ty,$name:literal) => {
                    if let Some(event) = event.downcast_ref::<$kind>() {
                        let user = event.user::<User>(context).unwrap();
                        assert_eq!(user.get_auth_identifier(), event.user);
                        let model = event.passkey::<Key>(context).unwrap();
                        assert!(model.get_key().is_some());
                        assert!(event.passkey.model::<Key>().is_some());
                        self.events.lock().unwrap().push($name);
                    }
                };
            }
            check!(openrust_passkeys::events::PasskeyRegistered, "registered");
            check!(openrust_passkeys::events::PasskeyVerified, "verified");
            check!(openrust_passkeys::events::PasskeyDeleted, "deleted");
            Ok(None)
        })
    }
}
#[tokio::test]
async fn native_transaction_actions_verify_real_signatures_and_preserve_selected_model_lifecycle() {
    let root =
        std::path::PathBuf::from(std::env::var_os("TMPDIR").expect("必须使用项目内临时目录"))
            .join(format!("passkey-transaction-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let connection = match std::env::var("OPENRUST_PASSKEY_PURGE_DRIVER")
        .as_deref()
        .unwrap_or("sqlite")
    {
        "sqlite" => json!({"driver":"sqlite","database":":memory:"}),
        driver @ ("mysql" | "pgsql") => {
            json!({"driver":driver,"url":std::env::var("OPENRUST_PASSKEY_PURGE_URL").expect("必须提供独立验收数据库")})
        }
        _ => panic!("不支持的通行密钥事务验收数据库"),
    };
    std::fs::write(
        root.join("database.json"),
        serde_json::to_vec(&connection).unwrap(),
    )
    .unwrap();
    let mut app=Application::configure(root.clone()).with_config(|root,_|{let connection:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("database.json")).unwrap()).unwrap();Ok([("app".into(),json!({"key":"x".repeat(32)})),("database".into(),json!({"default":"primary","connections":{"primary":connection,"other":connection}})),("passkeys".into(),json!({"relying_party_id":"localhost","allowed_origins":["http://localhost"],"user_handle_secret":"test-secret"}))].into())}).create().unwrap();
    app.boot().await.unwrap();
    let observer = Observe::default();
    for event in [ModelEvent::Saving, ModelEvent::Deleting] {
        Key::register_model_event(&app, event, observer.clone()).unwrap();
    }
    app.make::<openrust::events::Dispatcher>()
        .await
        .unwrap()
        .listen(
            "*",
            Arc::new(observer.clone()),
            openrust::events::Delivery::Sync,
        )
        .unwrap();
    App::set_facade_application(Some(&app), async {
        let manager = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        let connection = manager.connection(None).unwrap();
        let mut schema = connection.get_schema_builder();
        schema
            .create("tx_users", |t| {
                t.string("key", 64).primary();
            })
            .await
            .unwrap();
        schema
            .create("tx_passkeys", |t| {
                t.ulid("key").primary();
                t.string("user_id", 64);
                t.string("name", 255);
                t.string("credential_id", 255).unique();
                t.text("credential");
                t.timestamp("last_used_at", None).nullable();
                t.timestamps();
            })
            .await
            .unwrap();
        let mut user = User::from_state(Default::default());
        user.set_attribute("key", "018fc492-eac1-7327-82c2-18b091752744");
        user.save().await.unwrap();
        let mut neighbour = User::from_state(Default::default());
        neighbour.set_attribute("key", "001");
        neighbour.save().await.unwrap();
        let service = Arc::new(Passkeys::new(
            app.make::<dyn openrust::contracts::config::Repository>()
                .await
                .unwrap(),
        ));
        service.use_user_model::<User>();
        service.use_passkey_model::<Key>();
        let signing = authenticator::Authenticator::new(11);
        let options = GenerateRegistrationOptions(service.clone())
            .call(&user)
            .await
            .unwrap();
        let registration = signing.register(options.options());
        let store = StorePasskey(service.clone());
        let verify = VerifyPasskey(service.clone());
        let mut wrong = manager
            .connection(Some("other"))
            .unwrap()
            .begin_transaction()
            .await
            .unwrap();
        assert!(
            store
                .call_in_transaction(&user, &mut wrong, "phone", registration.clone(), &options)
                .await
                .is_err()
        );
        wrong.roll_back(None).await.unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        user.refresh_for_update(&mut tx).await.unwrap();
        let record = store
            .call_in_transaction(&user, &mut tx, "phone", registration.clone(), &options)
            .await
            .unwrap();
        assert_eq!(
            record.get_attribute_value("name").await.unwrap(),
            Some("PHONE".into())
        );
        assert!(matches!(record.get_key().await, Some(Value::Text(_))));
        tx.roll_back(None).await.unwrap();
        assert_eq!(connection.table("tx_passkeys").count("*").await.unwrap(), 0);
        let mut tx = connection.begin_transaction().await.unwrap();
        user.refresh_for_update(&mut tx).await.unwrap();
        let record = store
            .call_in_transaction(&user, &mut tx, "phone", registration, &options)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let key = record.get_key().await.unwrap();
        assert_eq!(
            service
                .user_for(&record)
                .await
                .unwrap()
                .unwrap()
                .get_auth_identifier(),
            user.get_auth_identifier()
        );
        let options = GenerateVerificationOptions(service.clone())
            .call(Some(&user))
            .await
            .unwrap();
        let handle = user.get_passkey_user_handle("test-secret").unwrap();
        let assertion = signing.assertion(options.options(), &handle, 1);
        assert_eq!(
            verify
                .get_passkey(&assertion)
                .await
                .unwrap()
                .get_key()
                .await,
            Some(key.clone())
        );
        let mut tx = connection.begin_transaction().await.unwrap();
        assert!(
            verify
                .call_in_transaction(&neighbour, &mut tx, assertion.clone(), &options)
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        user.refresh_for_update(&mut tx).await.unwrap();
        let checked = verify
            .call_in_transaction(&user, &mut tx, assertion.clone(), &options)
            .await
            .unwrap();
        assert_eq!(checked.credential().await.unwrap().counter(), 1);
        tx.roll_back(None).await.unwrap();
        assert_eq!(
            Key::find(key.clone())
                .await
                .unwrap()
                .unwrap()
                .credential()
                .unwrap()
                .counter(),
            0
        );
        let mut tx = connection.begin_transaction().await.unwrap();
        user.refresh_for_update(&mut tx).await.unwrap();
        verify
            .call_discoverable_in_transaction(&user, &mut tx, assertion.clone(), &options)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        assert!(
            verify
                .call_in_transaction(&user, &mut tx, assertion, &options)
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        let options = GenerateVerificationOptions(service.clone())
            .call(None)
            .await
            .unwrap();
        let invalid = signing.assertion(options.options(), "another-user", 2);
        let mut tx = connection.begin_transaction().await.unwrap();
        assert!(
            verify
                .call_discoverable_in_transaction(&user, &mut tx, invalid, &options)
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        observer.cancel_save.store(true, Ordering::SeqCst);
        let assertion = signing.assertion(options.options(), &handle, 2);
        let mut tx = connection.begin_transaction().await.unwrap();
        assert!(
            verify
                .call_in_transaction(&user, &mut tx, assertion, &options)
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        assert_eq!(
            Key::find(key.clone())
                .await
                .unwrap()
                .unwrap()
                .credential()
                .unwrap()
                .counter(),
            1
        );
        let second = authenticator::Authenticator::new(12);
        let options = GenerateRegistrationOptions(service.clone())
            .call(&user)
            .await
            .unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        assert!(
            store
                .call_in_transaction(
                    &user,
                    &mut tx,
                    "cancel",
                    second.register(options.options()),
                    &options
                )
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        assert_eq!(connection.table("tx_passkeys").count("*").await.unwrap(), 1);
        observer.cancel_save.store(false, Ordering::SeqCst);
        service.use_passkey_model::<NoOpSave>();
        let fresh = authenticator::Authenticator::new(14);
        let registration = GenerateRegistrationOptions(service.clone())
            .call(&user)
            .await
            .unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        let failure = store
            .call_in_transaction(
                &user,
                &mut tx,
                "no-op",
                fresh.register(registration.options()),
                &registration,
            )
            .await
            .err()
            .unwrap();
        let openrust::http::HttpError::Internal { source } = failure else {
            panic!("空保存应返回持久化错误")
        };
        assert!(source.to_string().contains("持久化"));
        tx.roll_back(None).await.unwrap();

        let options = GenerateVerificationOptions(service.clone())
            .call(None)
            .await
            .unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        assert!(
            verify
                .call_in_transaction(
                    &user,
                    &mut tx,
                    signing.assertion(options.options(), &handle, 2),
                    &options
                )
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        service.use_passkey_model::<Key>();
        let no_op = openrust_passkeys::PasskeyRecord::new(
            NoOpDelete::find(key.clone()).await.unwrap().unwrap(),
        );
        let mut tx = connection.begin_transaction().await.unwrap();
        assert!(
            DeletePasskey
                .call_in_transaction(&user, &mut tx, &no_op)
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        let record =
            openrust_passkeys::PasskeyRecord::new(Key::find(key.clone()).await.unwrap().unwrap());
        observer.cancel_delete.store(true, Ordering::SeqCst);
        let mut tx = connection.begin_transaction().await.unwrap();
        assert!(
            DeletePasskey
                .call_in_transaction(&user, &mut tx, &record)
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        observer.cancel_delete.store(false, Ordering::SeqCst);
        let mut tx = connection.begin_transaction().await.unwrap();
        assert!(
            DeletePasskey
                .call_in_transaction(&neighbour, &mut tx, &record)
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        DeletePasskey
            .call_in_transaction(&user, &mut tx, &record)
            .await
            .unwrap();
        tx.roll_back(None).await.unwrap();
        assert!(Key::find(key.clone()).await.unwrap().is_some());
        let record = openrust_passkeys::PasskeyRecord::new(Key::find(key).await.unwrap().unwrap());
        let mut tx = connection.begin_transaction().await.unwrap();
        DeletePasskey
            .call_in_transaction(&user, &mut tx, &record)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(connection.table("tx_passkeys").count("*").await.unwrap(), 0);
        let native = authenticator::Authenticator::new(13);
        let options = GenerateRegistrationOptions(service.clone())
            .call(&user)
            .await
            .unwrap();
        let record = store
            .call(
                &user,
                "native",
                native.register(options.options()),
                &options,
            )
            .await
            .unwrap();
        let options = GenerateVerificationOptions(service.clone())
            .call(None)
            .await
            .unwrap();
        let verified = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            verify.call(
                native.assertion(options.options(), &handle, 1),
                &options,
                None,
            ),
        )
        .await
        .expect("原生SQLite验证不得在凭据事务内死锁")
        .unwrap();
        assert_eq!(verified.get_key().await, record.get_key().await);
        DeletePasskey.call(&user, &verified).await.unwrap();
        {
            let events = observer.events.lock().unwrap();
            for name in ["registered", "verified", "deleted"] {
                assert!(events.contains(&name));
            }
        }
        schema.drop_if_exists("tx_passkeys").await.unwrap();
        schema.drop_if_exists("tx_users").await.unwrap();
    })
    .await;
    std::fs::remove_dir_all(root).unwrap();
}
