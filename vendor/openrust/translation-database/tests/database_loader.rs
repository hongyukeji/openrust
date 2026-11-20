use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use openrust::{
    config::Repository,
    contracts::translation::{Loader, Translator as _},
    database::{Connection, DatabaseManager, Value, query::Record},
    translation::{FileLoader, Line, Lines, TranslationError, TranslationResource, Translator},
};
use openrust_translation_database::DatabaseLoader;
use serde_json::{Value as Json, json};

fn connection(url: &str, name: &str) -> Connection {
    let repository = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({
            "default":name, "connections":{name:{"driver":"pgsql","url":url,"max_connections":2}}
        }),
    )]));
    DatabaseManager::new(Arc::new(repository), ".")
        .connection(None)
        .unwrap()
}

struct Fixture {
    connection: Connection,
    table: String,
}
impl Fixture {
    async fn new() -> Option<Self> {
        let Ok(url) = std::env::var("OPENRUST_TEST_POSTGRES_URL") else {
            assert_ne!(
                std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                Ok("1")
            );
            eprintln!("SKIP: requires isolated PostgreSQL acceptance wrapper");
            return None;
        };
        assert_eq!(
            std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
            Ok("1")
        );
        assert!(std::env::var("OPENRUST_ACCEPTANCE_POSTGRES_CONTAINER").is_ok());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let table = format!(
            "translation_loader_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let connection = connection(&url, &table);
        connection
            .getSchemaBuilder()
            .create(&table, |table| {
                for name in ["group", "key", "text"] {
                    table.text(name);
                }
            })
            .await
            .unwrap();
        Some(Self { connection, table })
    }
    fn loader(&self) -> DatabaseLoader {
        DatabaseLoader::new(self.connection.clone())
            .withTable(&self.table)
            .unwrap()
    }
    async fn put(&self, locale: &str, group: &str, _namespace: &str, key: &str, value: Json) {
        self.raw(
            locale,
            group,
            "",
            key,
            &serde_json::to_string(&json!({locale:value})).unwrap(),
        )
        .await;
    }
    async fn raw(&self, _locale: &str, group: &str, _namespace: &str, key: &str, text: &str) {
        let record: Record = [("group", group), ("key", key), ("text", text)]
            .into_iter()
            .map(|(k, v)| (k.into(), Value::from(v)))
            .collect();
        self.connection
            .table(&self.table)
            .insert(&[record])
            .await
            .unwrap();
    }
    async fn clear(&self) {
        self.connection.table(&self.table).delete().await.unwrap();
    }
    async fn close(self) {
        self.connection
            .getSchemaBuilder()
            .dropIfExists(&self.table)
            .await
            .unwrap();
        self.connection.disconnect();
    }
}

fn resource<'a>(
    locale: &'a str,
    group: &'a str,
    namespace: Option<&'a str>,
) -> TranslationResource<'a> {
    TranslationResource {
        locale,
        group,
        namespace,
    }
}
fn get(translator: &Translator, key: &str, locale: Option<&str>) -> String {
    translator.get(key, &BTreeMap::new(), locale)
}

#[tokio::test]
async fn postgres_locale_maps_nested_and_literal_keys_are_isolated() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.raw(
        "",
        "messages",
        "",
        "nested.title",
        r#"{"en_US":"Database","fr":"Bonjour","de":null}"#,
    )
    .await;
    f.put("en_US", "*", "", "nested.title", json!("Literal"))
        .await;
    let loader = f.loader();
    assert_eq!(
        loader.load("en-us", "messages", None).await.unwrap()["nested"],
        Line::Group(Lines::from([(
            "title".into(),
            Line::Text("Database".into())
        )]))
    );
    assert_eq!(
        loader.load("fr", "messages", None).await.unwrap()["nested"],
        Line::Group(Lines::from([(
            "title".into(),
            Line::Text("Bonjour".into())
        )]))
    );
    assert!(
        loader
            .load("de", "messages", None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        loader
            .load("en_US", "messages", Some("package"))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        loader.load("en_US", "*", None).await.unwrap()["nested.title"],
        Line::Text("Literal".into())
    );
    f.close().await;
}

