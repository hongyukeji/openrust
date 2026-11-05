//! 同一账户模型的双因素属性持久化；不创建账户Provider或另一份身份存储。

use std::fmt;

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::state::TwoFactorState;
use crate::{
    Error, Result,
    core::BoxFuture,
    db::{
        Db,
        query::{QueryBuilder, RawSql},
        sea_orm::{DbBackend, QueryResult, Value as SqlValue},
    },
};

const SECRET: &str = "two_factor_secret";
const RECOVERY: &str = "two_factor_recovery_codes";
const CONFIRMED: &str = "two_factor_confirmed_at";

/// 将双因素状态读写委托到模型现有的Db、逻辑表与标识字段。
///
/// 三字段条件更新只保证当前快照匹配，不承诺状态回到相同值后的永久代际隔离。
/// Debug不输出连接、表、标识或密文；读取始终使用当前主库。
#[derive(Clone)]
pub struct DatabaseTwoFactorState {
    db: Db,
    table: String,
    identifier_name: String,
    identifier: SqlValue,
    raw_identifier: Value,
}

impl fmt::Debug for DatabaseTwoFactorState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DatabaseTwoFactorState([REDACTED])")
    }
}

impl DatabaseTwoFactorState {
    /// 使用模型已选择的持久来源；表名及字段名复用现有查询构建器校验。
    pub fn new(
        db: Db,
        table: impl Into<String>,
        identifier_name: impl Into<String>,
        identifier: Value,
    ) -> Result<Self> {
        let state = Self {
            db,
            table: table.into(),
            identifier_name: identifier_name.into(),
            identifier: identifier_value(identifier.clone())?,
            raw_identifier: identifier,
        };
        state.query()?;
        Ok(state)
    }

    /// 从主库读取完整快照；账户缺失、标识不唯一、缺列或字段类型错误均返回错误。
    pub fn load(&self) -> BoxFuture<'_, Result<TwoFactorState>> {
        Box::pin(async move {
            // SELECT *后显式核列：SQLite双引号未知列可能按字符串解释，Option解码也会吞缺列。
            let query = self.query()?.limit(2);
            let rows = self.db.with_primary(query.get()).await??;
            match rows.as_slice() {
                [] => Err(state_error("fortify_account_missing", "双因素账户不存在")),
                [row] => decode_state(row),
                _ => Err(state_error(
                    "fortify_account_identifier_ambiguous",
                    "双因素账户标识不唯一",
                )),
            }
        })
    }

    /// 通过一条带原三字段谓词的UPDATE替换；0行返回false，1行返回true。
    ///
    /// 多行更新会回滚并返回错误；不把失败或取消解释为数据库写入已回滚。
    /// 调用方在未知提交结果后应重新load，不得复用旧模型快照宣称成功。
    pub fn compare_and_set<'a>(
        &'a self,
        expected: &'a TwoFactorState,
        replacement: &'a TwoFactorState,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let backend = self.db.backend()?;
            let base = self.query()?;
            let expected = expected.clone();
            let values = vec![
                (SECRET, replacement.secret.clone().into()),
                (RECOVERY, replacement.recovery_codes.clone().into()),
                (
                    CONFIRMED,
                    SqlValue::ChronoDateTimeUtc(replacement.confirmed_at),
                ),
            ];
            self.db.transaction_for_update(move |transaction| {
                Box::pin(async move {
                    // SQLite时间戳是文本；先在同事务读取并严格解码，不能把同一UTC时刻
                    // 的CURRENT_TIMESTAMP/NaiveDateTime历史表示误判成竞争失败。
                    let sqlite_confirmed = if backend == DbBackend::Sqlite {
                        let Some(row) = base.clone().using_transaction(transaction).first().await? else {
                            return Ok(false);
                        };
                        if decode_state(&row)? != expected {
                            return Ok(false);
                        }
                        row.try_get::<Option<String>>("", CONFIRMED).map_err(Error::internal)?
                    } else {
                        None
                    };
                    let query = match_ciphertext(base, backend, SECRET, expected.secret.as_deref())?;
                    let query = match_ciphertext(query, backend, RECOVERY, expected.recovery_codes.as_deref())?;
                    let query = match expected.confirmed_at {
                        Some(_) if backend == DbBackend::Sqlite => {
                            let raw = sqlite_confirmed.ok_or_else(|| state_error("fortify_state_timestamp_invalid", "账户确认时间存储类型无效"))?;
                            query.where_raw(RawSql::new("CAST(\"two_factor_confirmed_at\" AS BLOB) = CAST(? AS BLOB)"), vec![raw.into()])
                        }
                        Some(value) => query.where_eq(CONFIRMED, SqlValue::ChronoDateTimeUtc(Some(value)))?,
                        None => query.where_null(CONFIRMED)?,
                    };
                    // WHERE仍核对全部原字段；时间文本读取后的任何修改都不能无条件覆盖。
                    let affected = query.using_transaction(transaction).update(values).await?;
                    match affected {
                        0 => Ok(false),
                        1 => Ok(true),
                        _ => Err(state_error("fortify_account_identifier_ambiguous", "双因素状态更新匹配多个账户")),
                    }
                })
            }).await
        })
    }

    pub(super) fn db(&self) -> &Db {
        &self.db
    }
    pub(super) fn table(&self) -> &str {
        &self.table
    }
    pub(super) fn identifier_name(&self) -> &str {
        &self.identifier_name
    }
    pub(super) fn raw_identifier(&self) -> &Value {
        &self.raw_identifier
    }

    pub(super) fn query(&self) -> Result<QueryBuilder<'static>> {
        self.db
            .table(&self.table)?
            .where_eq(&self.identifier_name, self.identifier.clone())
    }
}

