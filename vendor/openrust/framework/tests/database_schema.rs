use std::{collections::BTreeMap, sync::Arc};

use futures_util::TryStreamExt as _;

use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseError, DatabaseManager, DatabaseResult, Value,
        schema::{GenerationType, MorphKeyType, Schema},
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
    testing::TestApp,
};
use serde_json::{Value as Json, json};

struct MorphKeyProvider(MorphKeyType);

impl ServiceProvider for MorphKeyProvider {
    fn boot(&mut self, _app: &Application) -> HttpFuture<'_, ProviderResult> {
        Box::pin(async {
            match self.0 {
                MorphKeyType::Int => Schema::defaultMorphKeyType(MorphKeyType::Int)?,
                MorphKeyType::Uuid => Schema::morphUsingUuids()?,
                MorphKeyType::Ulid => Schema::morphUsingUlids()?,
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn morph_columns_follow_the_current_app_and_keep_the_composite_index() {
    async fn verify(kind: Option<MorphKeyType>, expected: &str) {
        let mut app = TestApp::sqlite(|builder| match kind {
            Some(kind) => builder.withProviders(vec![Box::new(MorphKeyProvider(kind))]),
            None => builder,
        })
        .unwrap();
        let connection = app.database().await.unwrap();
        let mut schema = connection.getSchemaBuilder();
        schema
            .create("tokens", |table| table.morphs("tokenable"))
            .await
            .unwrap();
        let columns = schema.getColumns("tokens").await.unwrap();
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].name, "tokenable_type");
        assert_eq!(columns[1].name, "tokenable_id");
        assert_eq!(columns[1].full_type, expected);
        assert!(columns.iter().all(|column| !column.nullable));
        assert!(
            schema
                .getIndexes("tokens")
                .await
                .unwrap()
                .iter()
                .any(|index| {
                    !index.unique
                        && index.columns
                            == [Some("tokenable_type".into()), Some("tokenable_id".into())]
                })
        );
    }

    tokio::join!(
        verify(Some(MorphKeyType::Uuid), "varchar"),
        verify(Some(MorphKeyType::Ulid), "varchar"),
        verify(None, "integer"),
    );
}

fn connection(config: Json) -> Connection {
    let settings = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"test","connections":{"test":config}}),
    )]));
    DatabaseManager::new(Arc::new(settings), ".")
        .connection(None)
        .unwrap()
}

async fn verify(db: Connection, mysql: bool) {
    let mut schema = db.getSchemaBuilder();
    schema.dropIfExists("posts").await.unwrap();
    schema.dropIfExists("writers").await.unwrap();
    schema
        .create("writers", |table| {
            table.id();
            table.string("name", 80).unique();
            table.timestamps();
            table.date("active_on").useCurrent();
        })
        .await
        .unwrap();
    schema
        .create("posts", |table| {
            table.id();
            table
                .foreignId("writer_id")
                .nullable()
                .constrainedOn("writers", "id", Some("post_author_fk"))
                .cascadeOnUpdate()
                .nullOnDelete();
            table.string("title", 120).default("O'Reilly \\ notes");
            table.text("scratch").nullable();
            table.boolean("visible").default(false);
            table.decimal("amount", 12, 2).default(0);
            table.index(&["writer_id", "title"], Some("post_writer_title"));
        })
        .await
        .unwrap();

    assert!(schema.hasTable("posts").await.unwrap());
    assert!(!schema.hasTable("unknown").await.unwrap());
    assert!(
        schema
            .getTables(None)
            .await
            .unwrap()
            .iter()
            .any(|table| table.name == "app_posts")
    );
    assert_eq!(
        schema.getColumnListing("posts").await.unwrap(),
        ["id", "writer_id", "title", "scratch", "visible", "amount"]
    );
    assert!(schema.hasColumns("posts", &["ID", "Title"]).await.unwrap());
    let columns = schema.getColumns("posts").await.unwrap();
    assert!(columns[0].auto_increment && columns[0].primary);
    assert!(columns[1].nullable);
    assert_eq!(
        schema.getColumnType("posts", "title", true).await.unwrap(),
        if mysql { "varchar(120)" } else { "varchar" }
    );
    let indexes = schema.getIndexes("posts").await.unwrap();
    assert!(
        indexes
            .iter()
            .any(|index| index.primary && index.columns == [Some("id".into())])
    );
    assert!(indexes.iter().any(|index| index.name == "post_writer_title"
        && index.columns == [Some("writer_id".into()), Some("title".into())]));
    assert!(
        schema
            .hasIndex("writers", "app_writers_name_unique")
            .await
            .unwrap()
    );

    let keys = schema.getForeignKeys("posts").await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].foreign_table, "app_writers");
    assert_eq!(keys[0].columns, ["writer_id"]);
    assert_eq!(keys[0].foreign_columns, [Some("id".into())]);
    assert_eq!(keys[0].on_delete, "set null");
    assert_eq!(keys[0].on_update, "cascade");
    assert_eq!(keys[0].name.as_deref(), mysql.then_some("post_author_fk"));

    assert!(matches!(
        db.insert(
            "insert into app_posts (writer_id) values (?)",
            &[900.into()]
        )
        .await,
        Err(DatabaseError::Driver { .. })
    ));
    db.insert(
        "insert into app_writers (id, name) values (1, 'writer')",
        &[],
    )
    .await
    .unwrap();
    assert!(
        matches!(db.table("writers").value("active_on").await.unwrap(), Value::Text(date) if date.len() == 10)
    );
    db.insert("insert into app_posts (writer_id) values (1)", &[])
        .await
        .unwrap();
    assert_eq!(
        db.table("posts").value("title").await.unwrap(),
        "O'Reilly \\ notes".into()
    );
    db.update("update app_writers set id = 2 where id = 1", &[])
        .await
        .unwrap();
    assert!(matches!(
        db.table("posts").value("writer_id").await.unwrap(),
        Value::Integer(2) | Value::Unsigned(2)
    ));
    db.delete("delete from app_writers where id = 2", &[])
        .await
        .unwrap();
    assert_eq!(
        db.table("posts").value("writer_id").await.unwrap(),
        Value::Null
    );

    schema
        .table("posts", |table| {
            table.string("slug", 140).nullable();
            table.renameColumn("scratch", "draft");
            table.dropColumn(&["draft"]);
            table.unique(&["slug"], None);
        })
        .await
        .unwrap();
    assert!(schema.hasColumn("posts", "slug").await.unwrap());
    assert!(!schema.hasColumn("posts", "draft").await.unwrap());
    schema
        .table("posts", |table| {
            table.dropUnique("app_posts_slug_unique");
        })
        .await
        .unwrap();
    if mysql {
        schema
            .table("posts", |table| {
                table.renameIndex("post_writer_title", "post_lookup");
                table
                    .string("title", 150)
                    .nullable()
                    .comment("reviewed")
                    .change();
                table.dropForeign("post_author_fk");
            })
            .await
            .unwrap();
        assert_eq!(
            schema.getColumnType("posts", "title", true).await.unwrap(),
            "varchar(150)"
        );
        assert!(schema.hasIndex("posts", "post_lookup").await.unwrap());
        assert!(schema.getForeignKeys("posts").await.unwrap().is_empty());
    }

    schema.rename("posts", "articles").await.unwrap();
    assert!(schema.hasTable("articles").await.unwrap());
    assert!(!schema.hasTable("posts").await.unwrap());
    schema.drop("articles").await.unwrap();
    schema.drop("writers").await.unwrap();
}

