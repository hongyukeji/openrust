//! 同一物理管理会话上的资源互斥与终态屏障；不强制终止外部连接。

use super::{marker, ownership::Backend, session::AdminSession};
use crate::{Error, Result};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub(super) struct ServerLease {
    connection: AdminSession,
    pub(super) server_identity: String,
    pub(super) admin_database: String,
    session_id: i64,
    key: LeaseKey,
}

enum LeaseKey {
    Mysql(String),
    Postgres(i64),
}

fn failure(code: &'static str) -> Error {
    Error::custom(code, "测试数据库管理会话或活动状态未通过核验")
}

impl ServerLease {
    pub(super) async fn acquire(admin_url: &str, database_name: &str) -> Result<Self> {
        let connection =
            tokio::time::timeout(Duration::from_secs(5), AdminSession::connect(admin_url))
                .await
                .map_err(|_| failure("test_database_session_connect_timeout"))?
                .map_err(Error::internal)?;
        let (server_identity, admin_database) = marker::read_server_identity(&connection).await?;
        let session_id = session_id(&connection).await?;
        let mut hash = Sha256::new();
        hash.update(b"openrust-fixture-lease-v1\0");
        hash.update(server_identity.as_bytes());
        hash.update([0]);
        hash.update(database_name.as_bytes());
        let digest = hash.finalize();
        let key = match connection.get_database_backend() {
            DbBackend::MySql => {
                LeaseKey::Mysql(format!("openrust-fixture:{:.40}", format!("{digest:x}")))
            }
            DbBackend::Postgres => {
                let mut bytes = [0_u8; 8];
                bytes.copy_from_slice(&digest[..8]);
                LeaseKey::Postgres(i64::from_be_bytes(bytes) & i64::MAX)
            }
            _ => return Err(failure("test_database_lease_backend_invalid")),
        };
        let acquired = match &key {
            LeaseKey::Mysql(key) => {
                let row = query(
                    &connection,
                    "SELECT CAST(GET_LOCK(?, 0) AS SIGNED) AS held",
                    vec![key.clone().into()],
                )
                .await?;
                row.try_get::<Option<i64>>("", "held")
                    .map_err(Error::internal)?
                    == Some(1)
            }
            LeaseKey::Postgres(key) => query(
                &connection,
                "SELECT pg_try_advisory_lock($1::bigint) AS held",
                vec![(*key).into()],
            )
            .await?
            .try_get::<bool>("", "held")
            .map_err(Error::internal)?,
        };
        if !acquired {
            let _ = connection.close().await;
            return Err(failure("test_database_resource_busy"));
        }
        let lease = Self {
            connection,
            server_identity,
            admin_database,
            session_id,
            key,
        };
        lease.verify().await?;
        Ok(lease)
    }

    pub(super) fn connection(&self) -> &AdminSession {
        &self.connection
    }
    pub(super) fn backend(&self) -> Backend {
        match self.key {
            LeaseKey::Mysql(_) => Backend::Mysql,
            LeaseKey::Postgres(_) => Backend::Postgres,
        }
    }

    /// 每次破坏性操作前核会话和锁；池重连后不自动补取原锁。
    pub(super) async fn verify(&self) -> Result<()> {
        if session_id(&self.connection).await? != self.session_id {
            return Err(failure("test_database_lease_session_changed"));
        }
        let (server, database) = marker::read_server_identity(&self.connection).await?;
        if server != self.server_identity || database != self.admin_database {
            return Err(failure("test_database_lease_server_changed"));
        }
        if matches!(self.key, LeaseKey::Postgres(_)) {
            let writable = query(&self.connection,
                "SELECT (NOT pg_is_in_recovery() AND current_setting('transaction_read_only') = 'off') AS writable",
                vec![]).await?.try_get::<bool>("", "writable").map_err(Error::internal)?;
            if !writable {
                return Err(failure("test_database_authority_read_only"));
            }
        }
        let held = match &self.key {
            LeaseKey::Mysql(key) => {
                query(
                    &self.connection,
                    "SELECT CASE WHEN IS_USED_LOCK(?) = CONNECTION_ID() THEN 1 ELSE 0 END AS held",
                    vec![key.clone().into()],
                )
                .await?
                .try_get::<i64>("", "held")
                .map_err(Error::internal)?
                    == 1
            }
            LeaseKey::Postgres(key) => {
                let high = key >> 32;
                let low = key & 0xffff_ffff;
                query(&self.connection,"SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype='advisory' AND pid=pg_backend_pid() AND granted AND classid::bigint=$1 AND objid::bigint=$2 AND objsubid=1) AS held",vec![high.into(),low.into()]).await?
                    .try_get::<bool>("","held").map_err(Error::internal)?
            }
        };
        if !held {
            return Err(failure("test_database_lease_lost"));
        }
        Ok(())
    }

