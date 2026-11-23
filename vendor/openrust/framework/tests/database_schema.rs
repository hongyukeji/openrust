use std::{collections::BTreeMap, sync::Arc};

use futures_util::TryStreamExt as _;

use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseError, DatabaseManager, DatabaseResult, Value,
        schema::{GenerationType, MorphKeyType, Schema, SchemaFallback},
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
                MorphKeyType::Int => Schema::default_morph_key_type(MorphKeyType::Int)?,
                MorphKeyType::Uuid => Schema::morph_using_uuids()?,
                MorphKeyType::Ulid => Schema::morph_using_ulids()?,
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn morph_columns_follow_the_current_app_and_keep_the_composite_index() {
    async fn verify(kind: Option<MorphKeyType>, expected: &str) {
        let mut app = TestApp::sqlite(|builder| match kind {
            Some(kind) => builder.with_providers(vec![Box::new(MorphKeyProvider(kind))]),
            None => builder,
        })
        .unwrap();
        let connection = app.database().await.unwrap();
        let mut schema = connection.get_schema_builder();
        schema
            .create("tokens", |table| table.morphs("tokenable"))
            .await
            .unwrap();
        let columns = schema.get_columns("tokens").await.unwrap();
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].name, "tokenable_type");
        assert_eq!(columns[1].name, "tokenable_id");
        assert_eq!(columns[1].full_type, expected);
        assert!(columns.iter().all(|column| !column.nullable));
        assert!(
            schema
                .get_indexes("tokens")
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

#[tokio::test]
async fn postgres_schema_catalog_uses_the_configured_search_path_and_native_queries() {
    let settings = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"pgsql","connections":{"pgsql":{
            "driver":"pgsql", "username":"app_role", "prefix":"app_",
            "search_path": r#""Tenant, A", "$user", "Double""Quote""#,
            "read":{"search_path":["replica", "public"]},
            "direct":{"search_path":["Migration Admin", "$user"], "username":"migration_role"}
        }}}),
    )]));
    let manager = DatabaseManager::new(Arc::new(settings), ".");
    for (name, expected) in [
        (None, vec!["Tenant, A", "app_role", "Double\"Quote"]),
        (Some("pgsql::read"), vec!["replica", "public"]),
        (
            Some("pgsql::direct"),
            vec!["Migration Admin", "migration_role"],
        ),
    ] {
        let db = manager.connection(name).unwrap();
        let schema = db.get_schema_builder();
        assert_eq!(schema.get_current_schema_name(), expected[0]);
        assert_eq!(schema.get_current_schema_listing(), expected);
        assert_eq!(
            schema
                .parse_schema_and_table("records", SchemaFallback::Current)
                .unwrap(),
            (Some(expected[0]), "records"),
        );
    }
    let legacy = connection(json!({"driver":"pgsql", "schema":["legacy", "public"]}));
    assert_eq!(
        legacy.get_schema_builder().get_current_schema_listing(),
        ["legacy", "public"]
    );

    let db = manager.connection(None).unwrap();
    let preview = db
        .pretend(|db| {
            Box::pin(async move {
                let mut schema = db.get_schema_builder();
                assert!(schema.get_schemas().await?.is_empty());
                assert!(schema.get_tables(None).await?.is_empty());
                assert!(
                    schema
                        .get_tables_in(&["Tenant, A", "public"])
                        .await?
                        .is_empty()
                );
                assert!(schema.get_views(None).await?.is_empty());
                assert!(schema.get_views_in(&["Tenant, A"]).await?.is_empty());
                assert!(!schema.has_view("records").await?);
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(preview.len(), 6);
    for (entry, fragment) in preview.iter().zip([
        "pg_catalog.current_schema()",
        "pg_catalog.pg_total_relation_size",
        "n.nspname::text in ('Tenant, A', 'public')",
        "pg_catalog.pg_views",
        "schemaname::text in ('Tenant, A')",
        "pg_catalog.current_schemas(false)",
    ]) {
        assert!(entry.query.contains(fragment), "{}", entry.query);
        assert_eq!(
            entry.read_write_type,
            openrust::database::events::QueryRole::Write
        );
    }
}

#[tokio::test]
async fn postgres_online_index_preview_uses_concurrent_ddl() {
    let db = connection(json!({"driver":"pgsql"}));
    let preview = db
        .pretend(|db| {
            Box::pin(async move {
                db.get_schema_builder()
                    .table("documents", |table| {
                        table
                            .index(&["title"], Some("docs_title"))
                            .algorithm("hash")
                            .online();
                        table
                            .unique(&["external_id"], Some("docs_external"))
                            .online();
                        table.full_text(&["title"], Some("docs_search")).online();
                        table
                            .vector_index("embedding", Some("docs_vector"))
                            .online();
                    })
                    .await
            })
        })
        .await
        .unwrap();
    assert_eq!(preview.len(), 5);
    for (entry, expected) in preview.iter().zip([
        "create index concurrently \"docs_title\" on \"documents\" using hash (\"title\")",
        "create unique index concurrently \"docs_external\" on \"documents\" (\"external_id\")",
        "alter table \"documents\" add constraint \"docs_external\" unique using index \"docs_external\"",
        "create index concurrently \"docs_search\" on \"documents\" using gin",
        "create index concurrently \"docs_vector\" on \"documents\" using hnsw",
    ]) {
        assert!(entry.query.starts_with(expected), "{}", entry.query);
        assert_eq!(
            entry.read_write_type,
            openrust::database::events::QueryRole::Write
        );
    }
}

#[tokio::test]
async fn postgres_ordinary_index_methods_and_renames_preserve_schema() {
    let preview = connection(json!({"driver":"pgsql"}));
    let queries = preview
        .pretend(|db| {
            Box::pin(async move {
                db.get_schema_builder()
                    .table("tenant.entries", |table| {
                        table.rename_index("lookup", "ordered");
                        table.rename_index("tenant.ordered", "lookup");
                    })
                    .await
            })
        })
        .await
        .unwrap();
    assert_eq!(
        queries[0].query,
        "alter index \"tenant\".\"lookup\" rename to \"ordered\""
    );
    assert_eq!(
        queries[1].query,
        "alter index \"tenant\".\"ordered\" rename to \"lookup\""
    );
    for method in ["", "1brin", "brin btree", "brin; select 1"] {
        let rejected = preview
            .get_schema_builder()
            .table("tenant.entries", |table| {
                table.index(&["number"], Some("lookup")).algorithm(method);
            })
            .await;
        assert!(matches!(
            rejected,
            Err(DatabaseError::InvalidSchema("PostgreSQL index method"))
        ));
    }

    let Ok(url) = std::env::var("OPENRUST_TEST_POSTGRES_INDEX_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_POSTGRES_INDEX").is_none());
        return;
    };
    let db = connection(json!({"url":url}));
    db.unprepared("create schema tenant; create schema neighbor; create table tenant.entries (number integer, label text); create table neighbor.entries (number integer); create index lookup on neighbor.entries (number)")
        .await.unwrap();
    let mut schema = db.get_schema_builder();
    schema
        .table("tenant.entries", |table| {
            table.index(&["number"], Some("lookup")).algorithm("BRIN");
            table
                .index(&["number"], Some("hashed"))
                .algorithm("hash")
                .online();
            table
                .raw_index("lower(label)", "lookup_lower")
                .algorithm("btree");
        })
        .await
        .unwrap();
    let entries = schema.get_indexes("tenant.entries").await.unwrap();
    for (name, method) in [
        ("lookup", "brin"),
        ("hashed", "hash"),
        ("lookup_lower", "btree"),
    ] {
        let index = entries.iter().find(|index| index.name == name).unwrap();
        assert_eq!(index.index_type.as_deref(), Some(method));
    }
    let rollback = db
        .transaction(
            |tx| {
                Box::pin(async {
                    tx.get_schema_builder()
                        .table("tenant.entries", |table| {
                            table.rename_index("lookup", "rolled_back")
                        })
                        .await?;
                    Err::<(), _>(DatabaseError::InvalidSchema("rollback rename"))
                })
            },
            1,
        )
        .await;
    assert!(matches!(
        rollback,
        Err(DatabaseError::InvalidSchema("rollback rename"))
    ));
    assert!(
        schema
            .get_indexes("tenant.entries")
            .await
            .unwrap()
            .iter()
            .any(|index| index.name == "lookup")
    );
    schema
        .table("tenant.entries", |table| {
            table.rename_index("lookup", "ordered")
        })
        .await
        .unwrap();
    schema
        .table("tenant.entries", |table| {
            table.rename_index("tenant.ordered", "qualified")
        })
        .await
        .unwrap();
    assert!(
        schema
            .get_indexes("tenant.entries")
            .await
            .unwrap()
            .iter()
            .any(|index| index.name == "qualified" && index.index_type.as_deref() == Some("brin"))
    );
    assert_eq!(
        schema.get_indexes("neighbor.entries").await.unwrap()[0].name,
        "lookup"
    );
    schema
        .table("tenant.entries", |table| table.drop_index("qualified"))
        .await
        .unwrap();
    assert!(
        schema
            .get_indexes("tenant.entries")
            .await
            .unwrap()
            .iter()
            .all(|index| index.name != "qualified")
    );
}

#[tokio::test]
async fn postgres_unique_constraints_support_null_semantics_and_existing_indexes() {
    let Ok(url) = std::env::var("OPENRUST_TEST_POSTGRES_UNIQUE_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_POSTGRES_UNIQUE").is_none());
        return;
    };
    let db = connection(json!({"url":url}));
    db.unprepared("create schema tenant; create table tenant.accounts (reference integer); create table tenant.legacy (reference integer); create table tenant.other (reference integer); create unique index legacy_reference on tenant.legacy (reference); create index ordinary_reference on tenant.legacy (reference); create unique index other_reference on tenant.other (reference)")
        .await.unwrap();
    let mut schema = db.get_schema_builder();
    schema
        .table("tenant.accounts", |table| {
            table
                .unique(&["reference"], None)
                .nulls_not_distinct(true)
                .deferrable(true)
                .initially_immediate(false);
        })
        .await
        .unwrap();
    db.transaction(|tx| Box::pin(async {
        tx.statement("insert into tenant.accounts values (null), (null)", &[]).await?;
                tx.statement("update tenant.accounts set reference = 1 where ctid = (select ctid from tenant.accounts limit 1)", &[]).await?;
        Ok(())
    }),1).await.unwrap();
    assert!(
        db.statement("insert into tenant.accounts values (null)", &[])
            .await
            .is_err()
    );
    assert_eq!(db.table("tenant.accounts").count("*").await.unwrap(), 2);
    schema
        .table("tenant.accounts", |table| {
            table.drop_unique_columns(&["reference"])
        })
        .await
        .unwrap();
    schema
        .table("tenant.accounts", |table| {
            table
                .unique(&["reference"], Some("online_reference"))
                .algorithm("btree")
                .online()
                .nulls_not_distinct(true);
        })
        .await
        .unwrap();
    assert_eq!(
        db.scalar("select i.indnullsnotdistinct from pg_constraint c join pg_index i on i.indexrelid = c.conindid where c.conname = 'online_reference'", &[]).await.unwrap(),
        true.into()
    );
    assert!(
        db.statement("insert into tenant.accounts values (null)", &[])
            .await
            .is_err()
    );
    schema
        .table("tenant.accounts", |table| {
            table.drop_unique("online_reference")
        })
        .await
        .unwrap();
    schema
        .table("tenant.accounts", |table| {
            table
                .unique(&["reference"], Some("distinct_reference"))
                .algorithm("btree")
                .nulls_not_distinct(false)
                .deferrable(false);
        })
        .await
        .unwrap();
    db.statement("insert into tenant.accounts values (null)", &[])
        .await
        .unwrap();
    schema
        .table("tenant.accounts", |table| {
            table.drop_unique("distinct_reference")
        })
        .await
        .unwrap();
    schema
        .table("tenant.legacy", |table| {
            table.drop_unique("legacy_reference")
        })
        .await
        .unwrap();
    for index in ["ordinary_reference", "other_reference"] {
        assert!(
            schema
                .table("tenant.legacy", |table| table.drop_unique(index))
                .await
                .is_err()
        );
    }
    assert_eq!(
        db.scalar("select count(*) from pg_indexes where schemaname = 'tenant' and indexname in ('ordinary_reference','other_reference')", &[]).await.unwrap(),
        2.into()
    );
}

#[tokio::test]
async fn mariadb_vector_index_options_are_validated_and_compiled() {
    let db = connection(json!({"driver":"mariadb"}));
    let preview = db
        .pretend(|db| {
            Box::pin(async move {
                db.get_schema_builder()
                    .table("documents", |table| {
                        table
                            .vector_index("embedding", Some("semantic"))
                            .operator_class("distance=EUCLIDEAN M=8");
                    })
                    .await
            })
        })
        .await
        .unwrap();
    assert_eq!(
        preview[0].query,
        "alter table `documents` add vector index `semantic` (`embedding`) M=8 DISTANCE=euclidean"
    );
    for options in ["M=2", "M=201", "M=8 M=9", "DISTANCE=dot", "M=8; SELECT 1"] {
        let result = db
            .get_schema_builder()
            .table("documents", |table| {
                table
                    .vector_index("embedding", None)
                    .operator_class(options);
            })
            .await;
        assert!(matches!(
            result,
            Err(DatabaseError::InvalidSchema("MariaDB vector index options"))
        ));
    }
    let result = db
        .get_schema_builder()
        .table("documents", |table| {
            table.vector_index("embedding", None).online();
        })
        .await;
    assert!(matches!(
        result,
        Err(DatabaseError::UnsupportedSchema("online index"))
    ));
}

#[tokio::test]
async fn postgres_vector_extension_previews_on_the_writer_with_quoted_identifiers() {
    let db = connection(json!({"driver":"pgsql"}));
    let preview = db
        .pretend(|db| {
            Box::pin(async move {
                let mut schema = db.get_schema_builder();
                schema.ensure_vector_extension_exists(None).await?;
                schema
                    .ensure_extension_exists("special\"ext", Some("app\"extensions"))
                    .await
            })
        })
        .await
        .unwrap();
    assert_eq!(preview.len(), 2);
    assert_eq!(
        preview[0].query,
        "create extension if not exists \"vector\""
    );
    assert_eq!(
        preview[1].query,
        "create extension if not exists \"special\"\"ext\" schema \"app\"\"extensions\""
    );
    assert!(
        preview
            .iter()
            .all(|entry| entry.read_write_type == openrust::database::events::QueryRole::Write)
    );
}

#[tokio::test]
async fn mariadb_vector_ddl_previews_fluent_and_explicit_indexes() {
    let db = connection(json!({"driver":"mariadb"}));
    let preview = db
        .pretend(|db| {
            Box::pin(async move {
                let mut schema = db.get_schema_builder();
                schema
                    .create("documents", |table| {
                        table.id();
                        table.vector("embedding", Some(3)).index();
                    })
                    .await?;
                schema
                    .table("documents", |table| {
                        table.drop_vector_index_columns(&["embedding"])
                    })
                    .await?;
                schema
                    .table("documents", |table| {
                        table.vector_index("embedding", Some("semantic"));
                    })
                    .await?;
                schema
                    .table("documents", |table| table.drop_vector_index("semantic"))
                    .await
            })
        })
        .await
        .unwrap();
    assert_eq!(preview.len(), 5);
    assert!(preview[0].query.contains("`embedding` vector(3) not null"));
    assert_eq!(
        preview[1].query,
        "alter table `documents` add vector index `documents_embedding_vectorindex` (`embedding`) M=6 DISTANCE=cosine"
    );
    assert_eq!(
        preview[2].query,
        "alter table `documents` drop index `documents_embedding_vectorindex`"
    );
    assert_eq!(
        preview[3].query,
        "alter table `documents` add vector index `semantic` (`embedding`) M=6 DISTANCE=cosine"
    );
    assert_eq!(
        preview[4].query,
        "alter table `documents` drop index `semantic`"
    );
}

#[tokio::test]
async fn vector_schema_rejects_missing_dimensions_and_unsupported_drivers() {
    let maria = connection(json!({"driver":"mariadb"}));
    assert!(matches!(
        maria
            .get_schema_builder()
            .create("documents", |table| {
                table.vector("embedding", None);
            })
            .await,
        Err(DatabaseError::InvalidSchema("MariaDB vector dimensions"))
    ));
    for driver in ["mysql", "sqlite", "sqlsrv"] {
        let db = connection(json!({"driver":driver}));
        let mut schema = db.get_schema_builder();
        assert!(matches!(
            schema.ensure_vector_extension_exists(None).await,
            Err(DatabaseError::UnsupportedSchema(_))
        ));
        assert!(matches!(
            schema
                .create("documents", |table| {
                    table.vector("embedding", Some(3));
                })
                .await,
            Err(DatabaseError::UnsupportedSchema("vector column"))
        ));
        assert!(matches!(
            schema
                .table("documents", |table| {
                    table.vector_index("embedding", None);
                })
                .await,
            Err(DatabaseError::UnsupportedSchema("vector index"))
        ));
    }
}

