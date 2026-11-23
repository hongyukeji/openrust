use std::sync::{Arc, Mutex};

use openrust::database::eloquent::RelatedCollection;
use openrust::{
    contracts::{
        container::Container as _,
        events::{Dispatcher as _, Event, Listener, ShouldHandleEventsAfterCommit},
        foundation::Application as _,
    },
    database::{
        Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Db, Value,
        eloquent::{
            Model, ModelMetadata, ModelState, Observer, Touches,
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
            relations::{BelongsTo, HasMany, MorphTo},
        },
    },
    events::{
        Delivery, Dispatcher, EventContext, EventError, EventFuture, EventResult, ListenerResponse,
    },
    foundation::Application,
    support::{ProviderResult, ServiceProvider},
};
use serde_json::{Value as Json, json};

use ModelEvent::*;

#[derive(Clone)]
struct Article(ModelState);

impl Model for Article {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("model_event_articles")
            .timestamps(None, None)
            .soft_deletes("deleted_at")
            .fillable(&["title"]);
        &METADATA
    }

    fn dispatches_events(&self, event: ModelEvent) -> Option<Arc<dyn Event>> {
        let name = match event {
            Saving => "article.saving",
            Saved => "article.saved",
            Retrieved => "article.retrieved",
            _ => return None,
        };
        let Value::Text(title) = self.get_attribute("title")? else {
            return None;
        };
        title.starts_with("mapped-").then(|| {
            Arc::new(ArticleLifecycle {
                name,
                title: title.clone(),
            }) as Arc<dyn Event>
        })
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

struct ArticleLifecycle {
    name: &'static str,
    title: String,
}

impl Event for ArticleLifecycle {
    fn name(&self) -> &str {
        self.name
    }
}

struct MappedListener(Arc<Mutex<Vec<String>>>);

impl Listener for MappedListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event
                .downcast_ref::<ArticleLifecycle>()
                .ok_or(EventError::InvalidPayload)?;
            self.0
                .lock()
                .unwrap()
                .push(format!("{}:{}", event.name, event.title));
            if event.name == "article.retrieved" {
                let transaction = context
                    .transaction()
                    .expect("locked retrieval has a transaction");
                let title = transaction
                    .table("model_event_articles")
                    .r#where("title", "=", event.title.clone())
                    .value("title")
                    .await
                    .expect("read the uncommitted row");
                assert_eq!(title, Value::Text(event.title.clone()));
                return Ok(None);
            }
            if event.name == "article.saving" {
                return Ok(match event.title.as_str() {
                    "mapped-cancel" => Some(ListenerResponse::Value(Box::new(false))),
                    "mapped-handled" => Some(ListenerResponse::Value(Box::new(true))),
                    "mapped-zero" => Some(ListenerResponse::Value(Box::new("0"))),
                    "mapped-error" => return Err(EventError::InvalidPayload),
                    "mapped-transaction" => {
                        assert!(context.transaction().is_some());
                        None
                    }
                    _ => None,
                });
            }
            assert!(context.transaction().is_none());
            Db::connection(None).map_err(EventError::Transaction)?;
            Ok((event.title == "mapped-handled").then(|| ListenerResponse::Value(Box::new(1_u32))))
        })
    }
}

struct Record(Arc<Mutex<Vec<ModelEvent>>>);

impl ModelEventListener<Article> for Record {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut Article,
        connection: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(event);
            let title = model.get_attribute("title").cloned();
            match (event, title.as_ref()) {
                (Creating, Some(Value::Text(title))) if title == "cancel" => {
                    return Ok(ModelEventResponse::Cancel);
                }
                (Creating, Some(Value::Text(title))) if title == "draft" => {
                    model.set_attribute("title", "published");
                }
                (Saved, Some(Value::Text(title))) if title == "post-error" => {
                    return Err(DatabaseError::InvalidQuery("saved listener failed"));
                }
                (Created, _) => {
                    assert!(model.state().exists());
                    assert_eq!(
                        connection
                            .scalar(
                                "SELECT title FROM model_event_articles WHERE id = ?",
                                &[model.get_key().unwrap().clone()]
                            )
                            .await?,
                        model.get_attribute("title").unwrap().clone(),
                    );
                }
                _ => {}
            }
            Ok(ModelEventResponse::Continue)
        })
    }
}

struct ArticleObserver;

impl Observer<Article> for ArticleObserver {
    const EVENTS: &'static [ModelEvent] = &[Saving, Created, Created];

    fn saving<'a>(
        &'a self,
        model: &'a mut Article,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            match model.get_attribute("title") {
                Some(Value::Text(title)) if title == "blocked" => {
                    return Ok(ModelEventResponse::Cancel);
                }
                Some(Value::Text(title)) if title == "observer" => {
                    model.set_attribute("title", "registered");
                }
                _ => {}
            }
            Ok(ModelEventResponse::Continue)
        })
    }

    fn created<'a>(
        &'a self,
        model: &'a mut Article,
        connection: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            connection
                .insert(
                    "INSERT INTO model_observer_log (article_id) VALUES (?)",
                    &[model.get_key().unwrap().clone()],
                )
                .await?;
            Ok(ModelEventResponse::Continue)
        })
    }
}

struct ObserverProvider;