#[tokio::test]
async fn postgres_file_overlay_preserves_nested_lists_empty_strings_and_locale_fallback() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("fr")).unwrap();
    std::fs::create_dir_all(directory.path().join("en")).unwrap();
    std::fs::write(directory.path().join("fr/messages.json"),r#"{"welcome":"Fichier","nested":{"file":"kept","shared":"old"},"items":["old","kept"],"empty":"file"}"#).unwrap();
    std::fs::write(
        directory.path().join("en/messages.json"),
        r#"{"fallback":"English"}"#,
    )
    .unwrap();
    std::fs::write(directory.path().join("fr.json"), r#"{"Phrase":"file"}"#).unwrap();
    f.put("fr", "*", "*", "Phrase", json!("database")).await;
    f.put("fr", "messages", "", "welcome", json!("Base")).await;
    f.put("fr", "messages", "", "nested.shared", json!("database"))
        .await;
    f.put("fr", "messages", "", "items.0", json!("new")).await;
    f.put("fr", "messages", "", "empty", json!("")).await;
    let loader = f
        .loader()
        .withFallback(Arc::new(FileLoader::new(directory.path().into())));
    assert_eq!(
        loader.load("fr", "*", None).await.unwrap()["Phrase"],
        Line::Text("database".into())
    );
    let translator = Translator::new(
        FileLoader::new(directory.path().into()),
        "fr".into(),
        "en".into(),
    )
    .unwrap();
    translator
        .loadFrom(
            &loader,
            &[
                resource("fr", "messages", None),
                resource("en", "messages", None),
            ],
        )
        .await
        .unwrap();
    assert_eq!(get(&translator, "messages.welcome", None), "Base");
    assert_eq!(get(&translator, "messages.nested.file", None), "kept");
    assert_eq!(get(&translator, "messages.nested.shared", None), "database");
    assert_eq!(get(&translator, "messages.items.0", None), "new");
    assert_eq!(get(&translator, "messages.items.1", None), "kept");
    assert_eq!(get(&translator, "messages.empty", None), "");
    assert_eq!(get(&translator, "messages.fallback", None), "English");
    f.connection
        .table(&f.table)
        .r#where("key", "=", "welcome")
        .delete()
        .await
        .unwrap();
    translator
        .loadFrom(&loader, &[resource("fr", "messages", None)])
        .await
        .unwrap();
    assert_eq!(get(&translator, "messages.welcome", None), "Fichier");
    f.close().await;
}

#[tokio::test]
async fn postgres_invalid_rows_and_limits_never_publish_partial_catalogs() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    let translator = Translator::new(
        FileLoader::new(directory.path().into()),
        "en".into(),
        "en".into(),
    )
    .unwrap();
    let loader = f.loader();
    f.put("en", "messages", "", "title", json!("previous"))
        .await;
    translator
        .loadFrom(&loader, &[resource("en", "messages", None)])
        .await
        .unwrap();
    f.clear().await;
    f.put("en", "messages", "", "title", json!("new")).await;
    f.raw("en", "broken", "", "title", "invalid JSON").await;
    assert!(matches!(
        translator
            .loadFrom(
                &loader,
                &[
                    resource("en", "messages", None),
                    resource("en", "broken", None)
                ]
            )
            .await,
        Err(TranslationError::Source(_))
    ));
    assert_eq!(get(&translator, "messages.title", None), "previous");
    f.clear().await;
    f.put("en", "messages", "", "title", json!("one")).await;
    f.put("en", "messages", "", "second", json!("two")).await;
    assert_eq!(
        f.loader()
            .withMaxRows(2)
            .unwrap()
            .load("en", "messages", None)
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(matches!(
        f.loader()
            .withMaxRows(1)
            .unwrap()
            .load("en", "messages", None)
            .await,
        Err(TranslationError::Source(_))
    ));
    assert_eq!(get(&translator, "messages.title", None), "previous");
    f.close().await;
}

#[tokio::test]
async fn postgres_duplicate_and_overlapping_keys_fail_in_either_insert_order() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    for keys in [
        ["title", "title"],
        ["nested", "nested.title"],
        ["nested.title", "nested"],
        ["invalid..key", "other"],
    ] {
        f.clear().await;
        for key in keys {
            f.put("en", "messages", "", key, json!("value")).await;
        }
        assert!(matches!(
            f.loader().load("en", "messages", None).await,
            Err(TranslationError::Source(_))
        ));
    }
    f.clear().await;
    f.put(
        "en",
        "messages",
        "",
        &vec!["nested"; 129].join("."),
        json!("value"),
    )
    .await;
    assert!(matches!(
        f.loader().load("en", "messages", None).await,
        Err(TranslationError::Source(_))
    ));
    f.clear().await;
    f.raw("en", "messages", "", "title", "true").await;
    assert!(matches!(
        f.loader().load("en", "messages", None).await,
        Err(TranslationError::Source(_))
    ));
    f.close().await;
}

