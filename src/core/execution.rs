//! 单次 HTTP、命令或任务执行的关联信息、期限和协作取消。

use std::{future::Future, time::Duration};

use crate::{
    core::{Error, Result},
    i18n::Locale,
};
use serde::{Deserialize, Serialize};
use tokio::{sync::watch, time::Instant};

/// 可在队列任务间传递的非敏感关联元数据。
///
/// 仅包含有界 request ID、规范化 locale 与可信租户 ID，不包含身份、权限快照、deadline、取消状态或请求头。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ExecutionMetadataWire")]
pub struct ExecutionMetadata {
    request_id: Option<String>,
    locale: Option<Locale>,
    tenant_id: Option<crate::tenant::TenantId>,
}

#[derive(Deserialize)]
struct ExecutionMetadataWire {
    request_id: Option<String>,
    locale: Option<Locale>,
    tenant_id: Option<String>,
}

impl TryFrom<ExecutionMetadataWire> for ExecutionMetadata {
    type Error = String;

    fn try_from(value: ExecutionMetadataWire) -> std::result::Result<Self, Self::Error> {
        if value
            .request_id
            .as_deref()
            .is_some_and(|request_id| !valid_request_id(request_id))
        {
            return Err("request ID must contain 1..=128 safe ASCII characters".to_owned());
        }
        let tenant_id = value
            .tenant_id
            .map(crate::tenant::TenantId::new)
            .transpose()
            .map_err(|_| "tenant ID is invalid".to_owned())?;
        Ok(Self {
            request_id: value.request_id,
            locale: value.locale,
            tenant_id,
        })
    }
}

impl ExecutionMetadata {
    /// 设置可跨任务关联的 request ID。
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Result<Self> {
        let request_id = request_id.into();
        if !valid_request_id(&request_id) {
            return Err(Error::custom(
                "invalid_execution_request_id",
                "request ID must contain 1..=128 safe ASCII characters",
            ));
        }
        self.request_id = Some(request_id);
        Ok(self)
    }

    /// 设置此调用使用的规范化 locale。
    #[must_use]
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = Some(locale);
        self
    }

    /// 设置可跨队列任务传播的可信租户 ID。
    #[must_use]
    pub fn with_tenant_id(mut self, tenant_id: crate::tenant::TenantId) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }

    /// 返回 request ID；它只用于日志关联，不是 W3C trace parent。
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// 返回显式 locale。
    pub fn locale(&self) -> Option<&Locale> {
        self.locale.as_ref()
    }

    /// 返回显式传播的可信租户 ID。
    pub fn tenant_id(&self) -> Option<&crate::tenant::TenantId> {
        self.tenant_id.as_ref()
    }

    /// 返回是否没有可传递的关联元数据。
    pub fn is_empty(&self) -> bool {
        self.request_id.is_none() && self.locale.is_none() && self.tenant_id.is_none()
    }
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
}

/// 可由父调用和其子任务共享的合作取消信号。
#[derive(Clone)]
pub struct Cancellation {
    local: watch::Sender<bool>,
    parent: Option<std::sync::Arc<Cancellation>>,
}

impl Default for Cancellation {
    fn default() -> Self {
        Self::new()
    }
}

impl Cancellation {
    /// 创建尚未取消的信号。
    pub fn new() -> Self {
        Self {
            local: watch::channel(false).0,
            parent: None,
        }
    }

    /// 通知所有持有者尽快停止；不撤销已提交的副作用。
    pub fn cancel(&self) {
        self.local.send_replace(true);
    }

    /// 返回是否已请求取消。
    pub fn is_cancelled(&self) -> bool {
        *self.local.borrow()
            || self
                .parent
                .as_ref()
                .is_some_and(|parent| parent.is_cancelled())
    }

    /// 等待合作取消信号。
    pub async fn cancelled(&self) {
        if let Some(parent) = &self.parent {
            tokio::select! {
                () = self.local_cancelled() => {},
                () = Box::pin(parent.cancelled()) => {},
            }
        } else {
            self.local_cancelled().await;
        }
    }

