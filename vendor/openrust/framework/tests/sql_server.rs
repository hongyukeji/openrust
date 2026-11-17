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
    let rows = db.selectResultSets(
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
    let mut schema = db.getSchemaBuilder();
    schema
        .create(&table, |table| {
            table.id();
            table.string("name", 64).unique();
            table.integer("score");
            table.binary("blob").nullable();
        })
        .await
        .unwrap();
    let columns = schema.getColumns(&table).await.unwrap();
    assert!(
        columns
            .iter()
            .any(|column| column.name == "id" && column.auto_increment && column.primary)
    );
    assert!(
        schema
            .getSchemas()
            .await
            .unwrap()
            .iter()
            .any(|schema| schema.name == "dbo" && schema.default)
    );
    assert!(
        schema
            .getTables(Some("dbo"))
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
            .insertGetId(record("first", 1))
            .await
            .unwrap(),
        1.into()
    );
    let mut transaction = db.beginTransaction().await.unwrap();
    transaction
        .table(&table)
        .insert(&[record("kept", 2)])
        .await
        .unwrap();
    transaction.beginTransaction().await.unwrap();
    transaction
        .table(&table)
        .insert(&[record("rolled back", 3)])
        .await
        .unwrap();
    transaction.rollBack(None).await.unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(db.table(&table).count("*").await.unwrap(), 2);
    db.table(&table)
        .upsert(&[record("first", 5)], &["name"], Some(&["score"]))
        .await
        .unwrap();
    let rows = db
        .table(&table)
        .orderByDesc("score")
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
    schema.dropIfExists(&table).await.unwrap();
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
                db.getSchemaBuilder()
                    .table("dbo.entries", |table| {
                        table.index(&["score"], Some("search")).online();
                        table.unique(&["ref"], Some("unique_ref")).online();
                        table.dropForeign("entry]parent");
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
    let mut schema = db.getSchemaBuilder();
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
    let keys = schema.getForeignKeys(&child).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].name.as_deref(), Some(foreign.as_str()));
    assert_eq!(keys[0].columns, ["parent_id"]);
    assert_eq!(keys[0].foreign_schema.as_deref(), Some("dbo"));
    assert_eq!(keys[0].foreign_columns, [Some("id".into())]);
    assert_eq!(keys[0].on_delete, "no action");
    schema
        .table(&child, |table| {
            table.unique(&["ref"], Some("unique_ref")).online();
            table.dropForeign(&foreign);
        })
        .await
        .unwrap();

    let indexes = schema.getIndexes(&child).await.unwrap();
    assert!(indexes.iter().any(|index| index.name == "search"
        && !index.unique
        && index.columns == [Some("score".into())]));
    assert!(
        indexes
            .iter()
            .any(|index| index.name == "unique_ref" && index.unique)
    );
    assert!(schema.getForeignKeys(&child).await.unwrap().is_empty());
    schema.dropIfExists(&parent).await.unwrap();
    schema.dropIfExists(&child).await.unwrap();
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
                db.getSchemaBuilder()
                    .table("dbo.entries", |table| {
                        table
                            .string("label]'", Some(20))
                            .nullable()
                            .default("ignored")
                            .change();
                        table.dropColumn(&["score", "amount"]);
                    })
                    .await
            })
        })
        .await
        .unwrap();
    assert_eq!(statements.len(), 4);
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
    let mut schema = db.getSchemaBuilder();
    let result = schema
        .table("changes.entries", |table| {
            table.string("label", Some(2)).nullable().change();
        })
        .await;
    assert!(result.is_err());
    let column = schema
        .getColumn("changes.entries", "label")
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
                .default("ignored")
                .change();
            table.dropColumn(&["score", "amount"]);
        })
        .await
        .unwrap();

    let columns = schema.getColumns("changes.entries").await.unwrap();
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["label", "keeper"]
    );
    assert!(columns[0].nullable);
    assert!(columns[0].default.is_none());
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
                let mut schema = db.getSchemaBuilder();
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
    let mut schema = db.getSchemaBuilder();
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
    let column = schema.getColumn(&table, "twice").await.unwrap().unwrap();
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
    let persisted = schema.getColumn(&table, "twice").await.unwrap().unwrap();
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
    let computed = schema.getColumn(&table, "twice").await.unwrap().unwrap();
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
                let mut schema = db.getSchemaBuilder();
                schema
                    .table("tenant.entries", |table| {
                        table.renameColumn("old]'", "new]'");
                        table.renameIndex("ix_old", "ix_new");
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
            .getSchemaBuilder()
            .rename("tenant.entries", "other.archive")
            .await
            .is_err()
    );
    let long_name = "x".repeat(129);
    assert!(
        preview
            .getSchemaBuilder()
            .table("tenant.entries", |table| {
                table.renameIndex("ix_old", &long_name);
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
    let mut schema = db.getSchemaBuilder();
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
                table.renameColumn("score", "renamed");
                table.renameColumn("missing", "unused");
            })
            .await
            .is_err()
    );
    assert!(schema.hasColumn(&from, "score").await.unwrap());
    assert!(!schema.hasColumn(&from, "renamed").await.unwrap());
    schema
        .table(&from, |table| {
            table.renameColumn("score", "value]'新");
            table.renameIndex("ix_score", "ix]'新");
        })
        .await
        .unwrap();
    schema.rename(&from, &to).await.unwrap();
    assert!(!schema.hasTable(&from).await.unwrap());
    assert!(schema.hasTable(&to).await.unwrap());
    assert!(schema.hasColumn(&to, "value]'新").await.unwrap());
    assert!(schema.hasIndex(&to, "ix]'新").await.unwrap());
    assert_eq!(db.table(&to).value("value]'新").await.unwrap(), 7.into());
    schema
        .table(&to, |table| {
            table.renameColumn("value]'新", "score");
            table.renameIndex("ix]'新", "ix_score");
        })
        .await
        .unwrap();
    schema.rename(&to, &from).await.unwrap();
    assert_eq!(db.table(&from).value("score").await.unwrap(), 7.into());
    schema.drop(&from).await.unwrap();
}
