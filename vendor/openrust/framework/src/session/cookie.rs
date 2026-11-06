//! 无服务器影子记录的双 Cookie Session 驱动。

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};

use super::{
    SessionId, SessionRecord, SessionRequestContext, SessionStore, SessionStoreGuarantees,
    SessionWriteMode, SessionWriteOutcome,
    store::{guard_json_depth, limit_error, unavailable},
};
use crate::Error;
use crate::Result;
use crate::http::CookieOptions;
use crate::support::BoxFuture;

const MAX_IDS: usize = 1024;
const MAX_COOKIE_BYTES: usize = 64 * 1024;

/// Cookie Session：名称 Cookie 指向 ID，ID Cookie 保存有界 JSON 属性字符串。
///
/// 仅提供本请求快照与当前响应的写入/forget。其他客户端持有的副本可在到期前
/// 重放，并发响应由浏览器最后接收者决定；不支持服务器全局撤销。
/// Cookie 的保护与历史密钥由外层 Cookie Provider 统一负责。
pub struct CookieSessionStore {
    lifetime_minutes: i64,
    lifetime_seconds: i64,
    expire_on_close: bool,
    request: Option<BoundRequest>,
}

struct BoundRequest {
    context: SessionRequestContext,
    path: String,
    ids: Mutex<HashMap<SessionId, LocalId>>,
}

#[derive(Clone, Copy)]
enum LocalId {
    Written(i64, [u8; 32]),
    Deleted,
}

#[derive(Serialize)]
struct Payload<'a> {
    data: &'a str,
    expires: i64,
}

impl std::fmt::Debug for CookieSessionStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CookieSessionStore")
            .finish_non_exhaustive()
    }
}

impl CookieSessionStore {
    /// 使用正数分钟期限构造；关闭浏览器时删除不取消载荷中的到期上界。
    pub fn new(lifetime_minutes: u64, expire_on_close: bool) -> Result<Self> {
        let invalid = || Error::custom("session_config_invalid", "Cookie Session 的有效期无效");
        let lifetime_seconds = lifetime_minutes
            .checked_mul(60)
            .and_then(|seconds| i64::try_from(seconds).ok())
            .filter(|seconds| *seconds > 0)
            .ok_or_else(invalid)?;
        OffsetDateTime::now_utc()
            .checked_add(Duration::seconds(lifetime_seconds))
            .and_then(|date| date.checked_add(Duration::seconds(1)))
            .ok_or_else(invalid)?;
        Ok(Self {
            lifetime_minutes: i64::try_from(lifetime_minutes).map_err(|_| invalid())?,
            lifetime_seconds,
            expire_on_close,
            request: None,
        })
    }

    fn bound(&self) -> Result<&BoundRequest> {
        self.request.as_ref().ok_or_else(|| {
            Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "session_request_unbound",
                "Session 驱动需要绑定当前请求",
            )
        })
    }
}

impl BoundRequest {
    // 锁序固定为本驱动 metadata → CookieJar；临界区无 await 或应用回调。
    fn lock(&self) -> Result<MutexGuard<'_, HashMap<SessionId, LocalId>>> {
        self.ids.lock().map_err(|_| unavailable())
    }

    fn record(
        &self,
        id: &SessionId,
        ids: &HashMap<SessionId, LocalId>,
        max_bytes: usize,
    ) -> Result<Option<SessionRecord>> {
        let local = ids.get(id);
        if matches!(local, Some(LocalId::Deleted)) {
            return Ok(None);
        }
        let queued = self
            .context
            .cookie_jar()
            .queued(id.as_str(), Some(&self.path))?;
        let value = if let Some(LocalId::Written(_, digest)) = local {
            // 新创建 ID 的属性只在正式响应队列中，摘要证明队列仍是本驱动的输出。
            let cookie = queued.as_ref().ok_or_else(payload_invalid)?;
            let Some(value) = cookie.value() else {
                return Ok(None);
            };
            if payload_digest(value) != *digest {
                return Err(payload_invalid());
            }
            Some(value.to_owned())
        } else {
            match self.context.cookie(id.as_str()) {
                Some(Value::String(value)) => Some(value),
                None => None,
                Some(_) => return Err(payload_invalid()),
            }
        };
        let Some(value) = value else { return Ok(None) };
        let cap = match local {
            Some(LocalId::Written(expiry, _)) => Some(*expiry),
            _ => None,
        };
        decode(
            id,
            &value,
            cap,
            max_bytes,
            OffsetDateTime::now_utc().unix_timestamp(),
        )
    }

    fn capacity(ids: &HashMap<SessionId, LocalId>, id: &SessionId) -> Result<()> {
        if !ids.contains_key(id) && ids.len() >= MAX_IDS {
            return Err(limit_error());
        }
        Ok(())
    }
}