#[tokio::test]
async fn postgres_type_catalog_uses_the_writer_and_supports_schema_filters() {
    let db = connection(json!({"driver":"pgsql"}));
    let preview = db
        .pretend(|db| {
            Box::pin(async move {
                let mut schema = db.get_schema_builder();
                assert!(schema.get_types(None).await?.is_empty());
                assert!(schema.get_types(Some("tenant")).await?.is_empty());
                assert!(schema.get_types_in(&["tenant", "public"]).await?.is_empty());
                assert!(schema.get_types_in(&[]).await?.is_empty());
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(preview.len(), 3);
    assert!(preview[0].query.contains("pg_catalog.pg_type"));
    assert!(preview[0].query.contains("or t.typtype = 'm') as implicit"));
    assert!(preview[0].query.contains("schema <> 'information_schema'"));
    assert!(preview[1].query.contains("schema::text in ('tenant')"));
    assert!(
        preview[2]
            .query
            .contains("schema::text in ('tenant', 'public')")
    );
    assert!(
        preview
            .iter()
            .all(|entry| entry.read_write_type == openrust::database::events::QueryRole::Write)
    );
    let mysql = connection(json!({"driver":"mysql"}));
    assert!(matches!(
        mysql.get_schema_builder().get_types(None).await,
        Err(DatabaseError::UnsupportedSchema(_))
    ));
}

#[tokio::test]
async fn postgres_foreign_keys_support_validation_timing_and_qualified_targets() {
    let Ok(url) = std::env::var("OPENRUST_TEST_POSTGRES_FOREIGN_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_POSTGRES_FOREIGN").is_none());
        return;
    };
    let db = connection(json!({"url":url}));
    db.unprepared("create schema parents; create schema children; create table parents.records (id integer primary key); create table children.records (id integer primary key, parent_id integer not null); insert into children.records values (1, 100)")
        .await.unwrap();
    let mut schema = db.get_schema_builder();
    schema
        .table("children.records", |table| {
            table
                .foreign(&["parent_id"], None)
                .references("id")
                .on("parents.records")
                .deferrable(true)
                .initially_immediate(false)
                .not_valid();
        })
        .await
        .unwrap_or_else(|error| panic!("{error}: {:?}", std::error::Error::source(&error)));
    let keys = schema.get_foreign_keys("children.records").await.unwrap();
    assert_eq!(keys[0].foreign_schema.as_deref(), Some("parents"));
    let settings = db.select_one(
        "select condeferrable, condeferred, convalidated from pg_constraint where conname = ? and conrelid = 'children.records'::regclass",
        &[keys[0].name.as_deref().unwrap().into()],
    ).await.unwrap().unwrap();
    assert_eq!(settings.get_at(0), Some(&true.into()));
    assert_eq!(settings.get_at(1), Some(&true.into()));
    assert_eq!(settings.get_at(2), Some(&false.into()));
    db.transaction(
        |tx| {
            Box::pin(async move {
                tx.statement("insert into children.records values (2, 200)", &[])
                    .await?;
                tx.statement("insert into parents.records values (200)", &[])
                    .await?;
                Ok(())
            })
        },
        1,
    )
    .await
    .unwrap();
    let failed = db
        .transaction(
            |tx| {
                Box::pin(async move {
                    tx.statement("insert into children.records values (3, 300)", &[])
                        .await?;
                    Ok(())
                })
            },
            1,
        )
        .await;
    assert!(failed.is_err());
    assert_eq!(db.table("children.records").count("*").await.unwrap(), 2);
    schema
        .table("children.records", |table| {
            table.drop_foreign_columns(&["parent_id"])
        })
        .await
        .unwrap();
    assert!(
        schema
            .get_foreign_keys("children.records")
            .await
            .unwrap()
            .is_empty()
    );
    schema
        .create("children.fresh", |table| {
            table.integer("parent_id");
            table
                .foreign(&["parent_id"], Some("explicit_fk"))
                .references("id")
                .on("parents.records")
                .deferrable(true)
                .initially_immediate(true);
        })
        .await
        .unwrap();
    assert!(
        db.statement("insert into children.fresh values (999)", &[])
            .await
            .is_err()
    );
    schema
        .table("children.fresh", |table| table.drop_foreign("explicit_fk"))
        .await
        .unwrap();
    db.statement("insert into children.fresh values (999)", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn postgres_column_and_foreign_key_catalogs_use_native_metadata_and_qualified_names() {
    let db = connection(json!({"driver":"pgsql", "prefix":"app_"}));
    let preview = db
        .pretend(|db| {
            Box::pin(async move {
                let mut schema = db.get_schema_builder();
                assert!(schema.get_columns("tenant.posts").await?.is_empty());
                assert!(schema.get_foreign_keys("posts").await?.is_empty());
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(preview.len(), 2);
    let columns = &preview[0].query;
    assert!(columns.contains("pg_catalog.pg_attribute"));
    assert!(columns.contains("coalesce(nullif('tenant', ''), pg_catalog.current_schema()::text)"));
    assert!(columns.contains("c.relname::text = 'app_posts'"));
    assert!(columns.contains("not a.attisdropped"));
    assert!(columns.contains("to_jsonb(a)->>'attgenerated'"));
    assert!(columns.contains("a.attidentity <> ''"));
    assert!(columns.contains("a.attnum = any(p.conkey)"));

    let keys = &preview[1].query;
    assert!(keys.contains("coalesce(nullif('', ''), pg_catalog.current_schema()::text)"));
    assert!(keys.contains("source.relname::text = 'app_posts'"));
    assert!(keys.contains("unnest(con.conkey, con.confkey) with ordinality"));
    assert!(keys.contains("order by con.conname, pair.position"));
    assert!(keys.contains("target_ns.nspname::text as foreign_schema"));
    assert!(keys.contains("when 'n' then 'set null'"));
    assert!(
        preview
            .iter()
            .all(|entry| entry.read_write_type == openrust::database::events::QueryRole::Write)
    );
}

async fn verify(db: Connection, mysql: bool) {
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("posts").await.unwrap();
    schema.drop_if_exists("writers").await.unwrap();
    schema
        .create("writers", |table| {
            table.id();
            table.string("name", 80).unique();
            table.timestamps();
            table.date("active_on").use_current();
        })
        .await
        .unwrap();
    schema
        .create("posts", |table| {
            table.id();
            table
                .foreign_id("writer_id")
                .nullable()
                .constrained_on("writers", "id", Some("post_author_fk"))
                .cascade_on_update()
                .null_on_delete();
            table.string("title", 120).default("O'Reilly \\ notes");
            table.text("scratch").nullable();
            table.boolean("visible").default(false);
            table.decimal("amount", 12, 2).default(0);
            table.index(&["writer_id", "title"], Some("post_writer_title"));
        })
        .await
        .unwrap();

    assert!(schema.has_table("posts").await.unwrap());
    assert!(!schema.has_table("unknown").await.unwrap());
    assert!(
        schema
            .get_tables(None)
            .await
            .unwrap()
            .iter()
            .any(|table| table.name == "app_posts")
    );
    assert_eq!(
        schema.get_column_listing("posts").await.unwrap(),
        ["id", "writer_id", "title", "scratch", "visible", "amount"]
    );
    assert!(schema.has_columns("posts", &["ID", "Title"]).await.unwrap());
    let columns = schema.get_columns("posts").await.unwrap();
    assert!(columns[0].auto_increment && columns[0].primary);
    assert!(columns[1].nullable);
    assert_eq!(
        schema
            .get_column_type("posts", "title", true)
            .await
            .unwrap(),
        if mysql { "varchar(120)" } else { "varchar" }
    );
    let indexes = schema.get_indexes("posts").await.unwrap();
    assert!(
        indexes
            .iter()
            .any(|index| index.primary && index.columns == [Some("id".into())])
    );
    assert!(indexes.iter().any(|index| index.name == "post_writer_title"
        && index.columns == [Some("writer_id".into()), Some("title".into())]));
    assert!(
        schema
            .has_index("writers", "app_writers_name_unique")
            .await
            .unwrap()
    );

    let keys = schema.get_foreign_keys("posts").await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].foreign_table, "app_writers");
    assert_eq!(
        keys[0].foreign_schema.as_deref(),
        Some(if mysql {
            db.get_database_name()
        } else {
            "main"
        })
    );
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
            table.rename_column("scratch", "draft");
            table.drop_column(&["draft"]);
            table.unique(&["slug"], None);
        })
        .await
        .unwrap();
    assert!(schema.has_column("posts", "slug").await.unwrap());
    assert!(!schema.has_column("posts", "draft").await.unwrap());
    schema
        .table("posts", |table| {
            table.drop_unique("app_posts_slug_unique");
        })
        .await
        .unwrap();
    if mysql {
        schema
            .table("posts", |table| {
                table.rename_index("post_writer_title", "post_lookup");
                table
                    .string("title", 150)
                    .nullable()
                    .comment("reviewed")
                    .change();
                table.drop_foreign("post_author_fk");
            })
            .await
            .unwrap();
        assert_eq!(
            schema
                .get_column_type("posts", "title", true)
                .await
                .unwrap(),
            "varchar(150)"
        );
        let column = schema.get_column("posts", "TITLE").await.unwrap().unwrap();
        assert_eq!(column.name, "title");
        assert_eq!(column.full_type, "varchar(150)");
        assert_eq!(column.comment.as_deref(), Some("reviewed"));
        assert!(column.nullable);
        assert_eq!(schema.get_column("posts", "missing").await.unwrap(), None);
        assert!(matches!(
            schema.get_column_type("posts", "missing", false).await,
            Err(DatabaseError::InvalidSchema("unknown column"))
        ));
        assert!(schema.has_index("posts", "post_lookup").await.unwrap());
        assert!(schema.get_foreign_keys("posts").await.unwrap().is_empty());
    }

    schema.rename("posts", "articles").await.unwrap();
    assert!(schema.has_table("articles").await.unwrap());
    assert!(!schema.has_table("posts").await.unwrap());
    schema.drop("articles").await.unwrap();
    schema.drop("writers").await.unwrap();
}

#[tokio::test]
async fn schema_builds_and_inspects_real_sqlite() {
    let db = connection(
        json!({"driver":"sqlite","database":":memory:","prefix":"app_","prefix_indexes":true}),
    );
    verify(db.clone(), false).await;
    let mut schema = db.get_schema_builder();
    assert!(matches!(
        schema
            .create("bad", |table| {
                table.string("hidden", 80).invisible();
            })
            .await,
        Err(DatabaseError::UnsupportedSchema(_))
    ));
    assert!(!schema.has_table("bad").await.unwrap());
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
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("column_modifiers").await.unwrap();
    schema
        .create("column_modifiers", |table| {
            table.id();
            table.string("label", 80);
            table.string("hidden", 80).invisible().default("secret");
            table
                .timestamp("touched_at", 6)
                .use_current()
                .use_current_on_update();
            table
                .date_time("changed_at", 3)
                .use_current()
                .use_current_on_update();
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
        schema.get_column_listing("column_modifiers").await.unwrap(),
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
        .select_one("select * from app_column_modifiers", &[])
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
    let row = db.select_one("select date_format(touched_at, '%Y-%m-%d %H:%i:%s.%f') as touched_at, date_format(changed_at, '%Y-%m-%d %H:%i:%s.%f') as changed_at from app_column_modifiers", &[])
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
                table.integer("bad").use_current_on_update();
            })
            .await,
        Err(DatabaseError::InvalidSchema(_))
    ));
    assert!(
        !schema
            .has_column("column_modifiers", "not_added")
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
    assert!(!schema.has_table("bad_position").await.unwrap());
    schema.drop("column_modifiers").await.unwrap();
}

#[tokio::test]
async fn sqlite_ddl_rolls_back_and_invalid_blueprint_has_no_effect() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    let mut schema = db.get_schema_builder();
    assert!(matches!(
        schema
            .create("bad", |table| {
                table.id();
                table.full_text(&["id"], None);
            })
            .await,
        Err(DatabaseError::UnsupportedSchema("SQLite index kind"))
    ));
    assert!(!schema.has_table("bad").await.unwrap());
    let result = schema
        .create("partial", |table| {
            table.id();
            table.index(&["id"], Some("same"));
            table.index(&["id"], Some("same"));
        })
        .await;
    assert!(matches!(result, Err(DatabaseError::Driver { .. })));
    assert!(!schema.has_table("partial").await.unwrap());

    let mut transaction = db.begin_transaction().await.unwrap();
    transaction
        .get_schema_builder()
        .create("pending", |table| {
            table.id();
        })
        .await
        .unwrap();
    assert!(
        transaction
            .get_schema_builder()
            .has_table("pending")
            .await
            .unwrap()
    );
    transaction.roll_back(None).await.unwrap();
    assert!(!schema.has_table("pending").await.unwrap());
}

#[tokio::test]
async fn sqlite_rebuild_preserves_children_catalog_objects_and_sequence() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    let mut schema = db.get_schema_builder();
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
            table
                .foreign_id("parent_id")
                .constrained()
                .cascade_on_delete();
        })
        .await
        .unwrap();
    db.unprepared(
        "create table audit_log (value text);          alter table parents add column label_size integer generated always as (length(label)) virtual;          create index label_expr on parents (lower(label)) where label is not null;          create trigger parent_insert after insert on parents begin insert into audit_log values (new.label); end;          create view parent_view as select label_size from parents;          insert into parents (id,label) values (1,'first'),(100,'removed');          delete from parents where id=100;          insert into children (parent_id) values (1)",
    ).await.unwrap();

    schema
        .table("parents", |table| {
            table.text("label").nullable().change();
            table.rename_index("label_expr", "label_lookup");
        })
        .await
        .unwrap();
    assert_eq!(
        schema
            .get_column_type("parents", "label", true)
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
    assert!(schema.has_index("parents", "label_lookup").await.unwrap());
    assert!(!schema.has_index("parents", "label_expr").await.unwrap());
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
            .get_columns("parents")
            .await
            .unwrap()
            .iter()
            .any(|column| column.name == "label" && column.nullable)
    );
    assert!(schema.has_index("parents", "label_lookup").await.unwrap());

    schema
        .table("children", |table| {
            table.drop_foreign_columns(&["parent_id"]);
        })
        .await
        .unwrap();
    assert!(
        schema
            .get_foreign_keys("children")
            .await
            .unwrap()
            .is_empty()
    );
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
                    .cascade_on_delete();
            })
            .await,
        Err(DatabaseError::InvalidSchema("SQLite foreign key check"))
    ));
    assert!(
        schema
            .get_foreign_keys("children")
            .await
            .unwrap()
            .is_empty()
    );
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
                .cascade_on_delete();
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
                    assert!(Schema::has_column("schema_artisan_probe", "title").await?);
                    assert!(
                        Schema::connection("sqlite")?
                            .has_index("schema_artisan_probe", "schema_artisan_probe_title_unique")
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
        .with_commands(register)
        .create()
        .unwrap();
    assert_eq!(
        application
            .handle_command(&["schema:probe".into()])
            .await
            .unwrap(),
        0
    );
    let isolated = connection(json!({"driver":"sqlite","database":file}));
    assert!(
        isolated
            .get_schema_builder()
            .has_table("schema_artisan_probe")
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
    let mut schema = db.get_schema_builder();
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
            table.foreign_id("parent_id").constrained();
        })
        .await
        .unwrap();
    let mut blocker = db.begin_transaction().await.unwrap();
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
                .get_column_type("parents", "label", true)
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
    let mut first = db.begin_transaction().await.unwrap();
    let mut second = db.begin_transaction().await.unwrap();
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
        transaction.roll_back(None).await.unwrap();
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
    let schema = db.get_schema_builder();
    let cannot_insert = "INSERT INTO fk_scope_children VALUES (?, 99)";
    assert!(db.statement(cannot_insert, &[0.into()]).await.is_err());

    let count = schema
        .without_foreign_key_constraints(|session| {
            Box::pin(async move {
                assert_eq!(
                    session.select(state, &[]).await?[0].get_at(0),
                    Some(&0.into())
                );
                assert!(session.statement(cannot_insert, &[1.into()]).await?);
                let mut builder = session.get_schema_builder();
                builder
                    .create("fk_scope_marker", |table| {
                        table.id();
                    })
                    .await?;
                assert!(builder.has_table("fk_scope_marker").await?);
                Ok(1)
            })
        })
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert!(db.statement(cannot_insert, &[2.into()]).await.is_err());

    let result = schema
        .without_foreign_key_constraints(|_| {
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
            .get_schema_builder()
            .without_foreign_key_constraints(move |session| {
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
    session.disable_foreign_key_constraints().await.unwrap();
    session.statement(cannot_insert, &[5.into()]).await.unwrap();
    session.enable_foreign_key_constraints().await.unwrap();
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
    let mut session = db.get_schema_builder().session().await.unwrap();
    session.enable_foreign_key_constraints().await.unwrap();
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
    let mut schema = db.get_schema_builder();
    schema
        .drop_if_exists("additional_column_types")
        .await
        .unwrap();
    schema
        .create("additional_column_types", |table| {
            table.tiny_increments("id");
            table.unsigned_tiny_integer("tiny_value").default(255_i64);
            table
                .unsigned_small_integer("small_value")
                .default(65_535_i64);
            table
                .unsigned_medium_integer("medium_value")
                .default(16_777_215_i64);
            table.tinyText("summary").nullable();
            table
                .r#enum("state", &["ready", "customer's choice"])
                .default("ready");
            table.year("observed_year").use_current();
            table.ipAddress("network_ip").nullable();
            table.mac_address("network_mac").nullable();
            table.binary_with("fixed_data", 4, true).nullable();
            table.binary_with("variable_data", 12, false).nullable();
            table.time_tz("observed_time", 3).nullable();
            table.date_time_tz("seen_at", 3).nullable();
            table.timestamps_tz();
            table.soft_deletes_tz();
            if mysql {
                table.set("flags", &["one", "two"]).default("one,two");
            }
        })
        .await
        .unwrap_or_else(|error| match error {
            DatabaseError::Driver { source, .. } => panic!("column DDL failed: {source}"),
            _ => panic!("column DDL failed: {error}"),
        });

    let columns = schema.get_columns("additional_column_types").await.unwrap();
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
        .insert_get_id(BTreeMap::from([
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
                    table.binary_with("value", 256, true);
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
            table.small_increments("id");
        })
        .await
        .unwrap();
    schema
        .create("medium_identity_columns", |table| {
            table.medium_increments("id");
        })
        .await
        .unwrap();
    assert_eq!(
        schema
            .get_column_type("small_identity_columns", "id", true)
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
            .get_column_type("medium_identity_columns", "id", true)
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

#[tokio::test]
async fn mysql_configures_ulid_and_foreign_ulid_lengths() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url,"prefix":"app_"}));
    let mut schema = db.get_schema_builder();
    schema
        .create("ulid_imports", |table| {
            table.ulid_with("id", 32).primary();
            table.ulid("standard_id");
        })
        .await
        .unwrap();
    schema
        .create("ulid_import_entries", |table| {
            table.id();
            table
                .foreign_ulid_with("import_id", 32)
                .constrained_on("ulid_imports", "id", None)
                .cascade_on_delete();
            table.foreign_ulid("standard_ref").nullable();
        })
        .await
        .unwrap();

    for (table, column, width) in [
        ("ulid_imports", "id", "char(32)"),
        ("ulid_imports", "standard_id", "char(26)"),
        ("ulid_import_entries", "import_id", "char(32)"),
        ("ulid_import_entries", "standard_ref", "char(26)"),
    ] {
        assert_eq!(
            schema.get_column_type(table, column, true).await.unwrap(),
            width
        );
    }
    let import = "IMPORT01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let standard = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    assert_eq!(import.len(), 32);
    db.insert(
        "insert into app_ulid_imports (id, standard_id) values (?, ?)",
        &[import.into(), standard.into()],
    )
    .await
    .unwrap();
    let insert = "insert into app_ulid_import_entries (import_id) values (?)";
    db.insert(insert, &[import.into()]).await.unwrap();
    assert!(db.insert(insert, &[standard.into()]).await.is_err());
    db.delete(
        "delete from app_ulid_imports where id = ?",
        &[import.into()],
    )
    .await
    .unwrap();
    assert_eq!(db.table("ulid_import_entries").count("*").await.unwrap(), 0);
}

async fn verify_generated_columns(db: Connection, mysql: bool) {
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("generated_samples").await.unwrap();
    schema
        .create("generated_samples", |table| {
            table.id();
            table.integer("base_units");
            table.integer("bonus_units").default(4_i64);
            table.integer("twice").virtual_as("base_units * 2").index();
            table.integer("total").stored_as("base_units + bonus_units");
            table.json("payload").nullable();
            table
                .string("locale", 30)
                .virtual_as_json("payload->menu.name->value")
                .nullable();
            table
                .string("label", 50)
                .stored_as_json("payload->fields[0]->display")
                .nullable();
        })
        .await
        .unwrap();
    let columns = schema.get_columns("generated_samples").await.unwrap();
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
        Value::Json(json!({"menu.name":{"value":"zh-Hant"},"fields":[{"display":"customer's"}]}));
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
    assert_eq!(row.get("locale"), Some(&"zh-Hant".into()));
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
            table.integer("offset_total").virtual_as("total + 1");
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
                    .virtual_as("base + 1")
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
                    .stored_as("base_units + bonus_units * 2")
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
            "zh-Hant".into()
        );
        assert!(
            schema
                .get_indexes("generated_samples")
                .await
                .unwrap()
                .iter()
                .any(|index| index.columns == [Some("twice".into())])
        );

        schema
            .table("generated_samples", |table| {
                table
                    .integer("total")
                    .stored_as("base_units + bonus_units * 2")
                    .change();
                table.integer("twice").change();
                table
                    .integer("bonus_units")
                    .stored_as("base_units + 1")
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
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("spatial_sites").await.unwrap();
    schema
        .create("spatial_sites", |table| {
            table.id();
            table
                .geography("wgs_point", Some("POINT"), None)
                .spatial_index();
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
    let columns = schema.get_columns("spatial_sites").await.unwrap();
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
        .get_indexes("spatial_sites")
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
            table.drop_spatial_index("spatial_sites_wgs_point_spatialindex");
            table
                .geometry("extra_shapes", Some("multipoint"), None)
                .nullable();
        })
        .await
        .unwrap();
    assert!(
        !schema
            .has_index("spatial_sites", "spatial_sites_wgs_point_spatialindex")
            .await
            .unwrap()
    );
    assert_eq!(
        schema
            .get_column_type("spatial_sites", "extra_shapes", false)
            .await
            .unwrap(),
        "multipoint"
    );
    schema.drop("spatial_sites").await.unwrap();
}

#[tokio::test]
async fn sqlite_spatial_columns_use_geometry_without_a_spatial_index() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    let mut schema = db.get_schema_builder();
    schema
        .create("spatial_sites", |table| {
            table.geometry("shape", Some("point"), Some(3857));
            table
                .geography("location", Some("polygon"), None)
                .nullable();
        })
        .await
        .unwrap();
    let columns = schema.get_columns("spatial_sites").await.unwrap();
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
                table.geometry("shape", None, None).spatial_index();
            })
            .await,
        Err(DatabaseError::UnsupportedSchema("SQLite index kind"))
    ));
    assert!(!schema.has_table("spatial_index_unsupported").await.unwrap());
    assert!(matches!(
        schema
            .table("spatial_sites", |table| {
                table.geometry("shape", None, None).spatial_index().change();
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
    let mut schema = db.get_schema_builder();
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
    let mut builder = session.get_schema_builder();
    builder
        .create("temp_scope_items", |table| {
            table.temporary();
            table.id();
            table.string("label", 20).index();
        })
        .await
        .unwrap();
    assert!(builder.has_table("temp_scope_items").await.unwrap());
    if !mysql {
        assert!(
            builder
                .has_column("temp_scope_items", "label")
                .await
                .unwrap()
        );
        assert!(
            builder
                .has_index("temp_scope_items", "temp_temp_scope_items_label_index")
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
    let mut builder = session.get_schema_builder();
    builder
        .rename("temp_scope_items", "temp_scope_renamed")
        .await
        .unwrap();
    assert!(builder.has_table("temp_scope_renamed").await.unwrap());
    assert!(builder.has_table("temp_scope_items").await.unwrap());
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
    let mut builder = session.get_schema_builder();
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
        .get_schema_builder()
        .create("temp_scope_items", |table| {
            table.temporary();
            table.id();
        })
        .await
        .unwrap();
    session
        .get_schema_builder()
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
        let schema = owned.get_schema_builder();
        let mut session = schema.session().await.unwrap();
        session
            .get_schema_builder()
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
    let mut schema = db.get_schema_builder();
    schema
        .drop_if_exists("inherited_table_options")
        .await
        .unwrap();
    schema
        .drop_if_exists("explicit_table_options")
        .await
        .unwrap();
    schema
        .create("inherited_table_options", |table| {
            table.string("name", 30);
        })
        .await
        .unwrap();
    schema
        .create("explicit_table_options", |table| {
            table.inno_db();
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
    let tables = schema.get_tables(None).await.unwrap();
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
        schema.get_columns("inherited_table_options").await.unwrap()[0]
            .collation
            .as_deref(),
        Some("utf8mb4_bin")
    );
    assert_eq!(
        schema.get_columns("explicit_table_options").await.unwrap()[0]
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
            .get_tables(None)
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
    assert!(!schema.has_table("invalid_table_options").await.unwrap());
    schema.drop("explicit_table_options").await.unwrap();
    schema.drop("inherited_table_options").await.unwrap();
}

#[tokio::test]
async fn sqlite_uses_column_collation_through_rebuild() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    let mut schema = db.get_schema_builder();
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
        schema.get_columns("local_table_options").await.unwrap()[0]
            .collation
            .as_deref(),
        Some("nocase")
    );
    let tables = schema.get_tables(None).await.unwrap();
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
        schema.get_columns("local_table_options").await.unwrap()[0]
            .collation
            .as_deref(),
        Some("binary")
    );
}

#[tokio::test]
async fn sqlite_rebuild_preserves_inline_constraints_and_can_drop_foreign_keys() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    db.unprepared(
        "CREATE TABLE inline_parents (id INTEGER PRIMARY KEY);
         INSERT INTO inline_parents VALUES (1), (2);
         CREATE TABLE inline_children (
           id INTEGER CONSTRAINT \"inline primary\" PRIMARY KEY ON CONFLICT FAIL AUTOINCREMENT,
           owner_id INTEGER CONSTRAINT \"owner link\" REFERENCES inline_parents(id)
              ON UPDATE CASCADE ON DELETE SET NULL DEFERRABLE INITIALLY DEFERRED
              CONSTRAINT \"owner check\" CHECK (owner_id > 0),
           second_id INTEGER REFERENCES inline_parents(id) ON DELETE RESTRICT NOT NULL,
           label TEXT CONSTRAINT \"unique label\" UNIQUE ON CONFLICT IGNORE
              CONSTRAINT \"valid label\" CHECK (length(trim(label)) > 0)
         );
         INSERT INTO inline_children (owner_id, second_id, label) VALUES (1, 2, 'first'), (1, 2, 'second');
         DELETE FROM inline_children WHERE id = 2",
    ).await.unwrap();
    let mut schema = db.get_schema_builder();
    schema
        .table("inline_children", |table| {
            table.bigInteger("id").change();
            table.integer("owner_id").nullable().change();
            table.string("label", 60).change();
        })
        .await
        .unwrap();
    let keys = schema.get_foreign_keys("inline_children").await.unwrap();
    assert!(keys.iter().any(|key| key.columns == ["owner_id"]
        && key.on_delete.eq_ignore_ascii_case("set null")
        && key.on_update.eq_ignore_ascii_case("cascade")));
    let columns = schema.get_columns("inline_children").await.unwrap();
    assert!(
        columns
            .iter()
            .any(|column| column.name == "id" && column.primary && column.auto_increment)
    );
    assert_eq!(
        db.scalar(
            "SELECT seq FROM sqlite_sequence WHERE name = 'inline_children'",
            &[]
        )
        .await
        .unwrap(),
        2.into()
    );
    assert!(
        db.insert(
            "INSERT INTO inline_children (owner_id, second_id, label) VALUES (1, 2, '   ')",
            &[]
        )
        .await
        .is_err()
    );
    db.insert(
        "INSERT INTO inline_children (owner_id, second_id, label) VALUES (1, 2, 'first')",
        &[],
    )
    .await
    .unwrap();
    assert_eq!(db.table("inline_children").count("*").await.unwrap(), 1);
    db.delete("DELETE FROM inline_parents WHERE id = 1", &[])
        .await
        .unwrap();
    assert_eq!(
        db.table("inline_children").value("owner_id").await.unwrap(),
        Value::Null
    );

    schema
        .table("inline_children", |table| {
            table.drop_foreign("OWNER LINK");
            table.drop_foreign_columns(&["second_id"]);
        })
        .await
        .unwrap();
    assert!(
        schema
            .get_foreign_keys("inline_children")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        db.insert(
            "INSERT INTO inline_children (owner_id, second_id, label) VALUES (-1, 99, 'new')",
            &[]
        )
        .await
        .is_err()
    );
    assert!(
        db.insert(
            "INSERT INTO inline_children (owner_id, label) VALUES (99, 'new')",
            &[]
        )
        .await
        .is_err()
    );
    db.insert(
        "INSERT INTO inline_children (owner_id, second_id, label) VALUES (99, 99, 'valid')",
        &[],
    )
    .await
    .unwrap();
    assert!(
        schema
            .table("inline_children", |table| {
                table.drop_foreign("missing");
            })
            .await
            .is_err()
    );
    assert_eq!(db.table("inline_children").count("*").await.unwrap(), 2);
}

#[tokio::test]
async fn sqlite_can_drop_an_inline_primary_key_without_retaining_a_sequence() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    db.unprepared("CREATE TABLE inline_counter (id INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0)); INSERT INTO inline_counter VALUES (17)").await.unwrap();
    let mut schema = db.get_schema_builder();
    schema
        .table("inline_counter", |table| table.drop_primary())
        .await
        .unwrap();
    assert!(!schema.get_columns("inline_counter").await.unwrap()[0].primary);
    assert_eq!(
        db.scalar(
            "SELECT seq FROM sqlite_sequence WHERE name = 'inline_counter'",
            &[]
        )
        .await
        .unwrap(),
        Value::Null
    );
    db.insert("INSERT INTO inline_counter VALUES (17)", &[])
        .await
        .unwrap();
    assert!(
        db.insert("INSERT INTO inline_counter VALUES (-1)", &[])
            .await
            .is_err()
    );
    assert_eq!(db.table("inline_counter").count("*").await.unwrap(), 2);
}

#[tokio::test]
async fn mysql_adds_auto_increment_and_compound_primary_key_to_existing_rows() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url": url, "prefix": "it_"}));
    let mut schema = db.get_schema_builder();
    let unkeyed = "add_auto_unkeyed";
    let keyed = "add_auto_keyed";
    for name in [unkeyed, keyed] {
        schema.drop_if_exists(name).await.unwrap();
    }

    schema
        .create(unkeyed, |table| {
            table.integer("tenant_id");
            table.string("payload", 30);
        })
        .await
        .unwrap();
    db.insert(
        "insert into it_add_auto_unkeyed (tenant_id, payload) values (1, 'first'), (1, 'second')",
        &[],
    )
    .await
    .unwrap();
    schema
        .table(unkeyed, |table| {
            table.big_increments("id").first().starting_value(42);
            table.primary(&["id", "tenant_id"], None).lock("shared");
        })
        .await
        .unwrap();
    let indexes = schema.get_indexes(unkeyed).await.unwrap();
    assert_eq!(indexes.len(), 1);
    assert!(indexes[0].primary);
    assert_eq!(
        indexes[0].columns,
        [Some("id".into()), Some("tenant_id".into())]
    );
    assert_eq!(
        schema.get_column_listing(unkeyed).await.unwrap(),
        ["id", "tenant_id", "payload"]
    );
    assert_eq!(db.table(unkeyed).count("*").await.unwrap(), 2);
    assert_eq!(
        db.table(unkeyed)
            .insert_get_id(BTreeMap::from([
                ("tenant_id".into(), 2.into()),
                ("payload".into(), "third".into())
            ]))
            .await
            .unwrap(),
        42.into()
    );

    schema
        .create(keyed, |table| {
            table.integer("legacy_id").primary();
        })
        .await
        .unwrap();
    db.insert(
        "insert into it_add_auto_keyed (legacy_id) values (1), (2)",
        &[],
    )
    .await
    .unwrap();

    let invalid = schema
        .table(keyed, |table| {
            table.drop_primary();
            table.big_increments("id");
            table.primary(&["legacy_id", "id"], None);
        })
        .await;
    assert!(invalid.is_err());
    assert!(!schema.has_column(keyed, "id").await.unwrap());
    assert_eq!(
        schema.get_indexes(keyed).await.unwrap()[0].columns,
        [Some("legacy_id".into())]
    );
    assert_eq!(db.table(keyed).count("*").await.unwrap(), 2);

    let conflicting = schema
        .table(keyed, |table| {
            table.drop_primary();
            table.big_increments("id").lock("shared");
            table.primary(&["id", "legacy_id"], None).lock("exclusive");
        })
        .await;
    assert!(matches!(
        conflicting,
        Err(DatabaseError::InvalidSchema(
            "conflicting MySQL ALTER lock modes"
        ))
    ));
    assert!(!schema.has_column(keyed, "id").await.unwrap());

    schema
        .table(keyed, |table| {
            table.drop_primary();
            table.big_increments("id").lock("SHARED");
            table
                .primary(&["id", "legacy_id"], None)
                .inplace()
                .lock("shared");
        })
        .await
        .unwrap();
    let indexes = schema.get_indexes(keyed).await.unwrap();
    assert_eq!(indexes.len(), 1);
    assert_eq!(
        indexes[0].columns,
        [Some("id".into()), Some("legacy_id".into())]
    );
    let id = schema.get_column(keyed, "id").await.unwrap().unwrap();
    assert!(id.auto_increment && id.primary);
    assert_eq!(db.table(keyed).count("*").await.unwrap(), 2);
    for name in [unkeyed, keyed] {
        schema.drop(name).await.unwrap();
    }
}

#[tokio::test]
async fn mysql_replaces_primary_keys_in_one_alter_statement() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url, "prefix": "it_"}));
    let mut schema = db.get_schema_builder();
    let serial = "replace_primary_serial";
    let repeated = "replace_primary_duplicate";
    for name in [serial, repeated] {
        schema.drop_if_exists(name).await.unwrap();
    }
    schema
        .create(serial, |table| {
            table.id().starting_value(24);
            table.integer("tenant_id");
        })
        .await
        .unwrap();
    assert_eq!(
        db.table(serial)
            .insert_get_id(BTreeMap::from([("tenant_id".into(), 1.into())]))
            .await
            .unwrap(),
        24.into()
    );
    schema
        .table(serial, |table| {
            table.drop_primary();
            table
                .primary(&["id", "tenant_id"], None)
                .inplace()
                .lock("NONE");
        })
        .await
        .unwrap();
    let indexes = schema.get_indexes(serial).await.unwrap();
    assert_eq!(indexes.len(), 1);
    assert!(indexes[0].primary);
    assert_eq!(
        indexes[0].columns,
        [Some("id".into()), Some("tenant_id".into())]
    );
    assert!(
        schema
            .get_column(serial, "id")
            .await
            .unwrap()
            .unwrap()
            .auto_increment
    );
    assert_eq!(
        db.table(serial)
            .insert_get_id(BTreeMap::from([("tenant_id".into(), 2.into())]))
            .await
            .unwrap(),
        25.into()
    );

    schema
        .create(repeated, |table| {
            table.integer("id").primary();
            table.integer("repeated");
        })
        .await
        .unwrap();
    db.insert(
        "insert into it_replace_primary_duplicate (id, repeated) values (1, 7), (2, 7)",
        &[],
    )
    .await
    .unwrap();
    assert!(
        schema
            .table(repeated, |table| {
                table.drop_primary();
                table.primary(&["repeated"], None);
            })
            .await
            .is_err()
    );
    let indexes = schema.get_indexes(repeated).await.unwrap();
    assert_eq!(indexes.len(), 1);
    assert!(indexes[0].primary);
    assert_eq!(indexes[0].columns, [Some("id".into())]);
    assert_eq!(db.table(repeated).count("*").await.unwrap(), 2);
    schema
        .table(repeated, |table| table.drop_primary())
        .await
        .unwrap();
    assert!(schema.get_indexes(repeated).await.unwrap().is_empty());
    for name in [serial, repeated] {
        schema.drop(name).await.unwrap();
    }
}

