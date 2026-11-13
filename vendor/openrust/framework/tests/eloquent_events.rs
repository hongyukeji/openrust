use std::sync::{Arc, Mutex};

use openrust::{
    contracts::{
        container::Container as _, events::ShouldHandleEventsAfterCommit,
        foundation::Application as _,
    },
    database::{
        Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Model, ModelMetadata, ModelState, Observer,
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
        },
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

#[tokio::test]
async fn model_events_on_sqlite() {
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
        verify(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
        verify_transaction_backend(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
        verify_after_commit_backend(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
    }
}