impl SessionStore for CookieSessionStore {
    fn guarantees(&self) -> SessionStoreGuarantees {
        SessionStoreGuarantees::ClientSideSnapshot
    }
    fn supports_session_authentication(&self, cookie_name: &str) -> bool {
        self.request
            .as_ref()
            .is_some_and(|request| request.context.protects_session(cookie_name))
    }
    fn needs_request(&self) -> bool {
        true
    }
    fn bind_request(
        &self,
        context: SessionRequestContext,
    ) -> Result<Option<Arc<dyn SessionStore>>> {
        let path = context
            .cookie_jar()
            .factory()
            .forget("session_data", CookieOptions::default())?
            .path()
            .to_owned();
        Ok(Some(Arc::new(Self {
            lifetime_minutes: self.lifetime_minutes,
            lifetime_seconds: self.lifetime_seconds,
            expire_on_close: self.expire_on_close,
            request: Some(BoundRequest {
                context,
                path,
                ids: Mutex::new(HashMap::new()),
            }),
        })))
    }
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max_bytes: usize,
        _empty_expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(async move {
            let request = self.bound()?;
            let ids = request.lock()?;
            request.record(id, &ids, max_bytes)
        })
    }
    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max_bytes: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async move {
            let request = self.bound()?;
            record.encode(max_bytes)?;
            let now = OffsetDateTime::now_utc().unix_timestamp();
            let expires = now
                .checked_add(self.lifetime_seconds)
                .ok_or_else(payload_invalid)?
                .min(record.expiry_date.unix_timestamp());
            deadline(expires)?;
            let data = super::store::json_bytes(&record.data, max_bytes.min(MAX_COOKIE_BYTES))?;
            let data = std::str::from_utf8(&data).map_err(|_| payload_invalid())?;
            let payload = super::store::json_bytes(&Payload { data, expires }, MAX_COOKIE_BYTES)?;
            let payload = String::from_utf8(payload).map_err(|_| payload_invalid())?;
            let digest = payload_digest(&payload);
            let cookie = request.context.cookie_jar().factory().make(
                record.id.as_str(),
                payload,
                if self.expire_on_close {
                    0
                } else {
                    self.lifetime_minutes
                },
                CookieOptions::default(),
            )?;
            let mut ids = request.lock()?;
            BoundRequest::capacity(&ids, &record.id)?;
            let queued_delete = request
                .context
                .cookie_jar()
                .queued(record.id.as_str(), Some(&request.path))?
                .is_some_and(|cookie| cookie.value().is_none());
            if matches!(ids.get(&record.id), Some(LocalId::Deleted))
                || queued_delete
                || expires < now
            {
                return Ok(SessionWriteOutcome::Missing);
            }
            // 未由本驱动发布的出站同名值不是入站证明，也不能被悄悄覆盖。
            if !matches!(ids.get(&record.id), Some(LocalId::Written(_, _)))
                && request
                    .context
                    .cookie_jar()
                    .queued(record.id.as_str(), Some(&request.path))?
                    .is_some()
            {
                return Ok(SessionWriteOutcome::Conflict);
            }
            let exists = request.record(&record.id, &ids, max_bytes)?.is_some();
            match (mode, exists) {
                (SessionWriteMode::Insert, true) => return Ok(SessionWriteOutcome::Conflict),
                (SessionWriteMode::Update, false) => return Ok(SessionWriteOutcome::Missing),
                _ => {}
            }
            request.context.cookie_jar().queue(cookie)?;
            // queue 和 metadata 发布间没有 await；失败不生成伪造的写入证明。
            ids.insert(record.id.clone(), LocalId::Written(expires, digest));
            Ok(SessionWriteOutcome::Written)
        })
    }
    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let request = self.bound()?;
            let cookie = request
                .context
                .cookie_jar()
                .factory()
                .forget(id.as_str(), CookieOptions::default())?;
            let mut ids = request.lock()?;
            BoundRequest::capacity(&ids, id)?;
            request.context.cookie_jar().queue(cookie)?;
            ids.insert(id.clone(), LocalId::Deleted);
            Ok(())
        })
    }
}

