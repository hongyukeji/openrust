//! 每条业务物理连接在进入池前核对实例、目标数据库和归属见证；重连亦执行相同核验。

use super::ownership::OwnershipRecord;
use crate::{Error, Result};
use sea_orm::DatabaseConnection;
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
use {
    super::{marker::MarkerProof, ownership::Backend},
    sea_orm::{ConnectOptions, Database, sqlx},
};

fn rejected() -> Error {
    Error::custom(
        "test_database_owned_pool_rejected",
        "测试数据库业务连接归属核验失败",
    )
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
fn protocol() -> sqlx::Error {
    // SQLx 会记录 after_connect 错误，因此禁止透传 URL、SQL、绑定或服务端错误原文。
    sqlx::Error::Protocol("test_database_owned_pool_rejected".to_owned())
}

/// 建立业务池；每条初始或后续重连的物理连接必须先通过归属核验。
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
pub(super) async fn connect(url: &str, record: &OwnershipRecord) -> Result<DatabaseConnection> {
    let proof = MarkerProof::from_record(record)?;
    let identity = record.marker_identity.as_ref().ok_or_else(rejected)?;
    if identity.backend != record.backend || identity.database_name != record.database_name {
        return Err(rejected());
    }
    let mut options = ConnectOptions::new(url.to_owned());
    options
        .max_connections(2)
        .min_connections(0)
        .sqlx_logging(false);
    match record.backend {
        Backend::Mysql => {
            #[cfg(not(feature = "db-mysql"))]
            return Err(rejected());
            #[cfg(feature = "db-mysql")]
            {
                if identity.postgres_oid.is_some() || !url.starts_with("mysql://") {
                    return Err(rejected());
                }
                let record = record.clone();
                let comment = proof.comment().to_owned();
                options.map_sqlx_mysql_opts(|mysql| {
                    mysql.ssl_mode(sqlx::mysql::MySqlSslMode::Required)
                });
                options.map_sqlx_mysql_pool_opts(move |pool| {
                    let record = record.clone();
                    let comment = comment.clone();
                    pool.after_connect(move |connection, _| {
                        let record = record.clone();
                        let comment = comment.clone();
                        Box::pin(async move {
                            verify_mysql(connection, &record, &comment).await?;
                            sqlx::query("SET time_zone = '+00:00'")
                                .execute(connection)
                                .await
                                .map_err(|_| protocol())?;
                            Ok(())
                        })
                    })
                });
            }
        }
        Backend::Postgres => {
            #[cfg(not(feature = "db-postgres"))]
            return Err(rejected());
            #[cfg(feature = "db-postgres")]
            {
                let oid = identity
                    .postgres_oid
                    .filter(|oid| *oid > 0)
                    .ok_or_else(rejected)?;
                if !(url.starts_with("postgres://") || url.starts_with("postgresql://")) {
                    return Err(rejected());
                }
                let record = record.clone();
                let comment = proof.comment().to_owned();
                options.map_sqlx_postgres_pool_opts(move |pool| {
                    let record = record.clone();
                    let comment = comment.clone();
                    pool.after_connect(move |connection, _| {
                        let record = record.clone();
                        let comment = comment.clone();
                        Box::pin(async move {
                            verify_postgres(connection, &record, &comment, oid).await
                        })
                    })
                });
            }
        }
    }
    Database::connect(options).await.map_err(|_| rejected())
}

/// 未启用服务端数据库驱动时明确拒绝，不回退到 SQLite 或其他连接。
#[cfg(not(any(feature = "db-mysql", feature = "db-postgres")))]
pub(super) fn connect(
    url: &str,
    record: &OwnershipRecord,
) -> std::future::Ready<Result<DatabaseConnection>> {
    let _ = (url, record);
    std::future::ready(Err(rejected()))
}

#[cfg(feature = "db-mysql")]
type MysqlMarkerRow = (String, String, String, String, Option<String>);

#[cfg(feature = "db-mysql")]
async fn verify_mysql(
    connection: &mut sqlx::MySqlConnection,
    record: &OwnershipRecord,
    comment: &str,
) -> std::result::Result<(), sqlx::Error> {
    let (server, database): (String, Option<String>) =
        sqlx::query_as("SELECT @@server_uuid, DATABASE()")
            .fetch_one(&mut *connection)
            .await
            .map_err(|_| protocol())?;
    if server != record.server_identity
        || database.as_deref() != Some(record.database_name.as_str())
    {
        return Err(protocol());
    }
    let rows: Vec<MysqlMarkerRow> = sqlx::query_as(
        "SELECT ROUTINE_SCHEMA, ROUTINE_NAME, ROUTINE_TYPE, SECURITY_TYPE, ROUTINE_COMMENT FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = ? AND ROUTINE_NAME = ? AND ROUTINE_TYPE = ?")
        .bind(&record.database_name).bind("__openrust_test_database_owner").bind("PROCEDURE")
        .fetch_all(connection).await.map_err(|_| protocol())?;
    if !rows
        .iter()
        .any(|(database, name, kind, security, actual_comment)| {
            database == &record.database_name
                && name == "__openrust_test_database_owner"
                && kind == "PROCEDURE"
                && security == "INVOKER"
                && actual_comment.as_deref() == Some(comment)
        })
    {
        return Err(protocol());
    }
    Ok(())
}

#[cfg(feature = "db-postgres")]
async fn verify_postgres(
    connection: &mut sqlx::PgConnection,
    record: &OwnershipRecord,
    comment: &str,
    expected_oid: u32,
) -> std::result::Result<(), sqlx::Error> {
    let (server, database, recovery): (String, String, bool) = sqlx::query_as(
        "SELECT system_identifier::text, current_database(), pg_is_in_recovery() FROM pg_control_system()")
        .fetch_one(&mut *connection).await.map_err(|_| protocol())?;
    if server != record.server_identity || database != record.database_name || recovery {
        return Err(protocol());
    }
    let metadata: Option<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT oid::bigint, datname::text, shobj_description(oid, 'pg_database') FROM pg_database WHERE datname = $1")
        .bind(&record.database_name).fetch_optional(connection).await.map_err(|_| protocol())?;
    if !metadata.is_some_and(|(oid, database, actual_comment)| {
        oid == i64::from(expected_oid)
            && database == record.database_name
            && actual_comment.as_deref() == Some(comment)
    }) {
        return Err(protocol());
    }
    Ok(())
}