    /// 创建与父调用关联的子取消信号；父级取消会传播到子级，子级取消不会影响父级或兄弟。
    pub fn child(parent: &Self) -> Self {
        Self {
            local: watch::channel(false).0,
            parent: Some(std::sync::Arc::new(parent.clone())),
        }
    }

    async fn local_cancelled(&self) {
        let mut receiver = self.local.subscribe();
        if !*receiver.borrow() {
            let _ = receiver.changed().await;
        }
    }
}

/// 单次执行使用的非共享上下文。
///
/// Tokio task-local 不会自动传播到 `tokio::spawn`；需要时应显式克隆并重新建立作用域。
#[derive(Clone)]
pub struct ExecutionContext {
    metadata: ExecutionMetadata,
    deadline: Option<Instant>,
    cancellation: Cancellation,
}

tokio::task_local! {
    static EXECUTION_CONTEXT: ExecutionContext;
}

impl ExecutionContext {
    /// 创建一次执行上下文。持久任务应传入新的取消信号和该次尝试的 deadline。
    pub fn new(
        metadata: ExecutionMetadata,
        deadline: Option<Instant>,
        cancellation: Cancellation,
    ) -> Self {
        Self {
            metadata,
            deadline,
            cancellation,
        }
    }

    /// 返回当前异步作用域中的上下文。
    pub fn current() -> Option<Self> {
        EXECUTION_CONTEXT.try_with(Clone::clone).ok()
    }

    /// 返回可持久化的非敏感关联元数据。
    pub fn metadata(&self) -> &ExecutionMetadata {
        &self.metadata
    }