#[tokio::test]
async fn mysql_changes_columns_with_fluent_index_changes_and_foreign_keys() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url": url, "prefix": "it_", "prefix_indexes": true}));
    let mut schema = db.get_schema_builder();
    let parent = "fluent_index_parents";
    let child = "fluent_index_children";
    for name in [child, parent] {
        schema.drop_if_exists(name).await.unwrap();
    }
    schema
        .create(parent, |table| {
            table.id();
        })
        .await
        .unwrap();
    schema
        .create(child, |table| {
            table.id();
            table.unsigned_big_integer("parent_id");
            table.index(&["parent_id"], None);
            table
                .foreign(&["parent_id"], Some("fluent_parent"))
                .references("id")
                .on(parent);
            table.string("postal_code", 10).unique();
            table.text("body").full_text_with("fluent_body_search");
        })
        .await
        .unwrap();
    db.insert("insert into it_fluent_index_parents (id) values (1)", &[])
        .await
        .unwrap();
    db.insert("insert into it_fluent_index_children (parent_id, postal_code, body) values (1, 'AA', 'searchable')", &[]).await.unwrap();

    schema
        .table(child, |table| {
            table.string("postal_code", 20).change();
        })
        .await
        .unwrap();
    assert!(
        schema
            .has_index(child, "it_fluent_index_children_postal_code_unique")
            .await
            .unwrap()
    );
    assert_eq!(schema.get_foreign_keys(child).await.unwrap().len(), 1);
    assert!(
        db.insert("insert into it_fluent_index_children (parent_id, postal_code, body) values (9, 'BB', 'invalid parent')", &[]).await.is_err()
    );

    schema
        .table(child, |table| {
            table
                .string("postal_code", 30)
                .unique_with(false)
                .index_with("fluent_region_lookup")
                .rename_to("region_code")
                .change();
        })
        .await
        .unwrap();
    assert!(
        !schema
            .has_index(child, "it_fluent_index_children_postal_code_unique")
            .await
            .unwrap()
    );
    let indexes = schema.get_indexes(child).await.unwrap();
    assert!(
        indexes
            .iter()
            .any(|index| index.name == "fluent_region_lookup"
                && index.columns == [Some("region_code".into())]
                && !index.unique)
    );
    assert!(schema.has_index(child, "fluent_body_search").await.unwrap());
    assert_eq!(
        db.table(child).value("region_code").await.unwrap(),
        "AA".into()
    );
    db.insert("insert into it_fluent_index_children (parent_id, region_code, body) values (1, 'AA', 'duplicate allowed')", &[]).await.unwrap();

    schema
        .table(child, |table| {
            table
                .unsigned_big_integer("parent_id")
                .index_with(false)
                .change();
            table.drop_foreign("fluent_parent");
        })
        .await
        .unwrap();
    assert!(
        !schema
            .has_index(child, "it_fluent_index_children_parent_id_index")
            .await
            .unwrap()
    );
    assert!(schema.get_foreign_keys(child).await.unwrap().is_empty());
    assert_eq!(db.table(child).count("*").await.unwrap(), 2);
    for name in [child, parent] {
        schema.drop(name).await.unwrap();
    }
}

