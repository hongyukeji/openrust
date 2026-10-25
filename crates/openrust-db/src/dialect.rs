//! PostgreSQL and MySQL schema types with OpenRust's UTC and JSON conventions.

use sea_orm_migration::sea_orm::sea_query::IntoIden;
use sea_orm_migration::sea_orm::{DbBackend, sea_query::ColumnDef};

/// Creates a non-null UTC timestamp column for a supported database backend.
///
/// PostgreSQL uses `TIMESTAMP WITH TIME ZONE`; MySQL uses `DATETIME(6)` to retain microseconds
/// without the `TIMESTAMP` year-2038 limit. MySQL values are UTC because `DatabasePlugin` sets each
/// connection's session time zone to `+00:00`.
pub fn timestamp_utc(name: impl IntoIden, backend: DbBackend) -> ColumnDef {
    let mut column = ColumnDef::new(name);
    match backend {
        DbBackend::MySql => column.custom("DATETIME(6)"),
        _ => column.timestamp_with_time_zone(),
    };
    column.not_null().take()
}

/// Creates a non-null JSON column using the native portable type for each supported backend.
///
/// PostgreSQL uses `JSONB`; MySQL uses `JSON`. Query expressions that inspect JSON values remain
/// backend-specific and belong in the dialect boundary too.
pub fn json(name: impl IntoIden, backend: DbBackend) -> ColumnDef {
    match backend {
        DbBackend::MySql => ColumnDef::new(name).json().not_null().take(),
        _ => ColumnDef::new(name).json_binary().not_null().take(),
    }
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
    fn schema_helpers_emit_the_required_backend_types() {
        let postgres = Table::create()
            .table(Probe::Table)
            .col(timestamp_utc(Probe::CreatedAt, DbBackend::Postgres))
            .col(json(Probe::Payload, DbBackend::Postgres))
            .to_string(PostgresQueryBuilder);
        let postgres = postgres.to_ascii_uppercase();
        assert!(postgres.contains("TIMESTAMP WITH TIME ZONE"));
        assert!(postgres.contains("JSONB"));

        let mysql = Table::create()
            .table(Probe::Table)
            .col(timestamp_utc(Probe::CreatedAt, DbBackend::MySql))
            .col(json(Probe::Payload, DbBackend::MySql))
            .to_string(MysqlQueryBuilder);
        let mysql = mysql.to_ascii_uppercase();
        assert!(mysql.contains("DATETIME(6)"));
        assert!(mysql.contains("JSON"));
    }
}