pub(super) fn match_ciphertext(
    query: QueryBuilder<'static>,
    backend: DbBackend,
    field: &str,
    expected: Option<&str>,
) -> Result<QueryBuilder<'static>> {
    let Some(expected) = expected else {
        return query.where_null(field);
    };
    // 全部片段为固定官方字段；二进制比较不受账户表的大小写/尾空格collation影响。
    let sql = match (backend, field) {
        (DbBackend::MySql, SECRET) => "CAST(`two_factor_secret` AS BINARY) = CAST(? AS BINARY)",
        (DbBackend::MySql, RECOVERY) => {
            "CAST(`two_factor_recovery_codes` AS BINARY) = CAST(? AS BINARY)"
        }
        (DbBackend::Sqlite, SECRET) => "CAST(\"two_factor_secret\" AS BLOB) = CAST(? AS BLOB)",
        (DbBackend::Sqlite, RECOVERY) => {
            "CAST(\"two_factor_recovery_codes\" AS BLOB) = CAST(? AS BLOB)"
        }
        (DbBackend::Postgres, SECRET) => {
            "convert_to(\"two_factor_secret\", 'UTF8') = convert_to($1, 'UTF8')"
        }
        (DbBackend::Postgres, RECOVERY) => {
            "convert_to(\"two_factor_recovery_codes\", 'UTF8') = convert_to($1, 'UTF8')"
        }
        _ => {
            return Err(state_error(
                "fortify_database_backend_unsupported",
                "双因素状态不支持此数据库后端",
            ));
        }
    };
    Ok(query.where_raw(RawSql::new(sql), vec![expected.to_owned().into()]))
}

pub(super) fn decode_state(row: &QueryResult) -> Result<TwoFactorState> {
    let columns = row.column_names();
    if [SECRET, RECOVERY, CONFIRMED]
        .iter()
        .any(|field| !columns.iter().any(|column| column == field))
    {
        return Err(state_error(
            "fortify_state_column_missing",
            "账户缺少双因素状态字段",
        ));
    }
    Ok(TwoFactorState {
        secret: row
            .try_get::<Option<String>>("", SECRET)
            .map_err(Error::internal)?,
        recovery_codes: row
            .try_get::<Option<String>>("", RECOVERY)
            .map_err(Error::internal)?,
        confirmed_at: row
            .try_get::<Option<DateTime<Utc>>>("", CONFIRMED)
            .map_err(Error::internal)?,
    })
}

fn identifier_value(identifier: Value) -> Result<SqlValue> {
    match identifier {
        Value::String(identifier) if !identifier.trim().is_empty() => Ok(identifier.into()),
        Value::Number(identifier) => {
            if let Some(value) = identifier.as_i64() {
                Ok(value.into())
            } else if let Some(value) = identifier.as_u64() {
                Ok(value.into())
            } else {
                Err(invalid_identifier())
            }
        }
        _ => Err(invalid_identifier()),
    }
}

fn invalid_identifier() -> Error {
    state_error(
        "fortify_account_identifier_invalid",
        "双因素账户标识必须是非空字符串或整数",
    )
}

fn state_error(code: &'static str, message: &'static str) -> Error {
    Error::custom_with_status(http::StatusCode::INTERNAL_SERVER_ERROR, code, message)
}

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "database/tests.rs"]
mod tests;