#[tokio::test]
async fn mysql_generated_columns_keep_explicit_and_default_nullability() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url": url}));
    let mut schema = db.get_schema_builder();
    let name = "generated_nullability";
    schema.drop_if_exists(name).await.unwrap();
    schema
        .create(name, |table| {
            table.id();
            table.integer("base").nullable();
            table.json("payload").nullable();
            table.integer("twice").virtual_as("base * 2");
            table.integer("total").stored_as("base + 10").nullable();
            table.string("label", 24).virtual_as_json("payload->label");
            table
                .string("stored_label", 24)
                .stored_as_json("payload->label");
            table
                .integer("required")
                .stored_as("COALESCE(base, 0)")
                .nullable()
                .nullable_with(false);
        })
        .await
        .unwrap();

    for column in ["twice", "total", "label", "stored_label"] {
        assert!(
            schema
                .get_column(name, column)
                .await
                .unwrap()
                .unwrap()
                .nullable
        );
    }
    assert!(
        !schema
            .get_column(name, "required")
            .await
            .unwrap()
            .unwrap()
            .nullable
    );
    db.insert(
        "insert into generated_nullability (base) values (NULL)",
        &[],
    )
    .await
    .unwrap();
    let row = db.table(name).first().await.unwrap().unwrap();
    for column in ["twice", "total", "label", "stored_label"] {
        assert_eq!(row.get(column), Some(&Value::Null));
    }
    assert_eq!(row.get("required"), Some(&0.into()));

    db.update("update generated_nullability set base = 4", &[])
        .await
        .unwrap();
    schema
        .table(name, |table| {
            table
                .integer("twice")
                .virtual_as("base * 2")
                .nullable_with(false)
                .change();
        })
        .await
        .unwrap();
    assert!(
        !schema
            .get_column(name, "twice")
            .await
            .unwrap()
            .unwrap()
            .nullable
    );
    assert_eq!(db.table(name).value("twice").await.unwrap(), 8.into());
    assert!(
        db.update("update generated_nullability set base = NULL", &[])
            .await
            .is_err()
    );
    schema.drop(name).await.unwrap();
}

#[tokio::test]
async fn mysql_current_defaults_override_explicit_values_only_for_supported_types() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url": url}));
    let mut schema = db.get_schema_builder();
    let name = "temporal_current_defaults";
    schema.drop_if_exists(name).await.unwrap();
    schema
        .create(name, |table| {
            table.id();
            table.date("today").use_current().default("2000-01-01");
            table.year("this_year").default(2000).use_current();
            table
                .date_time("started_at", 3)
                .default("2000-01-01 00:00:00")
                .use_current();
            table
                .timestamp("touched_at", 6)
                .use_current()
                .default("2000-01-01 00:00:00");
            table
                .time("start_time", 3)
                .default("07:08:09.123")
                .use_current();
            table.integer("counted").use_current().default(7);
            table
                .integer("generated")
                .virtual_as("counted + 1")
                .use_current();
        })
        .await
        .unwrap();
    db.insert("insert into temporal_current_defaults () values ()", &[])
        .await
        .unwrap();
    let query = || {
        db.select_one(
        "select count(*) as matched from temporal_current_defaults where today = curdate() and this_year = year(curdate()) and abs(timestampdiff(second, started_at, now())) < 60 and abs(timestampdiff(second, touched_at, now())) < 60 and start_time = '07:08:09.123' and counted = 7 and `generated` = 8",
        &[],
    )
    };
    assert_eq!(
        query().await.unwrap().unwrap().get("matched"),
        Some(&1.into())
    );

    schema
        .table(name, |table| {
            table
                .date_time("started_at", 3)
                .use_current()
                .default("2000-01-01 00:00:00")
                .change();
            table
                .time("start_time", 3)
                .use_current()
                .default("07:08:09.123")
                .change();
        })
        .await
        .unwrap();
    db.insert("insert into temporal_current_defaults () values ()", &[])
        .await
        .unwrap();
    assert_eq!(
        query().await.unwrap().unwrap().get("matched"),
        Some(&2.into())
    );
    schema.drop(name).await.unwrap();
}

#[tokio::test]
async fn mysql_preserves_zero_and_boundary_column_lengths() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url": url}));
    let mut schema = db.get_schema_builder();
    let name = "column_width_edges";
    schema.drop_if_exists(name).await.unwrap();
    schema
        .create(name, |table| {
            table.id();
            table.char("optional_flag", 0).nullable();
            table.char("fixed_label", 255);
            table.binary_with("payload", 0, true);
            table.binary_with("token", 255, true).nullable();
            table.binary_with("variable", 511, false).nullable();
        })
        .await
        .unwrap();

    for (column, expected) in [
        ("optional_flag", "char(0)"),
        ("fixed_label", "char(255)"),
        ("payload", "blob"),
        ("token", "binary(255)"),
        ("variable", "varbinary(511)"),
    ] {
        assert_eq!(
            schema.get_column_type(name, column, true).await.unwrap(),
            expected
        );
    }
    let payload = vec![0, 1, 255, 0];
    db.insert("insert into column_width_edges (optional_flag, fixed_label, payload) values ('', 'valid', ?)", &[Value::Bytes(payload.clone())])
        .await.unwrap();
    assert_eq!(
        db.table(name).value("payload").await.unwrap(),
        Value::Bytes(payload)
    );

    let result = schema
        .table(name, |table| {
            table.integer("not_added");
            table.char("too_wide", 256);
        })
        .await;
    assert!(matches!(
        result,
        Err(DatabaseError::InvalidSchema("column length"))
    ));
    assert!(!schema.has_column(name, "not_added").await.unwrap());
    schema.drop(name).await.unwrap();
}

#[tokio::test]
async fn mysql_compiles_explicit_temporal_on_update_expressions() {
    use openrust::database::query::Expression;

    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url": url}));
    let mut schema = db.get_schema_builder();
    let name = "temporal_update_expression";
    let initial = "2020-01-01 00:00:00";
    schema.drop_if_exists(name).await.unwrap();
    schema
        .create(name, |table| {
            table.id();
            table.integer("status");
            table
                .date_time("handled_at", 3)
                .default(initial)
                .on_update(Expression::new("NOW(3)"));
            table
                .timestamp("changed_at", 6)
                .default(initial)
                .use_current_on_update()
                .on_update(Expression::new("unused_expression"));
        })
        .await
        .unwrap();

    db.insert(
        "insert into temporal_update_expression (status) values (0)",
        &[],
    )
    .await
    .unwrap();
    let timestamps = "select date_format(handled_at, '%Y-%m-%d %H:%i:%s.%f') as handled_at, date_format(changed_at, '%Y-%m-%d %H:%i:%s.%f') as changed_at from temporal_update_expression";
    let row = db.select_one(timestamps, &[]).await.unwrap().unwrap();
    for column in ["handled_at", "changed_at"] {
        assert_eq!(row.get(column), Some(&format!("{initial}.000000").into()));
    }
    db.update("update temporal_update_expression set status = 1", &[])
        .await
        .unwrap();
    let row = db.select_one(timestamps, &[]).await.unwrap().unwrap();
    for column in ["handled_at", "changed_at"] {
        assert!(
            matches!(row.get(column), Some(Value::Text(value)) if value.len() == 26 && !value.starts_with(initial))
        );
    }

    schema
        .table(name, |table| {
            table
                .date_time("handled_at", 6)
                .default(initial)
                .on_update(Expression::new("CURRENT_TIMESTAMP(6)"))
                .change();
        })
        .await
        .unwrap();
    assert_eq!(
        schema
            .get_column_type(name, "handled_at", true)
            .await
            .unwrap(),
        "datetime(6)"
    );

    let result = schema
        .table(name, |table| {
            table.integer("not_added");
            table
                .timestamp("bound", 3)
                .on_update(Expression::new("NOW(?)").with_bindings(vec![3.into()]));
        })
        .await;
    assert!(matches!(
        result,
        Err(DatabaseError::InvalidSchema("column on-update expression"))
    ));
    assert!(!schema.has_column(name, "not_added").await.unwrap());
    schema.drop(name).await.unwrap();
}

#[tokio::test]
async fn mysql_preserves_explicit_primary_and_integer_signedness_with_auto_increment() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url}));
    let mut schema = db.get_schema_builder();
    let name = "composite_increment";
    let alias = "integer_increment_alias";
    for table in [name, alias] {
        schema.drop_if_exists(table).await.unwrap();
    }

    schema
        .create(name, |table| {
            table.integer("id").auto_increment().starting_value(500);
            table.integer("tenant_id");
            table.primary(&["id", "tenant_id"], None);
        })
        .await
        .unwrap();
    let primary = schema.get_indexes(name).await.unwrap();
    assert_eq!(primary.len(), 1);
    assert!(primary[0].primary);
    assert_eq!(
        primary[0].columns,
        [Some("id".into()), Some("tenant_id".into())]
    );
    let id = schema.get_column(name, "id").await.unwrap().unwrap();
    assert_eq!(id.full_type, "int");
    assert!(id.auto_increment);
    db.insert(
        "insert into composite_increment (id, tenant_id) values (-1, 1)",
        &[],
    )
    .await
    .unwrap();
    assert_eq!(
        db.table(name)
            .insert_get_id(BTreeMap::from([("tenant_id".into(), 1.into())]))
            .await
            .unwrap(),
        500.into()
    );

    schema
        .table(name, |table| {
            table.bigInteger("id").auto_increment().change();
        })
        .await
        .unwrap();
    assert_eq!(
        schema.get_column_type(name, "id", true).await.unwrap(),
        "bigint"
    );
    assert_eq!(
        schema.get_indexes(name).await.unwrap()[0].columns,
        [Some("id".into()), Some("tenant_id".into())]
    );
    assert_eq!(
        db.table(name)
            .insert_get_id(BTreeMap::from([("tenant_id".into(), 1.into())]))
            .await
            .unwrap(),
        501.into()
    );

    schema
        .create(alias, |table| {
            table.integer_increments("serial");
        })
        .await
        .unwrap();
    let column = schema.get_column(alias, "serial").await.unwrap().unwrap();
    assert_eq!(column.full_type, "int unsigned");
    assert!(column.primary && column.auto_increment);
    for table in [name, alias] {
        schema.drop_if_exists(table).await.unwrap();
    }
}

#[tokio::test]
async fn mysql_applies_instant_column_changes_locks_and_starting_values() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url}));
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("online_column_ddl").await.unwrap();
    schema
        .create("online_column_ddl", |table| {
            table.id().from(500);
            table.integer("rank");
        })
        .await
        .unwrap();

    assert_eq!(
        db.table("online_column_ddl")
            .insert_get_id(BTreeMap::from([("rank".into(), 1.into())]))
            .await
            .unwrap(),
        500.into()
    );
    schema
        .table("online_column_ddl", |table| {
            table
                .string("label", 80)
                .nullable()
                .instant()
                .lock("DEFAULT");
            table.integer("rank").default(7).change().instant();
            table.string("note", 80).nullable().lock("NONE");
        })
        .await
        .unwrap();
    assert_eq!(
        schema
            .get_column_listing("online_column_ddl")
            .await
            .unwrap(),
        ["id", "rank", "label", "note"]
    );

    assert!(
        schema
            .table("online_column_ddl", |table| {
                table.string("rank", 80).change().instant();
            })
            .await
            .is_err()
    );
    assert!(
        schema
            .table("online_column_ddl", |table| {
                table.integer("not_added");
                table.integer("invalid_lock").lock("unknown");
            })
            .await
            .is_err()
    );
    assert!(
        !schema
            .has_column("online_column_ddl", "not_added")
            .await
            .unwrap()
    );

    schema
        .table("online_column_ddl", |table| {
            table.id().starting_value(900).change();
        })
        .await
        .unwrap();
    assert_eq!(
        db.table("online_column_ddl")
            .insert_get_id(BTreeMap::from([("label".into(), "online".into())]))
            .await
            .unwrap(),
        900.into()
    );
    assert_eq!(
        db.table("online_column_ddl")
            .r#where("id", "=", 500)
            .value("rank")
            .await
            .unwrap(),
        1.into()
    );
    assert_eq!(
        db.table("online_column_ddl")
            .r#where("id", "=", 900)
            .value("rank")
            .await
            .unwrap(),
        7.into()
    );
    schema.drop("online_column_ddl").await.unwrap();
}

#[tokio::test]
async fn sqlite_keeps_native_autoincrement_and_rejects_mysql_online_ddl() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    let mut schema = db.get_schema_builder();
    schema
        .create("online_column_ddl", |table| {
            table.id().from(500);
            table.integer("rank");
        })
        .await
        .unwrap();
    assert_eq!(
        db.table("online_column_ddl")
            .insert_get_id(BTreeMap::from([("rank".into(), 1.into())]))
            .await
            .unwrap(),
        1.into()
    );
    for result in [
        schema
            .table("online_column_ddl", |table| {
                table.integer("not_added");
                table.integer("instant").nullable().instant();
            })
            .await,
        schema
            .table("online_column_ddl", |table| {
                table.integer("rank").nullable().change().lock("none");
            })
            .await,
    ] {
        assert!(matches!(result, Err(DatabaseError::UnsupportedSchema(_))));
    }
    assert_eq!(
        schema
            .get_column_listing("online_column_ddl")
            .await
            .unwrap(),
        ["id", "rank"]
    );
    assert!(!schema.get_columns("online_column_ddl").await.unwrap()[1].nullable);
}

async fn verify_default_expressions(db: Connection) {
    use openrust::{database::query::Expression, support::facades::Db};

    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("expression_defaults").await.unwrap();
    schema
        .create("expression_defaults", |table| {
            table.id();
            table.json("movies").default(Db::raw("(JSON_ARRAY())"));
            table.string("label", 80).default("(40 + 2)");
            table.integer("points").default(Expression::new("(40 + 2)"));
            table.foreign_id("owner_id").default(Db::raw("(1 + 1)"));
        })
        .await
        .unwrap();
    db.insert("insert into expression_defaults (id) values (1)", &[])
        .await
        .unwrap();
    let first = db
        .select_one("select * from expression_defaults where id = 1", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(match first.get("movies") {
        Some(Value::Json(value)) => value == &json!([]),
        Some(Value::Text(value)) => value == "[]",
        _ => false,
    });
    assert_eq!(first.get("label"), Some(&"(40 + 2)".into()));
    assert_eq!(first.get("points"), Some(&42.into()));
    assert!(matches!(
        first.get("owner_id"),
        Some(Value::Integer(2) | Value::Unsigned(2))
    ));

    schema
        .table("expression_defaults", |table| {
            table.integer("points").default(Db::raw("(6 * 9)")).change();
        })
        .await
        .unwrap();
    db.insert("insert into expression_defaults (id) values (2)", &[])
        .await
        .unwrap();
    let rows = db
        .select("select points from expression_defaults order by id", &[])
        .await
        .unwrap();
    assert_eq!(rows[0].get("points"), Some(&42.into()));
    assert_eq!(rows[1].get("points"), Some(&54.into()));
    assert!(
        schema
            .get_columns("expression_defaults")
            .await
            .unwrap()
            .iter()
            .find(|column| column.name == "points")
            .and_then(|column| column.default.as_deref())
            .is_some_and(|default| default.contains('*'))
    );

    for invalid in [
        Db::raw("(?)").with_bindings(vec![1.into()]),
        Db::raw(" "),
        Db::raw("(1\0)"),
    ] {
        assert!(matches!(
            schema
                .table("expression_defaults", |table| {
                    table.integer("not_added");
                    table.integer("invalid").default(invalid);
                })
                .await,
            Err(DatabaseError::InvalidSchema("column default expression"))
        ));
        assert!(
            !schema
                .has_column("expression_defaults", "not_added")
                .await
                .unwrap()
        );
    }
    schema.drop("expression_defaults").await.unwrap();
}

#[tokio::test]
async fn mysql_evaluates_default_expressions_in_create_and_change() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    verify_default_expressions(connection(json!({"url":url}))).await;
}

#[tokio::test]
async fn sqlite_evaluates_default_expressions_after_rebuilding_the_table() {
    verify_default_expressions(connection(json!({"driver":"sqlite","database":":memory:"}))).await;
}

async fn verify_column_groups(db: Connection, mysql: bool) {
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("grouped_columns").await.unwrap();
    schema.drop_if_exists("group_owners").await.unwrap();
    schema
        .create("group_owners", |table| {
            table.id();
        })
        .await
        .unwrap();
    schema
        .create("grouped_columns", |table| {
            table.id();
            table.string("title", 80);
            table.nullable_morphs("owner", None, None);
            table.nullable_uuid_morphs("image", Some("grouped_image_lookup"), None);
            table.ulid_morphs("parent", None, None);
            table
                .foreign_id("reviewer_id")
                .nullable()
                .constrained_on("group_owners", "id", None);
            table.remember_token();
            table.nullable_timestamps_tz();
            table.soft_deletes_tz();
        })
        .await
        .unwrap();
    let columns = schema.get_columns("grouped_columns").await.unwrap();
    for name in [
        "owner_id",
        "owner_type",
        "image_id",
        "image_type",
        "remember_token",
        "created_at",
        "updated_at",
        "deleted_at",
    ] {
        assert!(
            columns
                .iter()
                .any(|column| column.name == name && column.nullable),
            "{name}"
        );
    }
    for (name, mysql_type) in [
        ("image_id", "char(36)"),
        ("parent_id", "char(26)"),
        ("remember_token", "varchar(100)"),
    ] {
        let expected = if mysql { mysql_type } else { "varchar" };
        assert!(
            columns.iter().any(
                |column| column.name == name && column.full_type.eq_ignore_ascii_case(expected)
            ),
            "{name}"
        );
    }
    assert!(
        columns
            .iter()
            .any(|column| column.name == "parent_id" && !column.nullable)
    );
    assert!(
        schema
            .has_index("grouped_columns", "grouped_image_lookup")
            .await
            .unwrap()
    );
    assert!(
        schema
            .has_index(
                "grouped_columns",
                "app_grouped_columns_owner_type_owner_id_index"
            )
            .await
            .unwrap()
    );
    assert!(
        !schema
            .get_foreign_keys("grouped_columns")
            .await
            .unwrap()
            .is_empty()
    );
    db.insert(
        "insert into app_grouped_columns (title, parent_type, parent_id) values (?, ?, ?)",
        &[
            "saved".into(),
            "post".into(),
            "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        ],
    )
    .await
    .unwrap();

    if mysql {
        schema
            .table("grouped_columns", |table| {
                table.after("title", |table| {
                    table.string("first_extra", 40).nullable();
                    table.string("second_extra", 40).nullable();
                });
                table.nullable_numeric_morphs("contact", None, Some("second_extra"));
                table.integer("at_end").nullable();
            })
            .await
            .unwrap();
        let listing = schema.get_column_listing("grouped_columns").await.unwrap();
        assert_eq!(
            &listing[..6],
            [
                "id",
                "title",
                "first_extra",
                "second_extra",
                "contact_type",
                "contact_id"
            ]
        );
        assert_eq!(listing.last().map(String::as_str), Some("at_end"));
    } else {
        schema
            .table("grouped_columns", |table| {
                table.after("title", |table| {
                    table.string("first_extra", 40).nullable();
                });
            })
            .await
            .unwrap();
        let listing = schema.get_column_listing("grouped_columns").await.unwrap();
        assert_eq!(listing.last().map(String::as_str), Some("first_extra"));
    }

    schema
        .table("grouped_columns", |table| {
            table.drop_morphs("owner", None);
            table.drop_morphs("image", Some("grouped_image_lookup"));
            table.drop_constrained_foreign_id("reviewer_id");
            table.drop_remember_token();
            table.drop_timestamps_tz();
            table.drop_soft_deletes_tz(None);
        })
        .await
        .unwrap();
    let listing = schema.get_column_listing("grouped_columns").await.unwrap();
    for removed in [
        "owner_type",
        "owner_id",
        "image_type",
        "image_id",
        "reviewer_id",
        "remember_token",
        "created_at",
        "updated_at",
        "deleted_at",
    ] {
        assert!(!listing.iter().any(|column| column == removed), "{removed}");
    }
    assert!(
        !schema
            .has_index("grouped_columns", "grouped_image_lookup")
            .await
            .unwrap()
    );
    assert!(
        !schema
            .has_index(
                "grouped_columns",
                "app_grouped_columns_owner_type_owner_id_index"
            )
            .await
            .unwrap()
    );
    assert!(
        schema
            .get_foreign_keys("grouped_columns")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.table("grouped_columns").value("title").await.unwrap(),
        "saved".into()
    );
    schema.drop("grouped_columns").await.unwrap();
    schema.drop("group_owners").await.unwrap();
}

#[tokio::test]
async fn mysql_compiles_column_groups_and_migration_aliases() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    verify_column_groups(
        connection(json!({"url":url,"prefix":"app_","prefix_indexes":true})),
        true,
    )
    .await;
}