fn payload_digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

fn deadline(expires: i64) -> Result<OffsetDateTime> {
    expires
        .checked_add(1)
        .and_then(|value| OffsetDateTime::from_unix_timestamp(value).ok())
        .ok_or_else(payload_invalid)
}

fn decode(
    id: &SessionId,
    value: &str,
    cap: Option<i64>,
    max_bytes: usize,
    now: i64,
) -> Result<Option<SessionRecord>> {
    if value.len() > MAX_COOKIE_BYTES {
        return Err(limit_error());
    }
    guard_json_depth(value.as_bytes())?;
    let Ok(Value::Object(payload)) = serde_json::from_str::<Value>(value) else {
        return Ok(None);
    };
    let Some(expiry) = payload.get("expires").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let expires = expiry.as_i64().ok_or_else(payload_invalid)?;
    let expiry_date = deadline(expires)?;
    let expires = cap.map_or(expires, |cap| expires.min(cap));
    if now > expires {
        return Ok(None);
    }
    let data = payload
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(payload_invalid)?;
    if data.len() > max_bytes {
        return Err(limit_error());
    }
    guard_json_depth(data.as_bytes())?;
    let data = match serde_json::from_str::<Value>(data) {
        Ok(data) if data.is_object() || data.is_array() => data,
        _ => Value::Array(Vec::new()),
    };
    let record = SessionRecord {
        id: id.clone(),
        data,
        expiry_date: expiry_date.min(deadline(expires)?),
    };
    record.encode(max_bytes)?;
    Ok(Some(record))
}