impl ServiceProvider for ObserverProvider {
    fn boot<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> openrust::http::HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            Article::observe(app, ArticleObserver)?;
            Ok(())
        })
    }
}

struct TransactionAudit;

impl ModelEventListener<Article> for TransactionAudit {
    fn handle<'a>(
        &'a self,
        _: ModelEvent,
        _: &'a mut Article,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async { panic!("expected the active transaction") })
    }

    fn handle_in_transaction<'a>(
        &'a self,
        _: ModelEvent,
        model: &'a mut Article,
        transaction: &'a mut openrust::database::Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            transaction
                .insert(
                    "INSERT INTO model_observer_log (article_id) VALUES (?)",
                    &[model.get_key().unwrap().clone()],
                )
                .await?;
            Ok(ModelEventResponse::Continue)
        })
    }
}

struct CommittedAudit;

impl ShouldHandleEventsAfterCommit for CommittedAudit {}

impl Observer<Article> for CommittedAudit {
    const EVENTS: &'static [ModelEvent] = &[Creating, Created];

    fn creating<'a>(
        &'a self,
        model: &'a mut Article,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            if model.get_attribute("title") == Some(&"after-veto".into()) {
                return Ok(ModelEventResponse::Cancel);
            }
            Ok(ModelEventResponse::Continue)
        })
    }

    fn created<'a>(
        &'a self,
        model: &'a mut Article,
        connection: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            let title = model.get_attribute("title").unwrap();
            if title == &"after-error".into() {
                return Err(DatabaseError::InvalidQuery("observer after commit"));
            }
            if title == &"event-snapshot".into() {
                assert_eq!(
                    openrust::database::Db::connection(None)?.get_name(),
                    connection.get_name()
                );
            }
            assert_eq!(
                connection
                    .scalar(
                        "SELECT COUNT(*) FROM model_event_articles WHERE id = ?",
                        &[model.get_key().unwrap().clone()]
                    )
                    .await?,
                Value::Integer(1)
            );
            connection
                .insert(
                    "INSERT INTO committed_observer_log (title) VALUES (?)",
                    std::slice::from_ref(title),
                )
                .await?;
            Ok(ModelEventResponse::Continue)
        })
    }
}

async fn verify_after_commit(app: &Application, db: &Connection) {
    Article::observe_after_commit(app, CommittedAudit).unwrap();
    let article = |title: &str| {
        Article::query_on(db)
            .make([("title".into(), title.into())].into())
            .unwrap()
    };
    let log_count = "SELECT COUNT(*) FROM committed_observer_log";

    let mut transaction = db.begin_transaction().await.unwrap();
    assert!(
        !article("after-veto")
            .save_in_transaction(&mut transaction)
            .await
            .unwrap()
    );
    article("outer-rollback")
        .save_in_transaction(&mut transaction)
        .await
        .unwrap();
    transaction.roll_back(None).await.unwrap();
    assert_eq!(db.scalar(log_count, &[]).await.unwrap(), Value::Integer(0));

    let mut transaction = db.begin_transaction().await.unwrap();
    transaction.begin_transaction().await.unwrap();
    article("savepoint-rollback")
        .save_in_transaction(&mut transaction)
        .await
        .unwrap();
    transaction.roll_back(None).await.unwrap();
    transaction.begin_transaction().await.unwrap();
    let mut committed = article("event-snapshot");
    committed
        .save_in_transaction(&mut transaction)
        .await
        .unwrap();
    committed.set_attribute("title", "later-change");
    transaction.commit().await.unwrap();
    assert_eq!(
        transaction.scalar(log_count, &[]).await.unwrap(),
        Value::Integer(0)
    );
    transaction.commit().await.unwrap();
    assert_eq!(
        db.scalar("SELECT title FROM committed_observer_log", &[])
            .await
            .unwrap(),
        "event-snapshot".into()
    );

    let mut transaction = db.begin_transaction().await.unwrap();
    article("after-error")
        .save_in_transaction(&mut transaction)
        .await
        .unwrap();
    assert!(matches!(
        transaction.commit().await,
        Err(DatabaseError::AfterCommit { .. })
    ));
    assert_eq!(
        db.scalar(
            "SELECT COUNT(*) FROM model_event_articles WHERE title = 'after-error'",
            &[]
        )
        .await
        .unwrap(),
        Value::Integer(1)
    );
    assert_eq!(db.scalar(log_count, &[]).await.unwrap(), Value::Integer(1));
    assert!(article("immediate").save().await.unwrap());
    assert_eq!(db.scalar(log_count, &[]).await.unwrap(), Value::Integer(2));
}

