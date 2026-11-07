//! 真实MySQL隔离库的四表结构与迁移回滚；无URL时明确跳过，不视为实库通过。

use super::super::{TABLES, default_passport_migrations};
use openrust::{
    database::{
        DatabaseTableNames, DbBackend,
        sea_orm::{ConnectionTrait, DatabaseConnection, QueryResult, Statement},
        sea_orm_migration::prelude::*,
    },
    testing::TestDatabase,
};
use std::collections::BTreeMap;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PassportSchemaMigrator;
impl MigratorTrait for PassportSchemaMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        default_passport_migrations()
    }
}

#[tokio::test]
async fn real_mysql_official_four_tables_conflict_and_reverse_rollback() -> TestResult {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!(
            "未执行Passport四表MySQL验收：缺少OPENRUST_TEST_DATABASE_URL，此跳过不算实库通过"
        );
        return Ok(());
    };
    // 不连接其他后端或业务库；TestDatabase将另建并持有自己隔离库的所有权。
    if !url.starts_with("mysql://") {
        eprintln!("未执行Passport四表MySQL验收：测试URL不是MySQL，此跳过不算实库通过");
        return Ok(());
    }
    let names = DatabaseTableNames::new(
        "p01_",
        BTreeMap::from([("oauth_clients".into(), "selected_clients".into())]),
    )?;
    let database =
        TestDatabase::create_with_table_names::<PassportSchemaMigrator>(&url, names.clone())
            .await?;
    // exercise中的SchemaManager及借用pool在返回/错误时先drop，再由close移交唯一清理责任。
    let result = Box::pin(exercise(&database, &names)).await;
    let cleanup = database.close().await;
    result?;
    cleanup?;
    Ok(())
}

async fn exercise(database: &TestDatabase, names: &DatabaseTableNames) -> TestResult {
    let connection = database.connection()?;
    require(
        connection.get_database_backend() == DbBackend::MySql,
        "隔离测试连接不是MySQL",
    )?;
    let manager = SchemaManager::new(&connection);
    // create_with_table_names已在同一names.scope内运行真实M::up，不另写夹具建表SQL。
    for (position, logical) in TABLES.iter().enumerate() {
        let physical = names.resolve(logical)?;
        require(
            manager.has_table(physical.as_str()).await?,
            "迁移未创建解析后的物理表",
        )?;
        require(
            !manager.has_table(*logical).await?,
            "迁移绕过了prefix或全名mapping",
        )?;
        Box::pin(assert_columns(&connection, position, &physical)).await?;
        Box::pin(assert_indexes_and_no_foreign_keys(
            &connection,
            position,
            &physical,
        ))
        .await?;
    }
    require(
        PassportSchemaMigrator::get_applied_migrations(&connection)
            .await?
            .len()
            == 4,
        "真实迁移ledger未记录四表",
    )?;
    for migration in default_passport_migrations() {
        // 直接up绕过migration ledger；表已存在必须报错，不能由if_not_exists吞掉。
        require(
            names.scope(migration.up(&manager)).await.is_err(),
            "直接重复建表被静默忽略",
        )?;
    }
    Box::pin(assert_reverse_rollback(&connection, names)).await?;
    // manager与connection只借用本TestDatabase的池，不在这里关闭共享pool。
    Ok(())
}

async fn rows(
    connection: &DatabaseConnection,
    sql: &str,
    table: &str,
) -> TestResult<Vec<QueryResult>> {
    Ok(connection
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::MySql,
            sql,
            [table.to_owned().into()],
        ))
        .await?)
}

#[derive(Debug, PartialEq, Eq)]
struct Column {
    name: String,
    kind: String,
    nullable: String,
    default: Option<String>,
    extra: String,
}

fn column(row: &QueryResult) -> Result<Column, DbErr> {
    Ok(Column {
        name: row.try_get("", "column_name")?,
        kind: row.try_get("", "column_type")?,
        nullable: row.try_get("", "is_nullable")?,
        default: row.try_get("", "column_default")?,
        extra: row.try_get("", "extra")?,
    })
}

async fn assert_columns(
    connection: &DatabaseConnection,
    position: usize,
    table: &str,
) -> TestResult {
    let actual = rows(connection, "SELECT COLUMN_NAME AS column_name, COLUMN_TYPE AS column_type, IS_NULLABLE AS is_nullable, COLUMN_DEFAULT AS column_default, EXTRA AS extra FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION", table).await?;
    let actual = actual
        .iter()
        .map(column)
        .collect::<Result<Vec<_>, DbErr>>()?;
    let expected: Vec<Column> = column_contract(position)?
        .into_iter()
        .map(|(name, column_type, nullable)| Column {
            name: name.into(),
            kind: column_type.into(),
            nullable: if nullable { "YES" } else { "NO" }.into(),
            default: None,
            extra: String::new(),
        })
        .collect();
    require(
        actual == expected,
        &format!("MySQL实际列定义与官方四表契约不符：{table}"),
    )?;
    Ok(())
}

