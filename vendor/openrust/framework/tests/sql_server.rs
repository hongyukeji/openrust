use openrust::database::{Bindings, Value, connectors::ConnectionFactory, query::Record};
use serde_json::json;

#[tokio::test]
async fn sql_server_tds_bindings_transactions_and_schema() {
    let url = std::env::var("OPENRUST_TEST_SQL_SERVER_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
    let Some(url) = url else { return };
    let root = tempfile::tempdir().unwrap();
    let db = ConnectionFactory
        .make("sqlsrv", json!({"url":url}), root.path())
        .unwrap();
    let tls = db
        .scalar(
            "select encrypt_option from sys.dm_exec_connections where session_id=@@spid",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(tls, "TRUE".into());
    let rows = db.select_result_sets(
        "select :name as [value], 'literal ? :n' as [literal]; select cast(:amount as decimal(20,4)) as [amount]",
        Bindings::Named(&[("name","测试 ' 声称?".into()), ("amount","1234567890123.1234".into())]), true,
    ).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0][0].get("value"),
        Some(&Value::Text("测试 ' 声称?".into()))
    );
    assert_eq!(
        rows[0][0].get("literal"),
        Some(&Value::Text("literal ? :n".into()))
    );
    assert_eq!(
        rows[1][0].get("amount"),
        Some(&Value::Text("1234567890123.1234".into()))
    );

    let table = format!("sqlserver_release_{:016x}", fastrand::u64(..));
    let mut schema = db.get_schema_builder();
    schema
        .create(&table, |table| {
            table.id();
            table.string("name", 64).unique();
            table.integer("score");
            table.binary("blob").nullable();
        })
        .await
        .unwrap();
    let columns = schema.get_columns(&table).await.unwrap();
    assert!(
        columns
            .iter()
            .any(|column| column.name == "id" && column.auto_increment && column.primary)
    );
    assert!(
        schema
            .get_schemas()
            .await
            .unwrap()
            .iter()
            .any(|schema| schema.name == "dbo" && schema.default)
    );
    assert!(
        schema
            .get_tables(Some("dbo"))
            .await
            .unwrap()
            .iter()
            .any(|item| item.name == table)
    );
    let record = |name: &str, score: i64| {
        Record::from([("name".into(), name.into()), ("score".into(), score.into())])
    };
    assert_eq!(
        db.table(&table)
            .insert_get_id(record("first", 1))
            .await
            .unwrap(),
        1.into()
    );
    let mut transaction = db.begin_transaction().await.unwrap();
    transaction
        .table(&table)
        .insert(&[record("kept", 2)])
        .await
        .unwrap();
    transaction.begin_transaction().await.unwrap();
    transaction
        .table(&table)
        .insert(&[record("rolled back", 3)])
        .await
        .unwrap();
    transaction.roll_back(None).await.unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(db.table(&table).count("*").await.unwrap(), 2);
    db.table(&table)
        .upsert(&[record("first", 5)], &["name"], Some(&["score"]))
        .await
        .unwrap();
    let rows = db
        .table(&table)
        .order_by_desc("score")
        .offset(1)
        .limit(1)
        .get()
        .await
        .unwrap();
    assert_eq!(rows[0].get("name"), Some(&Value::Text("kept".into())));
    let encoded = Value::Bytes(vec![0, 255, 33]);
    db.table(&table)
        .r#where("name", "=", "first")
        .update(Record::from([("blob".into(), encoded.clone())]))
        .await
        .unwrap();
    assert_eq!(
        db.table(&table)
            .r#where("name", "=", "first")
            .value("blob")
            .await
            .unwrap(),
        encoded
    );
    schema.drop_if_exists(&table).await.unwrap();
}