#[tokio::test]
async fn schema_builds_and_inspects_real_sqlite() {
    let db = connection(
        json!({"driver":"sqlite","database":":memory:","prefix":"app_","prefix_indexes":true}),
    );
    verify(db.clone(), false).await;
    let mut schema = db.getSchemaBuilder();
    assert!(matches!(
        schema
            .create("bad", |table| {
                table.string("hidden", 80).invisible();
            })
            .await,
        Err(DatabaseError::UnsupportedSchema(_))
    ));
    assert!(!schema.hasTable("bad").await.unwrap());
}

#[tokio::test]
async fn schema_builds_and_inspects_real_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url,"prefix":"app_","prefix_indexes":true}));
    verify(db.clone(), true).await;
    verify_mysql_column_modifiers(db).await;
}

async fn verify_mysql_column_modifiers(db: Connection) {
    let mut schema = db.getSchemaBuilder();
    schema.dropIfExists("column_modifiers").await.unwrap();
    schema
        .create("column_modifiers", |table| {
            table.id();
            table.string("label", 80);
            table.string("hidden", 80).invisible().default("secret");
            table
                .timestamp("touched_at", 6)
                .useCurrent()
                .useCurrentOnUpdate();
            table
                .dateTime("changed_at", 3)
                .useCurrent()
                .useCurrentOnUpdate();
        })
        .await
        .unwrap();
    schema
        .table("column_modifiers", |table| {
            table.string("lead", 80).nullable().first();
            table.string("middle", 80).nullable().after("label");
            table.string("label", 120).change().after("touched_at");
        })
        .await
        .unwrap();
    assert_eq!(
        schema.getColumnListing("column_modifiers").await.unwrap(),
        [
            "lead",
            "id",
            "middle",
            "hidden",
            "touched_at",
            "label",
            "changed_at"
        ]
    );

    db.insert(
        "insert into app_column_modifiers (label) values ('initial')",
        &[],
    )
    .await
    .unwrap();
    let row = db
        .selectOne("select * from app_column_modifiers", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(row.get("hidden").is_none());
    assert!(matches!(row.get("touched_at"), Some(Value::Text(value)) if !value.is_empty()));
    assert!(matches!(row.get("changed_at"), Some(Value::Text(value)) if !value.is_empty()));
    assert_eq!(
        db.table("column_modifiers").value("hidden").await.unwrap(),
        "secret".into()
    );

    db.update("update app_column_modifiers set touched_at = '2020-01-01 00:00:00', changed_at = '2020-01-01 00:00:00'", &[])
        .await.unwrap();
    db.update("update app_column_modifiers set label = 'updated'", &[])
        .await
        .unwrap();
    let row = db.selectOne("select date_format(touched_at, '%Y-%m-%d %H:%i:%s.%f') as touched_at, date_format(changed_at, '%Y-%m-%d %H:%i:%s.%f') as changed_at from app_column_modifiers", &[])
        .await.unwrap().unwrap();
    for column in ["touched_at", "changed_at"] {
        assert!(
            matches!(row.get(column), Some(Value::Text(value)) if value.len() == 26 && !value.starts_with("2020-01-01"))
        );
    }

    assert!(matches!(
        schema
            .table("column_modifiers", |table| {
                table.integer("not_added");
                table.integer("bad").useCurrentOnUpdate();
            })
            .await,
        Err(DatabaseError::InvalidSchema(_))
    ));
    assert!(
        !schema
            .hasColumn("column_modifiers", "not_added")
            .await
            .unwrap()
    );
    assert!(matches!(
        schema
            .create("bad_position", |table| {
                table.integer("bad").first();
            })
            .await,
        Err(DatabaseError::InvalidSchema(_))
    ));
    assert!(!schema.hasTable("bad_position").await.unwrap());
    schema.drop("column_modifiers").await.unwrap();
}

#[tokio::test]
async fn sqlite_ddl_rolls_back_and_invalid_blueprint_has_no_effect() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    let mut schema = db.getSchemaBuilder();
    assert!(matches!(
        schema
            .create("bad", |table| {
                table.id();
                table.fullText(&["id"], None);
            })
            .await,
        Err(DatabaseError::UnsupportedSchema("SQLite index kind"))
    ));
    assert!(!schema.hasTable("bad").await.unwrap());
    let result = schema
        .create("partial", |table| {
            table.id();
            table.index(&["id"], Some("same"));
            table.index(&["id"], Some("same"));
        })
        .await;
    assert!(matches!(result, Err(DatabaseError::Driver { .. })));
    assert!(!schema.hasTable("partial").await.unwrap());

    let mut transaction = db.beginTransaction().await.unwrap();
    transaction
        .getSchemaBuilder()
        .create("pending", |table| {
            table.id();
        })
        .await
        .unwrap();
    assert!(
        transaction
            .getSchemaBuilder()
            .hasTable("pending")
            .await
            .unwrap()
    );
    transaction.rollBack(None).await.unwrap();
    assert!(!schema.hasTable("pending").await.unwrap());
}