#[tokio::test]
async fn postgres_connections_tables_and_published_applications_do_not_share_catalogs() {
    let Some(first) = Fixture::new().await else {
        return;
    };
    let Some(second) = Fixture::new().await else {
        return;
    };
    first
        .put("en", "messages", "", "title", json!("first"))
        .await;
    second
        .put("en", "messages", "", "title", json!("second"))
        .await;
    let directory = tempfile::tempdir().unwrap();
    let app = || {
        Translator::new(
            FileLoader::new(directory.path().into()),
            "en".into(),
            "en".into(),
        )
        .unwrap()
    };
    let a = app();
    let b = app();
    a.loadFrom(&first.loader(), &[resource("en", "messages", None)])
        .await
        .unwrap();
    b.loadFrom(&second.loader(), &[resource("en", "messages", None)])
        .await
        .unwrap();
    assert_eq!(get(&a, "messages.title", None), "first");
    assert_eq!(get(&b, "messages.title", None), "second");
    first.close().await;
    let failed = DatabaseLoader::new(second.connection.clone())
        .withTable("missing_translation_table")
        .unwrap()
        .withFallback(Arc::new(FileLoader::new(directory.path().into())));
    let error = a
        .loadFrom(&failed, &[resource("en", "messages", None)])
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "Could not load translation catalog.");
    assert_eq!(get(&a, "messages.title", None), "first");
    second.close().await;
}

#[tokio::test]
async fn invalid_configuration_and_resource_names_are_rejected_before_sql() {
    let connection = connection("postgres://unused:unused@127.0.0.1:1/unused", "validation");
    for name in [
        "",
        "translations; drop table users",
        "../translations",
        "schema.translations",
    ] {
        assert!(
            DatabaseLoader::new(connection.clone())
                .withTable(name)
                .is_err()
        );
    }
    for limit in [0, u64::MAX] {
        assert!(
            DatabaseLoader::new(connection.clone())
                .withMaxRows(limit)
                .is_err()
        );
    }
    let loader = DatabaseLoader::new(connection.clone());
    assert!(matches!(
        loader.load("invalid_locale_!", "messages", None).await,
        Err(TranslationError::InvalidLocale)
    ));
    for (group, namespace) in [
        ("../messages", None),
        ("messages", Some("invalid/name")),
        ("*", Some("package")),
    ] {
        assert!(matches!(
            loader.load("en", group, namespace).await,
            Err(TranslationError::InvalidKey)
        ));
    }
    connection.disconnect();
}

