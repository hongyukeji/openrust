//! 遵循 OpenRust UTC 与 JSON 约定的 PostgreSQL、MySQL 与 SQLite 默认 schema 类型。

use sea_orm_migration::sea_orm::sea_query::IntoIden;
use sea_orm_migration::sea_orm::{DbBackend, DbErr, sea_query::ColumnDef};

/// 为受支持的数据库后端创建非空 UTC 时间戳列。
///
/// PostgreSQL 使用 `TIMESTAMP WITH TIME ZONE`；MySQL 使用 `DATETIME(6)` 保留微秒精度，避免 `TIMESTAMP`
/// 的 2038 年上限。SQLite 按固定 Laravel 默认使用 `datetime`，UTC 编码由现连接层负责。`DatabaseServiceProvider` 会将每个 MySQL 连接的 session 时区设为 `+00:00`，因此 MySQL 值均为 UTC。
pub fn timestamp_utc(name: impl IntoIden, backend: DbBackend) -> Result<ColumnDef, DbErr> {
    let mut column = ColumnDef::new(name);
    match backend {
        DbBackend::MySql => column.custom("DATETIME(6)"),
        DbBackend::Postgres => column.timestamp_with_time_zone(),
        DbBackend::Sqlite => column.custom("datetime"),
        _ => return Err(unsupported_backend(backend)),
    };
    Ok(column.not_null().take())
}

/// 使用各受支持后端的原生可移植类型创建非空 JSON 列。
///
/// PostgreSQL 使用 `JSONB`；MySQL 使用 `JSON`；SQLite 默认使用 `TEXT`。检查 JSON 值的查询表达式仍与后端有关，也应放在 dialect 边界内。
pub fn json(name: impl IntoIden, backend: DbBackend) -> Result<ColumnDef, DbErr> {
    match backend {
        DbBackend::MySql => Ok(ColumnDef::new(name).json().not_null().take()),
        DbBackend::Postgres => Ok(ColumnDef::new(name).json_binary().not_null().take()),
        DbBackend::Sqlite => Ok(ColumnDef::new(name).text().not_null().take()),
        _ => Err(unsupported_backend(backend)),
    }
}

/// 删除列；SQLite 通过重建表保留最低 3.26 版本，而非要求原生 DROP COLUMN。
///
/// SQLite 必须由标准 `MigrationServiceProvider` 执行，且不能放在用户显式事务中：
/// 外键开关必须作用于同一物理连接、位于重建事务之外。完整 Laravel Schema API 仍待实现。
pub async fn drop_column(
    manager: &sea_orm_migration::SchemaManager<'_>,
    table: impl IntoIden,
    column: impl IntoIden,
) -> Result<(), DbErr> {
    let table = table.into_iden();
    let column = column.into_iden();
    if manager.get_database_backend() == DbBackend::Sqlite {
        #[cfg(feature = "db-sqlite")]
        {
            if !super::migration::sqlite_execution_owner() {
                return Err(DbErr::Migration(
                    "SQLite 表重建需要标准迁移执行所有者".into(),
                ));
            }
            let sea_orm::DatabaseExecutor::Connection(connection) = manager.get_connection() else {
                return Err(DbErr::Migration(
                    "SQLite 表重建不能位于用户显式事务内".into(),
                ));
            };
            return super::sqlite::schema_rebuild::drop_column(
                connection.get_sqlite_connection_pool().clone(),
                table.to_string(),
                column.to_string(),
            )
            .await;
        }
        #[cfg(not(feature = "db-sqlite"))]
        return Err(DbErr::Migration("SQLite 表重建需要 db-sqlite".into()));
    }
    manager
        .alter_table(
            sea_orm::sea_query::Table::alter()
                .table(table)
                .drop_column(column)
                .to_owned(),
        )
        .await
}

fn unsupported_backend(backend: DbBackend) -> DbErr {
    DbErr::Migration(format!(
        "OpenRust UTC and JSON schema helpers support PostgreSQL, MySQL and SQLite, got {backend:?}"
    ))
}

#[cfg(test)]
mod tests {
    use sea_orm_migration::prelude::*;
    use sea_orm_migration::sea_orm::DbBackend;

    use super::{json, timestamp_utc};

    #[derive(Iden)]
    enum Probe {
        Table,
        CreatedAt,
        Payload,
    }

    #[test]
    fn schema_helpers_emit_the_required_backend_types() -> Result<(), DbErr> {
        let postgres = Table::create()
            .table(Probe::Table)
            .col(timestamp_utc(Probe::CreatedAt, DbBackend::Postgres)?)
            .col(json(Probe::Payload, DbBackend::Postgres)?)
            .to_string(PostgresQueryBuilder);
        let postgres = postgres.to_ascii_uppercase();
        assert!(postgres.contains("TIMESTAMP WITH TIME ZONE"));
        assert!(postgres.contains("JSONB"));

        let mysql = Table::create()
            .table(Probe::Table)
            .col(timestamp_utc(Probe::CreatedAt, DbBackend::MySql)?)
            .col(json(Probe::Payload, DbBackend::MySql)?)
            .to_string(MysqlQueryBuilder);
        let mysql = mysql.to_ascii_uppercase();
        assert!(mysql.contains("DATETIME(6)"));
        assert!(mysql.contains("JSON"));
        Ok(())
    }

    #[test]
    fn sqlite_default_schema_types_match_fixed_laravel_grammar() -> Result<(), DbErr> {
        let sqlite = Table::create()
            .table(Probe::Table)
            .col(timestamp_utc(Probe::CreatedAt, DbBackend::Sqlite)?)
            .col(json(Probe::Payload, DbBackend::Sqlite)?)
            .to_string(SqliteQueryBuilder)
            .to_ascii_uppercase();
        assert!(sqlite.contains("DATETIME NOT NULL"));
        assert!(sqlite.contains("TEXT NOT NULL"));
        assert!(!sqlite.contains("TIMESTAMP WITH TIME ZONE"));
        assert!(!sqlite.contains("JSONB"));
        Ok(())
    }
}