#[tokio::test]
async fn sqlite_rebuild_preserves_children_catalog_objects_and_sequence() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    let mut schema = db.getSchemaBuilder();
    schema
        .create("parents", |table| {
            table.id();
            table.string("label", 80).nullable();
        })
        .await
        .unwrap();
    schema
        .create("children", |table| {
            table.id();
            table.foreignId("parent_id").constrained().cascadeOnDelete();
        })
        .await
        .unwrap();
    db.unprepared(
        "create table audit_log (value text);          alter table parents add column label_size integer generated always as (length(label)) virtual;          create index label_expr on parents (lower(label)) where label is not null;          create trigger parent_insert after insert on parents begin insert into audit_log values (new.label); end;          create view parent_view as select label_size from parents;          insert into parents (id,label) values (1,'first'),(100,'removed');          delete from parents where id=100;          insert into children (parent_id) values (1)",
    ).await.unwrap();

    schema
        .table("parents", |table| {
            table.text("label").nullable().change();
            table.renameIndex("label_expr", "label_lookup");
        })
        .await
        .unwrap();
    assert_eq!(
        schema
            .getColumnType("parents", "label", true)
            .await
            .unwrap(),
        "text"
    );
    assert_eq!(
        db.scalar("select count(*) from children", &[])
            .await
            .unwrap(),
        1.into()
    );
    assert_eq!(
        db.scalar("select label_size from parent_view", &[])
            .await
            .unwrap(),
        5.into()
    );
    assert!(schema.hasIndex("parents", "label_lookup").await.unwrap());
    assert!(!schema.hasIndex("parents", "label_expr").await.unwrap());
    assert_eq!(
        db.scalar("select count(*) from audit_log", &[])
            .await
            .unwrap(),
        2.into()
    );
    db.insert("insert into parents (label) values (NULL)", &[])
        .await
        .unwrap();
    assert_eq!(
        db.scalar("select max(id) from parents", &[]).await.unwrap(),
        101.into()
    );
    assert_eq!(
        db.scalar("select count(*) from audit_log", &[])
            .await
            .unwrap(),
        3.into()
    );
    assert!(matches!(
        db.insert("insert into children (parent_id) values (900)", &[])
            .await,
        Err(DatabaseError::Driver { .. })
    ));

    assert!(matches!(
        schema
            .table("parents", |table| {
                table.text("label").change();
            })
            .await,
        Err(DatabaseError::Driver { .. })
    ));
    assert!(
        schema
            .getColumns("parents")
            .await
            .unwrap()
            .iter()
            .any(|column| column.name == "label" && column.nullable)
    );
    assert!(schema.hasIndex("parents", "label_lookup").await.unwrap());

    schema
        .table("children", |table| {
            table.dropForeignColumns(&["parent_id"]);
        })
        .await
        .unwrap();
    assert!(schema.getForeignKeys("children").await.unwrap().is_empty());
    db.insert("insert into children (parent_id) values (900)", &[])
        .await
        .unwrap();
    assert!(matches!(
        schema
            .table("children", |table| {
                table
                    .foreign(&["parent_id"], Some("child_parent"))
                    .references("id")
                    .on("parents")
                    .cascadeOnDelete();
            })
            .await,
        Err(DatabaseError::InvalidSchema("SQLite foreign key check"))
    ));
    assert!(schema.getForeignKeys("children").await.unwrap().is_empty());
    assert_eq!(
        db.scalar("select count(*) from children", &[])
            .await
            .unwrap(),
        2.into()
    );
    db.delete("delete from children where parent_id=900", &[])
        .await
        .unwrap();
    schema
        .table("children", |table| {
            table
                .foreign(&["parent_id"], Some("child_parent"))
                .references("id")
                .on("parents")
                .cascadeOnDelete();
        })
        .await
        .unwrap();
    db.delete("delete from parents where id=1", &[])
        .await
        .unwrap();
    assert_eq!(
        db.scalar("select count(*) from children", &[])
            .await
            .unwrap(),
        0.into()
    );
}

#[tokio::test]
async fn schema_facade_uses_the_artisan_application_connection() {
    use openrust::{
        console::{
            Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
            Output,
        },
        foundation::Application,
        support::facades::Schema,
    };

    struct Probe(CommandDefinition);
    impl Command for Probe {
        fn definition(&self) -> &CommandDefinition {
            &self.0
        }
        fn handle<'a>(
            &'a self,
            _: &'a Application,
            _: &'a Input,
            _: &'a mut Output,
        ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
            Box::pin(async {
                async fn probe() -> Result<(), DatabaseError> {
                    Schema::create("schema_artisan_probe", |table| {
                        table.id();
                        table.string("title", 80).unique();
                    })
                    .await?;
                    assert!(Schema::hasColumn("schema_artisan_probe", "title").await?);
                    assert!(
                        Schema::connection("sqlite")?
                            .hasIndex("schema_artisan_probe", "schema_artisan_probe_title_unique")
                            .await?
                    );
                    Ok(())
                }
                probe()
                    .await
                    .map_err(|error| ConsoleError::new(error.to_string()))?;
                Ok(0)
            })
        }
    }
    fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
        artisan.add_command(Box::new(Probe(CommandDefinition::new(clap::Command::new(
            "schema:probe",
        )))))
    }

    let root = std::env::temp_dir().join(format!(
        "openrust-schema-{}-{}",
        std::process::id(),
        fastrand::u64(..)
    ));
    std::fs::create_dir_all(root.join("database")).unwrap();
    let file = root.join("database/database.sqlite");
    std::fs::File::create(&file).unwrap();
    let application = Application::configure(root.clone())
        .withCommands(register)
        .create()
        .unwrap();
    assert_eq!(
        application
            .handleCommand(&["schema:probe".into()])
            .await
            .unwrap(),
        0
    );
    let isolated = connection(json!({"driver":"sqlite","database":file}));
    assert!(
        isolated
            .getSchemaBuilder()
            .hasTable("schema_artisan_probe")
            .await
            .unwrap()
    );
    drop(isolated);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn cancelled_sqlite_rebuild_finishes_and_restores_foreign_key_checks() {
    use std::time::Duration;

    let file = std::env::temp_dir().join(format!(
        "openrust-schema-cancel-{}-{}.sqlite",
        std::process::id(),
        fastrand::u64(..),
    ));
    std::fs::File::create(&file).unwrap();
    let db = connection(
        json!({"driver":"sqlite","database":file,"max_connections":2,"busy_timeout":4000}),
    );
    let mut schema = db.getSchemaBuilder();
    schema
        .create("parents", |table| {
            table.id();
            table.string("label", 40);
        })
        .await
        .unwrap();
    schema
        .create("children", |table| {
            table.id();
            table.foreignId("parent_id").constrained();
        })
        .await
        .unwrap();
    let mut blocker = db.beginTransaction().await.unwrap();
    blocker
        .insert("insert into parents (label) values ('in flight')", &[])
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(100),
            schema.table("parents", |table| {
                table.text("label").change();
            })
        )
        .await
        .is_err()
    );
    blocker.commit().await.unwrap();

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if schema
                .getColumnType("parents", "label", true)
                .await
                .unwrap()
                == "text"
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let mut first = db.beginTransaction().await.unwrap();
    let mut second = db.beginTransaction().await.unwrap();
    for transaction in [&mut first, &mut second] {
        assert_eq!(
            transaction
                .scalar("pragma foreign_keys", &[])
                .await
                .unwrap(),
            1.into()
        );
        assert_eq!(
            transaction
                .scalar("pragma legacy_alter_table", &[])
                .await
                .unwrap(),
            0.into()
        );
        transaction.rollBack(None).await.unwrap();
    }
    assert_eq!(
        db.table("parents").value("label").await.unwrap(),
        "in flight".into()
    );
    drop(schema);
    drop(db);
    std::fs::remove_file(file).unwrap();
}