#[tokio::test]
async fn sql_server_online_indexes_and_named_foreign_key_drop() {
    let root = tempfile::tempdir().unwrap();
    let preview = ConnectionFactory
        .make("sqlsrv", json!({"driver":"sqlsrv"}), root.path())
        .unwrap();
    let statements = preview
        .pretend(|db| {
            Box::pin(async move {
                db.get_schema_builder()
                    .table("dbo.entries", |table| {
                        table.index(&["score"], Some("search")).online();
                        table.unique(&["ref"], Some("unique_ref")).online();
                        table.drop_foreign("entry]parent");
                    })
                    .await
            })
        })
        .await
        .unwrap();
    assert_eq!(
        statements
            .iter()
            .map(|entry| entry.query.as_str())
            .collect::<Vec<_>>(),
        [
            "create index [search] on [dbo].[entries] ([score]) with (online = on)",
            "create unique index [unique_ref] on [dbo].[entries] ([ref]) with (online = on)",
            "alter table [dbo].[entries] drop constraint [entry]]parent]",
        ]
    );

    let Ok(url) = std::env::var("OPENRUST_TEST_SQL_SERVER_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
        return;
    };
    let db = ConnectionFactory
        .make("sqlsrv", json!({"url":url}), root.path())
        .unwrap();
    let suffix = fastrand::u64(..);
    let parent = format!("dbo.parents_{suffix:016x}");
    let child = format!("dbo.entries_{suffix:016x}");
    let foreign = format!("entry_parent_{suffix:016x}");
    let mut schema = db.get_schema_builder();
    schema
        .create(&parent, |table| {
            table.id();
        })
        .await
        .unwrap();
    schema
        .create(&child, |table| {
            table.id();
            table.bigInteger("parent_id");
            table.integer("score");
            table.integer("ref");
            table
                .foreign(&["parent_id"], Some(&foreign))
                .references("id")
                .on(&parent);
            table.index(&["score"], Some("search")).online();
        })
        .await
        .unwrap();
    let keys = schema.get_foreign_keys(&child).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].name.as_deref(), Some(foreign.as_str()));
    assert_eq!(keys[0].columns, ["parent_id"]);
    assert_eq!(keys[0].foreign_schema.as_deref(), Some("dbo"));
    assert_eq!(keys[0].foreign_columns, [Some("id".into())]);
    assert_eq!(keys[0].on_delete, "no action");
    schema
        .table(&child, |table| {
            table.unique(&["ref"], Some("unique_ref")).online();
            table.drop_foreign(&foreign);
        })
        .await
        .unwrap();

    let indexes = schema.get_indexes(&child).await.unwrap();
    assert!(indexes.iter().any(|index| index.name == "search"
        && !index.unique
        && index.columns == [Some("score".into())]));
    assert!(
        indexes
            .iter()
            .any(|index| index.name == "unique_ref" && index.unique)
    );
    assert!(schema.get_foreign_keys(&child).await.unwrap().is_empty());
    schema.drop_if_exists(&parent).await.unwrap();
    schema.drop_if_exists(&child).await.unwrap();
}