async fn verify_transaction(app: &Application, db: &Connection, other: &Connection) {
    Article::register_model_event(app, Created, TransactionAudit).unwrap();
    let article = |title: &str| {
        Article::query_on(db)
            .make([("title".into(), title.into())].into())
            .unwrap()
    };
    let mut wrong = other.begin_transaction().await.unwrap();
    assert!(matches!(
        article("wrong").save_in_transaction(&mut wrong).await,
        Err(DatabaseError::InvalidQuery("model transaction connection"))
    ));
    wrong.roll_back(None).await.unwrap();

    let mut transaction = db.begin_transaction().await.unwrap();
    assert!(
        !article("cancel")
            .save_in_transaction(&mut transaction)
            .await
            .unwrap()
    );
    let mut rolled_back = article("rolled-back");
    assert!(
        rolled_back
            .save_in_transaction(&mut transaction)
            .await
            .unwrap()
    );
    assert!(rolled_back.get_key().is_some());
    assert_eq!(
        transaction
            .scalar("SELECT COUNT(*) FROM model_observer_log", &[])
            .await
            .unwrap(),
        Value::Integer(1)
    );
    transaction.roll_back(None).await.unwrap();
    assert_eq!(
        db.scalar("SELECT COUNT(*) FROM model_observer_log", &[])
            .await
            .unwrap(),
        Value::Integer(0)
    );
    assert_eq!(
        db.scalar(
            "SELECT COUNT(*) FROM model_event_articles WHERE title = 'rolled-back'",
            &[]
        )
        .await
        .unwrap(),
        Value::Integer(0)
    );
    assert!(matches!(
        article("closed")
            .save_in_transaction(&mut transaction)
            .await,
        Err(DatabaseError::TransactionClosed)
    ));

    let mut transaction = db.begin_transaction().await.unwrap();
    let mut committed = article("commit");
    assert!(
        committed
            .save_in_transaction(&mut transaction)
            .await
            .unwrap()
    );
    assert!(
        committed
            .update_in_transaction(
                [("title".into(), "updated-tx".into())].into(),
                &mut transaction
            )
            .await
            .unwrap()
    );
    assert!(
        committed
            .delete_in_transaction(&mut transaction)
            .await
            .unwrap()
    );
    assert!(committed.trashed());
    assert!(
        committed
            .restore_in_transaction(&mut transaction)
            .await
            .unwrap()
    );
    assert!(!committed.trashed());
    transaction.commit().await.unwrap();
    let key = committed.get_key().unwrap().clone();
    let fresh = Article::query_on(db)
        .find_or_fail(key.clone())
        .await
        .unwrap();
    assert_eq!(fresh.get_attribute("title"), Some(&"updated-tx".into()));

    let mut transaction = db.begin_transaction().await.unwrap();
    transaction.begin_transaction().await.unwrap();
    assert!(
        committed
            .force_delete_in_transaction(&mut transaction)
            .await
            .unwrap()
    );
    transaction.roll_back(None).await.unwrap();
    transaction.commit().await.unwrap();
    assert!(Article::query_on(db).find(key).await.unwrap().is_some());
}

async fn app_with_database(
    config: Json,
    providers: Vec<Box<dyn ServiceProvider>>,
) -> (Application, Connection) {
    let mut app = Application::configure(std::env::temp_dir())
        .with_providers(providers)
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let repository = app.config().unwrap();
    repository
        .set("database.default", json!("lifecycle"))
        .unwrap();
    repository
        .set("database.connections.lifecycle", config)
        .unwrap();
    let connection = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    (app, connection)
}

async fn verify(config: Json, id: &str) {
    let (app, db) = app_with_database(config, vec![Box::new(ObserverProvider)]).await;
    db.statement("DROP TABLE IF EXISTS model_event_articles", &[])
        .await
        .unwrap();
    db.statement(&format!("CREATE TABLE model_event_articles (id {id}, title VARCHAR(30) UNIQUE NOT NULL, deleted_at DATETIME NULL)"), &[]).await.unwrap();

    db.statement("DROP TABLE IF EXISTS model_observer_log", &[])
        .await
        .unwrap();
    db.statement(
        "CREATE TABLE model_observer_log (article_id BIGINT NOT NULL)",
        &[],
    )
    .await
    .unwrap();
    let mut blocked = Article::query_on(&db)
        .make([("title".into(), "blocked".into())].into())
        .unwrap();
    assert!(!blocked.save().await.unwrap());
    assert!(!blocked.state().exists());
    let observed = Article::query_on(&db)
        .create([("title".into(), "observer".into())].into())
        .await
        .unwrap();
    assert_eq!(observed.get_attribute("title"), Some(&"registered".into()));
    assert_eq!(
        db.scalar("SELECT COUNT(*) FROM model_observer_log", &[])
            .await
            .unwrap(),
        Value::Integer(1)
    );

    let log = Arc::new(Mutex::new(Vec::new()));
    for event in [
        Retrieved,
        Saving,
        Saved,
        Creating,
        Created,
        Updating,
        Updated,
        Deleting,
        Deleted,
        Trashed,
        Restoring,
        Restored,
        ForceDeleting,
        ForceDeleted,
    ] {
        Article::register_model_event(&app, event, Record(log.clone())).unwrap();
    }
    let take = || std::mem::take(&mut *log.lock().unwrap());
    let article = |title: &str| {
        Article::query_on(&db)
            .make([("title".into(), title.into())].into())
            .unwrap()
    };

    let mut model = article("cancel");
    assert!(!model.save().await.unwrap());
    assert!(!model.state().exists());
    assert_eq!(take(), [Saving, Creating]);
    model.set_attribute("title", "draft");
    assert!(model.save().await.unwrap());
    assert_eq!(model.get_attribute("title"), Some(&"published".into()));
    assert_eq!(take(), [Saving, Creating, Created, Saved]);
    assert!(model.save().await.unwrap());
    assert_eq!(take(), [Saving, Saved]);
    model.set_attribute("title", "renamed");
    assert!(model.save().await.unwrap());
    assert_eq!(take(), [Saving, Updating, Updated, Saved]);

    let key = model.get_key().unwrap().clone();
    Article::query_on(&db)
        .find_or_fail(key.clone())
        .await
        .unwrap();
    assert_eq!(take(), [Retrieved]);
    assert!(model.delete().await.unwrap());
    assert!(model.trashed());
    assert_eq!(take(), [Deleting, Trashed, Deleted]);
    assert!(model.restore().await.unwrap());
    assert_eq!(
        take(),
        [Restoring, Saving, Updating, Updated, Saved, Restored]
    );
    assert!(model.force_delete().await.unwrap());
    assert_eq!(take(), [ForceDeleting, Deleting, Deleted, ForceDeleted]);
    assert!(
        Article::query_on(&db)
            .with_trashed()
            .find(key)
            .await
            .unwrap()
            .is_none()
    );

    let mut quiet = article("quiet");
    assert!(quiet.save_quietly().await.unwrap());
    assert!(take().is_empty());
    let mut duplicate = article("quiet");
    assert!(duplicate.save().await.is_err());
    assert_eq!(take(), [Saving, Creating]);
    duplicate.set_attribute("title", "post-error");
    assert!(matches!(
        duplicate.save().await,
        Err(DatabaseError::InvalidQuery("saved listener failed"))
    ));
    assert!(duplicate.state().exists());
    assert_eq!(take(), [Saving, Creating, Created, Saved]);
    duplicate.set_attribute("title", "retry");
    assert!(duplicate.save().await.unwrap());
    assert_eq!(take(), [Saving, Updating, Updated, Saved]);

    let (_other, other_db) =
        app_with_database(json!({"driver":"sqlite","database":":memory:"}), vec![]).await;
    other_db.statement("CREATE TABLE model_event_articles (id INTEGER PRIMARY KEY, title TEXT, deleted_at TEXT)", &[]).await.unwrap();
    let other = Article::query_on(&other_db)
        .create([("title".into(), "observer".into())].into())
        .await
        .unwrap();
    assert_eq!(other.get_attribute("title"), Some(&"observer".into()));
    assert!(take().is_empty());
    db.statement("DROP TABLE model_observer_log", &[])
        .await
        .unwrap();
    db.statement("DROP TABLE model_event_articles", &[])
        .await
        .unwrap();
}

