use openrust::{
    database::{
        DatabaseFuture, DatabaseResult, Value,
        eloquent::{
            Model, ModelMetadata, ModelState, UniqueId,
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
        },
    },
    foundation::Application,
    support::facades::{App, Facade as _},
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
#[derive(Clone)]
struct SoftKey(ModelState);
impl Model for SoftKey {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("purge_passkeys")
            .primary_key("key")
            .unique_id(UniqueId::Ulid)
            .soft_deletes("deleted_at");
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
impl openrust_passkeys::PasskeyModel for SoftKey {}
#[derive(Clone)]
struct NoOpKey(ModelState);
impl Model for NoOpKey {
    fn metadata() -> &'static ModelMetadata {
        SoftKey::metadata()
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
    async fn force_delete_in_transaction<'a>(
        &'a mut self,
        _: &'a mut openrust::database::Transaction,
    ) -> DatabaseResult<bool> {
        Ok(true)
    }
}
impl openrust_passkeys::PasskeyModel for NoOpKey {}
#[derive(Clone)]
struct Events {
    cancel: Arc<AtomicBool>,
    events: Arc<Mutex<Vec<ModelEvent>>>,
}
impl ModelEventListener<SoftKey> for Events {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut SoftKey,
        _: &'a openrust::database::Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            self.events.lock().unwrap().push(event);
            Ok(
                if event == ModelEvent::ForceDeleting
                    && model.get_attribute("name") == Some(&Value::Text("block".into()))
                    && self.cancel.load(Ordering::SeqCst)
                {
                    ModelEventResponse::Cancel
                } else {
                    ModelEventResponse::Continue
                },
            )
        })
    }
}
#[tokio::test]
async fn transactional_passkey_purge_respects_models_ownership_and_rollback() {
    let root = std::path::PathBuf::from(
        std::env::var_os("TMPDIR").expect("必须使用项目内临时目录"),
    )
    .join(format!(
        "passkey-purge-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let settings = match std::env::var("OPENRUST_PASSKEY_PURGE_DRIVER")
        .as_deref()
        .unwrap_or("sqlite")
    {
        "sqlite" => json!({"driver":"sqlite","database":":memory:"}),
        driver @ ("mysql" | "pgsql") => {
            json!({"driver":driver,"url":std::env::var("OPENRUST_PASSKEY_PURGE_URL").expect("必须提供独立通行密钥清理验收数据库")})
        }
        _ => panic!("不支持的令牌清理验收数据库"),
    };
    std::fs::write(
        root.as_path().join("database.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    let mut app=Application::configure(root.as_path().to_path_buf()).with_config(|base,_|Ok([
  ("app".into(),json!({"key":"s".repeat(32),"url":"http://localhost"})),
  ("database".into(),json!({"default":"purge","connections":{"purge":serde_json::from_slice::<serde_json::Value>(&std::fs::read(base.join("database.json")).unwrap()).unwrap()}})),
 ].into())).create().unwrap();
    app.boot().await.unwrap();
    let events = Events {
        cancel: Arc::new(AtomicBool::new(true)),
        events: Default::default(),
    };
    for event in [
        ModelEvent::ForceDeleting,
        ModelEvent::Deleting,
        ModelEvent::Deleted,
        ModelEvent::ForceDeleted,
    ] {
        SoftKey::register_model_event(&app, event, events.clone()).unwrap();
    }
    App::set_facade_application(Some(&app), async {
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        let conn = db.connection(None).unwrap();
        let mut schema = conn.get_schema_builder();
        schema
            .create("purge_passkeys", |table| {
                table.ulid("key").primary();
                table.string("user_id", 64);
                table.string("name", 255);
                table.timestamps();
                table.soft_deletes();
            })
            .await
            .unwrap();
        let keys = openrust_passkeys::Passkeys::new(Arc::new(openrust::config::Repository::new(
            Default::default(),
        )));
        keys.use_user_model::<User>();
        keys.use_passkey_model::<SoftKey>();
        schema
            .create("purge_users", |table| {
                table.string("key", 64).primary();
            })
            .await
            .unwrap();
        let mut user = User::from_state(Default::default());
        user.set_attribute("key", "owner-a");
        user.save().await.unwrap();
        let unregistered = openrust_passkeys::Passkeys::new(Arc::new(
            openrust::config::Repository::new(Default::default()),
        ));
        let mut tx = conn.begin_transaction().await.unwrap();
        assert!(
            unregistered
                .purge_user_in_transaction(&mut tx, &user)
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        let mut changed = user.clone();
        changed.set_attribute("key", "owner-b");
        let mut tx = conn.begin_transaction().await.unwrap();
        assert!(
            keys.purge_user_in_transaction(&mut tx, &changed)
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        keys.use_passkey_model::<openrust_passkeys::Passkey>();
        let mut tx = conn.begin_transaction().await.unwrap();
        assert_eq!(
            keys.purge_user_in_transaction(&mut tx, &user)
                .await
                .unwrap(),
            0
        );
        tx.roll_back(None).await.unwrap();
        keys.use_passkey_model::<SoftKey>();
        for (owner, name) in [
            ("owner-a", "old"),
            ("owner-a", "current"),
            ("owner-a", "block"),
            ("owner-b", "neighbour"),
            ("OWNER-A", "case-neighbour"),
        ] {
            let mut key = SoftKey::from_state(Default::default());
            key.force_fill(
                [
                    ("user_id".into(), owner.into()),
                    ("name".into(), name.into()),
                ]
                .into(),
            )
            .unwrap();
            key.save().await.unwrap();
            if name == "old" {
                assert!(key.delete().await.unwrap());
            }
        }
        for index in 0..102 {
            let mut key = SoftKey::from_state(Default::default());
            key.force_fill(
                [
                    ("user_id".into(), "owner-a".into()),
                    ("name".into(), format!("bulk-{index}").into()),
                ]
                .into(),
            )
            .unwrap();
            key.save().await.unwrap();
        }
        keys.use_passkey_model::<NoOpKey>();
        let mut tx = conn.begin_transaction().await.unwrap();
        assert!(
            keys.purge_user_in_transaction(&mut tx, &user)
                .await
                .unwrap_err()
                .to_string()
                .contains("物理清理")
        );
        tx.roll_back(None).await.unwrap();
        keys.use_passkey_model::<SoftKey>();
        let mut tx = conn.begin_transaction().await.unwrap();
        assert!(
            keys.purge_user_in_transaction(&mut tx, &user)
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        assert_eq!(conn.table("purge_passkeys").count("*").await.unwrap(), 107);
        events.cancel.store(false, Ordering::SeqCst);
        events.events.lock().unwrap().clear();
        let mut tx = conn.begin_transaction().await.unwrap();
        assert_eq!(
            keys.purge_user_in_transaction(&mut tx, &user)
                .await
                .unwrap(),
            105
        );
        tx.commit().await.unwrap();
        assert_eq!(conn.table("purge_passkeys").count("*").await.unwrap(), 2);
        for event in [
            ModelEvent::ForceDeleting,
            ModelEvent::Deleting,
            ModelEvent::Deleted,
            ModelEvent::ForceDeleted,
        ] {
            assert_eq!(
                events
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|item| **item == event)
                    .count(),
                105
            );
        }
        let mut tx = conn.begin_transaction().await.unwrap();
        assert_eq!(
            keys.purge_user_in_transaction(&mut tx, &user)
                .await
                .unwrap(),
            0
        );
        tx.commit().await.unwrap();
    })
    .await;
}

#[derive(Clone)]
struct User(ModelState);
impl Model for User {
    fn metadata() -> &'static ModelMetadata {
        static M: ModelMetadata = ModelMetadata::new("purge_users")
            .primary_key("key")
            .incrementing(false)
            .timestamps(None, None);
        &M
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
impl openrust_passkeys::PasskeyAuthenticatable for User {}