#[tokio::test]
async fn sql_server_column_changes_and_drops_handle_dependent_constraints() {
    let root = tempfile::tempdir().unwrap();
    let preview = ConnectionFactory
        .make(
            "sqlsrv",
            json!({"driver":"sqlsrv","prefix":"test_"}),
            root.path(),
        )
        .unwrap();
    let statements = preview
        .pretend(|db| {
            Box::pin(async move {
                db.get_schema_builder()
                    .table("dbo.entries", |table| {
                        table
                            .string("label]'", Some(20))
                            .nullable()
                            .default("replacement")
                            .change();
                        table.drop_column(&["score", "amount"]);
                    })
                    .await
            })
        })
        .await
        .unwrap();
    assert_eq!(statements.len(), 5);
    assert!(
        statements[0]
            .query
            .contains("object_id(N'[dbo].[test_entries]')")
    );
    assert!(statements[0].query.contains("c.name in (N'label]''')"));
    assert!(statements[0].query.contains("quotename(d.name)"));
    assert!(!statements[0].query.contains("sys.check_constraints"));
    assert_eq!(
        statements[1].query,
        "alter table [dbo].[test_entries] alter column [label]]'] nvarchar(20) null"
    );
    assert!(statements[2].query.contains("sys.check_constraints"));
    assert_eq!(
        statements[3].query,
        "alter table [dbo].[test_entries] drop column [score], [amount]"
    );
    assert_eq!(
        statements[4].query,
        "alter table [dbo].[test_entries] add default N'replacement' for [label]]']"
    );

    let Ok(url) = std::env::var("OPENRUST_TEST_SQL_SERVER_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
        return;
    };
    let db = ConnectionFactory
        .make("sqlsrv", json!({"url":url}), root.path())
        .unwrap();
    db.unprepared("create schema changes").await.unwrap();
    db.statement("create table changes.entries (label nvarchar(12) constraint [Default]]Label] default N'long value' not null, score int constraint [Default]]Score] default 4 constraint [Check]]Score] check (score >= 0), amount int default 5, keeper int constraint keep_default default 6, constraint keeper_check check (keeper >= 0))", &[]).await.unwrap();
    db.statement("insert into changes.entries default values", &[])
        .await
        .unwrap();
    let mut schema = db.get_schema_builder();
    let result = schema
        .table("changes.entries", |table| {
            table.string("label", Some(2)).nullable().change();
        })
        .await;
    assert!(result.is_err());
    let column = schema
        .get_column("changes.entries", "label")
        .await
        .unwrap()
        .unwrap();
    assert!(!column.nullable);
    assert!(column.default.is_some());
    assert_eq!(
        db.scalar("select label from changes.entries", &[])
            .await
            .unwrap(),
        "long value".into()
    );

    schema
        .table("changes.entries", |table| {
            table
                .string("label", Some(20))
                .nullable()
                .default("replacement")
                .change();
            table.drop_column(&["score", "amount"]);
        })
        .await
        .unwrap();

    let columns = schema.get_columns("changes.entries").await.unwrap();
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["label", "keeper"]
    );
    assert!(columns[0].nullable);
    assert!(columns[0].default.is_some());
    assert!(columns[1].default.is_some());
    assert_eq!(db.scalar("select count(*) from sys.check_constraints where parent_object_id = object_id('changes.entries') and name = 'keeper_check'", &[]).await.unwrap(), 1.into());
    assert_eq!(
        db.scalar("select label from changes.entries", &[])
            .await
            .unwrap(),
        "long value".into()
    );
    db.statement("insert into changes.entries default values", &[])
        .await
        .unwrap();
    assert_eq!(
        db.scalar(
            "select count(*) from changes.entries where label = N'replacement'",
            &[]
        )
        .await
        .unwrap(),
        1.into()
    );
    schema
        .table("changes.entries", |table| {
            table.string("label", Some(20)).nullable().change();
        })
        .await
        .unwrap();
    db.statement("insert into changes.entries default values", &[])
        .await
        .unwrap();
    assert_eq!(
        db.scalar(
            "select count(*) from changes.entries where label is null and keeper = 6",
            &[]
        )
        .await
        .unwrap(),
        1.into()
    );
    schema.drop("changes.entries").await.unwrap();
    db.statement("drop schema changes", &[]).await.unwrap();
}