async fn verify_transaction_backend(config: Json, id: &str) {
    let (app, db) = app_with_database(config, vec![]).await;
    let (_other, other) =
        app_with_database(json!({"driver":"sqlite","database":":memory:"}), vec![]).await;
    db.statement(&format!("CREATE TABLE model_event_articles (id {id}, title VARCHAR(30) UNIQUE NOT NULL, deleted_at DATETIME NULL)"), &[]).await.unwrap();
    db.statement(
        "CREATE TABLE model_observer_log (article_id BIGINT NOT NULL)",
        &[],
    )
    .await
    .unwrap();
    Article::register_model_event(&app, Creating, Record(Arc::default())).unwrap();
    verify_transaction(&app, &db, &other).await;
    db.statement("DROP TABLE model_observer_log", &[])
        .await
        .unwrap();
    db.statement("DROP TABLE model_event_articles", &[])
        .await
        .unwrap();
}

async fn verify_after_commit_backend(config: Json, id: &str) {
    let (app, db) = app_with_database(config, vec![]).await;
    db.statement(&format!("CREATE TABLE model_event_articles (id {id}, title VARCHAR(30) UNIQUE NOT NULL, deleted_at DATETIME NULL)"), &[]).await.unwrap();
    db.statement(
        "CREATE TABLE committed_observer_log (title VARCHAR(30) NOT NULL)",
        &[],
    )
    .await
    .unwrap();
    verify_after_commit(&app, &db).await;
    db.statement("DROP TABLE committed_observer_log", &[])
        .await
        .unwrap();
    db.statement("DROP TABLE model_event_articles", &[])
        .await
        .unwrap();
}

