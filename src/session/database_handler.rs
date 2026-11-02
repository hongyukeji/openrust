//! 固定 Laravel `DatabaseSessionHandler` 的原生数据库处理边界。
//!
//! 来源：laravel/framework d52c3b3129e4c8e213c97cdc68f94b0b7d27f181，
//! Illuminate/Session/DatabaseSessionHandler.php:105-303；opaque 字节以 Base64 存储，
//! 请求独占的 exists 状态不替代上层 `SessionStore` 的身份、撤销与容量策略。

use std::{fmt, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};

use crate::support::calendar::{CalendarClock, SystemClock};
use crate::{Error, Result, db::Db};

/// 本次请求提供的数据库 Session 元数据；缺省字段不会覆盖已有列。
#[derive(Clone, Default)]
pub struct DatabaseSessionMetadata {
    /// 用户标识；None 省略该列，Some 中的 SQL NULL 显式清空该列。
    pub user_id: Option<Value>,
    /// 客户端地址；None 省略，Some(None) 显式写入 SQL NULL。
    pub ip_address: Option<Option<String>>,
    /// 客户端标识；存在时按 Unicode 字符截断至 500 字符。
    pub user_agent: Option<String>,
}

impl fmt::Debug for DatabaseSessionMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DatabaseSessionMetadata([REDACTED])")
    }
}

/// 复用应用主连接的低层 Session handler；每个请求持有独立的可变 exists 状态。
///
/// 不限制 ID 为框架 `SessionId` 格式，也不将缺省表名偷偷解释成 sessions。
/// 高层 `SessionStore` 负责身份、撤销与记录格式等增强；本类型保持上游低层写入语义。
pub struct DatabaseSessionHandler {
    db: Db,
    table: Option<String>,
    lifetime_seconds: i64,
    clock: Arc<dyn CalendarClock>,
    max_bytes: usize,
    metadata: DatabaseSessionMetadata,
    exists: bool,
}

impl fmt::Debug for DatabaseSessionHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DatabaseSessionHandler")
            .field("exists", &self.exists)
            .field("max_bytes", &self.max_bytes)
            .finish_non_exhaustive()
    }
}

impl DatabaseSessionHandler {
    /// 构造不建立连接的 handler；minutes 只决定 read 的活动过期判断。
    ///
    /// # Errors
    ///
    /// 分钟数转换为秒溢出时返回固定配置错误。
    pub fn new(db: Db, table: Option<String>, minutes: i64) -> Result<Self> {
        let lifetime_seconds = minutes.checked_mul(60).ok_or_else(invalid_configuration)?;
        Ok(Self {
            db,
            table,
            lifetime_seconds,
            clock: Arc::new(SystemClock),
            max_bytes: crate::config::SessionConfig::MAX_BYTES,
            metadata: DatabaseSessionMetadata::default(),
            exists: false,
        })
    }

