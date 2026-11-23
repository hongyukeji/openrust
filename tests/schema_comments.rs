use openrust::{
    config::Repository,
    database::{
        DatabaseManager,
        migrations::{MigrationEntry, MigrationRegistry, Migrator, RunOptions},
    },
};
use openrust::{
    container::Callback,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    translation::{__, FileLoader, Translator},
};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

static NEXT_LANGUAGE_ROOT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Languages(PathBuf);
impl Languages {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "openrust-schema-languages-{}-{}-{}",
            std::process::id(),
            NEXT_LANGUAGE_ROOT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(path.join("lang")).unwrap();
        for locale in ["en", "zh-Hans", "zh-Hant"] {
            std::fs::create_dir_all(path.join(format!("lang/{locale}"))).unwrap();
            std::fs::copy(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("lang/{locale}/schema.json")),
                path.join(format!("lang/{locale}/schema.json")),
            )
            .unwrap();
        }
        // A new language uses a standard catalog, with no change to schema APIs or migrations.
        std::fs::create_dir_all(path.join("lang/fr")).unwrap();
        std::fs::write(
            path.join("lang/fr/schema.json"),
            r#"{"tables":{"users":"Utilisateurs"},"columns":{"email":"Courriel"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(path.join("lang/de")).unwrap();
        std::fs::write(path.join("lang/de/schema.json"), "{invalid").unwrap();
        Self(path)
    }
    fn application(&self, locale: &str, fallback: &str) -> Application {
        let app = Application::configure(self.0.clone()).create().unwrap();
        app.container()
            .instance_named(
                "translator",
                Arc::new(
                    Translator::new(
                        FileLoader::new(self.0.join("lang")),
                        locale.into(),
                        fallback.into(),
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
        app
    }
}
impl Drop for Languages {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[path = "../database/migrations/mod.rs"]
mod application_migrations;
#[path = "../vendor/openrust/fortify/database/migrations/2014_10_12_200000_add_two_factor_columns_to_users_table.rs"]
mod fortify;
#[path = "../vendor/openrust/passkeys/database/migrations/2024_01_01_000000_create_passkeys_table.rs"]
mod passkeys;
#[path = "../vendor/openrust/passport/src/migrations.rs"]
#[allow(dead_code)]
mod passport;
#[path = "../vendor/openrust/framework/src/sanctum/stubs/personal_access_tokens.stub"]
mod sanctum;
mod app {
    pub mod models {
        use openrust::database::eloquent::{Model, ModelMetadata, ModelState};
        pub struct User(ModelState);
        impl Model for User {
            fn metadata() -> &'static ModelMetadata {
                static META: ModelMetadata = ModelMetadata::new("users");
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
    }
}

fn builtin_registry() -> Arc<MigrationRegistry> {
    let mut entries: Vec<_> = application_migrations::entries()
        .iter()
        .map(|e| MigrationEntry::new(e.path, e.migration))
        .collect();
    entries.extend([
        MigrationEntry::new(
            "database/migrations/2014_10_12_200000_add_two_factor_columns_to_users_table.rs",
            &fortify::Definition,
        ),
        MigrationEntry::new(
            "database/migrations/2016_06_01_000001_create_oauth_auth_codes_table.rs",
            &passport::oauth_auth_codes::Definition,
        ),
        MigrationEntry::new(
            "database/migrations/2016_06_01_000002_create_oauth_access_tokens_table.rs",
            &passport::oauth_access_tokens::Definition,
        ),
        MigrationEntry::new(
            "database/migrations/2016_06_01_000003_create_oauth_refresh_tokens_table.rs",
            &passport::oauth_refresh_tokens::Definition,
        ),
        MigrationEntry::new(
            "database/migrations/2016_06_01_000004_create_oauth_clients_table.rs",
            &passport::oauth_clients::Definition,
        ),
        MigrationEntry::new(
            "database/migrations/2024_06_01_000001_create_oauth_device_codes_table.rs",
            &passport::oauth_device_codes::Definition,
        ),
        MigrationEntry::new(
            "database/migrations/2024_01_01_000000_create_passkeys_table.rs",
            &passkeys::Definition,
        ),
        MigrationEntry::new(
            "database/migrations/2024_01_01_000001_create_personal_access_tokens_table.rs",
            &sanctum::Definition,
        ),
    ]);
    Arc::new(MigrationRegistry(Box::leak(entries.into_boxed_slice())))
}

#[tokio::test]
async fn all_builtin_migrations_write_localized_comments_on_postgres() {
    let Ok(url) = std::env::var("OPENRUST_TEST_POSTGRES_URL") else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_POSTGRES").is_none(),
            "PostgreSQL required"
        );
        return;
    };
    let languages = Languages::new();
    let root = languages.0.clone();
    let registry = builtin_registry();
    for (number, (locale, user_name, email, status)) in [
        ("en", "Users", "Email", "accepted=accepted"),
        ("zh-Hans", "用户表", "邮箱", "accepted=已受理"),
        // 地区标签不自动合并到简体资源；精确目录缺失时使用配置的繁体回退。
        ("zh-CN", "使用者資料表", "電子郵件", "accepted=已受理"),
        ("zh-Hant", "使用者資料表", "電子郵件", "accepted=已受理"),
        ("fr", "Utilisateurs", "Courriel", "accepted=已受理"),
        ("ja", "使用者資料表", "電子郵件", "accepted=已受理"),
    ]
    .into_iter()
    .enumerate()
    {
        let app = languages.application(locale, "zh-Hant");
        let registry = registry.clone();
        let url = url.clone();
        let root = root.clone();
        app.call(Callback::new(&[], move |_| async move {
        assert_eq!(__("schema.tables.users", &Default::default()).unwrap(), user_name, "translation before migrations: {locale}");
        let namespace = format!("schema_comments_{}_{}",std::process::id(),number);
        let config = Arc::new(Repository::new(BTreeMap::from([
            ("app".into(),json!({"locale":locale,"fallback_locale":"zh-Hant"})),
            ("database".into(),json!({"default":"test","connections":{"test":{"url":url,"search_path":namespace}}})),
        ])));
        let manager = Arc::new(DatabaseManager::new(config,root.clone()));
        let db = manager.connection(None).unwrap();
        db.unprepared(&format!("create schema {namespace}")).await.unwrap();
        let migrator = Migrator::new(manager.clone(),registry.clone(),"migrations".into(),root.clone());
        let rows = migrator.run(None,&[],RunOptions::default()).await.unwrap();
        assert_eq!(rows.len(),12);
        let mut schema = db.getSchemaBuilder();
        let tables = schema.getTables(Some(&namespace)).await.unwrap();
        assert_eq!(tables.len(),17);
        for table in &tables {
            assert!(table.comment.as_deref().is_some_and(|s|!s.is_empty() && !s.starts_with("schema.")),"{locale}: {}",table.name);
            for column in schema.getColumns(&table.name).await.unwrap() {
                assert!(column.comment.as_deref().is_some_and(|s|!s.is_empty() && !s.starts_with("schema.")),"{locale}: {}.{}",table.name,column.name);
            }
        }
        assert_eq!(tables.iter().find(|t|t.name=="users").unwrap().comment.as_deref(),Some(user_name), "table comment: {locale}");
        let columns = schema.getColumns("users").await.unwrap();
        assert_eq!(columns.iter().find(|c|c.name=="email").unwrap().comment.as_deref(),Some(email));
        let columns = schema.getColumns("sms_deliveries").await.unwrap();
        assert!(columns.iter().find(|c|c.name=="status").unwrap().comment.as_deref().unwrap().contains(status), "locale={locale}, status={:?}", columns.iter().find(|c|c.name=="status").unwrap().comment);
        schema.table("users", |table| { table.string("nickname", None).nullable().comment("Nickname"); }).await.unwrap();
        assert_eq!(schema.getColumns("users").await.unwrap().iter().find(|c|c.name=="nickname").unwrap().comment.as_deref(),Some("Nickname"));
        schema.table("users", |table| { table.string("nickname", None).nullable().comment("Owner's nickname").change(); }).await.unwrap();
        assert_eq!(schema.getColumns("users").await.unwrap().iter().find(|c|c.name=="nickname").unwrap().comment.as_deref(),Some("Owner's nickname"));
        // Existing data, column definitions and constraints survive comment-only changes.
        db.table("users").insert(&[BTreeMap::from([("name".into(),"reader".into()),("email".into(),"reader@example.test".into()),("password".into(),"hash".into())])]).await.unwrap();
        let mut before = schema.getColumns("users").await.unwrap();
        let indexes = schema.getIndexes("users").await.unwrap();
        schema.table("users", |table| table.comment("User's table \\ archive")).await.unwrap();
        db.statement("comment on column users.email is 'Owner''s email \\ address'", &[]).await.unwrap();
        assert_eq!(db.table("users").count("*").await.unwrap(),1);
        let mut after = schema.getColumns("users").await.unwrap();
        for col in &mut before { col.comment=None; }
        for col in &mut after { col.comment=None; }
        assert_eq!(before,after);
        assert_eq!(indexes,schema.getIndexes("users").await.unwrap());
        {
            let tables=schema.getTables(Some(&namespace)).await.unwrap();
            assert_eq!(tables.iter().find(|t|t.name=="users").unwrap().comment.as_deref(),Some("User's table \\ archive"));
            assert_eq!(schema.getColumns("users").await.unwrap().iter().find(|c|c.name=="email").unwrap().comment.as_deref(),Some("Owner's email \\ address"));
        }
        db.statement("comment on column users.email is null", &[]).await.unwrap();
        assert!(schema.getColumns("users").await.unwrap().iter().find(|c|c.name=="email").unwrap().comment.is_none());
        assert_eq!(migrator.reset(None,&[],false).await.unwrap().len(),12);
        assert!(!schema.hasTable("users").await.unwrap());
        // Failed callbacks must not execute partial DDL.
        let result = openrust::translation::Lang::withLocale("de", schema.create("invalid_catalog", |table| -> openrust::database::DatabaseResult<()> {
            assert_eq!(openrust::translation::Lang::get_locale().unwrap(), "de");
            table.id();
            table.comment(__("schema.tables.users", &Default::default())?);
            Ok(())
        })).await.unwrap();
        assert!(matches!(result,Err(openrust::database::DatabaseError::Translation(_))), "callback result={result:?}, locale={locale}");
        assert!(!schema.hasTable("invalid_catalog").await.unwrap());
        db.unprepared(&format!("drop schema {namespace} cascade")).await.unwrap();
        Ok(())
        }), Default::default()).await.unwrap();
    }
}

#[tokio::test]
async fn mysql_comments_select_the_app_language_in_pretend_mode() {
    let languages = Languages::new();
    for (locale, expected) in [
        ("en", "Email"),
        ("zh-Hans", "邮箱"),
        ("zh-Hant", "電子郵件"),
        ("zh-CN", "Email"),
        ("fr", "Courriel"),
        ("ja", "Email"),
    ] {
        let app = languages.application(locale, "en");
        app.call(Callback::new(&[], move |_| async move {
        let manager = DatabaseManager::new(Arc::new(Repository::new(BTreeMap::from([
            ("app".into(), json!({"locale":locale,"fallback_locale":"en"})),
            ("database".into(), json!({"default":"test","connections":{"test":{"driver":"mysql","prefix":"app_"}}})),
        ]))), ".");
        let db=manager.connection(None).unwrap();
        let queries=db.pretend(|db|Box::pin(async move {
            db.getSchemaBuilder().create("users",|table| -> openrust::database::DatabaseResult<()> {
                table.comment(__("schema.tables.users", &Default::default())?);
                table.string("email",255).comment(__("schema.columns.email", &Default::default())?);
                table.integer("status").comment("Status: 0=enabled, 1=disabled");
                Ok(())
            }).await
        })).await.unwrap();
        assert_eq!(queries.len(),2);
        assert!(queries[0].query.contains("`app_users`"));
        assert!(queries[0].query.contains(&expected.replace('\'', "''")), "{}",queries[0].query);
        assert!(queries[0].query.contains("comment 'Status: 0=enabled, 1=disabled'"));
        assert!(queries[1].query.starts_with("alter table `app_users` comment = "));
        Ok(())
        }), Default::default()).await.unwrap();
    }
}

#[tokio::test]
async fn builtin_migrations_run_on_mysql_and_sqlite() {
    let languages = Languages::new();
    let mut connections = vec![("sqlite", json!({"driver":"sqlite","database":":memory:"}))];
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        connections.push(("mysql", json!({"url":url})));
    } else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
            "MySQL required"
        );
    }
    for (driver, connection) in connections {
        for (locale, users, email) in [
            ("en", "Users", "Email"),
            ("zh-Hans", "用户表", "邮箱"),
            ("zh-Hant", "使用者資料表", "電子郵件"),
            ("zh-CN", "Users", "Email"),
            ("fr", "Utilisateurs", "Courriel"),
            ("ja", "Users", "Email"),
        ] {
            let app = languages.application(locale, "en");
            let root = languages.0.clone();
            let connection = connection.clone();
            app.call(Callback::new(&[], move |_| async move {
                let manager = Arc::new(DatabaseManager::new(Arc::new(Repository::new(BTreeMap::from([
                    ("app".into(), json!({"locale":locale,"fallback_locale":"en"})),
                    ("database".into(), json!({"default":"test","connections":{"test":connection}})),
                ]))), root.clone()));
                let db = manager.connection(None).unwrap();
                let mut schema = db.getSchemaBuilder();
                let migrator = Migrator::new(manager.clone(), builtin_registry(), "migrations".into(), root);
                assert_eq!(migrator.run(None, &[], RunOptions::default()).await.unwrap().len(), 12);
                // MySQL 默认列举所有可见 schema；本夹具只检查本库的内置迁移。
                let namespace = (driver == "mysql").then(|| db.getDatabaseName());
                let tables = schema.getTables(namespace).await.unwrap();
                assert_eq!(tables.len(), 17, "{driver}: {locale}");
                if driver == "mysql" {
                    for table in &tables {
                        assert!(table.comment.as_deref().is_some_and(|s| !s.is_empty() && !s.starts_with("schema.")), "{locale}: {}", table.name);
                        for column in schema.getColumns(&table.name).await.unwrap() {
                            assert!(column.comment.as_deref().is_some_and(|s| !s.is_empty() && !s.starts_with("schema.")), "{locale}: {}.{}", table.name, column.name);
                        }
                    }
                    assert_eq!(tables.iter().find(|t| t.name == "users").unwrap().comment.as_deref(), Some(users));
                    assert_eq!(schema.getColumns("users").await.unwrap().iter().find(|c| c.name == "email").unwrap().comment.as_deref(), Some(email));
                } else {
                    // SQLite has no table/column comment metadata. the framework ignores these modifiers.
                    let version = db.select("select sqlite_version() as version", &[]).await.unwrap();
                    println!("SQLite {version:?}: {locale}");
                    assert!(tables.iter().all(|t| t.comment.is_none()));
                    assert!(schema.getColumns("users").await.unwrap().iter().all(|c| c.comment.is_none()));
                }
                db.table("users").insert(&[BTreeMap::from([("name".into(), "reader".into()), ("email".into(), "reader@example.test".into()), ("password".into(), "hash".into())])]).await.unwrap();
                assert_eq!(db.table("users").count("*").await.unwrap(), 1);
                let result = openrust::translation::Lang::withLocale("de", schema.create("invalid_catalog", |table| -> openrust::database::DatabaseResult<()> {
                    table.id();
                    table.comment(__("schema.tables.users", &Default::default())?);
                    Ok(())
                })).await.unwrap();
                assert!(matches!(result, Err(openrust::database::DatabaseError::Translation(_))), "{driver}: {locale}: {result:?}");
                assert!(!schema.hasTable("invalid_catalog").await.unwrap());
                assert_eq!(migrator.reset(None, &[], false).await.unwrap().len(), 12);
                assert!(!schema.hasTable("users").await.unwrap());
                schema.drop("migrations").await.unwrap();
                Ok(())
            }), Default::default()).await.unwrap();
        }
    }
}
