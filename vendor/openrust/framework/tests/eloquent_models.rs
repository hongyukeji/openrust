use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseError, DatabaseManager, DatabaseResult, Value,
        eloquent::{Attributes, Builder, Model, ModelMetadata, ModelState, Scope},
    },
};
use serde_json::{Value as Json, json};

struct Article(ModelState);

impl Model for Article {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("articles")
            .primaryKey("article_id")
            .fillable(&["title", "account", "status"])
            .softDeletes("deleted_at");
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

    fn globalScopes() -> &'static [(&'static str, &'static dyn Scope<Self>)] {
        &[("account", &CurrentAccount)]
    }
}

struct CurrentAccount;

impl Scope<Article> for CurrentAccount {
    fn apply(&self, builder: &mut Builder<'_, Article>) -> DatabaseResult<()> {
        builder.r#where("account", "=", 1);
        Ok(())
    }
}

struct Published;

impl Scope<Article> for Published {
    fn apply(&self, builder: &mut Builder<'_, Article>) -> DatabaseResult<()> {
        builder
            .r#where("status", "=", "published")
            .orWhere("status", "=", "review");
        Ok(())
    }
}

struct Label(ModelState);

impl Model for Label {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("labels")
            .primaryKey("slug")
            .incrementing(false)
            .timestamps(None, None);
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

fn connect(config: Json) -> Connection {
    let settings = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"models","connections":{"models":config}}),
    )]));
    DatabaseManager::new(Arc::new(settings), ".")
        .connection(None)
        .unwrap()
}

fn attrs<const N: usize>(values: [(&str, Value); N]) -> Attributes {
    values
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect()
}

