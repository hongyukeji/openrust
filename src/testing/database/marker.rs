//! 数据库内归属见证；调用方必须持有同一专属物理会话租约及本地 journal 锁。
//!
//! 安装仅用于 CREATE 已明确成功的本次对象，未知结果只能核验，不能补签。
//! 固定 Laravel `WipeCommand::handle` 只移除表、可选视图和类型；`MySqlGrammar::compileTables`
//! 从 `information_schema.tables` 枚举表。因此使用从不执行的 procedure，不进入该清单。
//! 原样复制含见证的同名备份、不合作的外部管理员 DDL 不具备 CAS 保证；本模块不删除数据库。

use super::ownership::{Backend, OwnershipRecord, ResourceKind};
use crate::{Error, Result};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

const ROUTINE: &str = "__openrust_test_database_owner";
const DOMAIN: &[u8] = b"openrust-fixture-marker\0";

/// 从本地归属记录派生的不可逆数据库见证，不直接保存原始 token。
#[derive(Clone)]
pub(super) struct MarkerProof {
    backend: Backend,
    server_identity: String,
    admin_database: String,
    database_name: String,
    comment: String,
}

impl fmt::Debug for MarkerProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MarkerProof")
            .field("backend", &self.backend)
            .field("database_name", &self.database_name)
            .finish_non_exhaustive()
    }
}

impl MarkerProof {
    #[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
    pub(super) fn comment(&self) -> &str {
        &self.comment
    }

    /// 校验记录并以固定域及逐字段长度前缀派生见证；可变清理状态不参与散列。
    pub(super) fn from_record(record: &OwnershipRecord) -> Result<Self> {
        let valid_hex = |value: &str, length: usize| {
            value.len() == length
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if record.version != 1
            || record.server_identity.is_empty()
            || record.admin_database.is_empty()
            || !valid_database_name(&record.database_name)
            || !record
                .resource_id
                .strip_prefix("openrust_resource_")
                .is_some_and(|suffix| valid_hex(suffix, 32))
            || !valid_hex(&record.token, 64)
        {
            return Err(invalid("数据库归属记录不能生成见证"));
        }
        let mut hash = Sha256::new();
        hash.update(DOMAIN);
        let version = record.version.to_be_bytes();
        let backend: &[u8] = match record.backend {
            Backend::Mysql => b"mysql",
            Backend::Postgres => b"postgres",
        };
        let kind: &[u8] = match record.resource_kind {
            ResourceKind::Target => b"target",
            ResourceKind::Template => b"template",
        };
        for field in [
            version.as_slice(),
            backend,
            record.server_identity.as_bytes(),
            record.admin_database.as_bytes(),
            record.database_name.as_bytes(),
            record.resource_id.as_bytes(),
            record.token.as_bytes(),
            kind,
        ] {
            hash.update((field.len() as u64).to_be_bytes());
            hash.update(field);
        }
        Ok(Self {
            backend: record.backend,
            server_identity: record.server_identity.clone(),
            admin_database: record.admin_database.clone(),
            database_name: record.database_name.clone(),
            comment: format!("openrust-fixture-v1:{:x}", hash.finalize()),
        })
    }
}

/// 安装时取得的对象身份；PostgreSQL 恢复必须同时匹配持久化 OID。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MarkerIdentity {
    pub(super) backend: Backend,
    pub(super) database_name: String,
    pub(super) postgres_oid: Option<u32>,
}

impl fmt::Debug for MarkerIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MarkerIdentity")
            .field("backend", &self.backend)
            .field("database_name", &self.database_name)
            .finish_non_exhaustive()
    }
}

fn invalid(message: &'static str) -> Error {
    Error::custom("test_database_marker_invalid", message)
}