#[tokio::test]
async fn sql_server_computed_columns_switch_persistence_in_place() {
    use openrust::database::schema::GenerationType;

    let root = tempfile::tempdir().unwrap();
    let preview = ConnectionFactory
        .make(
            "sqlsrv",
            json!({"driver":"sqlsrv","prefix":"test_"}),
            root.path(),
        )
        .unwrap();
    let statements = preview
        .pretend(|db| {
            Box::pin(async move {
                let mut schema = db.get_schema_builder();
                schema
                    .create("dbo.computed_totals", |table| {
                        table.integer("quantity");
                        table.computed("twice", "quantity * 2").persisted();
                    })
                    .await?;
                schema
                    .table("dbo.computed_totals", |table| {
                        table.computed("twice", "quantity * 2").persisted().change();
                        table.computed("twice", "quantity * 2").change();
                    })
                    .await
            })
        })
        .await
        .unwrap();
    assert!(
        statements[0]
            .query
            .contains("[twice] as (quantity * 2) persisted")
    );
    assert_eq!(
        statements[2].query,
        "alter table [dbo].[test_computed_totals] alter column [twice] add persisted"
    );
    assert_eq!(
        statements[4].query,
        "alter table [dbo].[test_computed_totals] alter column [twice] drop persisted"
    );

    let Ok(url) = std::env::var("OPENRUST_TEST_SQL_SERVER_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
        return;
    };
    let db = ConnectionFactory
        .make("sqlsrv", json!({"url":url}), root.path())
        .unwrap();
    let mut schema = db.get_schema_builder();
    let table = format!("dbo.computed_totals_{:016x}", fastrand::u64(..));
    schema
        .create(&table, |table| {
            table.integer("quantity");
            table.computed("twice", "quantity * 2");
            table.computed("wall_time", "getdate()");
        })
        .await
        .unwrap();
    db.statement(&format!("insert into {table} (quantity) values (3)"), &[])
        .await
        .unwrap();
    assert!(
        schema
            .table(&table, |table| {
                table.computed("twice", "quantity * 2").persisted().change();
                table
                    .computed("wall_time", "getdate()")
                    .persisted()
                    .change();
            })
            .await
            .is_err()
    );
    let column = schema.get_column(&table, "twice").await.unwrap().unwrap();
    assert_eq!(
        column.generation.as_ref().unwrap().kind,
        GenerationType::Virtual
    );

    schema
        .table(&table, |table| {
            table.computed("twice", "quantity * 2").persisted().change();
        })
        .await
        .unwrap();
    let persisted = schema.get_column(&table, "twice").await.unwrap().unwrap();
    assert_eq!(
        persisted.generation.as_ref().unwrap().kind,
        GenerationType::Stored
    );
    assert_eq!(
        db.scalar(&format!("select twice from {table}"), &[])
            .await
            .unwrap(),
        6.into()
    );
    db.statement(&format!("update {table} set quantity = 4"), &[])
        .await
        .unwrap();
    assert_eq!(
        db.scalar(&format!("select twice from {table}"), &[])
            .await
            .unwrap(),
        8.into()
    );

    schema
        .table(&table, |table| {
            table.computed("twice", "quantity * 2").change();
        })
        .await
        .unwrap();
    let computed = schema.get_column(&table, "twice").await.unwrap().unwrap();
    assert_eq!(
        computed.generation.as_ref().unwrap().kind,
        GenerationType::Virtual
    );
    assert_eq!(
        computed.generation.unwrap().expression,
        persisted.generation.unwrap().expression
    );
    db.statement(&format!("update {table} set quantity = 5"), &[])
        .await
        .unwrap();
    assert_eq!(
        db.scalar(&format!("select twice from {table}"), &[])
            .await
            .unwrap(),
        10.into()
    );
    schema.drop(&table).await.unwrap();
}

#[tokio::test]
async fn sql_server_renames_tables_columns_and_indexes_without_losing_data() {
    let root = tempfile::tempdir().unwrap();
    let preview = ConnectionFactory
        .make(
            "sqlsrv",
            json!({"driver":"sqlsrv","prefix":"app_"}),
            root.path(),
        )
        .unwrap();
    let statements = preview
        .pretend(|db| {
            Box::pin(async move {
                let mut schema = db.get_schema_builder();
                schema
                    .table("tenant.entries", |table| {
                        table.rename_column("old]'", "new]'");
                        table.rename_index("ix_old", "ix_new");
                    })
                    .await?;
                schema.rename("tenant.entries", "tenant.archive").await
            })
        })
        .await
        .unwrap();
    assert_eq!(
        statements
            .iter()
            .map(|entry| entry.query.as_str())
            .collect::<Vec<_>>(),
        [
            "exec sys.sp_rename N'[tenant].[app_entries].[old]]'']', N'new]''', N'COLUMN'",
            "exec sys.sp_rename N'[tenant].[app_entries].[ix_old]', N'ix_new', N'INDEX'",
            "exec sys.sp_rename N'[tenant].[app_entries]', N'app_archive', N'OBJECT'",
        ]
    );
    assert!(
        preview
            .get_schema_builder()
            .rename("tenant.entries", "other.archive")
            .await
            .is_err()
    );
    let long_name = "x".repeat(129);
    assert!(
        preview
            .get_schema_builder()
            .table("tenant.entries", |table| {
                table.rename_index("ix_old", &long_name);
            })
            .await
            .is_err()
    );

    let Ok(url) = std::env::var("OPENRUST_TEST_SQL_SERVER_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
        return;
    };
    let db = ConnectionFactory
        .make("sqlsrv", json!({"url":url,"prefix":"app_"}), root.path())
        .unwrap();
    let suffix = fastrand::u64(..);
    let from = format!("dbo.rename_from_{suffix:016x}");
    let to = format!("dbo.rename_to_]'新_{suffix:016x}");
    let mut schema = db.get_schema_builder();
    schema
        .create(&from, |table| {
            table.integer("score");
            table.index(&["score"], Some("ix_score"));
        })
        .await
        .unwrap();
    db.table(&from)
        .insert(&[Record::from([("score".into(), 7.into())])])
        .await
        .unwrap();
    assert!(
        schema
            .table(&from, |table| {
                table.rename_column("score", "renamed");
                table.rename_column("missing", "unused");
            })
            .await
            .is_err()
    );
    assert!(schema.has_column(&from, "score").await.unwrap());
    assert!(!schema.has_column(&from, "renamed").await.unwrap());
    schema
        .table(&from, |table| {
            table.rename_column("score", "value]'新");
            table.rename_index("ix_score", "ix]'新");
        })
        .await
        .unwrap();
    schema.rename(&from, &to).await.unwrap();
    assert!(!schema.has_table(&from).await.unwrap());
    assert!(schema.has_table(&to).await.unwrap());
    assert!(schema.has_column(&to, "value]'新").await.unwrap());
    assert!(schema.has_index(&to, "ix]'新").await.unwrap());
    assert_eq!(db.table(&to).value("value]'新").await.unwrap(), 7.into());
    schema
        .table(&to, |table| {
            table.rename_column("value]'新", "score");
            table.rename_index("ix]'新", "ix_score");
        })
        .await
        .unwrap();
    schema.rename(&to, &from).await.unwrap();
    assert_eq!(db.table(&from).value("score").await.unwrap(), 7.into());
    schema.drop(&from).await.unwrap();
}

