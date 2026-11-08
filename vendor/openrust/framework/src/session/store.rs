//! 唯一 Session driver 扩展契约及有界持久记录编码。

use std::{fmt, io::Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use super::SessionId;
use crate::{Error, Result, support::BoxFuture};

/// 默认整个持久记录（含身份绑定与属性）的字节上限。
pub const DEFAULT_MAX_BYTES: usize = crate::config::SessionConfig::MAX_BYTES;

/// 驱动能够兑现的存储保证；不是部署拓扑或跨节点共享声明。
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SessionStoreGuarantees {
    /// 当前后端的条件写入与删除；内存驱动仍仅限当前实例。
    ServerSide,
    /// 浏览器快照与本请求观察；旧副本可重放，响应接收顺序决定最终 Cookie。
    ClientSideSnapshot,
    /// 原生数据库 handler；DELETE 后的并发 INSERT 回退遵循 Laravel，不提供防复活承诺。
    NativeDatabase,
}

/// 同一 Session 内核使用的持久语义；增强能力必须显式选择。
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SessionStoreSemantics {
    /// 身份 envelope、绝对截止及条件写入。
    Guarded,
    /// 原始 ID、属性载荷及 handler 请求内 exists 状态。
    NativeDatabase,
}

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
    /// 驱动已处理写入；原生数据库遵循 Laravel，UPDATE 零行也可成功。
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
    /// 增强记录的持久截止；原生数据库以实际 `last_activity` 和最终配置判断过期。
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
    pub(crate) fn validate_for(
        &self,
        semantics: SessionStoreSemantics,
        max_bytes: usize,
    ) -> Result<()> {
        self.validate()?;
        match semantics {
            SessionStoreSemantics::Guarded => {
                self.encode(max_bytes)?;
            }
            SessionStoreSemantics::NativeDatabase => {
                json_bytes(&self.data, max_bytes)?;
            }
        }
        Ok(())
    }
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

/// 经实际驱动验证来源的只读凭据能力。数据库驱动须使用传入的账户事务；
/// 内存、Redis 等独立服务器存储保留实际读取时点以及真实过期时间。
#[cfg(feature = "auth-fortify")]
pub trait CredentialSessionReader: Send + Sync {
    /// 有界读取指定 Session 的身份和原截止时间，不续期、不新建记录。
    fn load_authorized_for_credential<'a>(
        &'a self,
        tx: &'a crate::database::DatabaseTransaction,
        id: &'a SessionId,
        maximum: usize,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>>;
}

/// 可由外部crate实现的唯一Session store接口。
///
/// `max_bytes`要求load在解码或复制完整载荷前限制读取；框架仍会验证返回记录。
/// Guarded 实现不得将 Update 的 Missing 改成插入；NativeDatabase 按 handler exists。
/// 错误不得包含凭据或原始属性。
pub trait SessionStore: Send + Sync + 'static {
    /// 持久语义；默认保留既有增强 driver 的严格契约。
    fn semantics(&self) -> SessionStoreSemantics {
        SessionStoreSemantics::Guarded
    }

    /// 每个受管 Session 独占的 handler 实例；无请求内状态的驱动返回 None。
    fn for_session(&self) -> Result<Option<std::sync::Arc<dyn SessionStore>>> {
        Ok(None)
    }

    /// 原生数据库活动期限；其他驱动使用记录自身的绝对截止。
    fn activity_lifetime_seconds(&self) -> Option<i64> {
        None
    }

    /// 迁移 ID 时同步清除 handler 的 exists；不能在此执行异步 I/O。
    fn reset_exists(&self) -> Result<()> {
        Ok(())
    }

    /// 将本次实际 Guard 身份投影到原生数据库元数据；SQL NULL 表示访客。
    ///
    /// 返回是否接受投影。框架在原生数据库驱动不支持时明确拒绝保存；
    /// 其他驱动不调用此接口，身份不会写入 Session 属性载荷。
    ///
    /// # Errors
    ///
    /// 驱动状态不可更新时返回错误；不得记录身份值。
    fn set_database_user_id(&self, _user_id: sea_orm::Value) -> Result<bool> {
        Ok(false)
    }

    /// 当前后端提供的保证；请求绑定前后必须一致。
    fn guarantees(&self) -> SessionStoreGuarantees {
        SessionStoreGuarantees::ServerSide
    }

    /// 是否能保证 Session 身份属性的完整性；服务器后端默认可以，客户端快照默认拒绝。
    ///
    /// 第三方客户端驱动返回 true 时，必须自行验证完整性；此声明不替代验证或服务器撤销。
    fn supports_session_authentication(&self, _cookie_name: &str) -> bool {
        self.guarantees() == SessionStoreGuarantees::ServerSide
    }

    /// 是否必须先绑定本请求上下文才可执行 I/O。
    fn needs_request(&self) -> bool {
        false
    }

    /// 接收可选 HTTP 元数据；不等于缺少 HTTP 请求就拒绝独立使用。
    fn accepts_request(&self) -> bool {
        self.needs_request()
    }

    /// 创建同一逻辑后端的请求内实例；不得修改单例或跨请求保存上下文。
    ///
    /// 需要请求的实现必须返回 Some；普通驱动不会被框架调用此方法。
    fn bind_request(
        &self,
        _context: super::SessionRequestContext,
    ) -> Result<Option<std::sync::Arc<dyn SessionStore>>> {
        Ok(None)
    }

    /// 回收已过期记录并返回删除数；默认由后端 TTL 或外部机制负责。
    ///
    /// 原生数据库按 `lifetime_seconds` 回溯活动时间；Guarded 不据此缩短绝对截止。取消不后台续跑，
    /// 已提交的外部删除不回滚；该操作不提供请求锁或跨后端事务。
    fn gc(&self, _lifetime_seconds: i64) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async { Ok(0) })
    }

    /// 有界加载ID的记录。驱动可返回过期记录供运行时区分“存在但已过期”。
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max_bytes: usize,
        empty_expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>>;

    /// 从实际驱动创建不保留请求slot的只读认证reader；None表示使用实际驱动自身。
    ///
    /// 默认无请求依赖的服务器端驱动可直接使用原Arc。需请求上下文的驱动若不
    /// 提供独立reader，后续 `load_authorized` 仍须明确拒绝，不能回落另一store域。
    fn independent_authorization_reader(&self) -> Result<Option<std::sync::Arc<dyn SessionStore>>> {
        Ok(None)
    }

    /// 独立请求生命周期的只读认证加载；返回实际存储截止，不续期。
    ///
    /// 默认只接受不依赖请求上下文的服务器端驱动；原生数据库须显式实现同次
    /// 读取的载荷与活动截止，客户端快照不具此能力。此接口不提升驱动原有的
    /// 删除/并发写保证，也不承诺撤销与后续网络发送线性化。
    fn load_authorized<'a>(
        &'a self,
        id: &'a SessionId,
        max_bytes: usize,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(async move {
            if self.guarantees() != SessionStoreGuarantees::ServerSide
                || self.needs_request()
                || self.accepts_request()
            {
                return Err(unsupported_guarantee());
            }
            self.load(id, max_bytes, OffsetDateTime::now_utc()).await
        })
    }

    /// 为指定账户库选择父 Session 的复核能力；新登录与已保存身份都必须先通过此来源检查。
    #[cfg(feature = "auth-fortify")]
    fn credential_parent_reader(
        &self,
        _db: &crate::database::Db,
    ) -> Result<&dyn CredentialSessionReader> {
        Err(crate::Error::custom_with_status(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "session_credential_parent_reader_unsupported",
            "Session 驱动未提供凭据派生所需的持久身份复核",
        ))
    }

    /// `ServerSide` 按模式原子写入；原生数据库按 exists；快照仅检查本请求观察。
    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max_bytes: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>>;

    /// 删除指定 ID；`ServerSide` 撤销后不得重建，快照驱动仅忘记当前响应 Cookie。
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
pub(super) fn guard_json_depth(bytes: &[u8]) -> Result<()> {
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

pub(super) fn json_bytes(value: &impl Serialize, maximum: usize) -> Result<Vec<u8>> {
    let mut writer = LimitedWriter {
        data: Vec::new(),
        maximum,
        exceeded: false,
    };
    let result = serde_json::to_writer(&mut writer, value);
    if writer.exceeded {
        return Err(limit_error());
    }
    result.map_err(|_| invalid_record())?;
    Ok(writer.data)
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

/// 请求所需的服务器撤销保证不可由浏览器快照兑现。
pub(super) fn unsupported_guarantee() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "session_store_guarantee_unsupported",
        "Session 驱动无法提供服务器端撤销保证",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SnapshotDeclaration;
    impl SessionStore for SnapshotDeclaration {
        fn guarantees(&self) -> SessionStoreGuarantees {
            SessionStoreGuarantees::ClientSideSnapshot
        }
        fn load<'a>(
            &'a self,
            _id: &'a SessionId,
            _max: usize,
            _expiry: OffsetDateTime,
        ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
            Box::pin(async { Err(unavailable()) })
        }
        fn write<'a>(
            &'a self,
            _record: &'a SessionRecord,
            _mode: SessionWriteMode,
            _max: usize,
        ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
            Box::pin(async { Err(unavailable()) })
        }
        fn delete<'a>(&'a self, _id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
            Box::pin(async { Err(unavailable()) })
        }
    }
    struct SelfVerifiedDeclaration;
    impl SessionStore for SelfVerifiedDeclaration {
        fn guarantees(&self) -> SessionStoreGuarantees {
            SessionStoreGuarantees::ClientSideSnapshot
        }
        // 只核对受信任实现的公开声明边界，不将该声明探针当成已实现的客户端驱动。
        fn supports_session_authentication(&self, _name: &str) -> bool {
            true
        }
        fn load<'a>(
            &'a self,
            _id: &'a SessionId,
            _max: usize,
            _expiry: OffsetDateTime,
        ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
            Box::pin(async { Err(unavailable()) })
        }
        fn write<'a>(
            &'a self,
            _record: &'a SessionRecord,
            _mode: SessionWriteMode,
            _max: usize,
        ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
            Box::pin(async { Err(unavailable()) })
        }
        fn delete<'a>(&'a self, _id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
            Box::pin(async { Err(unavailable()) })
        }
    }
    #[test]
    fn client_snapshot_authentication_defaults_to_refusal_and_requires_a_trusted_declaration() {
        assert!(!SnapshotDeclaration.supports_session_authentication("session"));
        assert!(SelfVerifiedDeclaration.supports_session_authentication("session"));
        assert!(
            super::super::MemorySessionStore::default().supports_session_authentication("session")
        );
    }

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
