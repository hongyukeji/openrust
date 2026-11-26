use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::{
        Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Transaction,
        Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState,
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
        },
    },
    foundation::Application,
};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

struct Item(ModelState);
impl Model for Item {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("parity_write_items")
            .timestamps(None, None)
            .fillable(&["label", "alt"]);
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
struct Record {
    events: Arc<Mutex<Vec<(ModelEvent, bool)>>>,
    paused: Arc<Notify>,
}
impl Record {
    async fn event(
        &self,
        event: ModelEvent,
        model: &mut Item,
        transaction: Option<&mut Transaction>,
    ) -> DatabaseResult<ModelEventResponse> {
        use ModelEvent::*;
        self.events
            .lock()
            .unwrap()
            .push((event, transaction.is_some()));
        let label = model.get_attribute("label").cloned();
        match (event, label.as_ref()) {
            (Saving, Some(Value::Text(label))) if label == "cancel_saving" => {
                return Ok(ModelEventResponse::Cancel);
            }
            (Updating, Some(Value::Text(label))) if label == "cancel_updating" => {
                return Ok(ModelEventResponse::Cancel);
            }
            (Creating, Some(Value::Text(label))) if label == "cancel_creating" => {
                return Ok(ModelEventResponse::Cancel);
            }
            (Deleting, Some(Value::Text(label))) if label == "cancel_delete" => {
                return Ok(ModelEventResponse::Cancel);
            }
            (Created, Some(Value::Text(label))) if label == "pause" => {
                self.paused.notify_one();
                std::future::pending::<()>().await;
            }
            (Saved, Some(Value::Text(label))) if label == "post_error" => {
                return Err(DatabaseError::InvalidQuery("保存后监听器失败"));
            }
            (Deleted, Some(Value::Text(label))) if label == "delete_error" => {
                return Err(DatabaseError::InvalidQuery("删除后监听器失败"));
            }
            (Saved, Some(Value::Text(label))) if label == "mutate_saved" => {
                model.set_attribute("label", "listener_label");
            }
            _ => {}
        }
        if event == Created
            && let Some(tx) = transaction
        {
            assert_eq!(tx.transaction_level(), 1);
            tx.statement(
                "insert into parity_write_audits (label) values (?)",
                &[label.unwrap_or(Value::Null)],
            )
            .await?;
        }
        Ok(ModelEventResponse::Continue)
    }
}
impl ModelEventListener<Item> for Record {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut Item,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(self.event(event, model, None))
    }
    fn handle_in_transaction<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut Item,
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(self.event(event, model, Some(transaction)))
    }
}
fn attributes(label: &str, alt: &str) -> Attributes {
    [("label".into(), label.into()), ("alt".into(), alt.into())].into()
}
async fn count(db: &Connection, table: &str, label: &str) -> u64 {
    db.table(table)
        .r#where("label", "=", label)
        .count("*")
        .await
        .unwrap()
}
#[tokio::test]
async fn model_write_variants_preserve_transactions_cancellation_events_and_ignore_conflicts() {
    use ModelEvent::*;
    let config = match std::env::var("OPENRUST_TEST_MODEL_WRITE_URL") {
        Ok(url) => json!({"url":url}),
        Err(_) => {
            assert!(std::env::var_os("OPENRUST_REQUIRE_MODEL_WRITE").is_none());
            json!({"driver":"sqlite","database":":memory:"})
        }
    };
    let mysql = config["url"]
        .as_str()
        .is_some_and(|url| url.starts_with("mysql:"));
    let postgres = config["url"]
        .as_str()
        .is_some_and(|url| url.starts_with("postgres"));
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app.config()
        .unwrap()
        .set("database.default", json!("variants"))
        .unwrap();
    app.config()
        .unwrap()
        .set("database.connections.variants", config)
        .unwrap();
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let id = if mysql {
        "BIGINT PRIMARY KEY AUTO_INCREMENT"
    } else if postgres {
        "BIGSERIAL PRIMARY KEY"
    } else {
        "INTEGER PRIMARY KEY"
    };
    db.statement(&format!("create table parity_write_items (id {id}, label varchar(64) unique, alt varchar(64) unique)"),&[]).await.unwrap();
    db.statement("create table parity_write_audits (label varchar(64))", &[])
        .await
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let paused = Arc::new(Notify::new());
    for event in [
        Saving, Creating, Created, Saved, Updating, Updated, Deleting, Deleted,
    ] {
        Item::register_model_event(
            &app,
            event,
            Record {
                events: events.clone(),
                paused: paused.clone(),
            },
        )
        .unwrap();
    }
    let take = || std::mem::take(&mut *events.lock().unwrap());

    let reference: serde_json::Value =
        serde_json::from_str(include_str!("reference/eloquent-write-variants.json")).unwrap();
    assert_eq!(reference.as_object().unwrap().len(), 22);
    let compare = |name: &str, model: &Item, result: DatabaseResult<bool>, transaction: bool| {
        let result = match result {
            Ok(value) => json!({"ok":value}),
            Err(DatabaseError::Driver { .. }) => json!({"error":"query"}),
            Err(DatabaseError::InvalidQuery("已存在的模型不能使用 saveOrIgnore")) => {
                json!({"error":"logic"})
            }
            Err(DatabaseError::InvalidQuery("保存后监听器失败" | "删除后监听器失败")) =>
            {
                json!({"error":"listener"})
            }
            other => panic!("未覆盖的结果：{other:?}"),
        };
        let recorded = take();
        assert!(recorded.iter().all(|(_, tx)| *tx == transaction));
        let actual = json!({"result":result,"events":recorded.iter().map(|(event,_)|event.as_str()).collect::<Vec<_>>(),
            "exists":model.state().exists(),"recent":model.state().was_recently_created(),"dirty":model.is_dirty().unwrap()});
        let mut expected = reference[name].clone();
        expected.as_object_mut().unwrap().remove("rows");
        assert_eq!(actual, expected, "{name}");
    };
    let mut item = Item::new_instance_on(&db, attributes("first", "first_alt")).unwrap();
    let result = item.save_or_fail().await;
    compare("save_success", &item, result, true);
    assert_eq!(count(&db, "parity_write_items", "first").await, 1);
    assert_eq!(count(&db, "parity_write_audits", "first").await, 1);
    let result = item
        .update_or_fail(attributes("updated", "first_alt"))
        .await;
    compare("update_success", &item, result, true);
    let result = item
        .update_or_fail(attributes("cancel_updating", "first_alt"))
        .await;
    compare("update_cancel", &item, result, true);
    let result = item.update_quietly(attributes("quiet", "first_alt")).await;
    compare("update_quiet", &item, result, false);
    assert_eq!(count(&db, "parity_write_items", "quiet").await, 1);
    let result = item
        .update_or_fail(attributes("post_error", "first_alt"))
        .await;
    compare("update_listener_error", &item, result, true);
    assert_eq!(count(&db, "parity_write_items", "quiet").await, 1);
    assert!(
        item.update_quietly(attributes("quiet", "first_alt"))
            .await
            .unwrap()
    );
    take();
    let mut occupied = Item::new_instance_on(&db, attributes("occupied", "occupied")).unwrap();
    assert!(occupied.save_or_fail().await.unwrap());
    take();
    let result = item
        .update_or_fail(attributes("occupied", "first_alt"))
        .await;
    compare("update_sql_error", &item, result, true);
    assert_eq!(count(&db, "parity_write_items", "quiet").await, 1);
    assert!(
        item.update_quietly(attributes("quiet", "first_alt"))
            .await
            .unwrap()
    );
    take();
    let mut absent = Item::new_instance_on(&db, attributes("absent", "absent")).unwrap();
    assert!(
        !absent
            .update_or_fail(attributes("unused", "unused"))
            .await
            .unwrap()
    );
    assert!(
        !absent
            .update_quietly(attributes("unused", "unused"))
            .await
            .unwrap()
    );
    assert!(!absent.delete_or_fail().await.unwrap());
    assert!(take().is_empty());
    assert_eq!(absent.get_attribute("label"), Some(&"absent".into()));
    for label in ["cancel_saving", "cancel_creating"] {
        let mut cancelled = Item::new_instance_on(&db, attributes(label, label)).unwrap();
        let result = cancelled.save_or_fail().await;
        compare(label, &cancelled, result, true);
        assert_eq!(count(&db, "parity_write_items", label).await, 0);
    }
    let mut failing = Item::new_instance_on(&db, attributes("post_error", "post_error")).unwrap();
    let result = failing.save_or_fail().await;
    compare("post_error", &failing, result, true);
    assert_eq!(count(&db, "parity_write_items", "post_error").await, 0);
    assert_eq!(count(&db, "parity_write_audits", "post_error").await, 0);
    let mut mutation =
        Item::new_instance_on(&db, attributes("mutate_saved", "mutate_saved")).unwrap();
    let result = mutation.save_or_fail().await;
    compare("mutate_saved", &mutation, result, true);
    assert_eq!(
        mutation.get_attribute("label"),
        Some(&"listener_label".into())
    );
    assert_eq!(
        mutation.get_raw_original("label"),
        Some(&"listener_label".into())
    );
    assert_eq!(count(&db, "parity_write_items", "mutate_saved").await, 1);
    let mut duplicate = Item::new_instance_on(&db, attributes("quiet", "duplicate_alt")).unwrap();
    let result = duplicate.save_or_fail().await;
    compare("duplicate", &duplicate, result, true);
    assert_eq!(count(&db, "parity_write_items", "quiet").await, 1);
    let mut cancelled_delete =
        Item::new_instance_on(&db, attributes("cancel_delete", "cancel_delete")).unwrap();
    assert!(cancelled_delete.save_or_fail().await.unwrap());
    take();
    let result = cancelled_delete.delete_or_fail().await;
    compare("delete_cancel", &cancelled_delete, result, true);
    assert_eq!(count(&db, "parity_write_items", "cancel_delete").await, 1);
    let mut failed_delete =
        Item::new_instance_on(&db, attributes("delete_error", "delete_error")).unwrap();
    assert!(failed_delete.save_or_fail().await.unwrap());
    take();
    let result = failed_delete.delete_or_fail().await;
    compare("delete_error", &failed_delete, result, true);
    assert_eq!(count(&db, "parity_write_items", "delete_error").await, 1);
    let result = item.delete_or_fail().await;
    compare("delete_success", &item, result, true);
    assert_eq!(count(&db, "parity_write_items", "quiet").await, 0);
    let mut paused_model = Item::new_instance_on(&db, attributes("pause", "pause")).unwrap();
    let task = tokio::spawn(async move { paused_model.save_or_fail().await });
    tokio::time::timeout(std::time::Duration::from_secs(5), paused.notified())
        .await
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    take();
    assert_eq!(
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            count(&db, "parity_write_items", "pause")
        )
        .await
        .unwrap(),
        0
    );
    let mut empty = Item::new_instance_on(&db, Attributes::new()).unwrap();
    let result = empty.save_or_ignore(None).await;
    compare("ignore_empty", &empty, result, false);
    for label in ["cancel_saving", "cancel_creating"] {
        let mut cancelled =
            Item::new_instance_on(&db, attributes(label, &format!("ignore_{label}"))).unwrap();
        let result = cancelled.save_or_ignore(None).await;
        compare(&format!("ignore_{label}"), &cancelled, result, false);
    }
    if !mysql {
        let mut failed =
            Item::new_instance_on(&db, attributes("post_error", "ignore_post_error")).unwrap();
        let result = failed.save_or_ignore(None).await;
        compare("ignore_post_error", &failed, result, false);
        assert_eq!(
            count(&db, "parity_write_items", "post_error").await,
            1,
            "忽略冲突入口不会自动开启事务"
        );
    }
    let mut ignored = Item::new_instance_on(&db, attributes("ignored", "ignored")).unwrap();
    if mysql {
        assert!(matches!(
            ignored.save_or_ignore(None).await,
            Err(DatabaseError::UnsupportedOperation(
                "insert or ignore returning"
            ))
        ));
        assert_eq!(take(), [(Saving, false), (Creating, false)]);
    } else {
        let result = ignored.save_or_ignore(None).await;
        compare("ignore_success", &ignored, result, false);
        let result = ignored.save_or_ignore(None).await;
        compare("ignore_existing", &ignored, result, false);
        let mut conflict = Item::new_instance_on(&db, attributes("ignored", "different")).unwrap();
        let result = conflict.save_or_ignore(Some(&["label"])).await;
        compare("ignore_conflict", &conflict, result, false);
        let mut other_unique =
            Item::new_instance_on(&db, attributes("new_label", "ignored")).unwrap();
        let result = other_unique.save_or_ignore(Some(&["label"])).await;
        compare("ignore_other_constraint", &other_unique, result, false);
        assert_eq!(count(&db, "parity_write_items", "ignored").await, 1);
    }
}