#[tokio::test]
async fn postgres_explicit_provider_publishes_the_app_catalog_to_real_http() {
    use axum::{extract::State, handler::Handler as _};
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing, http::Kernel},
        http::{HttpError, HttpFuture, Method, Request, Response, StatusCode},
        routing::RoutingError,
        support::{
            ProviderResult, ServiceProvider,
            facades::{Lang, Route},
        },
    };
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::{TcpListener, TcpStream},
    };
    struct Publish(DatabaseLoader);
    impl ServiceProvider for Publish {
        fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                app.make::<Translator>()
                    .await?
                    .loadFrom(&self.0, &[resource("en", "messages", None)])
                    .await?;
                Ok(())
            })
        }
    }
    fn response(_request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            let value = Lang::get("messages.title", &BTreeMap::new(), None).map_err(|source| {
                HttpError::Internal {
                    source: Box::new(source),
                }
            })?;
            Response::json(StatusCode::OK, json!({"title":value}))
        })
    }
    fn routes() -> Result<(), RoutingError> {
        Route::r#match(&[Method::GET], "/translations", response)?;
        Ok(())
    }
    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handleTransport(request).await
    }
    let Some(first) = Fixture::new().await else {
        return;
    };
    let Some(second) = Fixture::new().await else {
        return;
    };
    first
        .put("en", "messages", "", "title", json!("HTTP first"))
        .await;
    second
        .put("en", "messages", "", "title", json!("HTTP second"))
        .await;
    let directory = tempfile::tempdir().unwrap();
    let mut servers = Vec::new();
    for fixture in [&first, &second] {
        let app = Application::configure(directory.path().to_path_buf())
            .withProviders(vec![Box::new(Publish(fixture.loader()))])
            .withRouting(Routing::default().api(routes))
            .create()
            .unwrap();
        let mut kernel = Kernel::new(app);
        kernel.bootstrap().await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let service = transport.with_state(Arc::new(kernel)).into_make_service();
        servers.push((
            address,
            tokio::spawn(async move {
                axum::serve(listener, service).await.unwrap();
            }),
        ));
    }
    for index in [0, 1, 0, 1] {
        let response=tokio::time::timeout(std::time::Duration::from_secs(10),async {
            let mut socket=TcpStream::connect(servers[index].0).await.unwrap();
            socket.write_all(b"GET /api/translations HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nConnection: close\r\n\r\n").await.unwrap();
            let mut response=String::new();socket.read_to_string(&mut response).await.unwrap();response
        }).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
        assert!(
            response.contains(if index == 0 {
                "HTTP first"
            } else {
                "HTTP second"
            }),
            "{response}"
        );
    }
    for (_, server) in servers {
        server.abort();
        let _ = server.await;
    }
    first.close().await;
    second.close().await;
}

#[tokio::test]
async fn postgres_spatie_schema_native_json_and_default_table_work() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.connection
        .getSchemaBuilder()
        .create("language_lines", |table| {
            table.id();
            table.string("group", None).index();
            table.string("key", None);
            table.json("text");
            table.timestamps();
        })
        .await
        .unwrap();
    f.connection
        .table("language_lines")
        .insert(&[[
            ("group".into(), Value::from("messages")),
            ("key".into(), Value::from("welcome")),
            (
                "text".into(),
                Value::Json(json!({"en":"Hello :name","fr":"Bonjour :name"})),
            ),
        ]
        .into()])
        .await
        .unwrap();
    let loader = DatabaseLoader::new(f.connection.clone());
    assert_eq!(
        loader.load("en", "messages", None).await.unwrap()["welcome"],
        Line::Text("Hello :name".into())
    );
    assert_eq!(
        loader.load("fr", "messages", None).await.unwrap()["welcome"],
        Line::Text("Bonjour :name".into())
    );
    assert!(
        loader
            .load("de", "messages", None)
            .await
            .unwrap()
            .is_empty()
    );
    // A package lookup must not query SQL, including when the table is absent.
    let missing = DatabaseLoader::new(f.connection.clone())
        .withTable("absent_lines")
        .unwrap();
    assert!(
        missing
            .load("en", "messages", Some("shop"))
            .await
            .unwrap()
            .is_empty()
    );
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("en")).unwrap();
    std::fs::write(
        directory.path().join("en/messages.json"),
        r#"{"welcome":"Package file"}"#,
    )
    .unwrap();
    let mut files = FileLoader::new(directory.path().join("application"));
    files.addNamespace("shop", directory.path().to_path_buf());
    let namespaced = missing.withFallback(Arc::new(files));
    assert_eq!(
        namespaced
            .load("en", "messages", Some("shop"))
            .await
            .unwrap()["welcome"],
        Line::Text("Package file".into())
    );
    f.connection
        .getSchemaBuilder()
        .dropIfExists("language_lines")
        .await
        .unwrap();
    f.close().await;
}