fn valid_database_name(name: &str) -> bool {
    name.len() <= 63
        && name
            .strip_prefix("openrust_")
            .is_some_and(|suffix| !suffix.is_empty())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn backend_of<C: ConnectionTrait + ?Sized>(admin: &C) -> Result<Backend> {
    match admin.get_database_backend() {
        DbBackend::MySql => Ok(Backend::Mysql),
        DbBackend::Postgres => Ok(Backend::Postgres),
        _ => Err(invalid("数据库见证仅支持 MySQL 和 PostgreSQL")),
    }
}

/// 从服务端读取真实实例身份和当前管理库；权限错误原样返回，绝不回退连接 URL。
pub(super) async fn read_server_identity<C: ConnectionTrait + ?Sized>(
    admin: &C,
) -> Result<(String, String)> {
    let sql = match backend_of(admin)? {
        Backend::Mysql => "SELECT @@server_uuid AS server_identity, DATABASE() AS admin_database",
        Backend::Postgres => {
            "SELECT system_identifier::text AS server_identity, current_database() AS admin_database FROM pg_control_system()"
        }
    };
    let row = admin
        .query_one_raw(Statement::from_string(
            admin.get_database_backend(),
            sql.to_owned(),
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| invalid("服务端未返回实例身份"))?;
    let server: String = row
        .try_get("", "server_identity")
        .map_err(Error::internal)?;
    let database: String = row.try_get("", "admin_database").map_err(Error::internal)?;
    if server.is_empty() || database.is_empty() {
        return Err(invalid("服务端实例身份或管理库为空"));
    }
    Ok((server, database))
}

async fn context_matches<C: ConnectionTrait + ?Sized>(
    admin: &C,
    proof: &MarkerProof,
) -> Result<bool> {
    if backend_of(admin)? != proof.backend {
        return Ok(false);
    }
    let (server, database) = read_server_identity(admin).await?;
    Ok(server == proof.server_identity && database == proof.admin_database)
}

fn install_sql(proof: &MarkerProof) -> String {
    match proof.backend {
        Backend::Mysql => format!(
            "CREATE PROCEDURE `{}`.`{ROUTINE}`() COMMENT '{}' SQL SECURITY INVOKER BEGIN END",
            proof.database_name, proof.comment
        ),
        Backend::Postgres => format!(
            "COMMENT ON DATABASE \"{}\" IS '{}'",
            proof.database_name, proof.comment
        ),
    }
}

fn metadata_statement(proof: &MarkerProof) -> Statement {
    match proof.backend {
        Backend::Mysql => Statement::from_sql_and_values(
            DbBackend::MySql,
            "SELECT ROUTINE_SCHEMA AS database_name, ROUTINE_NAME AS marker_name, ROUTINE_TYPE AS marker_type, SECURITY_TYPE AS marker_security, ROUTINE_COMMENT AS marker_comment FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = ? AND ROUTINE_NAME = ? AND ROUTINE_TYPE = ?",
            [
                proof.database_name.clone().into(),
                ROUTINE.into(),
                "PROCEDURE".into(),
            ],
        ),
        Backend::Postgres => Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT oid::bigint AS database_oid, datname::text AS database_name, shobj_description(oid, 'pg_database') AS marker_comment FROM pg_database WHERE datname = $1",
            [proof.database_name.clone().into()],
        ),
    }
}

fn mysql_matches(
    proof: &MarkerProof,
    database: &str,
    name: &str,
    kind: &str,
    security: &str,
    comment: Option<&str>,
) -> bool {
    database == proof.database_name
        && name == ROUTINE
        && kind == "PROCEDURE"
        && security == "INVOKER"
        && comment == Some(proof.comment.as_str())
}

pub(super) async fn observe_marker<C: ConnectionTrait + ?Sized>(
    admin: &C,
    proof: &MarkerProof,
) -> Result<Option<MarkerIdentity>> {
    if !context_matches(admin, proof).await? {
        return Ok(None);
    }
    let rows = admin
        .query_all_raw(metadata_statement(proof))
        .await
        .map_err(Error::internal)?;
    for row in rows {
        let database: String = row.try_get("", "database_name").map_err(Error::internal)?;
        let comment: Option<String> = row.try_get("", "marker_comment").map_err(Error::internal)?;
        let oid = match proof.backend {
            Backend::Mysql => {
                let name: String = row.try_get("", "marker_name").map_err(Error::internal)?;
                let kind: String = row.try_get("", "marker_type").map_err(Error::internal)?;
                let security: String = row
                    .try_get("", "marker_security")
                    .map_err(Error::internal)?;
                if !mysql_matches(
                    proof,
                    &database,
                    &name,
                    &kind,
                    &security,
                    comment.as_deref(),
                ) {
                    continue;
                }
                None
            }
            Backend::Postgres => {
                if database != proof.database_name
                    || comment.as_deref() != Some(proof.comment.as_str())
                {
                    continue;
                }
                let oid: i64 = row.try_get("", "database_oid").map_err(Error::internal)?;
                Some(u32::try_from(oid).map_err(|_| invalid("数据库 OID 超出有效范围"))?)
            }
        };
        return Ok(Some(MarkerIdentity {
            backend: proof.backend,
            database_name: database,
            postgres_oid: oid,
        }));
    }
    Ok(None)
}

/// 为已确认本次 CREATE 成功的对象安装见证；不得用于未知创建结果或补签恢复。
pub(super) async fn install_marker<C: ConnectionTrait + ?Sized>(
    admin: &C,
    proof: &MarkerProof,
) -> Result<MarkerIdentity> {
    if !context_matches(admin, proof).await? {
        return Err(invalid("见证安装的服务端上下文不匹配"));
    }
    admin
        .execute_unprepared(&install_sql(proof))
        .await
        .map_err(Error::internal)?;
    observe_marker(admin, proof)
        .await?
        .ok_or_else(|| invalid("数据库见证安装后核验不匹配"))
}

fn identity_matches(
    proof: &MarkerProof,
    actual: &MarkerIdentity,
    expected: Option<&MarkerIdentity>,
) -> bool {
    if actual.backend != proof.backend || actual.database_name != proof.database_name {
        return false;
    }
    match expected {
        Some(expected) => {
            actual == expected
                && (proof.backend != Backend::Postgres || expected.postgres_oid.is_some())
        }
        None => proof.backend == Backend::Mysql && actual.postgres_oid.is_none(),
    }
}

/// 只读核验见证；缺失和错配返回 false，SQL/权限错误返回 Err，PostgreSQL 必须提供安装 OID。
pub(super) async fn verify_marker<C: ConnectionTrait + ?Sized>(
    admin: &C,
    proof: &MarkerProof,
    expected_identity: Option<&MarkerIdentity>,
) -> Result<bool> {
    Ok(observe_marker(admin, proof)
        .await?
        .is_some_and(|actual| identity_matches(proof, &actual, expected_identity)))
}

#[cfg(test)]
#[path = "marker_tests.rs"]
mod tests;