async fn verify_mapped_events(config: Json, id: &str) {
    let sqlite = config["driver"] == "sqlite";
    let (app, db) = app_with_database(config.clone(), vec![]).await;
    let (isolated, isolated_db) = app_with_database(config, vec![]).await;
    db.statement(&format!("CREATE TABLE model_event_articles (id {id}, title VARCHAR(30) UNIQUE NOT NULL, deleted_at DATETIME NULL)"), &[]).await.unwrap();

    let calls = Arc::new(Mutex::new(Vec::new()));
    let dispatcher = app.container().make::<Dispatcher>().unwrap();
    dispatcher
        .listen(
            "article.saving",
            Arc::new(MappedListener(calls.clone())),
            Delivery::Sync,
        )
        .unwrap();
    dispatcher
        .listen(
            "article.saved",
            Arc::new(MappedListener(calls.clone())),
            Delivery::AfterCommit,
        )
        .unwrap();
    let fallback = Arc::new(Mutex::new(Vec::new()));
    Article::register_model_event(&app, Saving, Record(fallback.clone())).unwrap();
    Article::register_model_event(&app, Saved, Record(fallback.clone())).unwrap();

    for (title, saved) in [
        ("mapped-cancel", false),
        ("mapped-handled", true),
        ("mapped-zero", true),
    ] {
        let mut article =
            Article::new_instance_on(&db, [("title".into(), title.into())].into()).unwrap();
        assert_eq!(article.save().await.unwrap(), saved);
        assert_eq!(article.state().exists(), saved);
    }
    assert_eq!(*fallback.lock().unwrap(), [Saving, Saved]);

    let mut error =
        Article::new_instance_on(&db, [("title".into(), "mapped-error".into())].into()).unwrap();
    assert!(matches!(
        error.save().await,
        Err(DatabaseError::ModelEvent(_))
    ));
    assert!(!error.state().exists());

    for (suffix, commit) in [("", true), ("-rollback", false)] {
        let mut transaction = db.begin_transaction().await.unwrap();
        let mut article =
            Article::new_instance_on(&db, [("title".into(), "mapped-transaction".into())].into())
                .unwrap();
        if !commit {
            transaction
                .delete(
                    "DELETE FROM model_event_articles WHERE title = ?",
                    &["mapped-transaction".into()],
                )
                .await
                .unwrap();
        }
        assert!(article.save_in_transaction(&mut transaction).await.unwrap());
        article.set_attribute("title", format!("edited{suffix}"));
        if commit {
            assert!(
                !calls
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|call| call == "article.saved:mapped-transaction")
            );
            transaction.commit().await.unwrap();
        } else {
            transaction.roll_back(None).await.unwrap();
        }
    }
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| *call == "article.saved:mapped-transaction")
            .count(),
        1
    );

    let mut article = Article::query_on(&db).first_or_fail().await.unwrap();
    dispatcher
        .listen(
            "article.retrieved",
            Arc::new(MappedListener(calls.clone())),
            Delivery::Sync,
        )
        .unwrap();
    let mut transaction = db.begin_transaction().await.unwrap();
    transaction
        .table("model_event_articles")
        .r#where("id", "=", article.get_key().unwrap().clone())
        .update([("title".into(), "mapped-retrieved".into())].into())
        .await
        .unwrap();
    article.refresh_for_update(&mut transaction).await.unwrap();
    transaction.roll_back(None).await.unwrap();
    assert!(
        calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call == "article.retrieved:mapped-retrieved")
    );

    let before = calls.lock().unwrap().len();
    let mut quiet =
        Article::new_instance_on(&db, [("title".into(), "mapped-quiet".into())].into()).unwrap();
    assert!(quiet.save_quietly().await.unwrap());
    if sqlite {
        isolated_db.statement(&format!("CREATE TABLE model_event_articles (id {id}, title VARCHAR(30) UNIQUE NOT NULL, deleted_at DATETIME NULL)"), &[]).await.unwrap();
    }
    let mut other = Article::new_instance_on(
        &isolated_db,
        [("title".into(), "mapped-isolated".into())].into(),
    )
    .unwrap();
    assert!(other.save().await.unwrap());
    assert_eq!(calls.lock().unwrap().len(), before);
    drop(isolated);
    db.statement("DROP TABLE model_event_articles", &[])
        .await
        .unwrap();
}

struct ReplicaArticle(ModelState);

impl Model for ReplicaArticle {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("replica_articles");
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

struct ReplicatingListener;

impl ModelEventListener<ReplicaArticle> for ReplicatingListener {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        replica: &'a mut ReplicaArticle,
        connection: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            assert_eq!(event, Replicating);
            assert!(!replica.state().exists());
            assert!(replica.get_key().is_none());
            assert!(replica.relation_loaded("children"));
            assert_eq!(replica.get_connection_name(), Some(connection.get_name()));
            replica.set_attribute("title", "from-listener");
            Ok(ModelEventResponse::Cancel)
        })
    }
}

async fn verify_replication(config: Json, id: &str) {
    let (app, db) = app_with_database(config, vec![]).await;
    db.statement(&format!("CREATE TABLE replica_articles (id {id}, parent_id BIGINT NULL, title VARCHAR(30) NOT NULL, secret VARCHAR(30) NULL, created_at DATETIME NULL, updated_at DATETIME NULL)"), &[]).await.unwrap();
    db.insert("INSERT INTO replica_articles (id, title, secret, created_at, updated_at) VALUES (1, 'original', 'private', '2020-01-01 00:00:00', '2020-01-01 00:00:00')", &[]).await.unwrap();
    db.insert(
        "INSERT INTO replica_articles (id, parent_id, title) VALUES (2, 1, 'child')",
        &[],
    )
    .await
    .unwrap();
    let mut source = ReplicaArticle::query_on(&db)
        .find(1_i64)
        .await
        .unwrap()
        .unwrap();
    source
        .load(
            "children",
            &HasMany::<ReplicaArticle, ReplicaArticle>::new("id", "parent_id"),
        )
        .await
        .unwrap();
    source.set_attribute("laravel_through_key", 1_i64);
    source.set_attribute("title", "unsaved-edit");
    ReplicaArticle::register_model_event(&app, Replicating, ReplicatingListener).unwrap();

    let mut copy = source.replicate(&["secret", "id"]).await.unwrap();
    assert_eq!(
        copy.get_attribute("title"),
        Some(&Value::from("from-listener"))
    );
    for column in [
        "id",
        "secret",
        "created_at",
        "updated_at",
        "laravel_through_key",
    ] {
        assert!(copy.get_attribute(column).is_none(), "{column}");
    }
    assert!(!copy.state().was_recently_created());
    assert!(copy.state().get_original().is_empty());
    assert!(copy.state().get_changes().is_empty());
    assert_eq!(copy.get_connection_name(), source.get_connection_name());
    copy.unset_relation("children");
    assert!(source.relation_loaded("children"));
    assert!(copy.save().await.unwrap());
    assert_ne!(copy.get_key(), source.get_key());

    let mut quiet = source.replicate_quietly(&[]).await.unwrap();
    assert_eq!(
        quiet.get_attribute("title"),
        Some(&Value::from("unsaved-edit"))
    );
    assert_eq!(quiet.get_attribute("secret"), Some(&Value::from("private")));
    assert!(quiet.relation_loaded("children"));
    assert!(quiet.save().await.unwrap());
    assert_ne!(quiet.get_key(), copy.get_key());
    assert_eq!(
        db.scalar("SELECT title FROM replica_articles WHERE id = 1", &[])
            .await
            .unwrap(),
        Value::from("original")
    );

    let detached = ReplicaArticle::from_state(ModelState::default());
    assert!(!detached.replicate(&[]).await.unwrap().state().exists());
    db.statement("DROP TABLE replica_articles", &[])
        .await
        .unwrap();
}

