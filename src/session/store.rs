//! 唯一 Session driver 扩展契约及有界持久记录编码。

use std::{fmt, io::Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use super::SessionId;
use crate::{Error, Result, core::BoxFuture};

/// 默认整个持久记录（含身份绑定与属性）的字节上限。
pub const DEFAULT_MAX_BYTES: usize = crate::config::SessionConfig::MAX_BYTES;

/// 写入是否允许创建新记录。
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SessionWriteMode {
    /// 插入未存ID；碰撞不覆盖其他会话。
    Insert,
    /// 仅更新本次已经读取的记录；撤销后不得重建。
    Update,
}

/// driver 原子写入的结果。
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SessionWriteOutcome {
    /// 记录已经写入。
    Written,
    /// Insert碰撞或driver检测到不相容的写入状态。
    Conflict,
    /// Update目标已过期或被删除；不得改成Insert。
    Missing,
}

/// 单个Session的持久记录；Debug不包含ID或属性。
#[derive(Clone)]
pub struct SessionRecord {
    /// 已验证的大小写敏感ID。
    pub id: SessionId,
    /// 有序PHP属性数组；只允许JSON对象或数组作为根。
    pub data: Value,
    /// 记录的持久截止时间。
    pub expiry_date: OffsetDateTime,
}

impl fmt::Debug for SessionRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionRecord([REDACTED])")
    }
}

#[derive(Serialize)]
struct Encoded<'a> {
    version: u8,
    id_hash: String,
    attributes: &'a Value,
    expiry_seconds: i64,
    expiry_nanoseconds: u32,
}

#[derive(Deserialize)]
struct Decoded {
    version: u8,
    id_hash: String,
    attributes: Value,
    expiry_seconds: i64,
    expiry_nanoseconds: u32,
}

impl SessionRecord {
    /// 编码一次完整记录；writer在越过上限前停止，不先生成无界字节缓冲。
    ///
    /// 属性树本身须由调用方先有界构造；该函数不承诺倒退限制既有树的分配。
    pub fn encode(&self, max_bytes: usize) -> Result<Vec<u8>> {
        self.encode_with_expiry(max_bytes, self.expiry_date)
    }

    pub(super) fn encode_with_expiry(
        &self,
        max_bytes: usize,
        expiry: OffsetDateTime,
    ) -> Result<Vec<u8>> {
        self.validate()?;
        let encoded = Encoded {
            version: 1,
            id_hash: digest_id(&self.id),
            attributes: &self.data,
            expiry_seconds: expiry.unix_timestamp(),
            expiry_nanoseconds: expiry.nanosecond(),
        };
        let mut writer = LimitedWriter {
            data: Vec::new(),
            maximum: max_bytes,
            exceeded: false,
        };
        let result = serde_json::to_writer(&mut writer, &encoded);
        if writer.exceeded {
            return Err(limit_error());
        }
        result.map_err(|_| invalid_record())?;
        Ok(writer.data)
    }

    /// 解析driver已经有界读取的字节，并校验请求ID与持久身份绑定。
    ///
    /// JSON语法损坏按固定Store反序列化规则返回空属性；有效JSON但记录字段、
    /// 版本或身份摘要不合法则安全失败。调用方负责在读取字节之前落实上限。
    pub fn decode(
        id: SessionId,
        bytes: &[u8],
        fallback_expiry: OffsetDateTime,
        max_bytes: usize,
    ) -> Result<Self> {
        if bytes.len() > max_bytes {
            return Err(limit_error());
        }
        guard_json_depth(bytes)?;
        let decoded = match serde_json::from_slice::<Decoded>(bytes) {
            Ok(decoded) => decoded,
            Err(error) if error.is_syntax() || error.is_eof() => {
                return Ok(Self {
                    id,
                    data: Value::Array(Vec::new()),
                    expiry_date: fallback_expiry,
                });
            }
            Err(_) => return Err(invalid_record()),
        };
        if decoded.version != 1 || decoded.id_hash != digest_id(&id) {
            return Err(invalid_record());
        }
        let expiry_date = OffsetDateTime::from_unix_timestamp(decoded.expiry_seconds)
            .map_err(|_| invalid_record())?
            .replace_nanosecond(decoded.expiry_nanoseconds)
            .map_err(|_| invalid_record())?;
        let record = Self {
            id,
            data: decoded.attributes,
            expiry_date,
        };
        record.validate()?;
        Ok(record)
    }

    pub(super) fn validate(&self) -> Result<()> {
        if !self.data.is_object() && !self.data.is_array() {
            return Err(invalid_record());
        }
        crate::support::data::validate_limits(&self.data).map_err(|_| limit_error())
    }
}