#[tokio::test]
async fn sqlite_drops_column_groups_indexes_and_foreign_constraints() {
    verify_column_groups(
        connection(
            json!({"driver":"sqlite","database":":memory:","prefix":"app_","prefix_indexes":true}),
        ),
        false,
    )
    .await;
}

mod model_foreign_keys {
    use super::*;
    use openrust::database::eloquent::{Model, ModelMetadata, ModelState, UniqueId};

    macro_rules! schema_model {
        ($model:ident, $metadata:expr) => {
            struct $model(ModelState);

            impl Model for $model {
                fn metadata() -> &'static ModelMetadata {
                    static METADATA: ModelMetadata = $metadata;
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
        };
    }

    schema_model!(
        AuditOwner,
        ModelMetadata::new("team_people")
            .primary_key("owner_no")
            .unique_id(UniqueId::Uuid)
            .unique_ids(&["external_code"])
    );
    schema_model!(
        AuditPhoto,
        ModelMetadata::new("photo_assets")
            .primary_key("photo_code")
            .unique_id(UniqueId::Uuid)
    );
    schema_model!(
        AuditAlbum,
        ModelMetadata::new("media_albums")
            .primary_key("album_code")
            .unique_id(UniqueId::Ulid)
    );

    async fn verify(db: Connection) {
        let mut schema = db.get_schema_builder();
        for table in [
            "model_attachments",
            "team_people",
            "photo_assets",
            "media_albums",
        ] {
            schema.drop_if_exists(table).await.unwrap();
        }
        schema
            .create("team_people", |table| {
                table.unsigned_big_integer("owner_no").primary();
            })
            .await
            .unwrap();
        schema
            .create("photo_assets", |table| {
                table.uuid("photo_code").primary();
            })
            .await
            .unwrap();
        schema
            .create("media_albums", |table| {
                table.ulid("album_code").primary();
            })
            .await
            .unwrap();
        schema
            .create("model_attachments", |table| {
                table.id();
                table
                    .foreign_id_for::<AuditOwner>(None)
                    .nullable()
                    .constrained()
                    .null_on_delete();
                table
                    .foreign_id_for::<AuditPhoto>(Some("photo_ref"))
                    .constrained();
                table.foreign_id_for::<AuditAlbum>(None).constrained();
                table
                    .foreign_uuid_for::<AuditPhoto>(Some("previous_photo"))
                    .nullable();
                table
                    .foreign_ulid_for::<AuditAlbum>(Some("previous_album"))
                    .nullable();
                table.foreign_uuid("external_photo").nullable();
                table.foreign_ulid("external_album").nullable();
            })
            .await
            .unwrap();

        let foreign = schema.get_foreign_keys("model_attachments").await.unwrap();
        for (column, table, key) in [
            ("audit_owner_owner_no", "app_team_people", "owner_no"),
            ("photo_ref", "app_photo_assets", "photo_code"),
            ("audit_album_album_code", "app_media_albums", "album_code"),
        ] {
            assert!(
                foreign.iter().any(|f| f.columns == [column]
                    && f.foreign_table == table
                    && f.foreign_columns.as_slice() == [Some(key.to_owned())]),
                "{column}"
            );
        }
        assert_eq!(foreign.len(), 3);

        let photo = "01942ccb-2d32-79fb-b722-b37912f515dc";
        let album = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        db.insert("insert into app_team_people (owner_no) values (5)", &[])
            .await
            .unwrap();
        db.insert(
            "insert into app_photo_assets (photo_code) values (?)",
            &[photo.into()],
        )
        .await
        .unwrap();
        db.insert(
            "insert into app_media_albums (album_code) values (?)",
            &[album.into()],
        )
        .await
        .unwrap();
        let insert = "insert into app_model_attachments (audit_owner_owner_no, photo_ref, audit_album_album_code) values (?, ?, ?)";
        db.insert(insert, &[5.into(), photo.into(), album.into()])
            .await
            .unwrap();
        assert!(
            db.insert(insert, &[6.into(), photo.into(), album.into()])
                .await
                .is_err()
        );
        db.delete("delete from app_team_people where owner_no = 5", &[])
            .await
            .unwrap();
        assert_eq!(
            db.table("model_attachments")
                .value("audit_owner_owner_no")
                .await
                .unwrap(),
            Value::Null
        );

        schema
            .table("model_attachments", |table| {
                table.drop_constrained_foreign_id_for::<AuditOwner>(None);
                table.drop_constrained_foreign_id_for::<AuditPhoto>(Some("photo_ref"));
                table.drop_constrained_foreign_id_for::<AuditAlbum>(None);
                table.drop_foreign_id_for::<AuditPhoto>(Some("previous_photo"));
                table.drop_foreign_id_for::<AuditAlbum>(Some("previous_album"));
            })
            .await
            .unwrap();
        assert!(
            schema
                .get_foreign_keys("model_attachments")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            schema
                .get_column_listing("model_attachments")
                .await
                .unwrap(),
            ["id", "external_photo", "external_album"]
        );
        assert_eq!(db.table("model_attachments").count("*").await.unwrap(), 1);
        for table in [
            "model_attachments",
            "team_people",
            "photo_assets",
            "media_albums",
        ] {
            schema.drop(table).await.unwrap();
        }
    }

    #[tokio::test]
    async fn mysql_references_and_drops_model_foreign_keys() {
        let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
            assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
            return;
        };
        verify(connection(
            json!({"url":url,"prefix":"app_","prefix_indexes":true}),
        ))
        .await;
    }

    #[tokio::test]
    async fn mysql_overrides_foreign_id_constraint_defaults_independently() {
        let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
            assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
            return;
        };
        let db = connection(json!({"url":url,"prefix":"override_"}));
        let mut schema = db.get_schema_builder();
        schema
            .create("workspaces", |table| {
                table.id();
                table.unsigned_big_integer("code").unique();
            })
            .await
            .unwrap();
        schema
            .create("team_people", |table| {
                table.id_with("owner_no");
                table.unsigned_big_integer("legacy_no").unique();
            })
            .await
            .unwrap();
        schema
            .create("reviewers", |table| {
                table.id_with("owner_no");
            })
            .await
            .unwrap();
        schema
            .create("assignments", |table| {
                table.id();
                table
                    .foreign_id("workspace_code")
                    .nullable()
                    .constrained_with(None, Some("code"), Some("assignments_workspace"))
                    .null_on_delete();
                table
                    .foreign_id_for::<AuditOwner>(Some("author_ref"))
                    .constrained_with(None, None, Some("assignments_author"));
                table
                    .foreign_id_for::<AuditOwner>(Some("legacy_ref"))
                    .constrained_with(None, Some("legacy_no"), Some("assignments_legacy"));
                table
                    .foreign_id_for::<AuditOwner>(Some("reviewer_ref"))
                    .constrained_with(Some("reviewers"), None, Some("assignments_reviewer"));
            })
            .await
            .unwrap();

        let foreign = schema.get_foreign_keys("assignments").await.unwrap();
        assert_eq!(foreign.len(), 4);
        for (name, column, table, key) in [
            (
                "assignments_workspace",
                "workspace_code",
                "workspaces",
                "code",
            ),
            (
                "assignments_author",
                "author_ref",
                "team_people",
                "owner_no",
            ),
            (
                "assignments_legacy",
                "legacy_ref",
                "team_people",
                "legacy_no",
            ),
            (
                "assignments_reviewer",
                "reviewer_ref",
                "reviewers",
                "owner_no",
            ),
        ] {
            assert!(
                foreign
                    .iter()
                    .any(|foreign| foreign.name.as_deref() == Some(name)
                        && foreign.columns == [column]
                        && foreign.foreign_table == format!("override_{table}")
                        && foreign.foreign_columns.as_slice() == [Some(key.to_owned())]),
                "{name}"
            );
        }

        db.insert(
            "insert into override_workspaces (id, code) values (1, 20)",
            &[],
        )
        .await
        .unwrap();
        db.insert(
            "insert into override_team_people (owner_no, legacy_no) values (2, 30)",
            &[],
        )
        .await
        .unwrap();
        db.insert("insert into override_reviewers (owner_no) values (3)", &[])
            .await
            .unwrap();
        let insert = "insert into override_assignments (workspace_code, author_ref, legacy_ref, reviewer_ref) values (20, 2, 30, ?)";
        db.insert(insert, &[3.into()]).await.unwrap();
        assert!(db.insert(insert, &[2.into()]).await.is_err());
        db.delete("delete from override_workspaces where id = 1", &[])
            .await
            .unwrap();
        assert_eq!(
            db.table("assignments")
                .value("workspace_code")
                .await
                .unwrap(),
            Value::Null
        );
    }

    #[tokio::test]
    async fn sqlite_references_and_drops_model_foreign_keys() {
        verify(connection(
            json!({"driver":"sqlite","database":":memory:","prefix":"app_","prefix_indexes":true}),
        ))
        .await;
    }
}

async fn verify_raw_column_and_float_precision(db: Connection, mysql: bool) {
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("column_precision").await.unwrap();
    schema
        .create("column_precision", |table| {
            table.id();
            table.float("default_precision");
            table.float_with("single_precision", 24);
            table.float_with("double_precision", 25);
            table.float_with("native_float", 0).nullable();
            table.double("native_double").nullable();
            table.raw_column("priority", "smallint").default(7);
        })
        .await
        .unwrap();
    let columns = schema.get_columns("column_precision").await.unwrap();
    let data_type = |name: &str| {
        &columns
            .iter()
            .find(|column| column.name == name)
            .unwrap()
            .type_name
    };
    assert_eq!(
        data_type("default_precision"),
        if mysql { "double" } else { "float" }
    );
    assert_eq!(data_type("single_precision"), "float");
    assert_eq!(
        data_type("double_precision"),
        if mysql { "double" } else { "float" }
    );
    assert_eq!(data_type("native_float"), "float");
    assert_eq!(data_type("native_double"), "double");
    assert_eq!(data_type("priority"), "smallint");

    db.insert(
        "insert into column_precision (default_precision, single_precision, double_precision) values (16777217, 16777217, 16777217)",
        &[],
    )
    .await
    .unwrap();
    let row = db
        .select_one("select * from column_precision", &[])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.get("default_precision"),
        Some(&Value::Real(16_777_217.0))
    );
    let stored_single = if mysql { 16_777_216.0 } else { 16_777_217.0 };
    assert_eq!(
        row.get("single_precision"),
        Some(&Value::Real(stored_single))
    );
    assert_eq!(
        row.get("double_precision"),
        Some(&Value::Real(16_777_217.0))
    );
    assert_eq!(row.get("priority"), Some(&7.into()));

    schema
        .table("column_precision", |table| {
            table.raw_column("priority", "bigint").default(9).change();
        })
        .await
        .unwrap();
    assert_eq!(
        db.table("column_precision")
            .value("priority")
            .await
            .unwrap(),
        7.into()
    );
    db.insert(
        "insert into column_precision (default_precision, single_precision, double_precision) values (0, 0, 0)",
        &[],
    )
    .await
    .unwrap();
    assert_eq!(
        db.table("column_precision")
            .r#where("id", "=", 2)
            .value("priority")
            .await
            .unwrap(),
        9.into()
    );

    for definition in [" ", "\0"] {
        assert!(matches!(
            schema
                .table("column_precision", |table| {
                    table.integer("not_added");
                    table.raw_column("invalid", definition);
                })
                .await,
            Err(DatabaseError::InvalidSchema("raw column definition"))
        ));
    }
    assert!(
        !schema
            .has_column("column_precision", "not_added")
            .await
            .unwrap()
    );
    if mysql {
        assert!(matches!(
            schema
                .table("column_precision", |table| {
                    table.float_with("invalid_precision", 54);
                })
                .await,
            Err(DatabaseError::InvalidSchema("floating-point precision"))
        ));
    }
    schema.drop("column_precision").await.unwrap();
}

#[tokio::test]
async fn mysql_uses_native_float_precision_and_raw_column_definitions() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    verify_raw_column_and_float_precision(connection(json!({"url":url})), true).await;
}

#[tokio::test]
async fn sqlite_uses_float_affinity_and_rebuilds_raw_columns() {
    verify_raw_column_and_float_precision(
        connection(json!({"driver":"sqlite","database":":memory:"})),
        false,
    )
    .await;
}

#[tokio::test]
async fn mysql_compiles_index_methods_and_atomic_online_column_drops() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url}));
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("online_schema").await.unwrap();
    schema
        .create("online_schema", |table| {
            table.unsigned_big_integer("id");
            table.primary(&["id"], None).algorithm("BTREE");
            table.integer("kept");
            table.integer("first_old").nullable();
            table.integer("second_old").nullable();
        })
        .await
        .unwrap();
    db.insert("insert into online_schema (id, kept) values (1, 42)", &[])
        .await
        .unwrap();
    schema
        .table("online_schema", |table| {
            table
                .index(&["kept"], Some("lookup"))
                .algorithm("btree")
                .inplace()
                .lock("none");
        })
        .await
        .unwrap();
    let indexes = schema.get_indexes("online_schema").await.unwrap();
    for name in ["primary", "lookup"] {
        assert!(indexes.iter().any(|index| {
            index.name == name
                && index
                    .index_type
                    .as_deref()
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("btree"))
        }));
    }

    assert!(
        schema
            .table("online_schema", |table| {
                table.drop_column(&["first_old", "missing"]).instant();
            })
            .await
            .is_err()
    );
    assert!(
        schema
            .has_column("online_schema", "first_old")
            .await
            .unwrap()
    );

    assert!(matches!(
        schema
            .table("online_schema", |table| {
                table.integer("not_applied");
                table.index(&["kept"], None).algorithm("invalid");
            })
            .await,
        Err(DatabaseError::InvalidSchema("index method"))
    ));
    assert!(matches!(
        schema
            .table("online_schema", |table| {
                table.integer("not_applied");
                table.drop_column(&["first_old"]).lock("invalid");
            })
            .await,
        Err(DatabaseError::InvalidSchema("DDL lock mode"))
    ));
    assert!(
        !schema
            .has_column("online_schema", "not_applied")
            .await
            .unwrap()
    );

    schema
        .table("online_schema", |table| {
            table
                .drop_column(&["first_old", "second_old"])
                .instant()
                .lock("default");
        })
        .await
        .unwrap();
    assert_eq!(
        schema.get_column_listing("online_schema").await.unwrap(),
        ["id", "kept"]
    );
    assert_eq!(
        db.table("online_schema").value("kept").await.unwrap(),
        42.into()
    );
    schema.drop_if_exists("hashed_schema").await.unwrap();
    schema
        .create("hashed_schema", |table| {
            table.engine("MEMORY");
            table.integer("key_value");
            table
                .index(&["key_value"], Some("memory_lookup"))
                .algorithm("HASH");
        })
        .await
        .unwrap();
    let indexes = schema.get_indexes("hashed_schema").await.unwrap();
    assert!(
        indexes
            .iter()
            .any(|index| index.name == "memory_lookup"
                && index.index_type.as_deref() == Some("hash"))
    );
    schema.drop("hashed_schema").await.unwrap();
    schema.drop("online_schema").await.unwrap();
}

#[tokio::test]
async fn mysql_renames_and_modifies_a_column_in_one_change_operation() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url}));
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("renamed_columns").await.unwrap();
    schema
        .create("renamed_columns", |table| {
            table.id();
            table.string("old label", 20);
            table.integer("rank");
            table.index(&["old label"], Some("stable_lookup"));
        })
        .await
        .unwrap();
    db.insert(
        "insert into renamed_columns (`old label`, `rank`) values ('retained', 4)",
        &[],
    )
    .await
    .unwrap();

    assert!(
        schema
            .table("renamed_columns", |table| {
                table
                    .string("old label", 60)
                    .nullable()
                    .rename_to("rank")
                    .change();
            })
            .await
            .is_err()
    );
    assert_eq!(
        schema
            .get_column_type("renamed_columns", "old label", true)
            .await
            .unwrap(),
        "varchar(20)"
    );

    assert!(matches!(
        schema
            .table("renamed_columns", |table| {
                table.integer("not_applied");
                table.string("invalid_add", 20).rename_to("still_invalid");
            })
            .await,
        Err(DatabaseError::InvalidSchema("renameTo requires change"))
    ));
    assert!(
        !schema
            .has_column("renamed_columns", "not_applied")
            .await
            .unwrap()
    );

    schema
        .table("renamed_columns", |table| {
            table
                .string("old label", 60)
                .nullable()
                .default("new")
                .rename_to("new label")
                .change()
                .unique();
        })
        .await
        .unwrap();
    assert_eq!(
        schema
            .get_column_type("renamed_columns", "new label", true)
            .await
            .unwrap(),
        "varchar(60)"
    );
    assert!(
        !schema
            .has_column("renamed_columns", "old label")
            .await
            .unwrap()
    );
    let indexes = schema.get_indexes("renamed_columns").await.unwrap();
    assert!(
        indexes
            .iter()
            .any(|index| index.name == "stable_lookup"
                && index.columns == [Some("new label".into())])
    );
    assert!(
        indexes
            .iter()
            .any(|index| index.unique && index.columns == [Some("new label".into())])
    );
    assert_eq!(
        db.table("renamed_columns")
            .value("new label")
            .await
            .unwrap(),
        "retained".into()
    );
    db.insert("insert into renamed_columns (`rank`) values (9)", &[])
        .await
        .unwrap();
    assert_eq!(
        db.table("renamed_columns")
            .r#where("id", "=", 2)
            .value("new label")
            .await
            .unwrap(),
        "new".into()
    );

    schema
        .table("renamed_columns", |table| {
            table
                .string("new label", 20)
                .rename_to("old label")
                .change();
        })
        .await
        .unwrap();
    assert!(
        !schema
            .has_column("renamed_columns", "new label")
            .await
            .unwrap()
    );
    assert_eq!(
        schema
            .get_column_type("renamed_columns", "old label", true)
            .await
            .unwrap(),
        "varchar(20)"
    );
    assert_eq!(
        db.table("renamed_columns")
            .r#where("id", "=", 1)
            .value("old label")
            .await
            .unwrap(),
        "retained".into()
    );
    schema.drop("renamed_columns").await.unwrap();
}