    /// 使用应用已有 `CalendarClock` 以便统一时间来源和可重复验证。
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn CalendarClock>) -> Self {
        self.clock = clock;
        self
    }

    /// 设置 opaque 载荷预算；0 仅允许空载荷，不改变 Provider 的配置验证。
    ///
    /// # Errors
    ///
    /// 上限超过 `SessionConfig::MAX_BYTES` 时返回固定配置错误。
    pub fn with_max_bytes(mut self, max_bytes: usize) -> Result<Self> {
        self.set_max_bytes(max_bytes)?;
        Ok(self)
    }

    /// 更新下一次操作的 opaque 载荷预算，不重置本请求 exists。
    ///
    /// # Errors
    ///
    /// 预算超过受管 Session 硬上限时拒绝。
    pub fn set_max_bytes(&mut self, max_bytes: usize) -> Result<()> {
        if max_bytes > crate::config::SessionConfig::MAX_BYTES {
            return Err(invalid_configuration());
        }
        encoded_limit(max_bytes)?;
        self.max_bytes = max_bytes;
        Ok(())
    }

    /// 安装本次写入的元数据；不推断或伪造用户身份。
    #[must_use]
    pub fn with_metadata(mut self, metadata: DatabaseSessionMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// 替换本次写入的元数据。
    pub fn set_metadata(&mut self, metadata: DatabaseSessionMetadata) {
        self.metadata = metadata;
    }

    /// 当前请求的上游 exists 标志；与载荷是否为空无关。
    #[must_use]
    pub const fn exists(&self) -> bool {
        self.exists
    }

    /// 显式设置本请求的上游 exists 标志。
    pub fn set_exists(&mut self, exists: bool) -> &mut Self {
        self.exists = exists;
        self
    }

    /// 打开 handler；不创建独立连接或持久资源。
    #[must_use]
    pub const fn open(&self) -> bool {
        true
    }

    /// 关闭 handler；主连接的生命周期仍由应用 Provider 管理。
    #[must_use]
    pub const fn close(&self) -> bool {
        true
    }

    /// 按上游 read 读取原始字节；过期行标记存在但不删除。
    ///
    /// # Errors
    ///
    /// 数据库、时间、字段类型或容量失败返回不含 SQL/ID/载荷的固定错误。
    pub async fn read(&mut self, id: &str) -> Result<Vec<u8>> {
        let (connection, backend, table) = self.context()?;
        let cutoff = self
            .now()?
            .checked_sub(self.lifetime_seconds)
            .ok_or_else(unavailable)?;
        let maximum = encoded_limit(self.max_bytes)?;
        let length = payload_length(backend)?;
        let placeholders = placeholders(backend, 3)?;
        // PostgreSQL 的官方 integer 列是 INT4；SQLx 的 i64 读取要求 INT8 投影。
        let activity_column = if backend == DbBackend::Postgres {
            "CAST(last_activity AS BIGINT)"
        } else {
            "last_activity"
        };
        let sql = format!(
            "SELECT {activity_column} AS last_activity, {length} AS payload_length, CASE WHEN last_activity < {} THEN NULL WHEN {length} <= {} THEN payload ELSE NULL END AS bounded_payload FROM {table} WHERE id = {}",
            placeholders[0], placeholders[1], placeholders[2]
        );
        let maximum = i64::try_from(maximum).map_err(|_| limit_error())?;
        let row = connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                sql,
                [cutoff.into(), maximum.into(), id.to_owned().into()],
            ))
            .await
            .map_err(|_| unavailable())?;
        let Some(row) = row else {
            return Ok(Vec::new());
        };
        let activity: Option<i64> = row
            .try_get("", "last_activity")
            .map_err(|_| unavailable())?;
        if activity.is_some_and(|activity| activity < cutoff) {
            self.exists = true;
            return Ok(Vec::new());
        }
        let length: Option<i64> = row
            .try_get("", "payload_length")
            .map_err(|_| unavailable())?;
        let Some(length) = length else {
            return Ok(Vec::new());
        };
        // isset(payload) 为真时，上游在解码之前就设置 exists。
        self.exists = true;
        if length < 0 || length > maximum {
            return Err(limit_error());
        }
        let payload: String = row
            .try_get("", "bounded_payload")
            .map_err(|_| unavailable())?;
        decode_relaxed(payload.as_bytes(), self.max_bytes)
    }

    /// 写入原始字节；INSERT 的任意数据库查询错误均按上游改走 UPDATE。
    ///
    /// # Errors
    ///
    /// 预算、时间或最终数据库执行失败返回固定错误；不返回 SQL/ID/载荷。
    pub async fn write(&mut self, id: &str, data: &[u8]) -> Result<bool> {
        if data.len() > self.max_bytes {
            return Err(limit_error());
        }
        let encoded = STANDARD.encode(data);
        if encoded.len() > encoded_limit(self.max_bytes)? {
            return Err(limit_error());
        }
        // 上游先构造 payload/last_activity，再在 exists=false 时调用 read。
        let activity = self.now()?;
        let mut columns = vec!["payload", "last_activity"];
        let mut values = vec![encoded.into(), activity.into()];
        self.metadata.append(&mut columns, &mut values);
        if !self.exists {
            self.read(id).await?;
        }
        let (connection, backend, table) = self.context()?;
        if self.exists {
            update(&connection, backend, &table, id, &columns, values).await?;
        } else {
            let mut insert_columns = columns.clone();
            insert_columns.push("id");
            let mut insert_values = values.clone();
            insert_values.push(id.to_owned().into());
            let markers = placeholders(backend, insert_values.len())?.join(",");
            let sql = format!(
                "INSERT INTO {table} ({}) VALUES ({markers})",
                insert_columns.join(",")
            );
            if let Err(error) = connection
                .execute_raw(Statement::from_sql_and_values(backend, sql, insert_values))
                .await
            {
                // 连接获取、传输及执行结果不确定的错误不能伪装成已处理的 INSERT。
                if !is_query_error(&error, backend) {
                    return Err(unavailable());
                }
                update(&connection, backend, &table, id, &columns, values).await?;
            }
        }
        self.exists = true;
        Ok(true)
    }

    /// 只判断 ID 对应行是否存在；过期行仍为 true，且不更改 exists。
    ///
    /// # Errors
    ///
    /// 数据库查询或表配置无效时返回固定错误。
    pub async fn validate_id(&self, id: &str) -> Result<bool> {
        let (connection, backend, table) = self.context()?;
        let marker = placeholders(backend, 1)?.remove(0);
        connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                format!("SELECT 1 AS present FROM {table} WHERE id={marker}"),
                [id.to_owned().into()],
            ))
            .await
            .map(|row| row.is_some())
            .map_err(|_| unavailable())
    }

    /// 删除指定 ID；目标缺失也返回 true，不重置 exists。
    ///
    /// # Errors
    ///
    /// 数据库查询或表配置无效时返回固定错误。
    pub async fn destroy(&self, id: &str) -> Result<bool> {
        let (connection, backend, table) = self.context()?;
        let marker = placeholders(backend, 1)?.remove(0);
        connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                format!("DELETE FROM {table} WHERE id={marker}"),
                [id.to_owned().into()],
            ))
            .await
            .map(|_| true)
            .map_err(|_| unavailable())
    }

    /// 按 `last_activity <= now-lifetime_seconds` 回收记录并返回删除数。
    ///
    /// # Errors
    ///
    /// 时间计算、数据库或表配置无效时返回固定错误。
    pub async fn gc(&self, lifetime_seconds: i64) -> Result<u64> {
        let cutoff = self
            .now()?
            .checked_sub(lifetime_seconds)
            .ok_or_else(unavailable)?;
        let (connection, backend, table) = self.context()?;
        let marker = placeholders(backend, 1)?.remove(0);
        connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                format!("DELETE FROM {table} WHERE last_activity<={marker}"),
                [cutoff.into()],
            ))
            .await
            .map(|result| result.rows_affected())
            .map_err(|_| unavailable())
    }

    fn now(&self) -> Result<i64> {
        self.clock
            .now()
            .map(crate::support::calendar::CalendarInstant::epoch_seconds)
            .map_err(|_| unavailable())
    }

    fn context(&self) -> Result<(DatabaseConnection, DbBackend, String)> {
        let table = self.table.as_deref().ok_or_else(|| {
            Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "session_database_table_missing",
                "Session database table is not configured",
            )
        })?;
        let connection = self.db.connection().map_err(|_| unavailable())?;
        let backend = connection.get_database_backend();
        payload_length(backend)?;
        let table = self
            .db
            .quoted_table_name(table)
            .map_err(|_| unavailable())?;
        Ok((connection, backend, table))
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql", feature = "db-sqlite"))]
fn is_query_error(error: &sea_orm::DbErr, backend: DbBackend) -> bool {
    let sea_orm::DbErr::Exec(sea_orm::RuntimeErr::SqlxError(error)) = error else {
        return false;
    };
    let sea_orm::sqlx::Error::Database(error) = error.as_ref() else {
        return false;
    };
    let Some(code) = error.code() else {
        return false;
    };
    // 已知中断、I/O、连接丢失及未知提交状态保守报错，不以 UPDATE 零行掩盖结果不确定。
    match backend {
        DbBackend::Postgres => {
            !code.starts_with("08")
                && !matches!(
                    code.as_ref(),
                    "40003" | "57P01" | "57P02" | "57P03" | "58030"
                )
        }
        DbBackend::MySql => {
            if code.starts_with("08") || code == "40003" {
                return false;
            }
            #[cfg(feature = "db-mysql")]
            {
                error
                    .try_downcast_ref::<sea_orm::sqlx::mysql::MySqlDatabaseError>()
                    .is_some_and(|error| {
                        !matches!(error.number(), 1053 | 2006 | 2013 | 2055 | 4031)
                    })
            }
            #[cfg(not(feature = "db-mysql"))]
            {
                false
            }
        }
        DbBackend::Sqlite => code
            .parse::<i32>()
            .is_ok_and(|code| !matches!(code & 255, 7 | 9 | 10 | 13 | 14 | 15 | 21)),
        _ => false,
    }
}