async fn verify_foreign_key_scope(db: Connection, mysql: bool) {
    let state = if mysql {
        "select @@foreign_key_checks"
    } else {
        "pragma foreign_keys"
    };
    db.unprepared(
        "CREATE TABLE fk_scope_parents (id BIGINT PRIMARY KEY);          CREATE TABLE fk_scope_children (id BIGINT PRIMARY KEY, parent BIGINT, FOREIGN KEY (parent) REFERENCES fk_scope_parents(id));",
    )
    .await
    .unwrap();
    let schema = db.getSchemaBuilder();
    let cannot_insert = "INSERT INTO fk_scope_children VALUES (?, 99)";
    assert!(db.statement(cannot_insert, &[0.into()]).await.is_err());

    let count = schema
        .withoutForeignKeyConstraints(|session| {
            Box::pin(async move {
                assert_eq!(
                    session.select(state, &[]).await?[0].get_at(0),
                    Some(&0.into())
                );
                assert!(session.statement(cannot_insert, &[1.into()]).await?);
                let mut builder = session.getSchemaBuilder();
                builder
                    .create("fk_scope_marker", |table| {
                        table.id();
                    })
                    .await?;
                assert!(builder.hasTable("fk_scope_marker").await?);
                Ok(1)
            })
        })
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert!(db.statement(cannot_insert, &[2.into()]).await.is_err());

    let result = schema
        .withoutForeignKeyConstraints(|_| {
            Box::pin(async { Err::<(), _>(DatabaseError::InvalidSchema("callback failed")) })
        })
        .await;
    assert!(matches!(
        result,
        Err(DatabaseError::InvalidSchema("callback failed"))
    ));
    assert!(db.statement(cannot_insert, &[3.into()]).await.is_err());

    let ready = Arc::new(tokio::sync::Notify::new());
    let started = ready.clone();
    let task_db = db.clone();
    let job = tokio::spawn(async move {
        task_db
            .getSchemaBuilder()
            .withoutForeignKeyConstraints(move |session| {
                Box::pin(async move {
                    assert_eq!(
                        session.select(state, &[]).await?[0].get_at(0),
                        Some(&0.into())
                    );
                    started.notify_one();
                    std::future::pending::<DatabaseResult<()>>().await
                })
            })
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), ready.notified())
        .await
        .unwrap();
    if mysql {
        // The live writer distinct from the leased session must continue enforcing constraints.
        assert!(db.statement(cannot_insert, &[4.into()]).await.is_err());
    }
    job.abort();
    assert!(job.await.unwrap_err().is_cancelled());

    let mut session = tokio::time::timeout(std::time::Duration::from_secs(5), schema.session())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        session.select(state, &[]).await.unwrap()[0].get_at(0),
        Some(&1.into())
    );
    session.disableForeignKeyConstraints().await.unwrap();
    session.statement(cannot_insert, &[5.into()]).await.unwrap();
    session.enableForeignKeyConstraints().await.unwrap();
    assert!(session.statement(cannot_insert, &[6.into()]).await.is_err());
    session.close().await.unwrap();
    assert_eq!(db.table("fk_scope_children").count("*").await.unwrap(), 2);
    db.unprepared(
        "DROP TABLE fk_scope_marker; DROP TABLE fk_scope_children; DROP TABLE fk_scope_parents",
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn sqlite_foreign_key_scope_restores_the_leased_session() {
    verify_foreign_key_scope(
        connection(json!({"driver":"sqlite","database":":memory:"})),
        false,
    )
    .await;
    let db = connection(
        json!({"driver":"sqlite","database":":memory:","foreign_key_constraints":false}),
    );
    let mut session = db.getSchemaBuilder().session().await.unwrap();
    session.enableForeignKeyConstraints().await.unwrap();
    assert_eq!(
        session.select("pragma foreign_keys", &[]).await.unwrap()[0].get_at(0),
        Some(&1.into())
    );
    session.close().await.unwrap();
    assert_eq!(
        db.scalar("pragma foreign_keys", &[]).await.unwrap(),
        0.into()
    );
}

#[tokio::test]
async fn mysql_foreign_key_scope_restores_the_leased_session() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    verify_foreign_key_scope(connection(json!({"url":url,"max_connections":2})), true).await;
}