#[tokio::test]
async fn mysql_inspects_visible_schemas_tables_and_views_on_the_write_connection() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url}));
    let alternate = format!("schema_catalog_{}", std::process::id());
    let qualified = format!("{alternate}.app_catalog_status");
    db.statement("drop view if exists app_catalog_status", &[])
        .await
        .unwrap();
    db.statement(&format!("create database {alternate}"), &[])
        .await
        .unwrap();
    db.statement("create view app_catalog_status as select 42 as id", &[])
        .await
        .unwrap();
    db.statement(&format!("create view {qualified} as select 99 as id"), &[])
        .await
        .unwrap();
    db.statement("drop table if exists app_catalog_entries", &[])
        .await
        .unwrap();
    db.statement(
        "create table app_catalog_entries (id bigint primary key) comment 'directory fixture'",
        &[],
    )
    .await
    .unwrap();
    db.statement(
        &format!("create table {alternate}.app_catalog_entries (id bigint primary key)"),
        &[],
    )
    .await
    .unwrap();
    let mut read_url = url::Url::parse(&url).unwrap();
    read_url.set_path(&format!("/{alternate}"));
    let db = connection(json!({"url":url,"prefix":"app_","read":{"url":read_url.as_str()}}));
    let mut schema = db.get_schema_builder();
    let current = db.get_database_name();
    let read = db
        .select_one("select database() as name", &[])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(read.get("name"), Some(&Value::Text(alternate.clone())));
    assert_eq!(schema.get_current_schema_name(), current);
    assert_eq!(schema.get_current_schema_listing(), [current]);
    assert_eq!(
        schema
            .parse_schema_and_table("catalog_entries", SchemaFallback::None)
            .unwrap(),
        (None, "catalog_entries")
    );
    assert_eq!(
        schema
            .parse_schema_and_table("catalog_entries", SchemaFallback::Current)
            .unwrap(),
        (Some(current), "catalog_entries")
    );
    assert_eq!(
        schema
            .parse_schema_and_table("catalog_entries", SchemaFallback::Named(&alternate))
            .unwrap(),
        (Some(alternate.as_str()), "catalog_entries")
    );
    let qualified_logical = format!("{alternate}.catalog_entries");
    assert_eq!(
        schema
            .parse_schema_and_table(&qualified_logical, SchemaFallback::Current)
            .unwrap(),
        (Some(alternate.as_str()), "catalog_entries")
    );
    assert!(schema.has_table(&qualified_logical).await.unwrap());
    for reference in ["", ".catalog_entries", "schema.", "connection.schema.table"] {
        assert!(matches!(
            schema.parse_schema_and_table(reference, SchemaFallback::Current),
            Err(DatabaseError::InvalidSchema("table name"))
        ));
        assert!(matches!(
            schema.has_table(reference).await,
            Err(DatabaseError::InvalidSchema("table name"))
        ));
    }
    let other = connection(json!({"url": read_url.as_str()}));
    assert_eq!(
        other.get_schema_builder().get_current_schema_listing(),
        [alternate.as_str()]
    );

    let schemas = schema.get_schemas().await.unwrap();
    assert!(
        schemas
            .iter()
            .any(|entry| entry.name == current && entry.default && entry.path.is_none())
    );
    assert!(
        schemas
            .iter()
            .any(|entry| entry.name == alternate && !entry.default)
    );
    assert!(
        !schemas
            .iter()
            .any(|entry| entry.name == "information_schema")
    );
    assert!(schema.has_view("CATALOG_STATUS").await.unwrap());
    assert!(
        schema
            .has_view(&format!("{alternate}.catalog_status"))
            .await
            .unwrap()
    );
    assert!(
        !schema
            .has_view(&format!("{alternate}.missing"))
            .await
            .unwrap()
    );
    assert!(!schema.has_table("catalog_status").await.unwrap());
    let current_views = schema.get_views(Some(current)).await.unwrap();
    assert!(current_views.iter().any(|view| {
        view.name == "app_catalog_status"
            && view.schema == current
            && view.schema_qualified_name == format!("{current}.app_catalog_status")
            && view
                .definition
                .as_deref()
                .is_some_and(|definition| definition.contains("42"))
    }));
    assert!(!current_views.iter().any(|view| view.schema == alternate));
    assert_eq!(
        schema
            .get_views_in(&[current, &alternate])
            .await
            .unwrap()
            .iter()
            .filter(|view| view.name == "app_catalog_status")
            .count(),
        2
    );
    let all_views = schema.get_views(None).await.unwrap();
    assert!(all_views.iter().any(|view| {
        view.schema_qualified_name == qualified
            && view
                .definition
                .as_deref()
                .is_some_and(|definition| definition.contains("99"))
    }));
    assert!(
        all_views
            .iter()
            .all(|view| view.schema != "sys" && view.schema != "information_schema")
    );
    assert!(
        schema
            .get_views(Some("missing_catalog"))
            .await
            .unwrap()
            .is_empty()
    );

    let tables = schema.get_tables(None).await.unwrap();
    for database in [current, alternate.as_str()] {
        assert!(tables.iter().any(|info| info.schema_qualified_name
            == format!("{database}.app_catalog_entries")
            && info.name == "app_catalog_entries"
            && info.size.is_some_and(|size| size > 0)));
    }
    assert!(
        tables
            .iter()
            .all(|info| info.name != "app_catalog_status" && info.schema != "mysql")
    );
    assert_eq!(
        schema
            .get_tables_in(&[current, &alternate])
            .await
            .unwrap()
            .iter()
            .filter(|info| info.name == "app_catalog_entries")
            .count(),
        2
    );
    let local = schema.get_tables(Some(current)).await.unwrap();
    assert!(local.iter().all(|info| info.schema == current));
    assert!(local.iter().any(|info| info.name == "app_catalog_entries"
        && info.comment.as_deref() == Some("directory fixture")
        && info.engine.as_deref() == Some("InnoDB")));
    assert_eq!(
        schema.get_table_listing(Some(&alternate)).await.unwrap(),
        [format!("{alternate}.app_catalog_entries")]
    );
    assert_eq!(
        schema
            .get_table_listing_with(Some(&alternate), false)
            .await
            .unwrap(),
        ["app_catalog_entries"]
    );
    assert!(schema.get_tables_in(&[]).await.unwrap().is_empty());
    db.statement("drop table app_catalog_entries", &[])
        .await
        .unwrap();

    db.statement("drop view app_catalog_status", &[])
        .await
        .unwrap();
    db.statement(&format!("drop database {alternate}"), &[])
        .await
        .unwrap();
    assert!(!schema.has_view("catalog_status").await.unwrap());
}

#[tokio::test]
async fn mysql_schema_column_defaults_are_application_local() {
    struct Defaults;
    impl ServiceProvider for Defaults {
        fn boot(&mut self, _app: &Application) -> HttpFuture<'_, ProviderResult> {
            Box::pin(async {
                Schema::default_string_length(80)?;
                Schema::default_time_precision(Some(3))?;
                assert!(Schema::default_string_length(0).is_err());
                assert!(Schema::default_time_precision(Some(7)).is_err());
                Ok(())
            })
        }
    }
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    }
    async fn check(use_defaults: bool, name: &str) {
        let mut app = TestApp::create(|builder| {
            let builder = builder.with_config(|_, _| {
                let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
                Ok([(
                    "database".into(),
                    json!({"default":"fixture","connections":{"fixture":{"url":url}}}),
                )]
                .into())
            });
            if use_defaults {
                builder.with_providers(vec![Box::new(Defaults)])
            } else {
                builder
            }
        })
        .unwrap();
        let db = app.database().await.unwrap();
        let mut schema = db.get_schema_builder();
        schema.drop_if_exists(name).await.unwrap();
        schema
            .create(name, |table| {
                table.id();
                table.string("label", None);
                table.char("fixed", None).nullable();
                table.string("explicit_width", 20).nullable();
                table.remember_token();
                table.morphs("owner");
                table.timestamps();
                table.date_time("seen_at", 2).nullable();
                table.soft_deletes_datetime(Some("removed_at"), 6);
            })
            .await
            .unwrap();
        let width = if use_defaults { 80 } else { 255 };
        let time = if use_defaults {
            "timestamp(3)"
        } else {
            "timestamp"
        };
        for (column, expected) in [
            ("label", format!("varchar({width})")),
            ("fixed", format!("char({width})")),
            ("owner_type", format!("varchar({width})")),
            ("explicit_width", "varchar(20)".into()),
            ("remember_token", "varchar(100)".into()),
            ("created_at", time.into()),
            ("updated_at", time.into()),
            ("seen_at", "datetime(2)".into()),
            ("removed_at", "datetime(6)".into()),
        ] {
            assert_eq!(
                schema.get_column_type(name, column, true).await.unwrap(),
                expected
            );
        }
        let group = format!("{name}_dates");
        schema.drop_if_exists(&group).await.unwrap();
        schema
            .create(&group, |table| {
                table.id();
                table.datetimes(4);
                table.soft_deletes_tz_with(Some("discarded_at"), 5);
            })
            .await
            .unwrap();
        assert_eq!(
            schema
                .get_column_type(&group, "created_at", true)
                .await
                .unwrap(),
            "datetime(4)"
        );
        assert_eq!(
            schema
                .get_column_type(&group, "discarded_at", true)
                .await
                .unwrap(),
            "timestamp(5)"
        );
        db.table(&group)
            .insert(&[BTreeMap::from([("id".into(), 1.into())])])
            .await
            .unwrap();
        assert_eq!(
            db.table(&group).value("created_at").await.unwrap(),
            Value::Null
        );
        schema.drop(&group).await.unwrap();
        schema.drop(name).await.unwrap();
    }
    tokio::join!(
        check(true, "schema_custom_defaults"),
        check(false, "schema_standard_defaults"),
    );
}

#[tokio::test]
async fn mysql_applies_schema_callbacks_only_when_column_and_index_conditions_match() {
    use openrust::database::schema::KeySelector;

    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url,"prefix":"app_"}));
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("conditional_items").await.unwrap();
    schema.drop_if_exists("conditional_owners").await.unwrap();
    schema
        .create("conditional_owners", |table| {
            table.id();
        })
        .await
        .unwrap();
    schema
        .create("conditional_items", |table| {
            table.id();
            table.foreign_id("owner_id").nullable().constrained_on(
                "conditional_owners",
                "id",
                Some("conditional_owner_fk"),
            );
            table.integer("score");
        })
        .await
        .unwrap();
    db.insert("insert into app_conditional_items (score) values (42)", &[])
        .await
        .unwrap();

    schema
        .when_table_has_column("conditional_items", "missing", |_| panic!("column absent"))
        .await
        .unwrap();
    schema
        .when_table_has_column("conditional_items", "SCORE", |table| {
            table.integer("score").default(7).change();
        })
        .await
        .unwrap();
    for _ in 0..2 {
        schema
            .when_table_doesnt_have_column("conditional_items", "status", |table| {
                table.string("status", 16).default("draft");
            })
            .await
            .unwrap();
    }
    assert_eq!(
        db.table("conditional_items").value("score").await.unwrap(),
        42.into()
    );
    assert_eq!(
        db.table("conditional_items").value("status").await.unwrap(),
        "draft".into()
    );

    let columns = KeySelector::Columns(&["status", "score"]);
    for _ in 0..2 {
        schema
            .when_table_doesnt_have_index(
                "conditional_items",
                columns,
                |table| {
                    table.unique(&["status", "score"], Some("conditional_lookup"));
                },
                Some("unique"),
            )
            .await
            .unwrap();
    }
    assert!(
        schema
            .has_index_with(
                "conditional_items",
                KeySelector::Name("CONDITIONAL_LOOKUP"),
                Some("BTREE")
            )
            .await
            .unwrap()
    );
    assert!(
        schema
            .has_index_with(
                "conditional_items",
                KeySelector::Columns(&["id"]),
                Some("primary")
            )
            .await
            .unwrap()
    );
    assert!(
        !schema
            .has_index_with(
                "conditional_items",
                KeySelector::Columns(&["score", "status"]),
                None
            )
            .await
            .unwrap()
    );
    assert!(
        !schema
            .has_index_with(
                "conditional_items",
                KeySelector::Columns(&["STATUS", "score"]),
                None
            )
            .await
            .unwrap()
    );
    schema
        .when_table_has_index(
            "conditional_items",
            columns,
            |_| panic!("wrong index type"),
            Some("primary"),
        )
        .await
        .unwrap();
    schema
        .when_table_has_index(
            "conditional_items",
            columns,
            |table| {
                table.drop_unique("conditional_lookup");
            },
            Some("unique"),
        )
        .await
        .unwrap();
    assert!(
        !schema
            .has_index_with("conditional_items", columns, None)
            .await
            .unwrap()
    );

    assert!(
        schema
            .has_foreign_key(
                "conditional_items",
                KeySelector::Name("conditional_owner_fk")
            )
            .await
            .unwrap()
    );
    assert!(
        schema
            .has_foreign_key("conditional_items", KeySelector::Columns(&["owner_id"]))
            .await
            .unwrap()
    );
    assert!(
        !schema
            .has_foreign_key("conditional_items", KeySelector::Columns(&["score"]))
            .await
            .unwrap()
    );
    assert!(
        schema
            .when_table_has_column("conditional_items", "score", |table| {
                table.integer("score");
            })
            .await
            .is_err()
    );
    assert_eq!(db.table("conditional_items").count("*").await.unwrap(), 1);
    schema.drop("conditional_items").await.unwrap();
    schema.drop("conditional_owners").await.unwrap();
}

#[tokio::test]
async fn postgres_previews_database_ddl_without_using_the_filesystem() {
    let db = connection(json!({
        "driver": "pgsql", "database": "postgres", "charset": "UTF8", "prefix": "app_"
    }));
    let directory = tempfile::tempdir().unwrap();
    let preserved = directory.path().join("retained");
    std::fs::write(&preserved, b"keep").unwrap();
    let name = preserved.to_str().unwrap().to_owned();
    let drop_sql = format!("drop database if exists \"{}\"", name.replace('"', "\"\""));
    let preview = db
        .pretend(|db| {
            Box::pin(async move {
                let mut schema = db.get_schema_builder();
                assert!(schema.create_database("reports\"archive").await?);
                assert!(schema.drop_database_if_exists(&name).await?);
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(preview.len(), 2);
    assert_eq!(
        preview[0].query,
        "create database \"reports\"\"archive\" encoding \"UTF8\""
    );
    assert_eq!(preview[1].query, drop_sql);
    assert_eq!(std::fs::read(preserved).unwrap(), b"keep");
}

#[tokio::test]
async fn mysql_creates_and_drops_explicit_databases_with_connection_defaults() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(
        json!({"url":url,"charset":"utf8mb4","collation":"utf8mb4_bin","prefix":"app_"}),
    );
    let mut schema = db.get_schema_builder();
    let name = format!("schema-life-{:x}", fastrand::u64(..));
    assert!(schema.create_database(&name).await.unwrap());
    let row = db.select_one(
        "select default_character_set_name as charset, default_collation_name as collation from information_schema.schemata where schema_name = ?",
        &[name.as_str().into()],
    ).await.unwrap().unwrap();
    assert_eq!(row.get("charset"), Some(&"utf8mb4".into()));
    assert_eq!(row.get("collation"), Some(&"utf8mb4_bin".into()));

    let mut child_url = url::Url::parse(&url).unwrap();
    child_url.set_path(&format!("/{name}"));
    let child = connection(json!({"url":child_url.as_str()}));
    child
        .statement("create table retained (id int primary key)", &[])
        .await
        .unwrap();
    assert!(schema.create_database(&name).await.is_err());
    assert!(
        child
            .get_schema_builder()
            .has_table("retained")
            .await
            .unwrap()
    );
    drop(child);

    for _ in 0..2 {
        assert!(schema.drop_database_if_exists(&name).await.unwrap());
    }
    let schemas = schema.get_schemas().await.unwrap();
    assert!(!schemas.iter().any(|schema| schema.name == name));
    assert!(
        schemas
            .iter()
            .any(|schema| schema.name == db.get_database_name() && schema.default)
    );
}

#[tokio::test]
async fn sqlite_database_files_are_created_without_overwriting_existing_data() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("schema.sqlite");
    let name = path.to_str().unwrap();
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    let mut schema = db.get_schema_builder();
    assert!(schema.create_database(name).await.unwrap());
    assert_eq!(tokio::fs::read(&path).await.unwrap(), b"");
    tokio::fs::write(&path, b"retained").await.unwrap();
    assert!(matches!(
        schema.create_database(name).await,
        Err(DatabaseError::SchemaIo(_))
    ));
    assert_eq!(tokio::fs::read(&path).await.unwrap(), b"retained");
    assert!(schema.drop_database_if_exists(name).await.unwrap());
    assert!(schema.drop_database_if_exists(name).await.unwrap());
    assert!(!path.exists());
    assert!(schema.create_database(":memory:").await.is_err());
}

#[tokio::test]
async fn mysql_bulk_drop_only_removes_requested_objects_in_the_selected_schema() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let admin = connection(json!({"url":url}));
    let name = format!("schema_drop_{:x}", fastrand::u64(..));
    admin
        .get_schema_builder()
        .create_database(&name)
        .await
        .unwrap();
    let mut target = url::Url::parse(&url).unwrap();
    target.set_path(&format!("/{name}"));
    let db = connection(
        json!({"url":target.as_str(),"read":{"url":url},"max_connections":1,"prefix":"app_"}),
    );
    let mut schema = db.get_schema_builder();

    admin
        .statement(
            "create table if not exists bulk_drop_sentinel (id int primary key)",
            &[],
        )
        .await
        .unwrap();
    db.statement("create table unprefixed_parent (id int primary key)", &[])
        .await
        .unwrap();
    db.statement(
        "create table app_child (id int references unprefixed_parent(id))",
        &[],
    )
    .await
    .unwrap();
    db.statement(
        "create view app_report as select id from unprefixed_parent",
        &[],
    )
    .await
    .unwrap();
    schema.drop_all_views().await.unwrap();
    assert!(!schema.has_view("report").await.unwrap());
    assert!(schema.has_table("child").await.unwrap());

    db.statement(
        "create view app_report as select id from unprefixed_parent",
        &[],
    )
    .await
    .unwrap();
    for _ in 0..2 {
        schema.drop_all_tables().await.unwrap();
    }
    assert!(schema.get_tables(Some(&name)).await.unwrap().is_empty());
    assert!(schema.has_view("report").await.unwrap());
    assert!(
        admin
            .get_schema_builder()
            .has_table("bulk_drop_sentinel")
            .await
            .unwrap()
    );

    let checks = db
        .select_from_write_connection("select @@foreign_key_checks as checks_enabled", &[])
        .await
        .unwrap();
    assert!(matches!(
        checks[0].get("checks_enabled"),
        Some(Value::Integer(1) | Value::Unsigned(1) | Value::Bool(true))
    ));
    schema.drop_all_views().await.unwrap();
    assert!(schema.get_views(Some(&name)).await.unwrap().is_empty());
    drop(schema);
    drop(db);
    admin
        .get_schema_builder()
        .drop_database_if_exists(&name)
        .await
        .unwrap();
    admin
        .statement("drop table bulk_drop_sentinel", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_bulk_drop_keeps_objects_in_other_schemas() {
    let db = connection(json!({"driver":"sqlite","database":":memory:","max_connections":1}));
    let mut schema = db.get_schema_builder();
    db.statement("attach database ':memory:' as retained", &[])
        .await
        .unwrap();
    db.statement(
        "create table retained.sentinel (id integer primary key)",
        &[],
    )
    .await
    .unwrap();
    db.statement("create table local_row (id integer primary key)", &[])
        .await
        .unwrap();
    db.statement("create view local_view as select id from local_row", &[])
        .await
        .unwrap();
    assert_eq!(schema.get_current_schema_name(), "main");
    assert_eq!(schema.get_current_schema_listing(), ["main"]);
    assert_eq!(
        schema
            .parse_schema_and_table("sentinel", SchemaFallback::Current)
            .unwrap(),
        (Some("main"), "sentinel")
    );
    assert_eq!(
        schema
            .parse_schema_and_table("retained.sentinel", SchemaFallback::Current)
            .unwrap(),
        (Some("retained"), "sentinel")
    );
    schema.drop_all_tables().await.unwrap();
    assert!(!schema.has_table("local_row").await.unwrap());
    assert!(schema.has_view("local_view").await.unwrap());
    assert!(schema.has_table("retained.sentinel").await.unwrap());
    schema.drop_all_views().await.unwrap();
    assert!(!schema.has_view("local_view").await.unwrap());
    assert!(schema.has_table("retained.sentinel").await.unwrap());
}

#[tokio::test]
async fn mysql_manages_raw_expression_indexes() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url,"prefix":"app_"}));
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("raw_index_labels").await.unwrap();
    schema
        .create("raw_index_labels", |table| {
            table.id();
            table.string("label", 50);
            table
                .raw_index("(lower(label))", "label_folded")
                .algorithm("btree");
        })
        .await
        .unwrap();
    assert!(
        schema
            .has_index("raw_index_labels", "label_folded")
            .await
            .unwrap()
    );
    schema
        .table("raw_index_labels", |table| {
            table.drop_index("label_folded");
            table.raw_index("(lower(label)), id", "label_folded_id");
        })
        .await
        .unwrap();
    assert!(
        !schema
            .has_index("raw_index_labels", "label_folded")
            .await
            .unwrap()
    );
    let index = schema
        .get_indexes("raw_index_labels")
        .await
        .unwrap()
        .into_iter()
        .find(|index| index.name == "label_folded_id")
        .unwrap();
    assert_eq!(index.columns, [None, Some("id".into())]);
    db.insert(
        "insert into app_raw_index_labels (label) values (?)",
        &["ALpha".into()],
    )
    .await
    .unwrap();
    let plan = db
        .select(
            "explain select id from app_raw_index_labels force index (label_folded_id) where lower(label) = 'alpha'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(plan[0].get("key"), Some(&"label_folded_id".into()));
    assert_eq!(
        db.scalar(
            "select id from app_raw_index_labels where lower(label) = 'alpha'",
            &[]
        )
        .await
        .unwrap(),
        Value::Unsigned(1)
    );
    for invalid in ["  ", "(lower(label))\0"] {
        assert!(matches!(
            schema
                .table("raw_index_labels", |table| {
                    table.integer("not_added");
                    table.raw_index(invalid, "invalid_label_idx");
                })
                .await,
            Err(DatabaseError::InvalidSchema("raw index expression"))
        ));
    }
    assert!(
        !schema
            .has_column("raw_index_labels", "not_added")
            .await
            .unwrap()
    );
    schema.drop("raw_index_labels").await.unwrap();
}