struct GeneratedArticle(ModelState);

impl Model for GeneratedArticle {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("generated_articles")
            .timestamps(None, None)
            .fillable(&["title"])
            .refreshes(&["title_length"]);
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

struct GeneratedObserver(Arc<Mutex<Vec<ModelEvent>>>);

impl ModelEventListener<GeneratedArticle> for GeneratedObserver {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut GeneratedArticle,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            let Some(Value::Text(title)) = model.get_attribute("title") else {
                panic!("missing title");
            };
            let length = i64::try_from(title.len()).unwrap();
            assert_eq!(
                model.get_attribute("title_length"),
                Some(&Value::Integer(length))
            );
            self.0.lock().unwrap().push(event);
            Ok(ModelEventResponse::Continue)
        })
    }
}

async fn verify_refreshes(config: Json, id: &str) {
    let (app, db) = app_with_database(config, vec![]).await;
    db.statement(&format!("CREATE TABLE generated_articles (id {id}, title VARCHAR(30) NOT NULL, title_length BIGINT GENERATED ALWAYS AS (LENGTH(title)) STORED)"), &[]).await.unwrap();

    let events = Arc::new(Mutex::new(Vec::new()));
    for event in [Created, Updated, Saved] {
        GeneratedArticle::register_model_event(&app, event, GeneratedObserver(events.clone()))
            .unwrap();
    }
    let mut article =
        GeneratedArticle::new_instance_on(&db, [("title".into(), "abc".into())].into()).unwrap();
    assert!(article.save().await.unwrap());
    assert_eq!(
        article.state().get_original().get("title_length"),
        Some(&Value::Integer(3))
    );

    let mut transaction = db.begin_transaction().await.unwrap();
    article.set_attribute("title", "longer");
    article.set_attribute("id", 100_i64);
    assert!(article.save_in_transaction(&mut transaction).await.unwrap());
    assert_eq!(
        article.state().get_changes().get("title_length"),
        Some(&Value::Integer(6))
    );
    assert_eq!(
        article.state().get_original().get("title_length"),
        Some(&Value::Integer(6))
    );
    assert!(!article.is_dirty().unwrap());
    transaction.commit().await.unwrap();

    let mut copy = article.replicate_quietly(&[]).await.unwrap();
    assert!(copy.get_attribute("title_length").is_none());
    assert!(copy.save().await.unwrap());
    assert_ne!(copy.get_key(), article.get_key());
    assert_eq!(copy.get_attribute("title_length"), Some(&Value::Integer(6)));
    assert_eq!(
        *events.lock().unwrap(),
        [Created, Saved, Updated, Saved, Created, Saved]
    );

    let key = copy.get_key().unwrap().clone();
    db.delete("DELETE FROM generated_articles WHERE id = ?", &[key])
        .await
        .unwrap();
    copy.set_attribute("title", "gone");
    assert!(matches!(
        copy.save().await,
        Err(DatabaseError::ModelNotFound("generated_articles"))
    ));
    assert!(copy.state().exists());
    assert_eq!(events.lock().unwrap().len(), 6);
    db.statement("DROP TABLE generated_articles", &[])
        .await
        .unwrap();
}

struct TouchNode(ModelState);

impl Model for TouchNode {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("touch_nodes")
            .timestamps(None, Some("updated_at"))
            .fillable(&["parent_id", "title"]);
        &METADATA
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

    fn touches() -> Touches<Self> {
        Touches::default()
            .belongs_to(BelongsTo::<Self, Self>::new("parent_id", "id"))
            .morph_to(
                MorphTo::<Self>::new("owner_type", "owner_id")
                    .target::<Self>("node", "id")
                    .constrain::<Self>("external", "external_id", |mut query| {
                        query.r#where("title", "=", "visible");
                        query
                    }),
            )
    }
}

struct RecordTouches(Arc<Mutex<Vec<(ModelEvent, Value)>>>);

impl ModelEventListener<TouchNode> for RecordTouches {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut TouchNode,
        _connection: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            self.0
                .lock()
                .unwrap()
                .push((event, model.get_key().unwrap().clone()));
            Ok(ModelEventResponse::Continue)
        })
    }
}