async fn verify_additional_column_types(db: Connection, mysql: bool) {
    let mut schema = db.getSchemaBuilder();
    schema
        .dropIfExists("additional_column_types")
        .await
        .unwrap();
    schema
        .create("additional_column_types", |table| {
            table.tinyIncrements("id");
            table.unsignedTinyInteger("tiny_value").default(255_i64);
            table
                .unsignedSmallInteger("small_value")
                .default(65_535_i64);
            table
                .unsignedMediumInteger("medium_value")
                .default(16_777_215_i64);
            table.tinyText("summary").nullable();
            table
                .r#enum("state", &["ready", "customer's choice"])
                .default("ready");
            table.year("observed_year").useCurrent();
            table.ipAddress("network_ip").nullable();
            table.macAddress("network_mac").nullable();
            table.binaryWith("fixed_data", 4, true).nullable();
            table.binaryWith("variable_data", 12, false).nullable();
            table.timeTz("observed_time", 3).nullable();
            table.dateTimeTz("seen_at", 3).nullable();
            table.timestampsTz();
            table.softDeletesTz();
            if mysql {
                table.set("flags", &["one", "two"]).default("one,two");
            }
        })
        .await
        .unwrap_or_else(|error| match error {
            DatabaseError::Driver { source, .. } => panic!("column DDL failed: {source}"),
            _ => panic!("column DDL failed: {error}"),
        });

    let columns = schema.getColumns("additional_column_types").await.unwrap();
    let kind = |name| {
        columns
            .iter()
            .find(|column| column.name == name)
            .unwrap()
            .full_type
            .as_str()
    };
    if mysql {
        assert_eq!(kind("id"), "tinyint unsigned");
        assert_eq!(kind("tiny_value"), "tinyint unsigned");
        assert_eq!(kind("small_value"), "smallint unsigned");
        assert_eq!(kind("medium_value"), "mediumint unsigned");
        assert_eq!(kind("summary"), "tinytext");
        assert_eq!(kind("flags"), "set('one','two')");
        assert!(kind("state").starts_with("enum("));
        assert_eq!(kind("observed_year"), "year");
        assert_eq!(kind("network_ip"), "varchar(45)");
        assert_eq!(kind("network_mac"), "varchar(17)");
        assert_eq!(kind("fixed_data"), "binary(4)");
        assert_eq!(kind("variable_data"), "varbinary(12)");
        assert_eq!(kind("observed_time"), "time(3)");
        assert_eq!(kind("seen_at"), "datetime(3)");
    } else {
        assert_eq!(kind("id"), "integer");
        assert_eq!(kind("summary"), "text");
        assert_eq!(kind("state"), "varchar");
        assert_eq!(kind("observed_year"), "integer");
        assert_eq!(kind("fixed_data"), "blob");
        assert_eq!(kind("variable_data"), "blob");
        assert_eq!(kind("observed_time"), "time");
        assert_eq!(kind("seen_at"), "datetime");
    }

    let id = db
        .table("additional_column_types")
        .insertGetId(BTreeMap::from([
            ("state".into(), "customer's choice".into()),
            ("fixed_data".into(), Value::Bytes(vec![0, 1, 2, 3])),
        ]))
        .await
        .unwrap();
    assert_eq!(id, 1.into());
    assert_eq!(
        db.table("additional_column_types")
            .value("state")
            .await
            .unwrap(),
        "customer's choice".into()
    );
    assert_eq!(
        db.table("additional_column_types")
            .value("fixed_data")
            .await
            .unwrap(),
        Value::Bytes(vec![0, 1, 2, 3])
    );
    let year = if mysql {
        "select cast(observed_year as signed) from additional_column_types"
    } else {
        "select observed_year from additional_column_types"
    };
    let current = if mysql {
        "select cast(year(curdate()) as signed)"
    } else {
        "select cast(strftime('%Y', 'now') as integer)"
    };
    assert_eq!(
        db.scalar(year, &[]).await.unwrap(),
        db.scalar(current, &[]).await.unwrap()
    );
    if mysql {
        assert_eq!(
            db.scalar(
                "select cast(flags as char) from additional_column_types",
                &[]
            )
            .await
            .unwrap(),
            "one,two".into()
        );
        assert!(
            schema
                .create("invalid_binary_size", |table| {
                    table.binaryWith("value", 256, true);
                })
                .await
                .is_err()
        );
    } else {
        assert!(matches!(
            schema
                .create("unsupported_set_type", |table| {
                    table.set("flags", &["one"]);
                })
                .await,
            Err(DatabaseError::UnsupportedSchema("SQLite set column"))
        ));
        schema
            .table("additional_column_types", |table| {
                table.ipAddress("network_ip").nullable().change();
            })
            .await
            .unwrap();
    }
    assert!(
        db.statement(
            "insert into additional_column_types (state) values (?)",
            &["unknown".into()]
        )
        .await
        .is_err()
    );
    assert!(matches!(
        schema
            .create("invalid_empty_enum", |table| {
                table.r#enum("status", &[]);
            })
            .await,
        Err(DatabaseError::InvalidSchema("allowed column values"))
    ));
    schema
        .create("small_identity_columns", |table| {
            table.smallIncrements("id");
        })
        .await
        .unwrap();
    schema
        .create("medium_identity_columns", |table| {
            table.mediumIncrements("id");
        })
        .await
        .unwrap();
    assert_eq!(
        schema
            .getColumnType("small_identity_columns", "id", true)
            .await
            .unwrap(),
        if mysql {
            "smallint unsigned"
        } else {
            "integer"
        }
    );
    assert_eq!(
        schema
            .getColumnType("medium_identity_columns", "id", true)
            .await
            .unwrap(),
        if mysql {
            "mediumint unsigned"
        } else {
            "integer"
        }
    );
    schema.drop("medium_identity_columns").await.unwrap();
    schema.drop("small_identity_columns").await.unwrap();
    schema.drop("additional_column_types").await.unwrap();
}

#[tokio::test]
async fn sqlite_compiles_and_enforces_additional_column_types() {
    verify_additional_column_types(
        connection(json!({"driver":"sqlite","database":":memory:"})),
        false,
    )
    .await;
}

#[tokio::test]
async fn mysql_compiles_and_enforces_additional_column_types() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    verify_additional_column_types(connection(json!({"url":url})), true).await;
}

