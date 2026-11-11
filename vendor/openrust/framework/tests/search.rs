use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseError, DatabaseManager, DatabaseResult, Value,
        eloquent::{Builder, Model, ModelMetadata, ModelState, Scope, SearchColumn, Searchable},
        query::FullTextOptions,
    },
    pagination::PageOptions,
};
use serde_json::{Value as Json, json};

fn connection(config: Json) -> Connection {
    let config = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"test","connections":{"test":config}}),
    )]));
    DatabaseManager::new(Arc::new(config), ".")
        .connection(None)
        .unwrap()
}

struct Article(ModelState);

impl Model for Article {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("articles").timestamps(None, None);
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

impl Searchable for Article {
    fn searchableColumns() -> &'static [SearchColumn] {
        &[
            SearchColumn::FullText("title"),
            SearchColumn::FullText("body"),
            SearchColumn::Prefix("slug"),
        ]
    }
}

struct Note(ModelState);

impl Model for Note {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("notes")
            .timestamps(None, None)
            .softDeletes("deleted_at");
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

    fn globalScopes() -> &'static [(&'static str, &'static dyn Scope<Self>)] {
        &[("tenant", &Tenant)]
    }
}

struct Tenant;

impl Scope<Note> for Tenant {
    fn apply(&self, query: &mut Builder<'_, Note>) -> DatabaseResult<()> {
        query.r#where("tenant_id", "=", 7);
        Ok(())
    }
}

impl Searchable for Note {
    fn searchableColumns() -> &'static [SearchColumn] {
        &[
            SearchColumn::Prefix("id"),
            SearchColumn::Contains("title"),
            SearchColumn::Prefix("slug"),
        ]
    }
}

async fn seed_notes(db: &Connection) -> DatabaseResult<()> {
    db.unprepared(
        "CREATE TABLE notes (id INTEGER PRIMARY KEY, tenant_id INTEGER, title TEXT, slug TEXT, deleted_at TEXT);
         INSERT INTO notes VALUES
            (12, 7, 'Rust cookbook', 'guide', NULL),
            (123, 7, 'Plant notes', 'rust-layout', NULL),
            (3, 7, 'Plant notes', 'deep-rust', NULL),
            (4, 7, 'Rust archive', 'archive', '2026-01-01 00:00:00'),
            (5, 8, 'Rust private', 'private', NULL);",
    ).await?;
    Ok(())
}

