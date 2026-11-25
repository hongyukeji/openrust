use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use openrust::{
    config::Repository,
    contracts::localization::{Loader, Localizer as _},
    database::{Connection, DatabaseManager, Value, query::Record},
    localization::{FileLoader, Line, Lines, Localizer, TranslationError, TranslationResource},
};
use openrust_translation_database::DatabaseLoader;
use serde_json::{Value as Json, json};

fn connection(url: &str, name: &str, driver: &str) -> Connection {
    let repository = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({
            "default":name, "connections":{name:{"driver":driver,"url":url,"max_connections":2}}
        }),
    )]));
    DatabaseManager::new(Arc::new(repository), ".")
        .connection(None)
        .unwrap()
}

struct Fixture {
    connection: Connection,
    table: String,
    _directory: Option<tempfile::TempDir>,
}
impl Fixture {
    async fn new() -> Option<Self> {
        let driver =
            std::env::var("OPENRUST_TEST_TRANSLATION_DRIVER").unwrap_or_else(|_| "pgsql".into());
        let (url, directory) = if driver == "sqlite" {
            let directory = tempfile::tempdir().unwrap();
            let database = directory.path().join("translations.sqlite");
            std::fs::File::create(&database).unwrap();
            let url = format!("sqlite:{}", database.display());
            (url, Some(directory))
        } else {
            let (url_key, required, container) = match driver.as_str() {
                "pgsql" => (
                    "OPENRUST_TEST_POSTGRES_URL",
                    "OPENRUST_REQUIRE_POSTGRES",
                    "OPENRUST_ACCEPTANCE_POSTGRES_CONTAINER",
                ),
                "mysql" => (
                    "OPENRUST_TEST_MYSQL_URL",
                    "OPENRUST_REQUIRE_MYSQL",
                    "OPENRUST_ACCEPTANCE_MYSQL_CONTAINER",
                ),
                _ => panic!("unsupported translation acceptance driver"),
            };
            let Ok(url) = std::env::var(url_key) else {
                assert_ne!(std::env::var(required).as_deref(), Ok("1"));
                eprintln!("SKIP: requires isolated {driver} acceptance wrapper");
                return None;
            };
            assert_eq!(std::env::var(required).as_deref(), Ok("1"));
            assert!(std::env::var_os(container).is_some());
            (url, None)
        };
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let table = format!(
            "translation_loader_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let connection = connection(&url, &table, &driver);
        connection
            .get_schema_builder()
            .create(&table, |table| {
                for name in ["group", "key", "text"] {
                    table.text(name);
                }
            })
            .await
            .unwrap();
        Some(Self {
            connection,
            table,
            _directory: directory,
        })
    }
    fn loader(&self) -> DatabaseLoader {
        DatabaseLoader::new(self.connection.clone())
            .with_table(&self.table)
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
            .get_schema_builder()
            .drop_if_exists(&self.table)
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
fn get(localizer: &Localizer, key: &str, locale: Option<&str>) -> String {
    localizer.get(key, &BTreeMap::new(), locale)
}

#[tokio::test]
async fn database_locale_maps_nested_and_literal_keys_are_isolated() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.raw(
        "",
        "messages",
        "",
        "nested.title",
        r#"{"en-US":"Database","fr":"Bonjour","de":null}"#,
    )
    .await;
    f.put("en-US", "*", "", "nested.title", json!("Literal"))
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
            .load("en-US", "messages", Some("package"))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        loader.load("en-US", "*", None).await.unwrap()["nested.title"],
        Line::Text("Literal".into())
    );
    f.close().await;
}

#[tokio::test]
async fn database_file_overlay_preserves_nested_lists_empty_strings_and_locale_fallback() {
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
        .with_fallback(Arc::new(FileLoader::new(directory.path().into())));
    assert_eq!(
        loader.load("fr", "*", None).await.unwrap()["Phrase"],
        Line::Text("database".into())
    );
    let localizer = Localizer::new(
        FileLoader::new(directory.path().into()),
        "fr".into(),
        "en".into(),
    )
    .unwrap();
    localizer
        .load_from(
            &loader,
            &[
                resource("fr", "messages", None),
                resource("en", "messages", None),
            ],
        )
        .await
        .unwrap();
    assert_eq!(get(&localizer, "messages.welcome", None), "Base");
    assert_eq!(get(&localizer, "messages.nested.file", None), "kept");
    assert_eq!(get(&localizer, "messages.nested.shared", None), "database");
    assert_eq!(get(&localizer, "messages.items.0", None), "new");
    assert_eq!(get(&localizer, "messages.items.1", None), "kept");
    assert_eq!(get(&localizer, "messages.empty", None), "");
    assert_eq!(get(&localizer, "messages.fallback", None), "English");
    f.connection
        .table(&f.table)
        .r#where("key", "=", "welcome")
        .delete()
        .await
        .unwrap();
    localizer
        .load_from(&loader, &[resource("fr", "messages", None)])
        .await
        .unwrap();
    assert_eq!(get(&localizer, "messages.welcome", None), "Fichier");
    f.close().await;
}

#[tokio::test]
async fn database_invalid_rows_and_limits_never_publish_partial_catalogs() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    let localizer = Localizer::new(
        FileLoader::new(directory.path().into()),
        "en".into(),
        "en".into(),
    )
    .unwrap();
    let loader = f.loader();
    f.put("en", "messages", "", "title", json!("previous"))
        .await;
    localizer
        .load_from(&loader, &[resource("en", "messages", None)])
        .await
        .unwrap();
    f.clear().await;
    f.put("en", "messages", "", "title", json!("new")).await;
    f.raw("en", "broken", "", "title", "invalid JSON").await;
    assert!(matches!(
        localizer
            .load_from(
                &loader,
                &[
                    resource("en", "messages", None),
                    resource("en", "broken", None)
                ]
            )
            .await,
        Err(TranslationError::Source(_))
    ));
    assert_eq!(get(&localizer, "messages.title", None), "previous");
    f.clear().await;
    f.put("en", "messages", "", "title", json!("one")).await;
    f.put("en", "messages", "", "second", json!("two")).await;
    assert_eq!(
        f.loader()
            .with_max_rows(2)
            .unwrap()
            .load("en", "messages", None)
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(matches!(
        f.loader()
            .with_max_rows(1)
            .unwrap()
            .load("en", "messages", None)
            .await,
        Err(TranslationError::Source(_))
    ));
    assert_eq!(get(&localizer, "messages.title", None), "previous");
    f.close().await;
}

#[tokio::test]
async fn database_duplicate_and_overlapping_keys_fail_in_either_insert_order() {
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
async fn database_connections_tables_and_published_applications_do_not_share_catalogs() {
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
        Localizer::new(
            FileLoader::new(directory.path().into()),
            "en".into(),
            "en".into(),
        )
        .unwrap()
    };
    let a = app();
    let b = app();
    a.load_from(&first.loader(), &[resource("en", "messages", None)])
        .await
        .unwrap();
    b.load_from(&second.loader(), &[resource("en", "messages", None)])
        .await
        .unwrap();
    assert_eq!(get(&a, "messages.title", None), "first");
    assert_eq!(get(&b, "messages.title", None), "second");
    first.close().await;
    let failed = DatabaseLoader::new(second.connection.clone())
        .with_table("missing_translation_table")
        .unwrap()
        .with_fallback(Arc::new(FileLoader::new(directory.path().into())));
    let error = a
        .load_from(&failed, &[resource("en", "messages", None)])
        .await
        .unwrap_err();
    assert!(
        matches!(&error, TranslationError::Source(source) if source.downcast_ref::<openrust::database::DatabaseError>().is_some())
    );
    assert!(error.to_string().starts_with("翻译或语言设置失败："));
    assert_eq!(get(&a, "messages.title", None), "first");
    second.close().await;
}

#[tokio::test]
async fn invalid_configuration_and_resource_names_are_rejected_before_sql() {
    let connection = connection(
        "postgres://unused:unused@127.0.0.1:1/unused",
        "validation",
        "pgsql",
    );
    for name in [
        "",
        "translations; drop table users",
        "../translations",
        "schema.translations",
    ] {
        assert!(
            DatabaseLoader::new(connection.clone())
                .with_table(name)
                .is_err()
        );
    }
    for limit in [0, u64::MAX] {
        assert!(
            DatabaseLoader::new(connection.clone())
                .with_max_rows(limit)
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
async fn database_explicit_provider_publishes_the_app_catalog_to_real_http() {
    use axum::{extract::State, handler::Handler as _};
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing, http::Kernel},
        http::{HttpError, HttpFuture, Request, Response, StatusCode},
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
                app.make::<Localizer>()
                    .await?
                    .load_from(&self.0, &[resource("en", "messages", None)])
                    .await?;
                Ok(())
            })
        }
    }
    fn response(_request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            let value = Lang::get("messages.title", &BTreeMap::new(), None)?;
            Response::json(StatusCode::OK, json!({"title":value}))
        })
    }
    fn routes() -> Result<(), RoutingError> {
        Route::get("/translations", response)?;
        Ok(())
    }
    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
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
            .with_providers(vec![Box::new(Publish(fixture.loader()))])
            .with_routing(Routing::default().api(routes))
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
async fn database_spatie_schema_native_json_and_default_table_work() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.connection
        .get_schema_builder()
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
        .with_table("absent_lines")
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
    files.add_namespace("shop", directory.path().to_path_buf());
    let namespaced = missing.with_fallback(Arc::new(files));
    assert_eq!(
        namespaced
            .load("en", "messages", Some("shop"))
            .await
            .unwrap()["welcome"],
        Line::Text("Package file".into())
    );
    f.connection
        .get_schema_builder()
        .drop_if_exists("language_lines")
        .await
        .unwrap();
    f.close().await;
}