    /// 返回父调用剩余预算；没有 deadline 时返回 `None`。
    pub fn remaining_budget(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    /// 返回是否已请求合作取消。
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    /// 等待合作取消信号。
    pub async fn cancelled(&self) {
        self.cancellation.cancelled().await;
    }

    /// 创建预算不超过父调用的子上下文。
    pub fn child_with_budget(&self, budget: Duration) -> Result<Self> {
        let requested_deadline = Instant::now().checked_add(budget).ok_or_else(|| {
            Error::custom("invalid_execution_budget", "execution budget is too large")
        })?;
        let deadline = Some(
            self.deadline
                .map_or(requested_deadline, |parent| parent.min(requested_deadline)),
        );
        Ok(Self {
            metadata: self.metadata.clone(),
            deadline,
            cancellation: Cancellation::child(&self.cancellation),
        })
    }

    /// 在此调用的 deadline 和取消信号内运行 future。
    ///
    /// deadline 到期会发出取消信号并返回 `execution_deadline_exceeded`；父调用取消时返回
    /// `execution_cancelled`。Future 被丢弃，已提交的外部副作用无法撤回。
    pub async fn run<F: Future>(&self, future: F) -> Result<F::Output> {
        let cancelled = self.cancellation.cancelled();
        with_execution_context(self.clone(), async {
            if let Some(deadline) = self.deadline {
                tokio::select! {
                biased;
                output = future => Ok(output),
                () = cancelled => Err(Error::custom("execution_cancelled", "execution was cancelled")),
                () = tokio::time::sleep_until(deadline) => {
                    self.cancellation.cancel();
                    Err(Error::custom("execution_deadline_exceeded", "execution deadline exceeded"))
                }
                }
            } else {
                tokio::select! {
                biased;
                output = future => Ok(output),
                () = cancelled => Err(Error::custom("execution_cancelled", "execution was cancelled")),
                }
            }
        }).await
    }
}

/// 在当前 task-local 作用域中运行 future。
pub async fn with_execution_context<F: Future>(context: ExecutionContext, future: F) -> F::Output {
    EXECUTION_CONTEXT.scope(context, future).await
}

/// Future 被 timeout 或调用方丢弃时通知合作任务。
pub(crate) struct CancelOnDrop(Cancellation);

impl CancelOnDrop {
    pub(crate) fn new(cancellation: Cancellation) -> Self {
        Self(cancellation)
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::{Cancellation, ExecutionContext, ExecutionMetadata, with_execution_context};
    use crate::{core::Error, i18n::Locale};
    use std::time::Duration;
    use tokio::time::Instant;

    fn metadata(id: &str, locale: &str) -> crate::core::Result<ExecutionMetadata> {
        Ok(ExecutionMetadata::default()
            .with_request_id(id)?
            .with_locale(Locale::new(locale)?))
    }

    #[tokio::test]
    async fn concurrent_scopes_keep_request_metadata_isolated() -> crate::core::Result<()> {
        let first = with_execution_context(
            ExecutionContext::new(metadata("request-a", "zh-CN")?, None, Cancellation::new()),
            async {
                tokio::task::yield_now().await;
                let current = ExecutionContext::current().ok_or_else(|| {
                    Error::custom("test_scope_missing", "execution scope missing")
                })?;
                Ok::<_, Error>((
                    current.metadata().request_id().map(str::to_owned),
                    current.metadata().locale().map(ToString::to_string),
                ))
            },
        );
        let second = with_execution_context(
            ExecutionContext::new(metadata("request-b", "en")?, None, Cancellation::new()),
            async {
                tokio::task::yield_now().await;
                let current = ExecutionContext::current().ok_or_else(|| {
                    Error::custom("test_scope_missing", "execution scope missing")
                })?;
                Ok::<_, Error>((
                    current.metadata().request_id().map(str::to_owned),
                    current.metadata().locale().map(ToString::to_string),
                ))
            },
        );
        let (first, second) = tokio::join!(first, second);
        assert_eq!(
            first?,
            (Some("request-a".to_owned()), Some("zh-CN".to_owned()))
        );
        assert_eq!(
            second?,
            (Some("request-b".to_owned()), Some("en".to_owned()))
        );
        Ok(())
    }

    #[tokio::test]
    async fn child_budget_never_exceeds_parent_and_expiry_cancels() -> crate::core::Result<()> {
        let parent = ExecutionContext::new(
            ExecutionMetadata::default(),
            Some(Instant::now() + Duration::from_secs(60)),
            Cancellation::new(),
        );
        let child = parent.child_with_budget(Duration::from_millis(1))?;
        assert!(
            child
                .remaining_budget()
                .is_some_and(|remaining| remaining <= Duration::from_millis(1))
        );
        let error = child.run(std::future::pending::<()>()).await.err();
        assert!(
            matches!(error, Some(Error::Custom { code, .. }) if code == "execution_deadline_exceeded")
        );
        assert!(!parent.is_cancelled());

        let expired = ExecutionContext::new(
            ExecutionMetadata::default(),
            Some(Instant::now()),
            Cancellation::new(),
        );
        let error = expired.run(std::future::pending::<()>()).await.err();
        assert!(
            matches!(error, Some(Error::Custom { code, .. }) if code == "execution_deadline_exceeded")
        );
        assert!(expired.is_cancelled());
        Ok(())
    }

    #[tokio::test]
    async fn cancellation_is_observable_to_cooperative_child() {
        let cancellation = Cancellation::new();
        let child = cancellation.clone();
        let waiting = tokio::spawn(async move { child.cancelled().await });
        tokio::task::yield_now().await;
        cancellation.cancel();
        assert!(waiting.await.is_ok());
        assert!(cancellation.is_cancelled());
    }

    #[tokio::test]
    async fn run_installs_its_context_for_the_future() -> crate::core::Result<()> {
        let context =
            ExecutionContext::new(metadata("run-scope", "zh-CN")?, None, Cancellation::new());
        let current = context
            .run(async {
                ExecutionContext::current()
                    .and_then(|value| value.metadata().request_id().map(str::to_owned))
            })
            .await?;
        assert_eq!(current.as_deref(), Some("run-scope"));
        Ok(())
    }

    #[test]
    fn metadata_rejects_unbounded_or_unsafe_request_ids() {
        assert!(
            ExecutionMetadata::default()
                .with_request_id("x\nforged")
                .is_err()
        );
        assert!(
            ExecutionMetadata::default()
                .with_request_id("x".repeat(129))
                .is_err()
        );
    }
}
