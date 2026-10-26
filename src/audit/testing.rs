//! 用于应用和集成测试的审计 fake sink。

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use super::{AuditOutcome, AuditRecord, AuditSink};
use crate::{BoxFuture, Error, Result};

/// 在内存中捕获已脱敏记录，并可模拟写入失败的 sink。
#[derive(Clone, Default)]
pub struct FakeAuditSink {
    records: Arc<Mutex<Vec<FakeAuditRecord>>>,
    fail: Arc<AtomicBool>,
}

impl std::fmt::Debug for FakeAuditSink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FakeAuditSink")
    }
}

impl FakeAuditSink {
    /// 创建一个可接收记录的空 sink。
    pub fn new() -> Self {
        Self::default()
    }

    /// 返回已捕获脱敏记录的快照。
    pub fn records(&self) -> Result<Vec<FakeAuditRecord>> {
        self.records
            .lock()
            .map(|records| records.clone())
            .map_err(|_| Error::internal(std::io::Error::other("fake audit sink lock poisoned")))
    }

    /// 使后续追加操作失败，以测试 sink 的错误处理。
    pub fn fail_appends(&self, fail: bool) {
        self.fail.store(fail, Ordering::SeqCst);
    }
}

impl AuditSink for FakeAuditSink {
    fn append<'a>(&'a self, record: &'a AuditRecord) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if self.fail.load(Ordering::SeqCst) {
                return Err(Error::internal(std::io::Error::other(
                    "fake audit sink failure",
                )));
            }
            let captured = FakeAuditRecord {
                actor_kind: record.actor_kind.clone(),
                actor_id: record.actor_id.clone(),
                action: record.action.clone(),
                object_kind: record.object_kind.clone(),
                object_id: record.object_id.clone(),
                outcome: record.outcome,
                request_id: record.request_id.clone(),
                source_ip: record.source_ip.clone(),
                details_json: record.details_json.clone(),
            };
            self.records
                .lock()
                .map_err(|_| {
                    Error::internal(std::io::Error::other("fake audit sink lock poisoned"))
                })?
                .push(captured);
            Ok(())
        })
    }
}

/// [`FakeAuditSink`] 捕获的脱敏记录快照。
#[derive(Clone, PartialEq, Eq)]
pub struct FakeAuditRecord {
    actor_kind: Option<String>,
    actor_id: Option<String>,
    action: String,
    object_kind: String,
    object_id: String,
    outcome: AuditOutcome,
    request_id: Option<String>,
    source_ip: Option<String>,
    details_json: String,
}

impl std::fmt::Debug for FakeAuditRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FakeAuditRecord")
            .field("actor_kind", &self.actor_kind)
            .field("actor_id", &self.actor_id.as_ref().map(|_| "[REDACTED]"))
            .field("action", &self.action)
            .field("object_kind", &self.object_kind)
            .field("object_id", &"[REDACTED]")
            .field("outcome", &self.outcome)
            .field(
                "request_id",
                &self.request_id.as_ref().map(|_| "[REDACTED]"),
            )
            .field("source_ip", &self.source_ip)
            .field("details", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl FakeAuditRecord {
    /// 如果提供了 actor，则返回其类型。
    pub fn actor_kind(&self) -> Option<&str> {
        self.actor_kind.as_deref()
    }
    /// 如果提供了 actor，则返回其 ID。
    pub fn actor_id(&self) -> Option<&str> {
        self.actor_id.as_deref()
    }
    /// 返回记录的操作。
    pub fn action(&self) -> &str {
        &self.action
    }
    /// 返回对象类型。
    pub fn object_kind(&self) -> &str {
        &self.object_kind
    }
    /// 返回对象 ID。
    pub fn object_id(&self) -> &str {
        &self.object_id
    }
    /// 返回记录的结果。
    pub fn outcome(&self) -> AuditOutcome {
        self.outcome
    }
    /// 返回请求关联 ID。
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
    /// 返回可信来源 IP 字符串。
    pub fn source_ip(&self) -> Option<&str> {
        self.source_ip.as_deref()
    }
    /// 返回经过策略筛选和脱敏的详情。
    pub fn details_json(&self) -> &str {
        &self.details_json
    }
}