async fn verify_generated_columns(db: Connection, mysql: bool) {
    let mut schema = db.getSchemaBuilder();
    schema.dropIfExists("generated_samples").await.unwrap();
    schema
        .create("generated_samples", |table| {
            table.id();
            table.integer("base_units");
            table.integer("bonus_units").default(4_i64);
            table.integer("twice").virtualAs("base_units * 2").index();
            table.integer("total").storedAs("base_units + bonus_units");
            table.json("payload").nullable();
            table
                .string("locale", 30)
                .virtualAsJson("payload->menu.name->value")
                .nullable();
            table
                .string("label", 50)
                .storedAsJson("payload->fields[0]->display")
                .nullable();
        })
        .await
        .unwrap();
    let columns = schema.getColumns("generated_samples").await.unwrap();
    let generation = |name| {
        columns
            .iter()
            .find(|column| column.name == name)
            .unwrap()
            .generation
            .as_ref()
            .unwrap()
    };
    assert_eq!(generation("twice").kind, GenerationType::Virtual);
    assert_eq!(generation("total").kind, GenerationType::Stored);
    assert!(generation("total").expression.contains("bonus_units"));
    assert_eq!(generation("locale").kind, GenerationType::Virtual);
    assert!(
        generation("locale")
            .expression
            .to_ascii_lowercase()
            .contains("json_extract")
    );
    assert_eq!(generation("label").kind, GenerationType::Stored);
    let json =
        Value::Json(json!({"menu.name":{"value":"zh_TW"},"fields":[{"display":"customer's"}]}));
    db.insert(
        "insert into generated_samples (base_units, payload) values (?, ?)",
        &[3.into(), json],
    )
    .await
    .unwrap();
    let row = db
        .table("generated_samples")
        .first()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.get("twice"), Some(&6.into()));
    assert_eq!(row.get("total"), Some(&7.into()));
    assert_eq!(row.get("locale"), Some(&"zh_TW".into()));
    assert_eq!(row.get("label"), Some(&"customer's".into()));

    db.update("update generated_samples set base_units = ?", &[5.into()])
        .await
        .unwrap();
    let row = db
        .table("generated_samples")
        .first()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.get("twice"), Some(&10.into()));
    assert_eq!(row.get("total"), Some(&9.into()));

    let query = "select * from generated_samples";
    let before = db.cursor(query, &[]).try_collect::<Vec<_>>().await.unwrap();
    assert!(before[0].get("offset_total").is_none());
    schema
        .table("generated_samples", |table| {
            table.integer("offset_total").virtualAs("total + 1");
        })
        .await
        .unwrap();
    assert_eq!(
        db.table("generated_samples")
            .value("offset_total")
            .await
            .unwrap(),
        10.into()
    );
    let after = db.cursor(query, &[]).try_collect::<Vec<_>>().await.unwrap();
    assert_eq!(after[0].get("offset_total"), Some(&10.into()));
    assert!(
        db.update("update generated_samples set total = 1", &[])
            .await
            .is_err()
    );
    assert!(matches!(
        schema
            .create("invalid_generated_default", |table| {
                table.integer("base");
                table
                    .integer("derived")
                    .virtualAs("base + 1")
                    .default(1_i64);
            })
            .await,
        Err(DatabaseError::InvalidSchema("generated column modifiers"))
    ));

    if mysql {
        schema
            .table("generated_samples", |table| {
                table
                    .integer("total")
                    .storedAs("base_units + bonus_units * 2")
                    .change();
            })
            .await
            .unwrap();
        assert_eq!(
            db.table("generated_samples").value("total").await.unwrap(),
            13.into()
        );
    } else {
        schema
            .table("generated_samples", |table| {
                table.integer("base_units").default(0_i64).change();
            })
            .await
            .unwrap();
        assert_eq!(
            db.table("generated_samples").value("twice").await.unwrap(),
            10.into()
        );
        assert_eq!(
            db.table("generated_samples").value("locale").await.unwrap(),
            "zh_TW".into()
        );
        assert!(
            schema
                .getIndexes("generated_samples")
                .await
                .unwrap()
                .iter()
                .any(|index| index.columns == [Some("twice".into())])
        );

        schema
            .table("generated_samples", |table| {
                table
                    .integer("total")
                    .storedAs("base_units + bonus_units * 2")
                    .change();
                table.integer("twice").change();
                table
                    .integer("bonus_units")
                    .storedAs("base_units + 1")
                    .change();
            })
            .await
            .unwrap();
        let row = db
            .table("generated_samples")
            .first()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.get("twice"), Some(&10.into()));
        assert_eq!(row.get("bonus_units"), Some(&6.into()));
        assert_eq!(row.get("total"), Some(&17.into()));
        assert_eq!(row.get("offset_total"), Some(&18.into()));
    }
    schema.drop("generated_samples").await.unwrap();
}

#[tokio::test]
async fn sqlite_generated_columns_survive_rebuild_and_mode_changes() {
    verify_generated_columns(
        connection(json!({"driver":"sqlite","database":":memory:"})),
        false,
    )
    .await;
}

#[tokio::test]
async fn mysql_generated_columns_compute_and_update_real_rows() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    verify_generated_columns(connection(json!({"url":url})), true).await;
}

#[tokio::test]
async fn mysql_spatial_columns_enforce_subtype_srid_and_index() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url}));
    let mut schema = db.getSchemaBuilder();
    schema.dropIfExists("spatial_sites").await.unwrap();
    schema
        .create("spatial_sites", |table| {
            table.id();
            table
                .geography("wgs_point", Some("POINT"), None)
                .spatialIndex();
            table.geometry("cartesian", None, None).nullable();
            table
                .geography("no_srid", Some("unrecognized"), Some(0))
                .nullable();
            table
                .geometry("projected", Some("point"), Some(3857))
                .nullable();
            for subtype in [
                "linestring",
                "polygon",
                "geometrycollection",
                "multipoint",
                "multilinestring",
                "multipolygon",
            ] {
                table
                    .geometry(&format!("shape_{subtype}"), Some(subtype), None)
                    .nullable();
            }
        })
        .await
        .unwrap_or_else(|error| match error {
            DatabaseError::Driver { source, .. } => panic!("spatial DDL failed: {source}"),
            _ => panic!("spatial DDL failed: {error}"),
        });
    let columns = schema.getColumns("spatial_sites").await.unwrap();
    let kind = |name: &str| {
        columns
            .iter()
            .find(|column| column.name == name)
            .unwrap()
            .type_name
            .as_str()
    };
    assert_eq!(kind("wgs_point"), "point");
    assert_eq!(kind("cartesian"), "geometry");
    assert_eq!(kind("no_srid"), "geometry");
    assert_eq!(kind("projected"), "point");
    for subtype in [
        "linestring",
        "polygon",
        "geometrycollection",
        "multipoint",
        "multilinestring",
        "multipolygon",
    ] {
        let catalog = if subtype == "geometrycollection" {
            "geomcollection"
        } else {
            subtype
        };
        assert_eq!(kind(&format!("shape_{subtype}")), catalog);
    }
    let declared_srid = "select cast(srs_id as signed) from information_schema.columns where table_schema = database() and table_name = 'spatial_sites' and column_name = ?";
    assert_eq!(
        db.scalar(declared_srid, &["wgs_point".into()])
            .await
            .unwrap(),
        4326.into()
    );
    assert_eq!(
        db.scalar(declared_srid, &["projected".into()])
            .await
            .unwrap(),
        3857.into()
    );
    assert_eq!(
        db.scalar(declared_srid, &["no_srid".into()]).await.unwrap(),
        Value::Null
    );
    let index = schema
        .getIndexes("spatial_sites")
        .await
        .unwrap()
        .into_iter()
        .find(|index| index.name == "spatial_sites_wgs_point_spatialindex")
        .unwrap();
    assert_eq!(index.columns, [Some("wgs_point".into())]);
    assert!(matches!(
        index.index_type.as_deref(),
        Some("spatial" | "rtree")
    ));

    db.insert("insert into spatial_sites (wgs_point, cartesian) values (st_geomfromtext(?, 4326), st_geomfromtext(?))", &["POINT(20 30)".into(), "POINT(1 2)".into()]).await.unwrap();
    assert_eq!(
        db.scalar("select st_astext(wgs_point) from spatial_sites", &[])
            .await
            .unwrap(),
        "POINT(20 30)".into()
    );
    assert!(
        db.insert(
            "insert into spatial_sites (wgs_point) values (st_geomfromtext(?, 0))",
            &["POINT(20 30)".into()]
        )
        .await
        .is_err()
    );
    assert!(
        db.insert(
            "insert into spatial_sites (wgs_point) values (st_geomfromtext(?, 4326))",
            &["LINESTRING(0 0, 1 1)".into()]
        )
        .await
        .is_err()
    );
    assert!(
        matches!(db.table("spatial_sites").value("wgs_point").await.unwrap(), Value::Bytes(data) if !data.is_empty())
    );

    schema
        .table("spatial_sites", |table| {
            table.dropSpatialIndex("spatial_sites_wgs_point_spatialindex");
            table
                .geometry("extra_shapes", Some("multipoint"), None)
                .nullable();
        })
        .await
        .unwrap();
    assert!(
        !schema
            .hasIndex("spatial_sites", "spatial_sites_wgs_point_spatialindex")
            .await
            .unwrap()
    );
    assert_eq!(
        schema
            .getColumnType("spatial_sites", "extra_shapes", false)
            .await
            .unwrap(),
        "multipoint"
    );
    schema.drop("spatial_sites").await.unwrap();
}

