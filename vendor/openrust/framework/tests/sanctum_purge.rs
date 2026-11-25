#![cfg(feature = "sanctum")]
use openrust::{
    database::{
        DatabaseFuture, DatabaseResult, Value,
        eloquent::{
            Model, ModelMetadata, ModelState, UniqueId,
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
        },
    },
    foundation::Application,
    sanctum::{PersonalAccessTokenModel, Sanctum},
    support::facades::{App, Facade as _},
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
struct SoftToken(ModelState);
impl Model for SoftToken {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("purge_soft_tokens")
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
impl PersonalAccessTokenModel for SoftToken {}
struct NoOpToken(ModelState);
impl Model for NoOpToken {
    fn metadata() -> &'static ModelMetadata {
        SoftToken::metadata()
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
    fn force_delete_in_transaction<'a>(
        &'a mut self,
        _: &'a mut openrust::database::Transaction,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send + 'a {
        async { Ok(true) }
    }
}
impl PersonalAccessTokenModel for NoOpToken {}
#[derive(Clone)]
struct Events {
    cancel: Arc<AtomicBool>,
    events: Arc<Mutex<Vec<ModelEvent>>>,
}
impl ModelEventListener<SoftToken> for Events {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut SoftToken,
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
async fn account_purge_removes_soft_deleted_tokens_and_preserves_force_events_rollback_and_other_owners()
 {
    let root = tempfile::tempdir().unwrap();
    let settings = match std::env::var("OPENRUST_TOKEN_PURGE_DRIVER")
        .as_deref()
        .unwrap_or("sqlite")
    {
        "sqlite" => json!({"driver":"sqlite","database":":memory:"}),
        driver @ ("mysql" | "pgsql") => {
            json!({"driver":driver,"url":std::env::var("OPENRUST_TOKEN_PURGE_URL").expect("必须提供独立令牌清理验收数据库")})
        }
        _ => panic!("不支持的令牌清理验收数据库"),
    };
    std::fs::write(
        root.path().join("database.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    let mut app=Application::configure(root.path().to_path_buf()).with_config(|base,_|Ok([
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
        SoftToken::register_model_event(&app, event, events.clone()).unwrap();
    }
    App::set_facade_application(Some(&app), async {
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        let conn = db.connection(None).unwrap();
        let mut schema = conn.get_schema_builder();
        schema
            .create("purge_soft_tokens", |table| {
                table.ulid("key").primary();
                table.string("tokenable_type", 128);
                table.string("tokenable_id", 64);
                table.string("name", 255);
                table.string("token", 64).unique();
                table.text("abilities");
                table.timestamp("last_used_at", None).nullable();
                table.timestamp("expires_at", None).nullable();
                table.timestamps();
                table.soft_deletes();
            })
            .await
            .unwrap();
        let sanctum = app.make::<Sanctum>().await.unwrap();
        sanctum.use_personal_access_token_model::<SoftToken>();
        let tokens = sanctum.personal_access_tokens(&db, "").unwrap();
        let hidden = tokens
            .create_token("users", "owner-a", "old", &[], None)
            .await
            .unwrap();
        let mut soft = SoftToken::find(Value::from(hidden.access_token.id.clone()))
            .await
            .unwrap()
            .unwrap();
        assert!(soft.delete().await.unwrap());
        assert!(
            tokens
                .find_token(&hidden.plain_text_token)
                .await
                .unwrap()
                .is_none()
        );
        tokens
            .create_token("users", "owner-a", "current", &[], None)
            .await
            .unwrap();
        tokens
            .create_token("users", "owner-a", "block", &[], None)
            .await
            .unwrap();
        let neighbour = tokens
            .create_token("users", "owner-b", "neighbour", &[], None)
            .await
            .unwrap();
        let different_morph = tokens
            .create_token("USERS", "owner-a", "case-neighbour", &[], None)
            .await
            .unwrap();
        // 超过单批100条，验证分批清理；不一次水合全部凭据。
        for index in 0..102 {
            tokens
                .create_token("users", "owner-a", &format!("bulk-{index}"), &[], None)
                .await
                .unwrap();
        }
        sanctum.use_personal_access_token_model::<NoOpToken>();
        let no_op = sanctum.personal_access_tokens(&db, "").unwrap();
        let mut tx = conn.begin_transaction().await.unwrap();
        let error = no_op
            .purge_owned_in_transaction(&mut tx, "users", "owner-a")
            .await
            .unwrap_err();
        assert!(
            std::error::Error::source(&error)
                .unwrap()
                .to_string()
                .contains("物理清理"),
            "{error:?}"
        );
        tx.roll_back(None).await.unwrap();
        sanctum.use_personal_access_token_model::<SoftToken>();
        let mut tx = conn.begin_transaction().await.unwrap();
        assert!(
            tokens
                .purge_owned_in_transaction(&mut tx, "users", "owner-a")
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        assert_eq!(
            conn.table("purge_soft_tokens").count("*").await.unwrap(),
            107
        );
        events.cancel.store(false, Ordering::SeqCst);
        events.events.lock().unwrap().clear();
        let mut tx = conn.begin_transaction().await.unwrap();
        assert_eq!(
            tokens
                .purge_owned_in_transaction(&mut tx, "users", "owner-a")
                .await
                .unwrap(),
            105
        );
        tx.commit().await.unwrap();
        assert_eq!(conn.table("purge_soft_tokens").count("*").await.unwrap(), 2);
        assert!(
            tokens
                .find_token(&neighbour.plain_text_token)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            tokens
                .find_token(&different_morph.plain_text_token)
                .await
                .unwrap()
                .is_some()
        );
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
            tokens
                .purge_owned_in_transaction(&mut tx, "users", "owner-a")
                .await
                .unwrap(),
            0
        );
        tx.commit().await.unwrap();
    })
    .await;
}