#[tokio::test]
async fn sql_server_creates_and_drops_named_and_discovered_primary_keys() {
    let root = tempfile::tempdir().unwrap();
    let preview = ConnectionFactory
        .make(
            "sqlsrv",
            json!({"driver":"sqlsrv","prefix":"app_","prefix_indexes":true}),
            root.path(),
        )
        .unwrap();
    let statements = preview
        .pretend(|db| {
            Box::pin(async move {
                db.get_schema_builder()
                    .table("dbo.entries", |table| {
                        table.primary(&["id"], Some("pk]entries"));
                        table.drop_primary_named("pk]entries");
                        table.drop_primary_columns(&["id", "tenant_id"]);
                        table.drop_primary();
                    })
                    .await
            })
        })
        .await
        .unwrap();
    assert_eq!(
        statements[0].query,
        "alter table [dbo].[app_entries] add constraint [pk]]entries] primary key ([id])"
    );
    assert_eq!(
        statements[1].query,
        "alter table [dbo].[app_entries] drop constraint [pk]]entries]"
    );
    assert_eq!(
        statements[2].query,
        "alter table [dbo].[app_entries] drop constraint [dbo_app_entries_id_tenant_id_primary]"
    );
    assert!(
        statements[3]
            .query
            .contains("object_id(N'[dbo].[app_entries]') and type = 'PK'")
    );

    let Ok(url) = std::env::var("OPENRUST_TEST_SQL_SERVER_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
        return;
    };
    let db = ConnectionFactory
        .make(
            "sqlsrv",
            json!({"url":url,"prefix":"app_","prefix_indexes":true}),
            root.path(),
        )
        .unwrap();
    let suffix = fastrand::u64(..);
    let parent_schema = format!("pk_test_{suffix:016x}");
    db.unprepared(&format!("create schema {parent_schema}"))
        .await
        .unwrap();
    let parent = format!("{parent_schema}.primary_parent");
    let child = format!("dbo.primary_child_{suffix:016x}");
    let key = format!("primary_]'父_{suffix:016x}");
    let foreign = format!("foreign_{suffix:016x}");
    let mut schema = db.get_schema_builder();
    schema
        .create(&parent, |table| {
            table.integer("id");
            table.integer("tenant_id");
            table.primary(&["id"], Some(&key));
        })
        .await
        .unwrap();
    schema
        .create(&child, |table| {
            table.integer("parent_id");
            table
                .foreign(&["parent_id"], Some(&foreign))
                .references("id")
                .on(&parent);
        })
        .await
        .unwrap();
    db.table(&parent)
        .insert(&[Record::from([
            ("id".into(), 1.into()),
            ("tenant_id".into(), 3.into()),
        ])])
        .await
        .unwrap();
    assert!(
        schema
            .table(&parent, |table| table.drop_primary_named(&key))
            .await
            .is_err()
    );
    assert!(schema.has_index(&parent, &key).await.unwrap());
    schema
        .table(&child, |table| table.drop_foreign(&foreign))
        .await
        .unwrap();
    schema
        .table(&parent, |table| table.drop_primary())
        .await
        .unwrap();
    assert!(!schema.has_index(&parent, &key).await.unwrap());
    assert!(
        schema
            .table(&parent, |table| table.drop_primary())
            .await
            .is_err()
    );
    schema
        .table(&parent, |table| {
            table.primary(&["tenant_id", "id"], None);
        })
        .await
        .unwrap();
    let index = schema
        .get_indexes(&parent)
        .await
        .unwrap()
        .into_iter()
        .find(|index| index.primary)
        .unwrap();
    assert_eq!(index.columns, [Some("tenant_id".into()), Some("id".into())]);
    schema
        .table(&parent, |table| {
            table.drop_primary_columns(&["tenant_id", "id"])
        })
        .await
        .unwrap();
    assert_eq!(
        db.table(&parent).value("tenant_id").await.unwrap(),
        3.into()
    );
    assert!(
        !schema
            .get_indexes(&parent)
            .await
            .unwrap()
            .iter()
            .any(|index| index.primary)
    );
    schema.drop(&child).await.unwrap();
    schema.drop(&parent).await.unwrap();
    db.statement(&format!("drop schema {parent_schema}"), &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn sql_server_keeps_original_error_when_server_already_rolled_back() {
    use openrust::database::DatabaseError;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let Ok(url) = std::env::var("OPENRUST_TEST_SQL_SERVER_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
        return;
    };
    let root = tempfile::tempdir().unwrap();
    let db = ConnectionFactory
        .make(
            "sqlsrv",
            json!({"url":url,"max_connections":1}),
            root.path(),
        )
        .unwrap();
    let table = "dbo.automatic_rollback_probe";
    db.get_schema_builder()
        .create(table, |table| {
            table.integer("id");
        })
        .await
        .unwrap();
    let callbacks = Arc::new(AtomicUsize::new(0));
    let mut tx = db.begin_transaction().await.unwrap();
    let outer = callbacks.clone();
    tx.after_roll_back(move || {
        Box::pin(async move {
            outer.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    })
    .unwrap();
    tx.statement("insert into dbo.automatic_rollback_probe values (1)", &[])
        .await
        .unwrap();
    let inner = callbacks.clone();
    let failure = tx
        .transaction(move |tx| {
            Box::pin(async move {
                tx.after_roll_back(move || {
                    Box::pin(async move {
                        inner.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                })?;
                tx.statement(
                    "rollback transaction; throw 51017, 'original SQL Server error', 1",
                    &[],
                )
                .await
            })
        })
        .await
        .unwrap_err();
    assert!(
        matches!(failure, DatabaseError::SqlServer { source: tiberius::error::Error::Server(error), .. } if error.code() == 51017)
    );
    assert_eq!(tx.transaction_level(), 0);
    assert_eq!(callbacks.load(Ordering::SeqCst), 2);
    assert!(matches!(
        tx.statement("insert into dbo.automatic_rollback_probe values (2)", &[])
            .await,
        Err(DatabaseError::TransactionClosed)
    ));
    assert_eq!(db.table(table).count("*").await.unwrap(), 0);

    let failure = db
        .transaction(
            |tx| {
                Box::pin(async move {
                    tx.get_schema_builder()
                        .table(table, |table| {
                            table
                                .foreign(&["id"], Some("rollback_probe_fk"))
                                .references("id")
                                .on("dbo.nonexistent_rollback_target");
                        })
                        .await
                })
            },
            1,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(failure, DatabaseError::SqlServer { source: tiberius::error::Error::Server(error), .. } if error.code() == 1767)
    );
    assert_eq!(
        db.scalar("select xact_state()", &[]).await.unwrap(),
        0.into()
    );
    db.transaction(
        |tx| {
            Box::pin(async move {
                tx.statement("insert into dbo.automatic_rollback_probe values (3)", &[])
                    .await?;
                Ok(())
            })
        },
        1,
    )
    .await
    .unwrap();
    assert_eq!(db.table(table).value("id").await.unwrap(), 3.into());
    db.get_schema_builder().drop(table).await.unwrap();
}