#[tokio::test]
async fn model_search_respects_scopes_soft_deletes_and_search_strategies() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    seed_notes(&db).await.unwrap();
    let rust = Note::searchOn(&db, "rust").paginate(1).await.unwrap();
    assert_eq!(rust.total(), 2);
    assert_eq!(rust.items()[0].getKey(), Some(&123.into()));
    let ordered = Note::searchOn(&db, "rust")
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(ordered.first().unwrap().getKey(), Some(&12.into()));
    let numeric = Note::searchOn(&db, "12").get().await.unwrap();
    assert_eq!(numeric.len(), 1);
    assert_eq!(numeric.first().unwrap().getKey(), Some(&12.into()));
    let archived = Note::searchOn(&db, "rust")
        .onlyTrashed()
        .get()
        .await
        .unwrap();
    assert_eq!(archived.len(), 1);
    assert_eq!(archived.first().unwrap().getKey(), Some(&4.into()));
    assert_eq!(Note::searchOn(&db, "  ").get().await.unwrap().len(), 3);
    assert!(
        Note::searchOn(&db, "rust")
            .r#where("id", "=", 3)
            .get()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn full_text_binds_the_search_string_and_rejects_unsupported_dialects() {
    let sqlite = connection(json!({"driver":"sqlite","database":":memory:"}));
    assert!(matches!(
        sqlite
            .table("articles")
            .whereFullText(&["title"], "rust")
            .toSql(),
        Err(DatabaseError::InvalidQuery(_))
    ));
    let mysql = connection(json!({"driver":"mysql","database":"example","prefix":"app_"}));
    let query = mysql
        .table("articles")
        .r#where("published", "=", true)
        .whereNested(|group| {
            group
                .whereFullText(&["articles.title", "articles.body"], "O'Reilly ?")
                .orWhereFullTextWith(&["title"], "+rust -draft", FullTextOptions::Boolean)
        });
    assert_eq!(
        query.toSql().unwrap(),
        "select * from `app_articles` where `published` = ? and (match (`app_articles`.`title`, `app_articles`.`body`) against (? in natural language mode) or match (`title`) against (? in boolean mode))"
    );
    assert_eq!(
        query.getBindings().unwrap(),
        [true.into(), "O'Reilly ?".into(), "+rust -draft".into()]
    );
    assert!(matches!(
        mysql.table("articles").whereFullText(&[], "rust").toSql(),
        Err(DatabaseError::InvalidQuery(_))
    ));
}

#[tokio::test]
async fn mysql_indexes_queries_pages_updates_and_deletes_searchable_rows() {
    let Ok(url) = std::env::var("OPENRUST_TEST_SEARCH_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SEARCH_MYSQL").is_none());
        return;
    };
    let prefix = format!("search_{:x}_", fastrand::u64(..));
    let db = connection(json!({"url":url,"prefix":prefix}));
    let table = "articles";
    let mut schema = db.getSchemaBuilder();
    schema
        .create(table, |definition| {
            definition.id();
            definition.string("title", 150);
            definition.text("body");
            definition.string("slug", 80).default("");
            definition.fullText(&["title", "body"], None);
        })
        .await
        .unwrap();
    let row = |title: &str, body: &str| {
        BTreeMap::from([("title".into(), title.into()), ("body".into(), body.into())])
    };
    db.table(table)
        .insert(&[
            row("OpenRust framework", "laravel search foundation"),
            row("OpenRust cookbook", "image tutorial"),
            row("Organic gardening", "plant guide"),
        ])
        .await
        .unwrap();

    let page = db
        .table(table)
        .whereFullText(&["title", "body"], "openrust")
        .orderBy("id", "asc")
        .paginateWith(PageOptions {
            perPage: Some(1),
            page: Some(2),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.total(), 2);
    assert_eq!(
        page.items()[0].get("title"),
        Some(&Value::from("OpenRust cookbook"))
    );
    let mut model_query = Article::queryOn(&db);
    model_query.whereFullText(&["title", "body"], "openrust");
    let article = model_query
        .orderBy("id", "asc")
        .first()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        article.getAttribute("title"),
        Some(&Value::from("OpenRust framework"))
    );

    db.table(table)
        .r#where("title", "=", "OpenRust cookbook")
        .update(BTreeMap::from([("slug".into(), "oxide-guide".into())]))
        .await
        .unwrap();
    let page = Article::searchOn(&db, "openrust")
        .r#where("id", ">", 1)
        .orderBy("id", "asc")
        .paginate(1)
        .await
        .unwrap();
    assert_eq!(page.total(), 1);
    assert_eq!(
        page.items()[0].getAttribute("title"),
        Some(&Value::from("OpenRust cookbook"))
    );
    assert_eq!(
        Article::searchOn(&db, "oxide").get().await.unwrap().len(),
        1
    );
    let boolean = db
        .table(table)
        .whereFullTextWith(
            &["title", "body"],
            "+openrust -cookbook",
            FullTextOptions::Boolean,
        )
        .get()
        .await
        .unwrap();
    assert_eq!(boolean.len(), 1);
    assert_eq!(
        boolean[0].get("title"),
        Some(&Value::from("OpenRust framework"))
    );
    assert!(
        !db.table(table)
            .whereFullTextWith(
                &["title", "body"],
                "openrust",
                FullTextOptions::QueryExpansion
            )
            .get()
            .await
            .unwrap()
            .is_empty()
    );

    let mut locker = db.beginTransaction().await.unwrap();
    locker
        .table(table)
        .r#where("title", "=", "Organic gardening")
        .update(BTreeMap::from([("body".into(), "temporary draft".into())]))
        .await
        .unwrap();
    let (release, released) = tokio::sync::oneshot::channel();
    let release = Arc::new(std::sync::Mutex::new(Some(release)));
    let locker = tokio::spawn(async move {
        released.await.unwrap();
        locker.rollBack(None).await.unwrap();
    });
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    db.transaction(
        |transaction| {
            let release = release.clone();
            let attempts = attempts.clone();
            Box::pin(async move {
                let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                transaction
                    .unprepared("SET SESSION innodb_lock_wait_timeout = 1")
                    .await?;
                let result = transaction
                    .table(table)
                    .r#where("title", "=", "Organic gardening")
                    .update(BTreeMap::from([("body".into(), "openrust example".into())]))
                    .await;
                if attempt == 0 {
                    release.lock().unwrap().take().unwrap().send(()).unwrap();
                    assert!(result.is_err(), "the held row should force a lock timeout");
                }
                result
            })
        },
        2,
    )
    .await
    .unwrap();
    locker.await.unwrap();
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(
        db.table(table)
            .whereFullText(&["title", "body"], "openrust")
            .get()
            .await
            .unwrap()
            .len(),
        3
    );
    db.table(table)
        .r#where("title", "=", "OpenRust cookbook")
        .delete()
        .await
        .unwrap();
    assert_eq!(
        db.table(table)
            .whereFullText(&["title", "body"], "openrust")
            .get()
            .await
            .unwrap()
            .len(),
        2
    );
    schema.dropIfExists(table).await.unwrap();
}

#[tokio::test]
async fn model_search_paginates_through_the_http_entry_point() {
    use axum::{Router, extract::State};
    use openrust::{
        contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
        foundation::{Application, configuration::Routing, http::Kernel},
        http::{HttpError, HttpFuture, Request, Response, StatusCode, client::Factory},
        support::facades::Route,
    };

    fn endpoint(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let term = request
                .query(Some("q"))?
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or(HttpError::InvalidRequest)?;
            let internal = |source: DatabaseError| HttpError::Internal {
                source: Box::new(source),
            };
            let page = Note::search(&term)
                .map_err(internal)?
                .paginate(1)
                .await
                .map_err(internal)?;
            Response::json(
                StatusCode::OK,
                page.map(|note| match note.getKey() {
                    Some(Value::Integer(id)) => *id,
                    _ => panic!("expected integer key"),
                }),
            )
        })
    }

    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handleTransport(request).await
    }

    let app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok(BTreeMap::from([(
                "database".into(),
                json!({
                    "default": "test",
                    "connections": {"test": {"driver":"sqlite","database":":memory:"}}
                }),
            )]))
        })
        .withRouting(Routing::default().web(|| Route::get("/search", endpoint)))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let db = kernel
        .get_application()
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    seed_notes(&db).await.unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new()
        .fallback(transport)
        .with_state(Arc::new(kernel));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let response = Factory::new()
        .unwrap()
        .get(&format!("http://{address}/search?q=rust&page=2"))
        .await
        .unwrap();
    assert!(
        response.ok(),
        "{}",
        String::from_utf8_lossy(response.body())
    );
    let page = response.json::<Json>().unwrap();
    assert_eq!(page["total"], 2);
    assert_eq!(page["current_page"], 2);
    assert_eq!(page["data"], json!([12]));
    server.abort();
}