#[tokio::test]
async fn sqlite_preserves_raw_expression_indexes_during_table_rebuild() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    let mut schema = db.get_schema_builder();
    schema
        .create("raw_index_labels", |table| {
            table.id();
            table.string("label", 50);
            table.raw_index("lower(label)", "label_folded");
        })
        .await
        .unwrap();
    schema
        .table("raw_index_labels", |table| {
            table.string("label", 100).nullable().change();
        })
        .await
        .unwrap();
    assert_eq!(
        schema.get_index_listing("raw_index_labels").await.unwrap(),
        ["label_folded", "primary"]
    );
    let plan = db
        .select(
            "explain query plan select id from raw_index_labels indexed by label_folded where lower(label) = 'alpha'",
            &[],
        )
        .await
        .unwrap();
    assert!(
        matches!(plan[0].get("detail"), Some(Value::Text(detail)) if detail.contains("label_folded"))
    );
}

#[tokio::test]
async fn mysql_drops_columns_through_the_named_schema_builder() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url,"prefix":"app_"}));
    let mut schema = db.get_schema_builder();
    schema
        .drop_if_exists("drop_columns_shortcut")
        .await
        .unwrap();
    schema
        .create("drop_columns_shortcut", |table| {
            table.id();
            table.integer("retired");
            table.string("old label", 40).nullable();
        })
        .await
        .unwrap();
    let target = format!("{}.drop_columns_shortcut", schema.get_current_schema_name());
    assert!(
        schema
            .drop_columns(&target, &["retired", "absent"])
            .await
            .is_err()
    );
    assert!(schema.has_column(&target, "retired").await.unwrap());
    assert!(matches!(
        schema.drop_columns(&target, &[]).await,
        Err(DatabaseError::InvalidSchema("empty column list"))
    ));
    schema
        .drop_columns(&target, &["retired", "old label"])
        .await
        .unwrap();
    assert_eq!(schema.get_column_listing(&target).await.unwrap(), ["id"]);
    schema.drop(&target).await.unwrap();
}

#[tokio::test]
async fn mysql_drops_conventionally_named_indexes_by_ordered_columns() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url,"prefix":"app_","prefix_indexes":true}));
    let mut schema = db.get_schema_builder();
    let database = schema.get_current_schema_name().to_owned();
    let table_name = format!("{database}.column_indexes");
    schema.drop_if_exists(&table_name).await.unwrap();
    schema
        .create(&table_name, |table| {
            table.id();
            table.string("label", 80);
            table.integer("rank");
            table.text("body");
            table.geometry("location", Some("point"), Some(4326));
            table.index(&["label", "rank"], None);
            table.index(&["label", "rank"], Some("named_index"));
            table.unique(&["label", "rank"], None);
            table.full_text(&["body"], None);
            table.spatial_index(&["location"], None);
        })
        .await
        .unwrap();
    let expected = |suffix| format!("{database}_app_column_indexes_{suffix}");
    let before = schema.get_index_listing(&table_name).await.unwrap();
    for suffix in [
        "label_rank_index",
        "label_rank_unique",
        "body_fulltext",
        "location_spatialindex",
    ] {
        assert!(before.contains(&expected(suffix)));
    }
    assert!(
        schema
            .table(&table_name, |table| table
                .drop_index_columns(&["rank", "label"]))
            .await
            .is_err()
    );
    assert_eq!(schema.get_index_listing(&table_name).await.unwrap(), before);
    assert!(matches!(
        schema
            .table(&table_name, |table| table.drop_index_columns(&[]))
            .await,
        Err(DatabaseError::InvalidSchema("empty column list"))
    ));
    schema
        .table(&table_name, |table| {
            table.drop_index_columns(&["label", "rank"]);
            table.drop_unique_columns(&["label", "rank"]);
            table.drop_full_text_columns(&["body"]);
            table.drop_spatial_index_columns(&["location"]);
        })
        .await
        .unwrap();
    assert_eq!(
        schema.get_index_listing(&table_name).await.unwrap(),
        ["named_index", "primary"]
    );
    schema.drop(&table_name).await.unwrap();
}

#[tokio::test]
async fn sqlite_json_declarations_follow_independent_connection_options() {
    for (json, jsonb, expected) in [
        (false, false, ["text", "text"]),
        (true, false, ["json", "text"]),
        (false, true, ["text", "jsonb"]),
        (true, true, ["json", "jsonb"]),
    ] {
        let db = connection(json!({
            "driver": "sqlite", "database": ":memory:",
            "use_native_json": json, "use_native_jsonb": jsonb,
        }));
        let mut schema = db.get_schema_builder();
        schema
            .create("documents", |table| {
                table.json("document");
            })
            .await
            .unwrap();
        schema
            .table("documents", |table| {
                table.jsonb("snapshot");
            })
            .await
            .unwrap();
        for (column, expected) in ["document", "snapshot"].into_iter().zip(expected) {
            assert_eq!(
                schema
                    .get_column_type("documents", column, true)
                    .await
                    .unwrap(),
                expected
            );
        }
        db.statement(
            "insert into documents values (json(?), jsonb(?))",
            &[r#"{"version":1}"#.into(), r#"{"version":2}"#.into()],
        )
        .await
        .unwrap();
        let row = db
            .select_one("select json_extract(document, '$.version') as current_version, json_extract(snapshot, '$.version') as snapshot_version, typeof(snapshot) as storage from documents", &[])
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.get("current_version"), Some(&Value::Integer(1)));
        assert_eq!(row.get("snapshot_version"), Some(&Value::Integer(2)));
        assert_eq!(row.get("storage"), Some(&"blob".into()));
    }
}

#[tokio::test]
async fn mysql_declares_json_and_jsonb_as_native_json() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url": url}));
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("schema_json_types").await.unwrap();
    schema
        .create("schema_json_types", |table| {
            table.json("document");
            table.jsonb("snapshot");
        })
        .await
        .unwrap();
    for column in ["document", "snapshot"] {
        assert_eq!(
            schema
                .get_column_type("schema_json_types", column, true)
                .await
                .unwrap(),
            "json"
        );
    }
    db.statement(
        "insert into schema_json_types values (?, ?)",
        &[r#"{"version":1}"#.into(), r#"{"version":2}"#.into()],
    )
    .await
    .unwrap();
    assert!(
        db.statement(
            "insert into schema_json_types values ('invalid JSON', '{}')",
            &[],
        )
        .await
        .is_err()
    );
    schema.drop("schema_json_types").await.unwrap();
}

#[tokio::test]
async fn mysql_edits_pending_blueprint_columns_and_creates_named_ids() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url": url, "prefix": "app_", "prefix_indexes": true}));
    let mut schema = db.get_schema_builder();
    let table_name = "blueprint_items";
    schema.drop_if_exists(table_name).await.unwrap();
    schema
        .create(table_name, |table| {
            table.id_with("item_id").starting_value(10);
            table.string("label", 32);
            table.integer("pending").unique();
            table.string("pending", 32);
            table.remove_column("pending").remove_column("unknown");
            table.index(&["label"], Some("blueprint_label"));
        })
        .await
        .unwrap();
    let key = schema
        .get_column(table_name, "item_id")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(key.full_type, "bigint unsigned");
    assert!(key.primary && key.auto_increment);
    assert!(!schema.has_column(table_name, "pending").await.unwrap());
    assert!(
        schema
            .has_index(table_name, "blueprint_label")
            .await
            .unwrap()
    );
    assert!(
        !schema
            .has_index(table_name, "app_blueprint_items_pending_unique")
            .await
            .unwrap()
    );

    db.statement(
        "insert into app_blueprint_items (label) values (?)",
        &["preserved".into()],
    )
    .await
    .unwrap();
    assert_eq!(
        db.table(table_name).value("item_id").await.unwrap(),
        Value::Unsigned(10)
    );
    schema
        .table(table_name, |table| {
            table.integer("label").change();
            table.remove_column("label").integer("rank").default(1);
        })
        .await
        .unwrap();
    assert_eq!(
        schema
            .get_column_type(table_name, "label", true)
            .await
            .unwrap(),
        "varchar(32)"
    );
    assert_eq!(
        db.table(table_name).value("label").await.unwrap(),
        "preserved".into()
    );
    assert_eq!(
        db.table(table_name).value("rank").await.unwrap(),
        Value::Integer(1)
    );
    schema.drop(table_name).await.unwrap();
}

#[tokio::test]
async fn sqlite_distinguishes_native_and_synthetic_primary_indexes_from_named_indexes() {
    use openrust::database::schema::KeySelector;

    let db = connection(json!({"driver": "sqlite", "database": ":memory:", "prefix": "app_"}));
    let mut schema = db.get_schema_builder();
    schema
        .create("events", |table| {
            table.integer("sequence");
            table.integer("tenant");
            table.primary(&["tenant", "sequence"], None);
            table.index(&["sequence"], Some("primary"));
        })
        .await
        .unwrap();
    let indexes = schema.get_indexes("main.events").await.unwrap();
    let native_primary = indexes.iter().find(|index| index.primary).unwrap();
    assert_eq!(native_primary.name, "sqlite_autoindex_app_events_1");
    assert_eq!(
        native_primary.columns,
        [Some("tenant".into()), Some("sequence".into())]
    );
    assert!(native_primary.unique);
    assert!(
        schema
            .has_index_with("events", KeySelector::Name("primary"), Some("primary"))
            .await
            .unwrap()
    );
    assert!(indexes.iter().any(|index| index.name == "primary"
        && !index.primary
        && index.columns == [Some("sequence".into())]));

    schema
        .table("events", |table| table.drop_index("primary"))
        .await
        .unwrap();
    schema
        .create("deliveries", |table| {
            table.id();
            table.integer("sequence");
            table.index(&["sequence"], Some("primary"));
        })
        .await
        .unwrap();
    let indexes = schema.get_indexes("deliveries").await.unwrap();
    assert_eq!(indexes.len(), 2);
    assert!(indexes.iter().all(|index| index.name == "primary"));
    assert!(
        indexes
            .iter()
            .any(|index| index.primary && index.unique && index.columns == [Some("id".into())])
    );
    assert!(indexes.iter().any(|index| !index.primary
        && !index.unique
        && index.columns == [Some("sequence".into())]));
    assert!(schema.get_indexes("missing").await.unwrap().is_empty());
}