async fn verify_touch_owners(config: Json, id: &str) {
    let (app, db) = app_with_database(config, Vec::new()).await;
    let events = Arc::new(Mutex::new(Vec::new()));
    for event in [Saving, Saved, Updating, Updated] {
        TouchNode::register_model_event(&app, event, RecordTouches(events.clone())).unwrap();
    }
    db.statement("DROP TABLE IF EXISTS touch_nodes", &[])
        .await
        .unwrap();
    db.statement(
        &format!("CREATE TABLE touch_nodes (id {id}, parent_id BIGINT NULL, owner_id BIGINT NULL, owner_type VARCHAR(30), external_id BIGINT NULL UNIQUE, title VARCHAR(30), updated_at VARCHAR(30))"),
        &[],
    ).await.unwrap();
    let seed = "2000-01-01 00:00:00";
    for (id, parent) in [(1, 2), (2, 3), (3, 1), (4, 99)] {
        db.statement(
            "INSERT INTO touch_nodes (id, parent_id, title, updated_at) VALUES (?, ?, ?, ?)",
            &[id.into(), parent.into(), "seed".into(), seed.into()],
        )
        .await
        .unwrap();
    }
    let read_timestamp = "SELECT updated_at FROM touch_nodes WHERE id = ?";
    let mut child = TouchNode::query_on(&db).find_or_fail(1).await.unwrap();
    child.save().await.unwrap();
    assert_eq!(
        events.lock().unwrap().drain(..).collect::<Vec<_>>(),
        [(Saving, 1.into()), (Saved, 1.into())]
    );
    assert_eq!(
        db.scalar(read_timestamp, &[2.into()]).await.unwrap(),
        seed.into()
    );

    child.set_attribute("title", "rolled back");
    let mut transaction = db.begin_transaction().await.unwrap();
    child.save_in_transaction(&mut transaction).await.unwrap();
    for owner in [2, 3] {
        assert_ne!(
            transaction
                .scalar(read_timestamp, &[owner.into()])
                .await
                .unwrap(),
            seed.into()
        );
    }
    transaction.roll_back(None).await.unwrap();
    assert_eq!(
        events.lock().unwrap().drain(..).collect::<Vec<_>>(),
        [
            (Saving, 1.into()),
            (Updating, 1.into()),
            (Updated, 1.into()),
            (Saved, 1.into()),
            (Saved, 2.into()),
            (Saved, 3.into()),
        ]
    );
    for owner in [2, 3] {
        assert_eq!(
            db.scalar(read_timestamp, &[owner.into()]).await.unwrap(),
            seed.into()
        );
    }
    let mut child = TouchNode::query_on(&db).find_or_fail(1).await.unwrap();
    child.set_attribute("title", "committed");
    child.save().await.unwrap();
    for owner in [2, 3] {
        assert_ne!(
            db.scalar(read_timestamp, &[owner.into()]).await.unwrap(),
            seed.into()
        );
    }
    let mut orphan = TouchNode::query_on(&db).find_or_fail(4).await.unwrap();
    orphan.set_attribute("title", "missing owner");
    orphan.save().await.unwrap();
    orphan.set_attribute("parent_id", Value::Null);
    orphan.save().await.unwrap();

    db.statement(
        "INSERT INTO touch_nodes (id, external_id, title, updated_at) VALUES (?, ?, ?, ?), (?, ?, ?, ?)",
        &[5.into(), 6.into(), "visible".into(), seed.into(), 6.into(), 5.into(), "hidden".into(), seed.into()],
    ).await.unwrap();
    let mut tx = db.begin_transaction().await.unwrap();
    orphan
        .set_attribute("owner_type", "external")
        .set_attribute("owner_id", 6);
    orphan.save_in_transaction(&mut tx).await.unwrap();
    assert_ne!(
        tx.scalar(read_timestamp, &[5.into()]).await.unwrap(),
        seed.into()
    );
    assert_eq!(
        tx.scalar(read_timestamp, &[6.into()]).await.unwrap(),
        seed.into()
    );
    tx.roll_back(None).await.unwrap();
    assert_eq!(
        db.scalar(read_timestamp, &[5.into()]).await.unwrap(),
        seed.into()
    );

    let mut orphan = TouchNode::query_on(&db).find_or_fail(4).await.unwrap();
    orphan
        .set_attribute("owner_type", "external")
        .set_attribute("owner_id", 5);
    orphan.save().await.unwrap();
    assert_eq!(
        db.scalar(read_timestamp, &[6.into()]).await.unwrap(),
        seed.into()
    );
    orphan
        .set_attribute("owner_type", "node")
        .set_attribute("owner_id", 6);
    orphan.save().await.unwrap();
    assert_ne!(
        db.scalar(read_timestamp, &[6.into()]).await.unwrap(),
        seed.into()
    );

    let mut tx = db.begin_transaction().await.unwrap();
    orphan.set_attribute("owner_type", "unregistered");
    assert!(matches!(
        orphan.save_in_transaction(&mut tx).await,
        Err(DatabaseError::InvalidQuery("unknown morph type"))
    ));
    tx.roll_back(None).await.unwrap();
    assert_eq!(
        db.scalar("SELECT owner_type FROM touch_nodes WHERE id = 4", &[])
            .await
            .unwrap(),
        "node".into()
    );
    let mut orphan = TouchNode::query_on(&db).find_or_fail(4).await.unwrap();
    orphan.set_attribute("owner_type", Value::Null);
    orphan.save().await.unwrap();
    db.statement("DROP TABLE touch_nodes", &[]).await.unwrap();
}