#[cfg(not(any(feature = "db-postgres", feature = "db-mysql", feature = "db-sqlite")))]
fn is_query_error(_error: &sea_orm::DbErr, _backend: DbBackend) -> bool {
    false
}

#[cfg(all(
    test,
    any(feature = "db-sqlite", feature = "db-postgres", feature = "db-mysql")
))]
#[path = "database_handler/tests.rs"]
mod contract_tests;

impl DatabaseSessionMetadata {
    /// 按固定上游默认 UTF-8 / `mb_substitute_character(63)` 转换并截断 UA。
    ///
    /// 非法序列以问号替换；合法的 U+FFFD 字符仍保留，不进行 grapheme 合并。
    #[must_use]
    pub fn with_user_agent_bytes(mut self, mut bytes: &[u8]) -> Self {
        let mut output = String::new();
        let mut count = 0;
        while !bytes.is_empty() && count < 500 {
            let (valid, invalid_length) = match std::str::from_utf8(bytes) {
                Ok(valid) => (valid, None),
                Err(error) => {
                    let valid = std::str::from_utf8(&bytes[..error.valid_up_to()]);
                    let Ok(valid) = valid else {
                        break;
                    };
                    let length = error
                        .error_len()
                        .unwrap_or(bytes.len() - error.valid_up_to());
                    (valid, Some(length))
                }
            };
            for character in valid.chars().take(500 - count) {
                output.push(character);
                count += 1;
            }
            if count == 500 {
                break;
            }
            let consumed = valid.len();
            if let Some(length) = invalid_length {
                output.push('?');
                count += 1;
                bytes = &bytes[consumed + length..];
            } else {
                break;
            }
        }
        self.user_agent = Some(output);
        self
    }