/// 可由外部crate实现的唯一Session store接口。
///
/// `max_bytes`要求load在解码或复制完整载荷前限制读取；框架仍会验证返回记录。
/// 实现不得将Update的Missing自动改成插入。错误不得包含凭据或原始属性。
pub trait SessionStore: Send + Sync + 'static {
    /// 有界加载ID的记录。驱动可返回过期记录供运行时区分“存在但已过期”。
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max_bytes: usize,
        empty_expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>>;

    /// 根据模式原子写入，并返回碰撞或撤销信息。
    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max_bytes: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>>;

    /// 撤销指定ID；重复删除成功，不得因此重建记录。
    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>>;
}

pub(super) fn digest_id(id: &SessionId) -> String {
    format!("{:x}", Sha256::digest(id.as_str().as_bytes()))
}

pub(super) fn limit_error() -> Error {
    Error::custom_with_status(
        http::StatusCode::PAYLOAD_TOO_LARGE,
        "session_limits_exceeded",
        "Session resource limits exceeded",
    )
}

pub(super) fn invalid_record() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "session_store_data_invalid",
        "Stored Session data is invalid",
    )
}

pub(super) fn unavailable() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "session_store_unavailable",
        "Session storage is unavailable",
    )
}

struct LimitedWriter {
    data: Vec<u8>,
    maximum: usize,
    exceeded: bool,
}

impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.data.len()) {
            self.exceeded = true;
            return Err(std::io::Error::other("Session byte limit exceeded"));
        }
        self.data.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// 先于Serde的递归限制；资源错误不能被当成损坏JSON的匿名fallback。
fn guard_json_depth(bytes: &[u8]) -> Result<()> {
    let mut depth = 0_usize;
    let mut quoted = false;
    let mut escaped = false;
    for &byte in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    depth += 1;
                    // 持久envelope比属性根多一层，另为最大深度边界留容器层。
                    if depth > 67 {
                        return Err(limit_error());
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(())
}

pub(super) fn value_from<T: Serialize>(value: &T, maximum: usize) -> Result<Value> {
    let mut writer = LimitedWriter {
        data: Vec::new(),
        maximum,
        exceeded: false,
    };
    let result = serde_json::to_writer(&mut writer, &crate::support::Finite(value));
    if writer.exceeded {
        return Err(limit_error());
    }
    result.map_err(|_| {
        Error::custom_with_status(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "session_data_invalid",
            "Session value cannot be encoded",
        )
    })?;
    guard_json_depth(&writer.data)?;
    let value: Value = serde_json::from_slice(&writer.data).map_err(|_| invalid_record())?;
    crate::support::data::validate_limits(&value).map_err(|_| limit_error())?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_bind_case_sensitive_hash_and_limit_before_json_decode() -> Result<()> {
        let id = SessionId::parse(&"a".repeat(40))?;
        let record = SessionRecord {
            id: id.clone(),
            data: serde_json::json!({"token":"  keep  ","x":null}),
            expiry_date: OffsetDateTime::from_unix_timestamp(1_800_000_000)
                .map_err(Error::internal)?,
        };
        let bytes = record.encode(DEFAULT_MAX_BYTES)?;
        assert!(!String::from_utf8_lossy(&bytes).contains(id.as_str()));
        let decoded =
            SessionRecord::decode(id.clone(), &bytes, record.expiry_date, DEFAULT_MAX_BYTES)?;
        assert_eq!(decoded.data, record.data);
        assert!(
            SessionRecord::decode(
                SessionId::parse(&"A".repeat(40))?,
                &bytes,
                record.expiry_date,
                DEFAULT_MAX_BYTES
            )
            .is_err()
        );
        assert!(record.encode(bytes.len() - 1).is_err());
        assert!(
            SessionRecord::decode(id.clone(), &bytes, record.expiry_date, bytes.len() - 1).is_err()
        );
        assert!(
            SessionRecord::decode(id.clone(), b"{}", record.expiry_date, DEFAULT_MAX_BYTES)
                .is_err()
        );
        let anonymous =
            SessionRecord::decode(id.clone(), b"{", record.expiry_date, DEFAULT_MAX_BYTES)?;
        assert_eq!(anonymous.data, serde_json::json!([]));
        let deep = format!("{}0{}", "[".repeat(129), "]".repeat(129));
        assert!(
            SessionRecord::decode(id, deep.as_bytes(), record.expiry_date, DEFAULT_MAX_BYTES)
                .is_err()
        );
        Ok(())
    }
}
