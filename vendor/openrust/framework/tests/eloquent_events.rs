use std::sync::{Arc, Mutex};

use openrust::{
    contracts::{
        container::Container as _,
        events::{Dispatcher as _, Event, Listener, ShouldHandleEventsAfterCommit},
        foundation::Application as _,
    },
    database::{
        Connection, DB, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Model, ModelMetadata, ModelState, Observer,
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
            relations::HasMany,
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
            .softDeletes("deleted_at")
            .fillable(&["title"]);
        &METADATA
    }

    #[allow(non_snake_case, reason = "Implements the Eloquent event mapping hook")]
    fn dispatchesEvents(&self, event: ModelEvent) -> Option<Arc<dyn Event>> {
        let name = match event {
            Saving => "article.saving",
            Saved => "article.saved",
            _ => return None,
        };
        let Value::Text(title) = self.getAttribute("title")? else {
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
            DB::connection(None).map_err(EventError::Transaction)?;
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
            let title = model.getAttribute("title").cloned();
            match (event, title.as_ref()) {
                (Creating, Some(Value::Text(title))) if title == "cancel" => {
                    return Ok(ModelEventResponse::Cancel);
                }
                (Creating, Some(Value::Text(title))) if title == "draft" => {
                    model.setAttribute("title", "published");
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
                                &[model.getKey().unwrap().clone()]
                            )
                            .await?,
                        model.getAttribute("title").unwrap().clone(),
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
            match model.getAttribute("title") {
                Some(Value::Text(title)) if title == "blocked" => {
                    return Ok(ModelEventResponse::Cancel);
                }
                Some(Value::Text(title)) if title == "observer" => {
                    model.setAttribute("title", "registered");
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
                    &[model.getKey().unwrap().clone()],
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

    fn handleInTransaction<'a>(
        &'a self,
        _: ModelEvent,
        model: &'a mut Article,
        transaction: &'a mut openrust::database::Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            transaction
                .insert(
                    "INSERT INTO model_observer_log (article_id) VALUES (?)",
                    &[model.getKey().unwrap().clone()],
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
            if model.getAttribute("title") == Some(&"after-veto".into()) {
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
            let title = model.getAttribute("title").unwrap();
            if title == &"after-error".into() {
                return Err(DatabaseError::InvalidQuery("observer after commit"));
            }
            if title == &"event-snapshot".into() {
                assert_eq!(
                    openrust::database::DB::connection(None)?.getName(),
                    connection.getName()
                );
            }
            assert_eq!(
                connection
                    .scalar(
                        "SELECT COUNT(*) FROM model_event_articles WHERE id = ?",
                        &[model.getKey().unwrap().clone()]
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
    Article::observeAfterCommit(app, CommittedAudit).unwrap();
    let article = |title: &str| {
        Article::queryOn(db)
            .make([("title".into(), title.into())].into())
            .unwrap()
    };
    let log_count = "SELECT COUNT(*) FROM committed_observer_log";

    let mut transaction = db.beginTransaction().await.unwrap();
    assert!(
        !article("after-veto")
            .saveInTransaction(&mut transaction)
            .await
            .unwrap()
    );
    article("outer-rollback")
        .saveInTransaction(&mut transaction)
        .await
        .unwrap();
    transaction.rollBack(None).await.unwrap();
    assert_eq!(db.scalar(log_count, &[]).await.unwrap(), Value::Integer(0));

    let mut transaction = db.beginTransaction().await.unwrap();
    transaction.beginTransaction().await.unwrap();
    article("savepoint-rollback")
        .saveInTransaction(&mut transaction)
        .await
        .unwrap();
    transaction.rollBack(None).await.unwrap();
    transaction.beginTransaction().await.unwrap();
    let mut committed = article("event-snapshot");
    committed.saveInTransaction(&mut transaction).await.unwrap();
    committed.setAttribute("title", "later-change");
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

    let mut transaction = db.beginTransaction().await.unwrap();
    article("after-error")
        .saveInTransaction(&mut transaction)
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
    Article::registerModelEvent(app, Created, TransactionAudit).unwrap();
    let article = |title: &str| {
        Article::queryOn(db)
            .make([("title".into(), title.into())].into())
            .unwrap()
    };
    let mut wrong = other.beginTransaction().await.unwrap();
    assert!(matches!(
        article("wrong").saveInTransaction(&mut wrong).await,
        Err(DatabaseError::InvalidQuery("model transaction connection"))
    ));
    wrong.rollBack(None).await.unwrap();

    let mut transaction = db.beginTransaction().await.unwrap();
    assert!(
        !article("cancel")
            .saveInTransaction(&mut transaction)
            .await
            .unwrap()
    );
    let mut rolled_back = article("rolled-back");
    assert!(
        rolled_back
            .saveInTransaction(&mut transaction)
            .await
            .unwrap()
    );
    assert!(rolled_back.getKey().is_some());
    assert_eq!(
        transaction
            .scalar("SELECT COUNT(*) FROM model_observer_log", &[])
            .await
            .unwrap(),
        Value::Integer(1)
    );
    transaction.rollBack(None).await.unwrap();
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
        article("closed").saveInTransaction(&mut transaction).await,
        Err(DatabaseError::TransactionClosed)
    ));

    let mut transaction = db.beginTransaction().await.unwrap();
    let mut committed = article("commit");
    assert!(committed.saveInTransaction(&mut transaction).await.unwrap());
    assert!(
        committed
            .updateInTransaction(
                [("title".into(), "updated-tx".into())].into(),
                &mut transaction
            )
            .await
            .unwrap()
    );
    assert!(
        committed
            .deleteInTransaction(&mut transaction)
            .await
            .unwrap()
    );
    assert!(committed.trashed());
    assert!(
        committed
            .restoreInTransaction(&mut transaction)
            .await
            .unwrap()
    );
    assert!(!committed.trashed());
    transaction.commit().await.unwrap();
    let key = committed.getKey().unwrap().clone();
    let fresh = Article::queryOn(db).findOrFail(key.clone()).await.unwrap();
    assert_eq!(fresh.getAttribute("title"), Some(&"updated-tx".into()));

    let mut transaction = db.beginTransaction().await.unwrap();
    transaction.beginTransaction().await.unwrap();
    assert!(
        committed
            .forceDeleteInTransaction(&mut transaction)
            .await
            .unwrap()
    );
    transaction.rollBack(None).await.unwrap();
    transaction.commit().await.unwrap();
    assert!(Article::queryOn(db).find(key).await.unwrap().is_some());
}

async fn app_with_database(
    config: Json,
    providers: Vec<Box<dyn ServiceProvider>>,
) -> (Application, Connection) {
    let mut app = Application::configure(std::env::temp_dir())
        .withProviders(providers)
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
    let mut blocked = Article::queryOn(&db)
        .make([("title".into(), "blocked".into())].into())
        .unwrap();
    assert!(!blocked.save().await.unwrap());
    assert!(!blocked.state().exists());
    let observed = Article::queryOn(&db)
        .create([("title".into(), "observer".into())].into())
        .await
        .unwrap();
    assert_eq!(observed.getAttribute("title"), Some(&"registered".into()));
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
        Article::registerModelEvent(&app, event, Record(log.clone())).unwrap();
    }
    let take = || std::mem::take(&mut *log.lock().unwrap());
    let article = |title: &str| {
        Article::queryOn(&db)
            .make([("title".into(), title.into())].into())
            .unwrap()
    };

    let mut model = article("cancel");
    assert!(!model.save().await.unwrap());
    assert!(!model.state().exists());
    assert_eq!(take(), [Saving, Creating]);
    model.setAttribute("title", "draft");
    assert!(model.save().await.unwrap());
    assert_eq!(model.getAttribute("title"), Some(&"published".into()));
    assert_eq!(take(), [Saving, Creating, Created, Saved]);
    assert!(model.save().await.unwrap());
    assert_eq!(take(), [Saving, Saved]);
    model.setAttribute("title", "renamed");
    assert!(model.save().await.unwrap());
    assert_eq!(take(), [Saving, Updating, Updated, Saved]);

    let key = model.getKey().unwrap().clone();
    Article::queryOn(&db).findOrFail(key.clone()).await.unwrap();
    assert_eq!(take(), [Retrieved]);
    assert!(model.delete().await.unwrap());
    assert!(model.trashed());
    assert_eq!(take(), [Deleting, Trashed, Deleted]);
    assert!(model.restore().await.unwrap());
    assert_eq!(
        take(),
        [Restoring, Saving, Updating, Updated, Saved, Restored]
    );
    assert!(model.forceDelete().await.unwrap());
    assert_eq!(take(), [ForceDeleting, Deleting, Deleted, ForceDeleted]);
    assert!(
        Article::queryOn(&db)
            .withTrashed()
            .find(key)
            .await
            .unwrap()
            .is_none()
    );

    let mut quiet = article("quiet");
    assert!(quiet.saveQuietly().await.unwrap());
    assert!(take().is_empty());
    let mut duplicate = article("quiet");
    assert!(duplicate.save().await.is_err());
    assert_eq!(take(), [Saving, Creating]);
    duplicate.setAttribute("title", "post-error");
    assert!(matches!(
        duplicate.save().await,
        Err(DatabaseError::InvalidQuery("saved listener failed"))
    ));
    assert!(duplicate.state().exists());
    assert_eq!(take(), [Saving, Creating, Created, Saved]);
    duplicate.setAttribute("title", "retry");
    assert!(duplicate.save().await.unwrap());
    assert_eq!(take(), [Saving, Updating, Updated, Saved]);

    let (_other, other_db) =
        app_with_database(json!({"driver":"sqlite","database":":memory:"}), vec![]).await;
    other_db.statement("CREATE TABLE model_event_articles (id INTEGER PRIMARY KEY, title TEXT, deleted_at TEXT)", &[]).await.unwrap();
    let other = Article::queryOn(&other_db)
        .create([("title".into(), "observer".into())].into())
        .await
        .unwrap();
    assert_eq!(other.getAttribute("title"), Some(&"observer".into()));
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
    Article::registerModelEvent(&app, Creating, Record(Arc::default())).unwrap();
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
    Article::registerModelEvent(&app, Saving, Record(fallback.clone())).unwrap();
    Article::registerModelEvent(&app, Saved, Record(fallback.clone())).unwrap();

    for (title, saved) in [
        ("mapped-cancel", false),
        ("mapped-handled", true),
        ("mapped-zero", true),
    ] {
        let mut article =
            Article::newInstanceOn(&db, [("title".into(), title.into())].into()).unwrap();
        assert_eq!(article.save().await.unwrap(), saved);
        assert_eq!(article.state().exists(), saved);
    }
    assert_eq!(*fallback.lock().unwrap(), [Saving, Saved]);

    let mut error =
        Article::newInstanceOn(&db, [("title".into(), "mapped-error".into())].into()).unwrap();
    assert!(matches!(
        error.save().await,
        Err(DatabaseError::ModelEvent(_))
    ));
    assert!(!error.state().exists());

    for (suffix, commit) in [("", true), ("-rollback", false)] {
        let mut transaction = db.beginTransaction().await.unwrap();
        let mut article =
            Article::newInstanceOn(&db, [("title".into(), "mapped-transaction".into())].into())
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
        assert!(article.saveInTransaction(&mut transaction).await.unwrap());
        article.setAttribute("title", format!("edited{suffix}"));
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
            transaction.rollBack(None).await.unwrap();
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

    let before = calls.lock().unwrap().len();
    let mut quiet =
        Article::newInstanceOn(&db, [("title".into(), "mapped-quiet".into())].into()).unwrap();
    assert!(quiet.saveQuietly().await.unwrap());
    if sqlite {
        isolated_db.statement(&format!("CREATE TABLE model_event_articles (id {id}, title VARCHAR(30) UNIQUE NOT NULL, deleted_at DATETIME NULL)"), &[]).await.unwrap();
    }
    let mut other = Article::newInstanceOn(
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
            assert!(replica.getKey().is_none());
            assert!(replica.relationLoaded("children"));
            assert_eq!(replica.getConnectionName(), Some(connection.getName()));
            replica.setAttribute("title", "from-listener");
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
    let mut source = ReplicaArticle::queryOn(&db)
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
    source.setAttribute("laravel_through_key", 1_i64);
    source.setAttribute("title", "unsaved-edit");
    ReplicaArticle::registerModelEvent(&app, Replicating, ReplicatingListener).unwrap();

    let mut copy = source.replicate(&["secret", "id"]).await.unwrap();
    assert_eq!(
        copy.getAttribute("title"),
        Some(&Value::from("from-listener"))
    );
    for column in [
        "id",
        "secret",
        "created_at",
        "updated_at",
        "laravel_through_key",
    ] {
        assert!(copy.getAttribute(column).is_none(), "{column}");
    }
    assert!(!copy.state().wasRecentlyCreated());
    assert!(copy.state().getOriginal().is_empty());
    assert!(copy.state().getChanges().is_empty());
    assert_eq!(copy.getConnectionName(), source.getConnectionName());
    copy.unsetRelation("children");
    assert!(source.relationLoaded("children"));
    assert!(copy.save().await.unwrap());
    assert_ne!(copy.getKey(), source.getKey());

    let mut quiet = source.replicateQuietly(&[]).await.unwrap();
    assert_eq!(
        quiet.getAttribute("title"),
        Some(&Value::from("unsaved-edit"))
    );
    assert_eq!(quiet.getAttribute("secret"), Some(&Value::from("private")));
    assert!(quiet.relationLoaded("children"));
    assert!(quiet.save().await.unwrap());
    assert_ne!(quiet.getKey(), copy.getKey());
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

#[tokio::test]
async fn model_events_on_sqlite() {
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
        verify_replication(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
        verify(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
        verify_transaction_backend(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
        verify_after_commit_backend(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
        verify_mapped_events(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
    }
}