fn payload_invalid() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "session_cookie_payload_invalid",
        "Cookie Session 载荷无效",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_expiry_is_inclusive_at_the_exact_second_and_checked() -> Result<()> {
        let id = SessionId::parse(&"a".repeat(40))?;
        let value = r#"{"data":"{\"x\":1}","expires":1800000000}"#;
        let record = decode(
            &id,
            value,
            None,
            super::super::DEFAULT_MAX_BYTES,
            1_800_000_000,
        )?
        .ok_or_else(payload_invalid)?;
        assert_eq!(record.expiry_date.unix_timestamp(), 1_800_000_001);
        assert_eq!(record.data, serde_json::json!({"x": 1}));
        assert!(
            decode(
                &id,
                value,
                None,
                super::super::DEFAULT_MAX_BYTES,
                1_800_000_001
            )?
            .is_none()
        );
        assert!(deadline(i64::MAX).is_err());
        assert!(CookieSessionStore::new(0, false).is_err());
        assert!(CookieSessionStore::new(u64::MAX, false).is_err());
        Ok(())
    }
    #[tokio::test]
    async fn only_driver_written_queue_values_can_replace_the_inbound_snapshot() -> Result<()> {
        let id = SessionId::parse(&"a".repeat(40))?;
        let expiry = OffsetDateTime::now_utc() + Duration::minutes(5);
        let payload = |value| -> Result<String> {
            serde_json::to_string(&serde_json::json!({"data": format!("{{\"value\":{value}}}"), "expires": expiry.unix_timestamp()}))
                .map_err(Error::internal)
        };
        for inbound in [true, false] {
            let jar = crate::http::CookieJar::new(crate::http::CookieFactory::default());
            let mut cookies = serde_json::Map::new();
            if inbound {
                cookies.insert(id.as_str().to_owned(), Value::String(payload(1)?));
            }
            let driver = CookieSessionStore::new(5, false)?
                .bind_request(SessionRequestContext::new(
                    Value::Object(cookies),
                    jar.clone(),
                )?)?
                .ok_or_else(payload_invalid)?;
            jar.queue(jar.factory().make(
                id.as_str(),
                payload(2)?,
                5,
                CookieOptions::default(),
            )?)?;
            let loaded = driver
                .load(&id, super::super::DEFAULT_MAX_BYTES, expiry)
                .await?;
            if inbound {
                assert_eq!(
                    loaded.ok_or_else(payload_invalid)?.data,
                    serde_json::json!({"value": 1})
                );
            } else {
                assert!(loaded.is_none());
            }
            let record = SessionRecord {
                id: id.clone(),
                data: serde_json::json!({"value": 3}),
                expiry_date: expiry,
            };
            assert_eq!(
                driver
                    .write(
                        &record,
                        if inbound {
                            SessionWriteMode::Update
                        } else {
                            SessionWriteMode::Insert
                        },
                        super::super::DEFAULT_MAX_BYTES
                    )
                    .await?,
                SessionWriteOutcome::Conflict
            );
            jar.unqueue(id.as_str(), None)?;
            assert_eq!(
                driver
                    .write(
                        &record,
                        if inbound {
                            SessionWriteMode::Update
                        } else {
                            SessionWriteMode::Insert
                        },
                        super::super::DEFAULT_MAX_BYTES
                    )
                    .await?,
                SessionWriteOutcome::Written
            );
            let loaded = driver
                .load(&id, super::super::DEFAULT_MAX_BYTES, expiry)
                .await?
                .ok_or_else(payload_invalid)?;
            assert_eq!(loaded.data, record.data);
        }
        Ok(())
    }
    #[tokio::test]
    async fn replacing_driver_output_breaks_its_proof_without_publishing_a_write() -> Result<()> {
        let id = SessionId::parse(&"a".repeat(40))?;
        let expiry = OffsetDateTime::now_utc() + Duration::minutes(5);
        let jar = crate::http::CookieJar::new(crate::http::CookieFactory::default());
        let driver = CookieSessionStore::new(5, false)?
            .bind_request(SessionRequestContext::new(
                serde_json::json!({}),
                jar.clone(),
            )?)?
            .ok_or_else(payload_invalid)?;
        let record = SessionRecord {
            id: id.clone(),
            data: serde_json::json!({"value": 1}),
            expiry_date: expiry,
        };
        assert_eq!(
            driver
                .write(
                    &record,
                    SessionWriteMode::Insert,
                    super::super::DEFAULT_MAX_BYTES
                )
                .await?,
            SessionWriteOutcome::Written
        );
        let original = jar.queued(id.as_str(), None)?.ok_or_else(payload_invalid)?;
        let changed = serde_json::to_string(
            &serde_json::json!({"data": "{\"value\":2}", "expires": expiry.unix_timestamp()}),
        )
        .map_err(Error::internal)?;
        jar.queue(original.with_value(changed)?)?;
        assert!(
            driver
                .load(&id, super::super::DEFAULT_MAX_BYTES, expiry)
                .await
                .is_err()
        );
        assert!(
            driver
                .write(
                    &record,
                    SessionWriteMode::Update,
                    super::super::DEFAULT_MAX_BYTES
                )
                .await
                .is_err()
        );
        jar.queue(original)?;
        assert_eq!(
            driver
                .load(&id, super::super::DEFAULT_MAX_BYTES, expiry)
                .await?
                .ok_or_else(payload_invalid)?
                .data,
            record.data
        );
        assert_eq!(
            driver
                .write(
                    &record,
                    SessionWriteMode::Update,
                    super::super::DEFAULT_MAX_BYTES
                )
                .await?,
            SessionWriteOutcome::Written
        );
        jar.expire(id.as_str(), CookieOptions::default())?;
        assert!(
            driver
                .load(&id, super::super::DEFAULT_MAX_BYTES, expiry)
                .await?
                .is_none()
        );
        assert_eq!(
            driver
                .write(
                    &record,
                    SessionWriteMode::Update,
                    super::super::DEFAULT_MAX_BYTES
                )
                .await?,
            SessionWriteOutcome::Missing
        );
        Ok(())
    }
}