#[tokio::test]
async fn sqlite_spatial_columns_use_geometry_without_a_spatial_index() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    let mut schema = db.getSchemaBuilder();
    schema
        .create("spatial_sites", |table| {
            table.geometry("shape", Some("point"), Some(3857));
            table
                .geography("location", Some("polygon"), None)
                .nullable();
        })
        .await
        .unwrap();
    let columns = schema.getColumns("spatial_sites").await.unwrap();
    assert!(columns.iter().all(|column| column.type_name == "geometry"));
    let data = Value::Bytes(vec![1, 2, 3]);
    db.insert(
        "insert into spatial_sites (shape) values (?)",
        std::slice::from_ref(&data),
    )
    .await
    .unwrap();
    assert_eq!(
        db.table("spatial_sites").value("shape").await.unwrap(),
        data
    );

    assert!(matches!(
        schema
            .create("spatial_index_unsupported", |table| {
                table.geometry("shape", None, None).spatialIndex();
            })
            .await,
        Err(DatabaseError::UnsupportedSchema("SQLite index kind"))
    ));
    assert!(!schema.hasTable("spatial_index_unsupported").await.unwrap());
    assert!(matches!(
        schema
            .table("spatial_sites", |table| {
                table.geometry("shape", None, None).spatialIndex().change();
            })
            .await,
        Err(DatabaseError::UnsupportedSchema("SQLite index kind"))
    ));
    assert_eq!(
        db.table("spatial_sites").value("shape").await.unwrap(),
        data
    );
}

async fn verify_temporary_tables(db: Connection, mysql: bool) {
    db.unprepared("DROP TABLE IF EXISTS temp_scope_items; CREATE TABLE temp_scope_items (id BIGINT PRIMARY KEY); INSERT INTO temp_scope_items VALUES (41)").await.unwrap();
    let mut schema = db.getSchemaBuilder();
    assert!(matches!(
        schema
            .create("unleased_temporary_table", |table| {
                table.temporary();
                table.id();
            })
            .await,
        Err(DatabaseError::UnsupportedSchema(
            "temporary table requires a schema session"
        ))
    ));

    let mut session = schema.session().await.unwrap();
    let id = if mysql {
        Some(
            session.select("select connection_id()", &[]).await.unwrap()[0]
                .get_at(0)
                .unwrap()
                .clone(),
        )
    } else {
        None
    };
    let mut builder = session.getSchemaBuilder();
    builder
        .create("temp_scope_items", |table| {
            table.temporary();
            table.id();
            table.string("label", 20).index();
        })
        .await
        .unwrap();
    assert!(builder.hasTable("temp_scope_items").await.unwrap());
    if !mysql {
        assert!(
            builder
                .hasColumn("temp_scope_items", "label")
                .await
                .unwrap()
        );
        assert!(
            builder
                .hasIndex("temp_scope_items", "temp_temp_scope_items_label_index")
                .await
                .unwrap()
        );
    }
    session
        .statement(
            "insert into temp_scope_items (id, label) values (?, ?)",
            &[2.into(), "temporary".into()],
        )
        .await
        .unwrap();
    assert_eq!(
        session
            .select("select id from temp_scope_items", &[])
            .await
            .unwrap()[0]
            .get_at(0),
        Some(&if mysql { Value::Unsigned(2) } else { 2.into() })
    );
    let mut builder = session.getSchemaBuilder();
    builder
        .rename("temp_scope_items", "temp_scope_renamed")
        .await
        .unwrap();
    assert!(builder.hasTable("temp_scope_renamed").await.unwrap());
    assert!(builder.hasTable("temp_scope_items").await.unwrap());
    assert_eq!(
        session
            .select("select label from temp_scope_renamed", &[])
            .await
            .unwrap()[0]
            .get_at(0),
        Some(&"temporary".into())
    );
    assert_eq!(
        session
            .select("select id from temp_scope_items", &[])
            .await
            .unwrap()[0]
            .get_at(0),
        Some(&41.into())
    );
    session.close().await.unwrap();

    let mut session = schema.session().await.unwrap();
    if let Some(id) = id {
        assert_eq!(
            session.select("select connection_id()", &[]).await.unwrap()[0].get_at(0),
            Some(&id)
        );
    }
    assert!(
        session
            .select("select * from temp_scope_renamed", &[])
            .await
            .is_err()
    );
    let mut builder = session.getSchemaBuilder();
    assert!(
        builder
            .create("temp_scope_failed", |table| {
                table.temporary();
                table.id();
                table.index(&["id"], Some("temp_scope_duplicate"));
                table.index(&["id"], Some("temp_scope_duplicate"));
            })
            .await
            .is_err()
    );
    assert!(
        session
            .select("select * from temp_scope_failed", &[])
            .await
            .is_err()
    );

    session
        .getSchemaBuilder()
        .create("temp_scope_items", |table| {
            table.temporary();
            table.id();
        })
        .await
        .unwrap();
    session
        .getSchemaBuilder()
        .drop("temp_scope_items")
        .await
        .unwrap();
    assert_eq!(
        session
            .select("select id from temp_scope_items", &[])
            .await
            .unwrap()[0]
            .get_at(0),
        Some(&41.into())
    );
    session.close().await.unwrap();

    let ready = Arc::new(tokio::sync::Notify::new());
    let started = ready.clone();
    let owned = db.clone();
    let job = tokio::spawn(async move {
        let schema = owned.getSchemaBuilder();
        let mut session = schema.session().await.unwrap();
        session
            .getSchemaBuilder()
            .create("temp_scope_cancelled", |table| {
                table.temporary();
                table.id();
            })
            .await
            .unwrap();
        started.notify_one();
        std::future::pending::<()>().await;
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), ready.notified())
        .await
        .unwrap();
    job.abort();
    assert!(job.await.unwrap_err().is_cancelled());
    let mut session = tokio::time::timeout(std::time::Duration::from_secs(5), schema.session())
        .await
        .unwrap()
        .unwrap();
    assert!(
        session
            .select("select * from temp_scope_cancelled", &[])
            .await
            .is_err()
    );
    assert_eq!(
        session
            .select("select id from temp_scope_items", &[])
            .await
            .unwrap()[0]
            .get_at(0),
        Some(&41.into())
    );
    session.close().await.unwrap();
    schema.drop("temp_scope_items").await.unwrap();
}