fn column_contract(position: usize) -> TestResult<Vec<(&'static str, &'static str, bool)>> {
    let columns = match position {
        0 => vec![
            ("id", "char(80)", false),
            ("user_id", "bigint unsigned", false),
            ("client_id", "char(36)", false),
            ("scopes", "text", true),
            ("revoked", "tinyint(1)", false),
            ("expires_at", "datetime", true),
        ],
        1 => vec![
            ("id", "char(80)", false),
            ("user_id", "bigint unsigned", true),
            ("client_id", "char(36)", false),
            ("name", "varchar(255)", true),
            ("scopes", "text", true),
            ("revoked", "tinyint(1)", false),
            ("created_at", "timestamp", true),
            ("updated_at", "timestamp", true),
            ("expires_at", "datetime", true),
        ],
        2 => vec![
            ("id", "char(80)", false),
            ("access_token_id", "char(80)", false),
            ("revoked", "tinyint(1)", false),
            ("expires_at", "datetime", true),
        ],
        3 => vec![
            ("id", "char(36)", false),
            ("owner_type", "varchar(255)", true),
            ("owner_id", "bigint unsigned", true),
            ("name", "varchar(255)", false),
            ("secret", "varchar(255)", true),
            ("provider", "varchar(255)", true),
            ("redirect_uris", "text", false),
            ("grant_types", "text", false),
            ("revoked", "tinyint(1)", false),
            ("created_at", "timestamp", true),
            ("updated_at", "timestamp", true),
        ],
        _ => return Err(invalid_contract().into()),
    };
    Ok(columns)
}

#[derive(Debug, PartialEq, Eq)]
struct IndexColumn {
    name: String,
    non_unique: i64,
    sequence: i64,
    column: String,
}

async fn assert_indexes_and_no_foreign_keys(
    connection: &DatabaseConnection,
    position: usize,
    table: &str,
) -> TestResult {
    let data = rows(connection, "SELECT INDEX_NAME AS index_name, CAST(NON_UNIQUE AS SIGNED) AS non_unique, CAST(SEQ_IN_INDEX AS SIGNED) AS sequence_number, COLUMN_NAME AS column_name FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? ORDER BY INDEX_NAME, SEQ_IN_INDEX", table).await?;
    let mut actual = data
        .iter()
        .map(|row| {
            Ok(IndexColumn {
                name: row.try_get("", "index_name")?,
                non_unique: row.try_get("", "non_unique")?,
                sequence: row.try_get("", "sequence_number")?,
                column: row.try_get("", "column_name")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;
    actual.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.sequence.cmp(&right.sequence))
    });
    let columns: &[&str] = match position {
        0 | 1 => &["user_id"],
        2 => &["access_token_id"],
        3 => &["owner_type", "owner_id"],
        _ => return Err(invalid_contract().into()),
    };
    // 固定本fixture命名的独立预期，不调用被测索引命名器生成断言。
    let name = match position {
        0 => "p01_oauth_auth_codes_user_id_index",
        1 => "p01_oauth_access_tokens_user_id_index",
        2 => "p01_oauth_refresh_tokens_access_token_id_index",
        3 => "selected_clients_owner_type_owner_id_index",
        _ => return Err(invalid_contract().into()),
    };
    let mut expected = vec![IndexColumn {
        name: "PRIMARY".into(),
        non_unique: 0,
        sequence: 1,
        column: "id".into(),
    }];
    for (position, column) in columns.iter().enumerate() {
        expected.push(IndexColumn {
            name: name.to_owned(),
            non_unique: 1,
            sequence: <i64 as TryFrom<usize>>::try_from(position + 1)?,
            column: (*column).into(),
        });
    }
    expected.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.sequence.cmp(&right.sequence))
    });
    require(
        actual == expected,
        &format!("MySQL实际索引含未授权unique/缺列或顺序不符：{table}"),
    )?;
    let foreign = rows(connection, "SELECT CONSTRAINT_NAME AS constraint_name FROM information_schema.TABLE_CONSTRAINTS WHERE CONSTRAINT_SCHEMA = DATABASE() AND TABLE_NAME = ? AND CONSTRAINT_TYPE = 'FOREIGN KEY'", table).await?;
    require(
        foreign.is_empty(),
        &format!("官方四表不应自动添加FK或cascade：{table}"),
    )?;
    Ok(())
}

async fn assert_reverse_rollback(
    connection: &DatabaseConnection,
    names: &DatabaseTableNames,
) -> TestResult {
    let manager = SchemaManager::new(connection);
    for removed in 0..4 {
        names
            .scope(PassportSchemaMigrator::down(connection, Some(1)))
            .await?;
        for (position, logical) in TABLES.iter().enumerate() {
            let exists = manager.has_table(names.resolve(logical)?).await?;
            require(
                exists == (position < 3 - removed),
                "M::down未按clients→refresh→access→authcodes逐项回滚",
            )?;
        }
    }
    require(
        PassportSchemaMigrator::get_applied_migrations(connection)
            .await?
            .is_empty(),
        "回滚后迁移ledger仍有四表记录",
    )?;
    names
        .scope(PassportSchemaMigrator::down(connection, None))
        .await?;
    for migration in default_passport_migrations().into_iter().rev() {
        names.scope(migration.down(&manager)).await?;
        names.scope(migration.down(&manager)).await?;
    }
    for logical in TABLES {
        require(
            !manager.has_table(names.resolve(logical)?).await?,
            "幂等down后仍存在四表",
        )?;
    }
    Ok(())
}

fn invalid_contract() -> openrust::Error {
    openrust::Error::custom("passport_schema_fixture_invalid", "四表结构验收序号无效")
}

// 用Result报告验收失败，让测试仍能在返回前显式close自己的隔离库。
fn require(condition: bool, message: &str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(openrust::Error::custom("passport_schema_assertion_failed", message).into())
    }
}
