//! 账户记住我属性修改的拥有式回执；内存恢复与数据库提交分别取证。

use crate::{Result, database::eloquent::AttributeMutation};

/// 外部账户可提供自己的同步提交/恢复后端，不要求复制为内置账户模型。
///
/// 后端必须持有原始修改的所有权与代次。回滚只恢复仍归本次修改的字段，
/// 不运行用户访问器、不启动异步工作，也不能把内存恢复称作数据库回滚。
/// 两个方法及后端析构必须不panic，rollback须幂等；Err表示仍可安全重试恢复，
/// 不能在Err后继续签发Cookie。外部后端违反该契约时不承诺其原始属性已恢复。
pub trait RememberTokenMutation: Send {
    /// 确认保留已安装的属性修改，并释放其未决所有权。
    fn commit(&mut self) -> Result<()>;
    /// 同步恢复本次仍拥有的原始字段；false表示已有后继字段更新。
    fn rollback(&mut self) -> Result<bool>;
}

/// 已安装且尚未确认持久化的账户属性修改，取消时同步恢复原始补丁。
#[must_use = "必须确认提交，或保留回执直到其Drop恢复本次修改"]
pub struct RememberTokenUpdate {
    backend: Box<dyn RememberTokenMutation>,
    completed: bool,
}
impl std::fmt::Debug for RememberTokenUpdate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RememberTokenUpdate([REDACTED])")
    }
}
impl RememberTokenUpdate {
    /// 封装已经armed的拥有式后端；构造后不得再以独立通道恢复同一修改。
    pub fn new(backend: impl RememberTokenMutation + 'static) -> Self {
        Self {
            backend: Box::new(backend),
            completed: false,
        }
    }
    /// Provider成功后确认内存修改；不会再次调用账户setter。
    pub fn commit(mut self) -> Result<()> {
        // 调用前先disarm，外部后端panic时不得在展开中再次调用同一后端。
        self.completed = true;
        let result = self.backend.commit();
        if result.is_err() {
            self.completed = false;
        }
        result
    }
    /// Provider失败时同步恢复；未明确成功的恢复还会在Drop中尝试。
    pub fn rollback(&mut self) -> Result<bool> {
        if self.completed {
            return Ok(true);
        }
        self.completed = true;
        let result = self.backend.rollback();
        if result.is_err() {
            self.completed = false;
        }
        result
    }
}
impl Drop for RememberTokenUpdate {
    fn drop(&mut self) {
        if !self.completed {
            self.completed = true;
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.backend.rollback()));
            if !matches!(result, Ok(Ok(_))) {
                tracing::error!(
                    error_code = "auth_token_restore_failed",
                    "账户属性恢复后端违反可恢复契约"
                );
            }
        }
    }
}
struct ModelMutation(Option<AttributeMutation>);
impl RememberTokenMutation for ModelMutation {
    fn commit(&mut self) -> Result<()> {
        self.0.take().map_or(Ok(()), AttributeMutation::commit)
    }
    fn rollback(&mut self) -> Result<bool> {
        self.0
            .as_mut()
            .map_or(Ok(true), AttributeMutation::rollback)
    }
}
impl From<AttributeMutation> for RememberTokenUpdate {
    fn from(mutation: AttributeMutation) -> Self {
        Self::new(ModelMutation(Some(mutation)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct ExternalBackend {
        committed: Arc<AtomicUsize>,
        restored: Arc<AtomicUsize>,
    }
    impl RememberTokenMutation for ExternalBackend {
        fn commit(&mut self) -> Result<()> {
            self.committed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn rollback(&mut self) -> Result<bool> {
            self.restored.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }
    }
    struct PanickingBackend(Arc<AtomicUsize>);
    impl RememberTokenMutation for PanickingBackend {
        fn commit(&mut self) -> Result<()> {
            Ok(())
        }
        #[allow(
            clippy::panic,
            reason = "验证不可信外部回滚后端异常时不重复调用或二次展开"
        )]
        fn rollback(&mut self) -> Result<bool> {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("fixture backend violates non-panicking contract")
        }
    }
    #[test]
    fn external_rollback_panic_is_not_called_twice_during_unwind() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let mut update = RememberTokenUpdate::new(PanickingBackend(observed));
            let _ = update.rollback();
        }));
        assert!(outcome.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(RememberTokenUpdate::new(PanickingBackend(calls.clone())));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cancelled_external_update_recovers_without_an_async_drop_task() {
        let committed = Arc::new(AtomicUsize::new(0));
        let restored = Arc::new(AtomicUsize::new(0));
        let update = RememberTokenUpdate::new(ExternalBackend {
            committed: committed.clone(),
            restored: restored.clone(),
        });
        let cancelled = tokio::time::timeout(std::time::Duration::from_millis(5), async move {
            let _armed = update;
            std::future::pending::<()>().await;
        })
        .await;
        assert!(cancelled.is_err());
        assert_eq!(restored.load(Ordering::SeqCst), 1);
        assert_eq!(committed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn external_backend_commit_and_explicit_rollback_do_not_restore_twice() -> Result<()> {
        let committed = Arc::new(AtomicUsize::new(0));
        let restored = Arc::new(AtomicUsize::new(0));
        let update = || {
            RememberTokenUpdate::new(ExternalBackend {
                committed: committed.clone(),
                restored: restored.clone(),
            })
        };
        update().commit()?;
        assert_eq!(committed.load(Ordering::SeqCst), 1);
        assert_eq!(restored.load(Ordering::SeqCst), 0);
        let mut pending = update();
        assert!(pending.rollback()?);
        drop(pending);
        assert_eq!(restored.load(Ordering::SeqCst), 1);
        drop(update());
        assert_eq!(restored.load(Ordering::SeqCst), 2);
        Ok(())
    }
}
