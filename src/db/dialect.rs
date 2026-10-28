//! 遵循 OpenRust UTC 与 JSON 约定的 PostgreSQL 和 MySQL schema 类型。

use sea_orm_migration::sea_orm::sea_query::IntoIden;
use sea_orm_migration::sea_orm::{DbBackend, DbErr, sea_query::ColumnDef};

/// 为受支持的数据库后端创建非空 UTC 时间戳列。
///
/// PostgreSQL 使用 `TIMESTAMP WITH TIME ZONE`；MySQL 使用 `DATETIME(6)` 保留微秒精度，避免 `TIMESTAMP`
/// 的 2038 年上限。`DatabaseServiceProvider` 会将每个 MySQL 连接的 session 时区设为 `+00:00`，因此 MySQL 值均为 UTC。
pub fn timestamp_utc(name: impl IntoIden, backend: DbBackend) -> Result<ColumnDef, DbErr> {
    let mut column = ColumnDef::new(name);
    match backend {
        DbBackend::MySql => column.custom("DATETIME(6)"),
        DbBackend::Postgres => column.timestamp_with_time_zone(),
        _ => return Err(unsupported_backend(backend)),
    };
    Ok(column.not_null().take())
}

/// 使用各受支持后端的原生可移植类型创建非空 JSON 列。
///
/// PostgreSQL 使用 `JSONB`；MySQL 使用 `JSON`。检查 JSON 值的查询表达式仍与后端有关，也应放在 dialect 边界内。
pub fn json(name: impl IntoIden, backend: DbBackend) -> Result<ColumnDef, DbErr> {
    match backend {
        DbBackend::MySql => Ok(ColumnDef::new(name).json().not_null().take()),
        DbBackend::Postgres => Ok(ColumnDef::new(name).json_binary().not_null().take()),
        _ => Err(unsupported_backend(backend)),
    }
}

fn unsupported_backend(backend: DbBackend) -> DbErr {
    DbErr::Migration(format!(
        "OpenRust UTC and JSON schema helpers support PostgreSQL and MySQL, got {backend:?}"
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
    fn schema_helpers_reject_unimplemented_backends() {
        assert!(timestamp_utc(Probe::CreatedAt, DbBackend::Sqlite).is_err());
        assert!(json(Probe::Payload, DbBackend::Sqlite).is_err());
    }
}