    /// 独立应用池仍选中目标库时保留；MySQL必须有全局threads表可见性。
    /// 跨库限定名访问仍依赖专用测试命名域协议，不承诺排斥不合作的管理员DDL。
    pub(super) async fn require_idle(&self, database_name: &str) -> Result<()> {
        self.verify().await?;
        match &self.key {
            LeaseKey::Mysql(_) => {
                let lost=query(&self.connection,
                    "SELECT VARIABLE_VALUE AS lost FROM performance_schema.global_status WHERE VARIABLE_NAME='Performance_schema_thread_instances_lost'", vec![])
                    .await?.try_get::<String>("", "lost").map_err(Error::internal)?;
                if lost != "0" {
                    return Err(failure("test_database_activity_unavailable"));
                }
                let row=query(&self.connection,"SELECT CAST(COALESCE(SUM(CASE WHEN PROCESSLIST_ID=CONNECTION_ID() THEN 1 ELSE 0 END),0) AS SIGNED) AS observer_seen, CAST(COALESCE(SUM(CASE WHEN BINARY PROCESSLIST_DB=BINARY ? AND PROCESSLIST_ID<>CONNECTION_ID() THEN 1 ELSE 0 END),0) AS SIGNED) AS active_users FROM performance_schema.threads WHERE TYPE='FOREGROUND'",vec![database_name.to_owned().into()]).await?;
                if row
                    .try_get::<i64>("", "observer_seen")
                    .map_err(Error::internal)?
                    != 1
                {
                    return Err(failure("test_database_activity_unavailable"));
                }
                if row
                    .try_get::<i64>("", "active_users")
                    .map_err(Error::internal)?
                    != 0
                {
                    return Err(failure("test_database_resource_in_use"));
                }
            }
            LeaseKey::Postgres(_) => {
                let users=query(&self.connection,"SELECT COUNT(*)::bigint AS active_users FROM pg_stat_activity WHERE datname=$1 AND pid<>pg_backend_pid()",vec![database_name.to_owned().into()]).await?
                    .try_get::<i64>("","active_users").map_err(Error::internal)?;
                if users != 0 {
                    return Err(failure("test_database_resource_in_use"));
                }
            }
        }
        Ok(())
    }

    pub(super) async fn database_exists(&self, name: &str) -> Result<bool> {
        self.verify().await?;
        match self.key {
            LeaseKey::Mysql(_) => {
                // 精确 SHOW CREATE 不把权限过滤后的空目录当作不存在。
                let result = self
                    .connection
                    .query_all_raw(Statement::from_string(
                        DbBackend::MySql,
                        format!("SHOW CREATE DATABASE `{name}`"),
                    ))
                    .await;
                match result {
                    Ok(rows) if !rows.is_empty() => Ok(true),
                    Ok(_) => Err(failure("test_database_existence_unconfirmed")),
                    Err(error) if mysql_unknown_database(&error) => Ok(false),
                    Err(error) => Err(Error::internal(error)),
                }
            }
            LeaseKey::Postgres(_) => {
                let row = query(
                    &self.connection,
                    "SELECT COUNT(*)::bigint AS n FROM pg_database WHERE datname=$1",
                    vec![name.to_owned().into()],
                )
                .await?;
                Ok(row.try_get::<i64>("", "n").map_err(Error::internal)? != 0)
            }
        }
    }

    pub(super) async fn close(self) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(2), self.connection.close())
            .await
            .map_err(|_| failure("test_database_session_close_timeout"))?
            .map_err(Error::internal)
    }
}

async fn session_id(connection: &AdminSession) -> Result<i64> {
    let sql = match connection.get_database_backend() {
        DbBackend::MySql => "SELECT CAST(CONNECTION_ID() AS SIGNED) AS session_id",
        DbBackend::Postgres => "SELECT pg_backend_pid()::bigint AS session_id",
        _ => return Err(failure("test_database_lease_backend_invalid")),
    };
    query(connection, sql, vec![])
        .await?
        .try_get("", "session_id")
        .map_err(Error::internal)
}

async fn query(
    connection: &AdminSession,
    sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<sea_orm::QueryResult> {
    connection
        .query_one_raw(Statement::from_sql_and_values(
            connection.get_database_backend(),
            sql,
            values,
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| failure("test_database_lease_row_missing"))
}

fn mysql_unknown_database(error: &sea_orm::DbErr) -> bool {
    matches!(error, sea_orm::DbErr::Custom(code) if code == "test_database_mysql_unknown_database")
}