#[tokio::test]
async fn sqlite_reports_base_types_and_primary_key_storage_behavior() {
    let db = connection(json!({"driver": "sqlite", "database": ":memory:"}));
    db.unprepared(
        "CREATE TABLE generated_ids (id INTEGER PRIMARY KEY, title VARCHAR(32), total DECIMAL(8,2), untyped);
         CREATE TABLE composite_ids (tenant INTEGER, sequence INTEGER, PRIMARY KEY (tenant, sequence));
         CREATE TABLE descending_ids (id INTEGER PRIMARY KEY DESC);
         CREATE TABLE separate_ids (id INTEGER PRIMARY KEY) WITHOUT ROWID;
         CREATE TABLE strict_ids (id INTEGER PRIMARY KEY, title TEXT) STRICT;
         INSERT INTO generated_ids (id, title) VALUES (NULL, 'entry');
         INSERT INTO composite_ids (tenant, sequence) VALUES (NULL, 2);
         INSERT INTO descending_ids (id) VALUES (NULL);
         INSERT INTO strict_ids (title) VALUES ('strict entry');",
    )
    .await
    .unwrap();
    let mut schema = db.get_schema_builder();
    for (name, expected, full) in [
        ("title", "varchar", "varchar(32)"),
        ("total", "decimal", "decimal(8,2)"),
        ("untyped", "", ""),
    ] {
        let column = schema
            .get_column("generated_ids", name)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(column.type_name, expected);
        assert_eq!(column.full_type, full);
        assert_eq!(
            schema
                .get_column_type("generated_ids", name, false)
                .await
                .unwrap(),
            expected
        );
        assert_eq!(
            schema
                .get_column_type("generated_ids", name, true)
                .await
                .unwrap(),
            full
        );
    }

    for (table, name, auto_increment, nullable) in [
        ("generated_ids", "id", true, false),
        ("strict_ids", "id", true, false),
        ("composite_ids", "tenant", false, true),
        ("descending_ids", "id", false, true),
        ("separate_ids", "id", false, false),
    ] {
        let column = schema.get_column(table, name).await.unwrap().unwrap();
        assert!(column.primary, "{table}.{name}");
        assert_eq!(column.auto_increment, auto_increment, "{table}.{name}");
        assert_eq!(column.nullable, nullable, "{table}.{name}");
    }
    assert_eq!(
        db.table("generated_ids").value("id").await.unwrap(),
        Value::Integer(1)
    );
    assert_eq!(
        db.table("strict_ids").value("id").await.unwrap(),
        Value::Integer(1)
    );
    assert_eq!(
        db.table("composite_ids").value("tenant").await.unwrap(),
        Value::Null
    );
    assert_eq!(
        db.table("descending_ids").value("id").await.unwrap(),
        Value::Null
    );
    assert!(
        db.statement("insert into separate_ids (id) values (null)", &[])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn sqlite_reports_foreign_keys_in_catalog_order_with_the_attached_schema() {
    use openrust::database::schema::KeySelector;

    let db = connection(json!({"driver": "sqlite", "database": ":memory:", "prefix": "app_"}));
    let mut session = db.get_schema_builder().session().await.unwrap();
    for statement in [
        "attach database ':memory:' as warehouse",
        "create table warehouse.app_single (id integer primary key)",
        "create table warehouse.app_parent (tenant integer, number integer, primary key (tenant, number))",
        "insert into warehouse.app_parent values (10, 20)",
    ] {
        session.statement(statement, &[]).await.unwrap();
    }

    let children: Vec<_> = (0..12).map(|id| format!("parent_{id}")).collect();
    let mut definitions: Vec<_> = children
        .iter()
        .map(|column| format!("{column} integer references app_single(id)"))
        .collect();
    definitions.push("number integer".into());
    definitions.push("tenant integer".into());
    definitions.push(
        "constraint child_parent foreign key (tenant, number) references app_parent on delete cascade on update restrict".into(),
    );

    session
        .statement(
            &format!(
                "create table warehouse.app_child ({})",
                definitions.join(", ")
            ),
            &[],
        )
        .await
        .unwrap();
    session
        .statement(
            "insert into warehouse.app_child (tenant, number) values (10, 20)",
            &[],
        )
        .await
        .unwrap();
    assert!(
        session
            .statement(
                "insert into warehouse.app_child (tenant, number) values (10, 21)",
                &[],
            )
            .await
            .is_err()
    );

    let mut schema = session.get_schema_builder();
    let keys = schema.get_foreign_keys("warehouse.child").await.unwrap();
    assert_eq!(keys.len(), children.len() + 1);
    assert!(
        keys.iter().all(|key| {
            key.name.is_none() && key.foreign_schema.as_deref() == Some("warehouse")
        })
    );
    let composite = &keys[0];
    assert_eq!(composite.foreign_table, "app_parent");
    assert_eq!(composite.columns, ["tenant", "number"]);
    assert_eq!(composite.foreign_columns, [None, None]);
    assert_eq!(composite.on_delete, "cascade");
    assert_eq!(composite.on_update, "restrict");
    for (key, column) in keys[1..].iter().zip(children.iter().rev()) {
        assert_eq!(&key.columns, &[column.to_owned()]);
        assert_eq!(key.foreign_table, "app_single");
        assert_eq!(key.foreign_columns, [Some("id".into())]);
    }
    assert!(
        schema
            .has_foreign_key(
                "warehouse.child",
                KeySelector::Columns(&["tenant", "number"])
            )
            .await
            .unwrap()
    );
    assert!(
        !schema
            .has_foreign_key(
                "warehouse.child",
                KeySelector::Columns(&["number", "tenant"])
            )
            .await
            .unwrap()
    );
    assert!(
        schema
            .get_foreign_keys("main.child")
            .await
            .unwrap()
            .is_empty()
    );

    session
        .statement("detach database warehouse", &[])
        .await
        .unwrap();
    session.close().await.unwrap();
}

async fn verify_blueprint_context(db: Connection, table_ref: &str) {
    use openrust::database::schema::ColumnDefinition;

    assert_eq!(db.get_table_prefix(), "app_");
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists(table_ref).await.unwrap();
    schema
        .create(table_ref, |table| {
            assert_eq!(table.get_table(), table_ref);
            assert_eq!(table.get_prefix(), "app_");
            assert!(table.creating());
            assert_eq!(table.get_columns().count(), 0);
            table.id();
            table.string("code", 16);
            table.string("transient", 20);
            table.remove_column("transient");
            table.index(&["code"], Some("blueprint_context_code"));
            assert_eq!(
                table
                    .get_added_columns()
                    .map(ColumnDefinition::get_name)
                    .collect::<Vec<_>>(),
                ["id", "code"]
            );
            assert_eq!(table.get_changed_columns().count(), 0);
        })
        .await
        .unwrap();
    schema
        .when_table_has_column(table_ref, "code", |table| {
            assert_eq!(table.get_table(), table_ref);
            assert_eq!(table.get_prefix(), "app_");
            assert!(!table.creating());
            table.string("code", 32).nullable().change();
            table.integer("quantity").default(0);
            assert_eq!(
                table
                    .get_columns()
                    .map(ColumnDefinition::get_name)
                    .collect::<Vec<_>>(),
                ["code", "quantity"]
            );
            assert!(
                table
                    .get_changed_columns()
                    .map(ColumnDefinition::get_name)
                    .eq(["code"])
            );
            assert!(
                table
                    .get_added_columns()
                    .map(ColumnDefinition::get_name)
                    .eq(["quantity"])
            );
        })
        .await
        .unwrap();
    assert_eq!(
        schema.get_column_listing(table_ref).await.unwrap(),
        ["id", "code", "quantity"]
    );
    assert!(
        schema
            .get_column(table_ref, "code")
            .await
            .unwrap()
            .unwrap()
            .nullable
    );
    assert!(
        schema
            .has_index(table_ref, "blueprint_context_code")
            .await
            .unwrap()
    );
    schema.drop(table_ref).await.unwrap();
}

#[tokio::test]
async fn sqlite_exposes_blueprint_context_for_creation_and_conditional_alter() {
    let db = connection(json!({"driver": "sqlite", "database": ":memory:", "prefix": "app_"}));
    verify_blueprint_context(db, "main.blueprint_context").await;
}

#[tokio::test]
async fn mysql_exposes_blueprint_context_for_creation_and_conditional_alter() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url": url, "prefix": "app_"}));
    let table = format!("{}.blueprint_context", db.get_database_name());
    verify_blueprint_context(db, &table).await;
}

#[tokio::test]
async fn sqlite_scopes_blueprint_resolvers_to_the_builder_and_transaction() {
    let db = connection(json!({"driver": "sqlite", "database": ":memory:", "prefix": "app_"}));
    let mut tx = db.begin_transaction().await.unwrap();
    {
        let mut schema = tx.get_schema_builder();
        schema.blueprint_resolver(|table| {
            assert_eq!(table.get_prefix(), "app_");
            if table.creating() {
                table.id();
            }
            table.string("from_resolver", 16).nullable();
        });
        schema
            .create("resolved", |table| {
                assert_eq!(table.get_table(), "resolved");
                assert!(
                    table
                        .get_added_columns()
                        .any(|c| c.get_name() == "from_resolver")
                );
                table.string("from_callback", 16);
            })
            .await
            .unwrap();
        assert_eq!(
            schema.get_column_listing("resolved").await.unwrap(),
            ["id", "from_resolver", "from_callback"]
        );
        schema.blueprint_resolver(|table| {
            assert!(!table.creating());
            table.integer("updated_resolver").default(1);
        });
        schema
            .when_table_has_column("resolved", "from_callback", |table| {
                assert!(
                    table
                        .get_added_columns()
                        .any(|c| c.get_name() == "updated_resolver")
                );
                table.integer("quantity").default(0);
            })
            .await
            .unwrap();
    }
    {
        let mut fresh = tx.get_schema_builder();
        assert!(
            fresh
                .has_column("resolved", "updated_resolver")
                .await
                .unwrap()
        );
        fresh
            .create("fresh", |table| {
                assert_eq!(table.get_columns().count(), 0);
                table.id();
            })
            .await
            .unwrap();
        assert_eq!(fresh.get_column_listing("fresh").await.unwrap(), ["id"]);
    }
    tx.roll_back(None).await.unwrap();
    let mut schema = db.get_schema_builder();
    assert!(!schema.has_table("resolved").await.unwrap());
    assert!(!schema.has_table("fresh").await.unwrap());
}

#[tokio::test]
async fn mysql_resolves_blueprints_in_preview_and_real_ddl() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url": url, "prefix": "app_"}));
    let configure = |table: &mut openrust::database::schema::Blueprint| {
        assert!(table.creating());
        assert_eq!(table.get_prefix(), "app_");
        table.id();
        table.string("external_ref", 32);
    };
    let mut schema = db.get_schema_builder();
    schema.drop_if_exists("resolver_probe").await.unwrap();
    let preview = db
        .pretend(|connection| {
            Box::pin(async move {
                let mut builder = connection.get_schema_builder();
                builder.blueprint_resolver(configure);
                builder.create("resolver_probe", |_| {}).await
            })
        })
        .await
        .unwrap();
    assert_eq!(preview.len(), 1);
    assert!(preview[0].query.contains("`app_resolver_probe`"));
    assert!(preview[0].query.contains("`external_ref` varchar(32)"));
    assert!(!schema.has_table("resolver_probe").await.unwrap());

    schema.blueprint_resolver(configure);
    schema.create("resolver_probe", |_| {}).await.unwrap();
    assert_eq!(
        schema.get_column_listing("resolver_probe").await.unwrap(),
        ["id", "external_ref"]
    );
    schema.drop("resolver_probe").await.unwrap();
}

#[tokio::test]
async fn postgres_changes_column_types_and_modifiers_atomically() {
    use openrust::database::query::Expression;

    let preview = connection(json!({"driver":"pgsql","prefix":"spec_"}));
    let queries = preview
        .pretend(|db| {
            Box::pin(async move {
                db.get_schema_builder()
                    .table("tenant.entries", |table| {
                        table
                            .decimal("amount", 7, 2)
                            .using(Expression::new("trim(amount)::numeric / 100"))
                            .default(1)
                            .nullable()
                            .comment("owner's amount")
                            .change();
                    })
                    .await
            })
        })
        .await
        .unwrap();
    assert_eq!(queries.len(), 2);
    assert_eq!(
        queries[0].query,
        "alter table \"tenant\".\"spec_entries\" alter column \"amount\" type decimal(7, 2) using trim(amount)::numeric / 100, alter column \"amount\" drop not null, alter column \"amount\" drop identity if exists, alter column \"amount\" set default 1"
    );
    assert_eq!(
        queries[1].query,
        "comment on column \"tenant\".\"spec_entries\".\"amount\" is 'owner''s amount'"
    );
    assert!(matches!(
        preview
            .get_schema_builder()
            .table("entries", |table| {
                table
                    .integer("amount")
                    .using(Expression::new("?").with_bindings(vec![1.into()]))
                    .change();
            })
            .await,
        Err(DatabaseError::InvalidSchema("column conversion expression"))
    ));

    let Ok(url) = std::env::var("OPENRUST_TEST_POSTGRES_CHANGE_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_POSTGRES_CHANGE").is_none());
        return;
    };
    let db = connection(json!({"url":url}));
    db.unprepared("create schema tenant; create table tenant.amounts (amount varchar(8) not null, marker varchar(16)); insert into tenant.amounts values ('01250', 'known'); create index amount_marker on tenant.amounts (marker)")
        .await.unwrap();
    let mut schema = db.get_schema_builder();
    assert!(
        schema
            .table("tenant.amounts", |table| {
                table.string("marker", Some(12)).nullable().change();
                table
                    .integer("amount")
                    .using(Expression::new("'invalid' || amount"))
                    .change();
            })
            .await
            .is_err()
    );
    assert_eq!(
        schema
            .get_column_type("tenant.amounts", "marker", true)
            .await
            .unwrap(),
        "character varying(16)"
    );
    schema
        .table("tenant.amounts", |table| {
            table
                .decimal("amount", 7, 2)
                .using(Expression::new("amount::numeric / 100"))
                .default(1)
                .nullable()
                .comment("owner's amount")
                .change();
        })
        .await
        .unwrap();
    let column = schema
        .get_column("tenant.amounts", "amount")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(column.full_type, "numeric(7,2)");
    assert!(column.nullable);
    assert_eq!(column.comment.as_deref(), Some("owner's amount"));
    assert_eq!(
        db.scalar(
            "select amount::text from tenant.amounts where marker='known'",
            &[]
        )
        .await
        .unwrap(),
        "12.50".into()
    );
    db.statement("insert into tenant.amounts(marker) values ('default')", &[])
        .await
        .unwrap();
    assert_eq!(
        db.scalar(
            "select amount::text from tenant.amounts where marker='default'",
            &[]
        )
        .await
        .unwrap(),
        "1.00".into()
    );
    assert!(
        schema
            .has_index("tenant.amounts", "amount_marker")
            .await
            .unwrap()
    );
    schema
        .table("tenant.amounts", |table| {
            table.decimal("amount", 7, 2).change();
        })
        .await
        .unwrap();
    let column = schema
        .get_column("tenant.amounts", "amount")
        .await
        .unwrap()
        .unwrap();
    assert!(!column.nullable);
    assert!(column.default.is_none());
    assert!(column.comment.is_none());
    db.unprepared("drop schema tenant cascade").await.unwrap();
}

#[tokio::test]
async fn postgres_generated_columns_and_explicit_identity_use_native_modes() {
    use openrust::database::query::Expression;

    let preview = connection(json!({"driver":"pgsql"}));
    let statements = preview
        .pretend(|db| {
            Box::pin(async move {
                db.get_schema_builder()
                    .create("tenant.generated", |table| {
                        table
                            .bigInteger("seq")
                            .generated_as_with(Expression::new("start with 10 increment by 2"))
                            .always();
                        table.integer("base");
                        table.integer("saved").stored_as("base * 2");
                        table.integer("computed").virtual_as("base + 1");
                    })
                    .await
            })
        })
        .await
        .unwrap();
    let sql = &statements[0].query;
    assert!(sql.contains(
        "\"seq\" bigint not null generated always as identity (start with 10 increment by 2)"
    ));
    assert!(sql.contains("\"saved\" integer generated always as (base * 2) stored"));
    assert!(sql.contains("\"computed\" integer generated always as (base + 1) virtual"));
    assert!(!sql.contains("primary key"));
    let statements = preview
        .pretend(|db| {
            Box::pin(async move {
                db.get_schema_builder()
                    .create("tenant.other", |table| {
                        table.bigInteger("seq").generated_as();
                        table.integer("plain").always();
                        table.primary(&["seq", "plain"], Some("other_primary"));
                    })
                    .await
            })
        })
        .await
        .unwrap();
    let sql = &statements[0].query;
    assert!(sql.contains("\"seq\" bigint not null generated by default as identity"));
    assert!(sql.contains("\"plain\" integer not null,"));
    assert_eq!(sql.matches("primary key").count(), 1);
    let invalid = preview
        .get_schema_builder()
        .create("tenant.other", |table| {
            table
                .bigInteger("seq")
                .generated_as_with(Expression::new("start with ?").with_bindings(vec![10.into()]));
        })
        .await;
    assert!(matches!(
        invalid,
        Err(DatabaseError::InvalidSchema("identity sequence options"))
    ));

    let result = preview
        .get_schema_builder()
        .table("tenant.generated", |table| {
            table.integer("saved").stored_as("base + 2").change();
        })
        .await;
    assert!(matches!(
        result,
        Err(DatabaseError::UnsupportedSchema(
            "PostgreSQL column change modifier"
        ))
    ));

    let Ok(url) = std::env::var("OPENRUST_TEST_POSTGRES_GENERATION_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_POSTGRES_GENERATION").is_none());
        return;
    };
    let db = connection(json!({"url": url}));
    db.statement("create schema tenant", &[]).await.unwrap();
    let mut schema = db.get_schema_builder();
    schema
        .create("tenant.generated", |table| {
            table
                .bigInteger("seq")
                .generated_as_with(Expression::new("start with 10 increment by 2"))
                .always();
            table.integer("base");
            table.integer("saved").stored_as("base * 2");
            table.integer("computed").virtual_as("base + 1");
        })
        .await
        .unwrap();
    for (name, expected) in [
        ("saved", GenerationType::Stored),
        ("computed", GenerationType::Virtual),
    ] {
        let column = schema
            .get_column("tenant.generated", name)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(column.generation.unwrap().kind, expected);
    }
    db.statement("insert into tenant.generated(base) values (3)", &[])
        .await
        .unwrap();
    assert!(
        db.statement(
            "insert into tenant.generated(seq, base) values (90, 3)",
            &[]
        )
        .await
        .is_err()
    );
    assert_eq!(
        db.scalar("select seq from tenant.generated", &[])
            .await
            .unwrap(),
        10.into()
    );
    assert_eq!(
        db.scalar("select computed from tenant.generated", &[])
            .await
            .unwrap(),
        4.into()
    );
    schema
        .table("tenant.generated", |table| {
            table.integer("saved").drop_expression().change();
            table
                .bigInteger("seq")
                .generated_as_with(Expression::new("start with 20"))
                .change();
        })
        .await
        .unwrap();
    assert!(
        schema
            .get_column("tenant.generated", "saved")
            .await
            .unwrap()
            .unwrap()
            .generation
            .is_none()
    );
    db.statement("update tenant.generated set base = 8", &[])
        .await
        .unwrap();
    assert_eq!(
        db.scalar("select saved from tenant.generated", &[])
            .await
            .unwrap(),
        6.into()
    );
    assert_eq!(
        db.scalar("select computed from tenant.generated", &[])
            .await
            .unwrap(),
        9.into()
    );
    db.statement(
        "insert into tenant.generated (base, saved) values (2, 11)",
        &[],
    )
    .await
    .unwrap();
    db.statement(
        "insert into tenant.generated (seq, base, saved) values (99, 2, 11)",
        &[],
    )
    .await
    .unwrap();
    assert_eq!(
        db.scalar(
            "select count(*) from tenant.generated where seq in (20, 99)",
            &[]
        )
        .await
        .unwrap(),
        2.into()
    );
    schema
        .table("tenant.generated", |table| {
            table.bigInteger("seq").change();
        })
        .await
        .unwrap();
    assert!(
        !schema
            .get_column("tenant.generated", "seq")
            .await
            .unwrap()
            .unwrap()
            .auto_increment
    );
    assert!(
        db.statement(
            "insert into tenant.generated (base, saved) values (5, 6)",
            &[]
        )
        .await
        .is_err()
    );
    db.statement("drop schema tenant cascade", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn tsvector_rejects_unsupported_schema_dialects_before_executing_ddl() {
    for driver in ["sqlite", "mysql", "mariadb", "sqlsrv"] {
        let db = connection(json!({"driver": driver, "database": ":memory:"}));
        let error = db
            .get_schema_builder()
            .create("documents", |table| {
                table.tsvector("search_terms");
            })
            .await
            .unwrap_err();
        assert!(
            matches!(error, DatabaseError::UnsupportedSchema("tsvector column")),
            "{driver}: {error:?}"
        );
    }
}

#[tokio::test]
async fn postgres_generated_json_columns_use_native_selector_semantics() {
    let preview = connection(json!({"driver":"pgsql"}));
    let statements = preview
        .pretend(|db| {
            Box::pin(async move {
                db.get_schema_builder()
                    .create("tenant.documents", |table| {
                        table.jsonb("payload");
                        table
                            .string("label", 80)
                            .stored_as_json("payload->items[0][1]->owner's\\name");
                        table
                            .string("code", 20)
                            .virtual_as_json("payload->codes->01");
                    })
                    .await
            })
        })
        .await
        .unwrap();
    let sql = &statements[0].query;
    assert!(sql.contains("\"payload\"->E'items'->0->1->>E'owner''s\\\\name'"));
    assert!(sql.contains("\"payload\"->E'codes'->>E'01'"));
    assert!(!sql.contains("json_extract"));

    let Ok(url) = std::env::var("OPENRUST_TEST_POSTGRES_GENERATED_JSON_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_POSTGRES_GENERATED_JSON").is_none());
        return;
    };
    let db = connection(json!({"url":url}));
    let mut tx = db.begin_transaction().await.unwrap();
    tx.statement("set local standard_conforming_strings = off", &[])
        .await
        .unwrap();
    tx.get_schema_builder()
        .create("generated_json", |table| {
            table.integer("id");
            table.jsonb("payload");
            table
                .string("label", 80)
                .stored_as_json("payload->items[0][1]->owner's\\name")
                .nullable()
                .index();
            table
                .string("code", 20)
                .virtual_as_json("payload->codes->01")
                .nullable();
            table
                .string("last", 20)
                .virtual_as_json("payload->names[-1]")
                .nullable();
        })
        .await
        .unwrap();
    let payload = |label: &str| {
        Value::Json(json!({
            "items":[[{},{"owner's\\name":label}]],
            "codes":{"01":"zero-prefixed"},
            "names":["first","last"]
        }))
    };
    tx.statement(
        "insert into generated_json (id,payload) values (?, ?), (?, ?)",
        &[
            1.into(),
            payload("one"),
            2.into(),
            Value::Json(json!({"items":null})),
        ],
    )
    .await
    .unwrap();
    let mut schema = tx.get_schema_builder();
    let column = schema
        .get_column("generated_json", "label")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(column.generation.unwrap().kind, GenerationType::Stored);
    assert!(
        schema
            .has_index("generated_json", "generated_json_label_index")
            .await
            .unwrap()
    );
    assert_eq!(
        tx.scalar(
            "select label || ':' || code || ':' || last from generated_json where id=1",
            &[]
        )
        .await
        .unwrap(),
        "one:zero-prefixed:last".into()
    );
    assert_eq!(
        tx.scalar(
            "select label is null and code is null and last is null from generated_json where id=2",
            &[]
        )
        .await
        .unwrap(),
        true.into()
    );
    tx.statement(
        "update generated_json set payload = ? where id = 1",
        &[payload("two")],
    )
    .await
    .unwrap();
    assert_eq!(
        tx.scalar("select label from generated_json where id=1", &[])
            .await
            .unwrap(),
        "two".into()
    );
    tx.roll_back(None).await.unwrap();
}