#[tokio::test]
async fn sqlite_temporary_tables_stay_on_the_leased_session_and_are_cleaned() {
    verify_temporary_tables(
        connection(json!({"driver":"sqlite","database":":memory:"})),
        false,
    )
    .await;
}

#[tokio::test]
async fn mysql_temporary_tables_stay_on_the_leased_session_and_are_cleaned() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    verify_temporary_tables(connection(json!({"url":url,"max_connections":1})), true).await;
}

#[tokio::test]
async fn mysql_blueprints_apply_table_options_and_column_collation() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(
        json!({"url":url,"charset":"utf8mb4","collation":"utf8mb4_bin","engine":"MyISAM"}),
    );
    let mut schema = db.getSchemaBuilder();
    schema
        .dropIfExists("inherited_table_options")
        .await
        .unwrap();
    schema.dropIfExists("explicit_table_options").await.unwrap();
    schema
        .create("inherited_table_options", |table| {
            table.string("name", 30);
        })
        .await
        .unwrap();
    schema
        .create("explicit_table_options", |table| {
            table.engine("InnoDB");
            table.charset("latin1");
            table.collation("latin1_bin");
            table.comment("Customer's \\ archive");
            table
                .string("name", 30)
                .charset("utf8mb4")
                .collation("utf8mb4_0900_ai_ci");
        })
        .await
        .unwrap();
    let tables = schema.getTables(None).await.unwrap();
    let inherited = tables
        .iter()
        .find(|table| table.name == "inherited_table_options")
        .unwrap();
    assert_eq!(inherited.engine.as_deref(), Some("MyISAM"));
    assert_eq!(inherited.collation.as_deref(), Some("utf8mb4_bin"));
    let explicit = tables
        .iter()
        .find(|table| table.name == "explicit_table_options")
        .unwrap();
    assert_eq!(explicit.engine.as_deref(), Some("InnoDB"));
    assert_eq!(explicit.collation.as_deref(), Some("latin1_bin"));
    assert_eq!(explicit.comment.as_deref(), Some("Customer's \\ archive"));
    assert_eq!(
        schema.getColumns("inherited_table_options").await.unwrap()[0]
            .collation
            .as_deref(),
        Some("utf8mb4_bin")
    );
    assert_eq!(
        schema.getColumns("explicit_table_options").await.unwrap()[0]
            .collation
            .as_deref(),
        Some("utf8mb4_0900_ai_ci")
    );
    assert_eq!(db.scalar("select character_set_name as charset from information_schema.columns where table_schema = database() and table_name = ? and column_name = ?", &["explicit_table_options".into(),"name".into()]).await.unwrap(), "utf8mb4".into());
    db.insert(
        "insert into explicit_table_options values (?)",
        &["abc".into()],
    )
    .await
    .unwrap();
    assert_eq!(
        db.table("explicit_table_options")
            .r#where("name", "=", "ABC")
            .count("*")
            .await
            .unwrap(),
        1
    );
    schema
        .table("explicit_table_options", |table| {
            table.comment("Updated comment");
            table
                .string("name", 30)
                .charset("utf8mb4")
                .collation("utf8mb4_bin")
                .change();
        })
        .await
        .unwrap();
    assert_eq!(
        db.table("explicit_table_options")
            .r#where("name", "=", "ABC")
            .count("*")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        schema
            .getTables(None)
            .await
            .unwrap()
            .into_iter()
            .find(|table| table.name == "explicit_table_options")
            .unwrap()
            .comment
            .as_deref(),
        Some("Updated comment")
    );

    assert!(matches!(
        schema
            .create("invalid_table_options", |table| {
                table.engine("invalid engine name");
                table.id();
            })
            .await,
        Err(DatabaseError::InvalidSchema("table storage engine"))
    ));
    assert!(!schema.hasTable("invalid_table_options").await.unwrap());
    schema.drop("explicit_table_options").await.unwrap();
    schema.drop("inherited_table_options").await.unwrap();
}

#[tokio::test]
async fn sqlite_uses_column_collation_through_rebuild() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    let mut schema = db.getSchemaBuilder();
    schema
        .create("local_table_options", |table| {
            table.engine("InnoDB");
            table.charset("utf8mb4");
            table.collation("utf8mb4_bin");
            table.comment("MySQL metadata");
            table.string("name", 30).collation("nocase");
        })
        .await
        .unwrap();
    assert_eq!(
        schema.getColumns("local_table_options").await.unwrap()[0]
            .collation
            .as_deref(),
        Some("nocase")
    );
    let tables = schema.getTables(None).await.unwrap();
    let table = tables
        .iter()
        .find(|table| table.name == "local_table_options")
        .unwrap();
    assert_eq!(
        (
            table.engine.as_deref(),
            table.collation.as_deref(),
            table.comment.as_deref()
        ),
        (None, None, None)
    );
    db.insert(
        "insert into local_table_options values (?)",
        &["abc".into()],
    )
    .await
    .unwrap();
    assert_eq!(
        db.table("local_table_options")
            .r#where("name", "=", "ABC")
            .count("*")
            .await
            .unwrap(),
        1
    );
    schema
        .table("local_table_options", |table| {
            table.string("name", 30).collation("binary").change();
        })
        .await
        .unwrap();
    assert_eq!(
        db.table("local_table_options")
            .r#where("name", "=", "ABC")
            .count("*")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        schema.getColumns("local_table_options").await.unwrap()[0]
            .collation
            .as_deref(),
        Some("binary")
    );
}