async fn verify_push(config: Json, id: &str) {
    let (_app, db) = app_with_database(config, vec![Box::new(ObserverProvider)]).await;
    db.statement("DROP TABLE IF EXISTS model_event_articles", &[])
        .await
        .unwrap();
    db.statement("DROP TABLE IF EXISTS model_observer_log", &[])
        .await
        .unwrap();
    db.statement(&format!(
        "CREATE TABLE model_event_articles (id {id}, title VARCHAR(30) UNIQUE NOT NULL, deleted_at DATETIME NULL)"
    ), &[]).await.unwrap();
    db.statement(
        "CREATE TABLE model_observer_log (article_id BIGINT NOT NULL)",
        &[],
    )
    .await
    .unwrap();

    let new = |title: &str| {
        Article::new_instance_on(&db, [("title".into(), title.into())].into()).unwrap()
    };
    let mut root = new("root");
    let mut first = new("first");
    first.set_relation::<BelongsTo<Article, Article>>("nested", Some(Arc::new(new("nested"))));
    let first = Arc::new(first);
    root.set_relation::<HasMany<Article, Article>>(
        "children",
        RelatedCollection::from_shared(vec![
            first.clone(),
            Arc::new(new("blocked")),
            Arc::new(new("after")),
        ]),
    );
    root.set_relation::<BelongsTo<Article, Article>>("alias", Some(first.clone()));
    root.set_relation::<BelongsTo<Article, Article>>("empty", None);

    assert!(!root.push().await.unwrap());
    assert!(root.state().exists());
    let children = root
        .get_relation::<RelatedCollection<Article>>("children")
        .unwrap();
    assert!(children[0].state().exists());
    assert!(!children[1].state().exists());
    assert!(!children[2].state().exists());
    assert!(!first.state().exists());
    assert_eq!(Article::query_on(&db).get().await.unwrap().len(), 3);

    // Retry with the successfully persisted snapshot and share it across two relation outputs.
    let saved_first = children[0].clone();
    root.set_relation::<BelongsTo<Article, Article>>("alias", Some(saved_first));
    let before = db.table("model_observer_log").get().await.unwrap().len();
    assert!(root.push_quietly().await.unwrap());
    assert_eq!(
        db.table("model_observer_log").get().await.unwrap().len(),
        before
    );
    assert_eq!(Article::query_on(&db).get().await.unwrap().len(), 5);
    let children = root
        .get_relation::<RelatedCollection<Article>>("children")
        .unwrap();
    let alias = root.get_relation::<Option<Arc<Article>>>("alias").unwrap();
    assert!(Arc::ptr_eq(&children[0], alias.as_ref().unwrap()));
    assert!(children.iter().all(|child| child.state().exists()));
    assert!(
        children[0]
            .get_relation::<Option<Arc<Article>>>("nested")
            .unwrap()
            .as_ref()
            .unwrap()
            .state()
            .exists()
    );

    let morph = MorphTo::<Article>::new("title", "id").target::<Article>("root", "id");
    root.load("morph", &morph).await.unwrap();
    root.set_attribute("title", "renamed");
    assert!(root.push_quietly().await.unwrap());
    let target = root
        .get_relation::<Option<openrust::database::eloquent::relations::MorphTarget>>("morph")
        .unwrap();
    assert_eq!(target.as_ref().unwrap().alias(), "root");
    assert!(
        target
            .as_ref()
            .unwrap()
            .downcast_ref::<Article>()
            .unwrap()
            .state()
            .exists()
    );

    let mut duplicate = new("duplicate root");
    duplicate.set_relation::<HasMany<Article, Article>>(
        "children",
        RelatedCollection::from_shared(vec![Arc::new(new("first")), Arc::new(new("skipped"))]),
    );
    assert!(matches!(
        duplicate.push().await,
        Err(DatabaseError::Driver { .. })
    ));
    assert!(duplicate.state().exists());
    let mut skipped = Article::query_on(&db);
    skipped.r#where("title", "=", "skipped");
    assert!(skipped.first().await.unwrap().is_none());
}

#[tokio::test]
async fn model_events_on_sqlite() {
    verify_push(
        json!({"driver":"sqlite","database":":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
    verify_touch_owners(
        json!({"driver":"sqlite","database":":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
    verify_refreshes(
        json!({"driver":"sqlite","database":":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
    verify_replication(
        json!({"driver":"sqlite","database":":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
    verify(
        json!({"driver":"sqlite","database":":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
    verify_transaction_backend(
        json!({"driver":"sqlite","database":":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
    verify_mapped_events(
        json!({"driver":"sqlite","database":":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
    verify_after_commit_backend(
        json!({"driver":"sqlite","database":":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
}

#[tokio::test]
async fn model_events_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify_push(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
        verify_touch_owners(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
        verify_refreshes(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
        verify_replication(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
        verify(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
        verify_transaction_backend(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
        verify_after_commit_backend(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
        verify_mapped_events(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
    }
}