async fn verify(db: Connection, mysql: bool) {
    db.statement("DROP TABLE IF EXISTS articles", &[])
        .await
        .unwrap();
    let id = if mysql {
        "BIGINT AUTO_INCREMENT PRIMARY KEY"
    } else {
        "INTEGER PRIMARY KEY AUTOINCREMENT"
    };
    db.statement(
        &format!("CREATE TABLE articles (article_id {id}, title VARCHAR(80) NOT NULL UNIQUE, account BIGINT NOT NULL, status VARCHAR(20) NOT NULL, protected BIGINT DEFAULT 7, created_at DATETIME, updated_at DATETIME, deleted_at DATETIME)"),
        &[],
    ).await.unwrap();

    let query = || Article::queryOn(&db);
    let mut model = query()
        .create(attrs([
            ("title", "Initial".into()),
            ("account", 1.into()),
            ("status", "published".into()),
            ("protected", 99.into()),
        ]))
        .await
        .unwrap();
    assert!(model.state().exists() && model.state().wasRecentlyCreated());
    assert!(!model.state().isDirty());
    assert_eq!(model.getKey(), Some(&1.into()));
    assert_eq!(model.getConnectionName(), Some("models"));
    assert_eq!(
        model.getAttribute("created_at"),
        model.getAttribute("updated_at")
    );
    assert!(model.getAttribute("created_at").is_some());

    let mut hydrated = query().findOrFail(1).await.unwrap();
    assert!(!hydrated.state().wasRecentlyCreated());
    assert_eq!(hydrated.getAttribute("protected"), Some(&7.into()));
    hydrated
        .setAttribute("article_id", 11)
        .setAttribute("title", "Renamed");
    assert_eq!(hydrated.state().getDirty().len(), 2);
    assert!(hydrated.save().await.unwrap());
    assert_eq!(
        hydrated.state().getChanges().get("title"),
        Some(&"Renamed".into())
    );
    assert!(query().find(1).await.unwrap().is_none());
    assert_eq!(
        query().findOrFail(11).await.unwrap().getKey(),
        Some(&11.into())
    );

    model = query()
        .create(attrs([
            ("title", "Other account".into()),
            ("account", 2.into()),
            ("status", "review".into()),
        ]))
        .await
        .unwrap();
    let other = model.getKey().unwrap().clone();
    let mut draft = query()
        .create(attrs([
            ("title", "Draft".into()),
            ("account", 1.into()),
            ("status", "draft".into()),
        ]))
        .await
        .unwrap();
    let draft_key = draft.getKey().unwrap().clone();

    let mut scoped = query();
    scoped
        .r#where("title", "=", "Renamed")
        .orWhere("title", "=", "Other account");
    scoped.withScope(&Published).unwrap();
    assert_eq!(scoped.get().await.unwrap().len(), 1);
    assert!(matches!(
        query().findOrFail(other.clone()).await,
        Err(DatabaseError::ModelNotFound("articles"))
    ));
    assert!(
        query()
            .withoutGlobalScope("account")
            .find(other.clone())
            .await
            .unwrap()
            .is_some()
    );

    assert!(draft.delete().await.unwrap());
    assert!(draft.state().exists() && draft.trashed());
    assert!(query().find(draft_key.clone()).await.unwrap().is_none());
    assert_eq!(query().onlyTrashed().get().await.unwrap().len(), 1);
    assert!(draft.restore().await.unwrap());
    assert!(!draft.trashed());
    assert!(query().find(draft_key.clone()).await.unwrap().is_some());

    let mut bulk = query();
    bulk.whereKey(draft_key.clone());
    assert_eq!(bulk.delete().await.unwrap(), 1);
    let mut bulk = query().onlyTrashed();
    bulk.whereKey(draft_key.clone());
    assert_eq!(bulk.restore().await.unwrap(), 1);
    let mut bulk = query();
    bulk.r#where("account", "=", 1);
    assert_eq!(
        bulk.update(attrs([("status", "review".into())]))
            .await
            .unwrap(),
        2
    );

    // Failed writes retain the original snapshot so that a corrected retry is safe.
    hydrated.setAttribute("title", "Draft");
    assert!(hydrated.save().await.is_err());
    assert_eq!(
        hydrated.state().getOriginal().get("title"),
        Some(&"Renamed".into())
    );
    hydrated.setAttribute("title", "Retried");
    assert!(hydrated.save().await.unwrap());

    assert!(draft.forceDelete().await.unwrap());
    assert!(!draft.state().exists());
    assert!(
        query()
            .withTrashed()
            .find(draft_key)
            .await
            .unwrap()
            .is_none()
    );
    let mut bulk = query();
    bulk.whereKey(other);
    assert_eq!(bulk.forceDelete().await.unwrap(), 1);
    assert_eq!(query().get().await.unwrap().len(), 1);
    db.statement("DROP TABLE articles", &[]).await.unwrap();

    db.statement("DROP TABLE IF EXISTS labels", &[])
        .await
        .unwrap();
    db.statement(
        "CREATE TABLE labels (slug VARCHAR(80) PRIMARY KEY, title VARCHAR(80))",
        &[],
    )
    .await
    .unwrap();
    let mut label = Label::queryOn(&db).make(Attributes::new()).unwrap();
    assert!(matches!(
        label.fill(attrs([("slug", "old".into())])),
        Err(DatabaseError::MassAssignment(key)) if key == "slug"
    ));
    label.forceFill(attrs([
        ("slug", "old".into()),
        ("title", "Original".into()),
    ]));
    assert!(label.save().await.unwrap());
    assert_eq!(label.getKey(), Some(&"old".into()));
    label.setAttribute("slug", "new");
    assert!(label.save().await.unwrap());
    assert!(Label::queryOn(&db).find("old").await.unwrap().is_none());
    assert!(Label::queryOn(&db).find("new").await.unwrap().is_some());
    assert!(label.delete().await.unwrap());
    assert!(!label.state().exists());
    db.statement("DROP TABLE labels", &[]).await.unwrap();
}

#[tokio::test]
async fn model_persistence_and_scopes_on_sqlite() {
    verify(
        connect(json!({"driver":"sqlite","database":":memory:"})),
        false,
    )
    .await;
}

#[tokio::test]
async fn model_persistence_and_scopes_on_mysql_8_4() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(connect(json!({"url":url})), true).await;
    }
}