#[tokio::test]
async fn bcp47_upgrade_sql_is_atomic_idempotent_and_conflict_safe() {
    let driver =
        std::env::var("OPENRUST_TEST_TRANSLATION_DRIVER").unwrap_or_else(|_| "pgsql".into());
    let script = match driver.as_str() {
        "pgsql" => include_str!("../database/upgrade-bcp47/pgsql.sql"),
        "mysql" => include_str!("../database/upgrade-bcp47/mysql.sql"),
        "sqlite" => include_str!("../database/upgrade-bcp47/sqlite.sql"),
        _ => panic!("未知验收驱动"),
    };
    for native_json in [false, true] {
        let f = Fixture::new()
            .await
            .expect("迁移验收必须提供隔离实例，不得跳过");
        if native_json {
            let mut schema = f.connection.get_schema_builder();
            schema.drop_if_exists(&f.table).await.unwrap();
            schema
                .create(&f.table, |table| {
                    table.string("group", None);
                    table.string("key", None);
                    table.json("text");
                })
                .await
                .unwrap();
        }
        let old = json!({"zh_CN":"简体", "zh_TW":null, "en_US":"American", "en":"English", "zh-CN":"独立地区"});
        put_locale_map(&f, "greeting", &old, native_json).await;
        let expected = json!({"zh-Hans":"简体", "zh-Hant":null, "en-US":"American", "en":"English", "zh-CN":"独立地区"});
        // 正式 SQL 默认回滚；由框架事务承载同一 SQL 正文，保证所有语句使用同一连接。
        let body = script
            .lines()
            .filter(|line| {
                !matches!(
                    *line,
                    "BEGIN;" | "START TRANSACTION;" | "BEGIN IMMEDIATE;" | "ROLLBACK;"
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
            .replace("language_lines", &f.table);
        let mut transaction = f.connection.begin_transaction().await.unwrap();
        transaction.unprepared(&body).await.unwrap();
        transaction.roll_back(None).await.unwrap();
        assert_eq!(locale_map(&f).await, old);
        for _ in 0..2 {
            let mut transaction = f.connection.begin_transaction().await.unwrap();
            transaction.unprepared(&body).await.unwrap();
            transaction.commit().await.unwrap();
            assert_eq!(locale_map(&f).await, expected);
        }
        assert_eq!(
            f.loader().load("ZH-hans", "messages", None).await.unwrap()["greeting"],
            Line::Text("简体".into())
        );
        assert_eq!(
            f.loader().load("zh-CN", "messages", None).await.unwrap()["greeting"],
            Line::Text("独立地区".into())
        );
        assert!(f.loader().load("zh_CN", "messages", None).await.is_err());
        for conflict in [
            json!({"zh_CN":"旧", "zh-Hans":"新"}),
            json!({"custom_locale":"未审阅映射"}),
        ] {
            put_locale_map(&f, "conflict", &conflict, native_json).await;
            put_locale_map(&f, "pending", &json!({"zh_CN":"待迁移"}), native_json).await;
            let mut transaction = f.connection.begin_transaction().await.unwrap();
            assert!(transaction.unprepared(&body).await.is_err());
            // MySQL 临时表 DDL 不随回滚恢复，在同一专属连接清理失败守卫。
            if driver == "mysql" {
                transaction
                    .unprepared("DROP TEMPORARY TABLE IF EXISTS openrust_locale_guard")
                    .await
                    .unwrap();
            }
            transaction.roll_back(None).await.unwrap();
            assert_eq!(locale_map(&f).await, expected);
            let value = f
                .connection
                .table(&f.table)
                .r#where("key", "=", "conflict")
                .value("text")
                .await
                .unwrap();
            assert_eq!(json_map(value), conflict);
            let pending = f
                .connection
                .table(&f.table)
                .r#where("key", "=", "pending")
                .value("text")
                .await
                .unwrap();
            assert_eq!(json_map(pending), json!({"zh_CN":"待迁移"}));
            for key in ["conflict", "pending"] {
                f.connection
                    .table(&f.table)
                    .r#where("key", "=", key)
                    .delete()
                    .await
                    .unwrap();
            }
        }
        f.close().await;
    }
}

fn json_map(value: Value) -> Json {
    match value {
        Value::Text(text) => serde_json::from_str(&text).unwrap(),
        Value::Json(value) => value,
        other => panic!("语言映射必须为 JSON：{other:?}"),
    }
}

async fn locale_map(f: &Fixture) -> Json {
    json_map(
        f.connection
            .table(&f.table)
            .r#where("key", "=", "greeting")
            .value("text")
            .await
            .unwrap(),
    )
}

async fn put_locale_map(f: &Fixture, key: &str, map: &Json, native: bool) {
    let text = if native {
        Value::Json(map.clone())
    } else {
        Value::Text(map.to_string())
    };
    let row: Record = [
        ("group".into(), Value::from("messages")),
        ("key".into(), Value::from(key)),
        ("text".into(), text),
    ]
    .into();
    f.connection.table(&f.table).insert(&[row]).await.unwrap();
}