    fn append(&self, columns: &mut Vec<&'static str>, values: &mut Vec<Value>) {
        if let Some(user_id) = &self.user_id {
            columns.push("user_id");
            values.push(user_id.clone());
        }
        if let Some(address) = &self.ip_address {
            columns.push("ip_address");
            values.push(address.clone().into());
        }
        if let Some(agent) = &self.user_agent {
            columns.push("user_agent");
            values.push(agent.chars().take(500).collect::<String>().into());
        }
    }
}

async fn update(
    connection: &DatabaseConnection,
    backend: DbBackend,
    table: &str,
    id: &str,
    columns: &[&str],
    mut values: Vec<Value>,
) -> Result<()> {
    let markers = placeholders(backend, values.len() + 1)?;
    let assignments = columns
        .iter()
        .zip(&markers)
        .map(|(column, marker)| format!("{column}={marker}"))
        .collect::<Vec<_>>()
        .join(",");
    let marker = markers.last().ok_or_else(unavailable)?;
    values.push(id.to_owned().into());
    connection
        .execute_raw(Statement::from_sql_and_values(
            backend,
            format!("UPDATE {table} SET {assignments} WHERE id={marker}"),
            values,
        ))
        .await
        .map(|_| ())
        .map_err(|_| unavailable())
}

fn placeholders(backend: DbBackend, count: usize) -> Result<Vec<String>> {
    match backend {
        DbBackend::Postgres => Ok((1..=count).map(|number| format!("${number}")).collect()),
        DbBackend::MySql | DbBackend::Sqlite => Ok(vec!["?".to_owned(); count]),
        _ => Err(unavailable()),
    }
}

fn payload_length(backend: DbBackend) -> Result<&'static str> {
    match backend {
        DbBackend::Postgres => Ok("CAST(octet_length(payload) AS BIGINT)"),
        DbBackend::MySql => Ok("CAST(OCTET_LENGTH(payload) AS SIGNED)"),
        DbBackend::Sqlite => Ok("length(CAST(payload AS BLOB))"),
        _ => Err(unavailable()),
    }
}

fn encoded_limit(maximum: usize) -> Result<usize> {
    maximum
        .checked_add(2)
        .and_then(|value| value.checked_div(3))
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(limit_error)
}

fn decode_relaxed(encoded: &[u8], maximum: usize) -> Result<Vec<u8>> {
    let capacity = encoded
        .len()
        .checked_mul(3)
        .and_then(|length| length.checked_div(4))
        .ok_or_else(limit_error)?
        .min(maximum);
    let mut decoded = Vec::with_capacity(capacity);
    let mut buffer = 0_u16;
    let mut bits = 0_u8;
    for byte in encoded {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        buffer = (buffer << 6) | u16::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            if decoded.len() == maximum {
                return Err(limit_error());
            }
            decoded.push(u8::try_from((buffer >> bits) & 255).map_err(|_| limit_error())?);
            buffer &= (1 << bits) - 1;
        }
    }
    Ok(decoded)
}

fn unavailable() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "session_database_unavailable",
        "Session database handler is unavailable",
    )
}
fn limit_error() -> Error {
    super::store::limit_error()
}
fn invalid_configuration() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "session_database_config_invalid",
        "Session database handler configuration is invalid",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_matches_php_85_mbstring_utf8_probe() {
        for (raw, expected) in [
            (b"A\xffB".as_slice(), "A?B"),
            (b"A\xc0\xafB".as_slice(), "A??B"),
            (b"A\xed\xa0\x80B".as_slice(), "A???B"),
            (b"A\xe2\x82".as_slice(), "A?"),
            (b"A\xe2\x28\xa1B".as_slice(), "A?(?B"),
            ("A�B".as_bytes(), "A�B"),
        ] {
            assert_eq!(
                DatabaseSessionMetadata::default()
                    .with_user_agent_bytes(raw)
                    .user_agent
                    .as_deref(),
                Some(expected)
            );
        }
        let long = "汉🙂".repeat(300);
        let metadata = DatabaseSessionMetadata::default().with_user_agent_bytes(long.as_bytes());
        assert_eq!(metadata.user_agent, Some("汉🙂".repeat(250)));
        let combining = "e\u{301}".repeat(300);
        assert_eq!(
            DatabaseSessionMetadata::default()
                .with_user_agent_bytes(combining.as_bytes())
                .user_agent,
            Some("e\u{301}".repeat(250))
        );
    }

    #[test]
    fn non_strict_base64_matches_fixed_php_probe() -> Result<()> {
        for (input, expected) in [
            ("a=Gk=", b"hi".as_slice()),
            ("aG=k", b"hi".as_slice()),
            ("====YQ==", b"a".as_slice()),
            ("a", b"".as_slice()),
            ("aGk=YQ==", b"hi\x18".as_slice()),
            ("YQ===", b"a".as_slice()),
            ("YWJj=ZGVm", b"abcdef".as_slice()),
            ("aGk===", b"hi".as_slice()),
        ] {
            assert_eq!(decode_relaxed(input.as_bytes(), 100)?, expected);
        }
        assert_eq!(decode_relaxed(b"!a\nGk=\xff", 2)?, b"hi");
        Ok(())
    }

    #[test]
    fn byte_budget_checks_decoded_bytes_and_base64_rounding() -> Result<()> {
        assert_eq!(encoded_limit(0)?, 0);
        assert_eq!(encoded_limit(1)?, 4);
        assert_eq!(encoded_limit(3)?, 4);
        assert_eq!(encoded_limit(4)?, 8);
        assert!(encoded_limit(usize::MAX).is_err());
        assert_eq!(decode_relaxed(b"", 0)?, b"");
        assert!(decode_relaxed(b"AAAA", 1).is_err());
        assert_eq!(decode_relaxed(b"YQ==", 1)?, b"a");
        Ok(())
    }

    #[test]
    fn metadata_is_omitted_or_explicitly_null_and_agent_counts_unicode_chars() {
        let mut columns = Vec::new();
        let mut values = Vec::new();
        DatabaseSessionMetadata::default().append(&mut columns, &mut values);
        assert!(columns.is_empty() && values.is_empty());
        let metadata = DatabaseSessionMetadata {
            user_id: Some(Value::BigInt(None)),
            ip_address: Some(None),
            user_agent: Some("汉🙂".repeat(300)),
        };
        metadata.append(&mut columns, &mut values);
        assert_eq!(columns, ["user_id", "ip_address", "user_agent"]);
        assert_eq!(values[0], Value::BigInt(None));
        assert_eq!(values[1], Value::String(None));
        assert_eq!(values[2], Value::String(Some("汉🙂".repeat(250))));
        assert!(!format!("{metadata:?}").contains('汉'));
    }
}
