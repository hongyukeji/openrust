//! 同一调用栈的局部观察责任；不创建任务或替调用方驱动 Future。

use super::{ResourceRegistrar, ResourceTaskCompletion, lock};
use crate::foundation::lifecycle::{catch_isolated, disposal};
use crate::{Error, Result};

// 与原 completion 同锁，不创建另一份索引或 owner 账本。
pub(super) struct InlineState {
    pub(super) released: bool,
    pub(super) slots: usize,
}

/// 内联调用的唯一完成资格。必须在取得 State 锁前创建，释放 State/owner 后登记。
///
/// 完成钩子固定为空；Resolver 必须在 finish/Drop 前清除自己的 Attempt 和等待边。
/// 用户持有物必须放入独立 slot；guard 先退役时仍由这些槽阻止栅栏过早完成。
/// 本类型既不认领用户阶段，也不承诺最终业务交付成功。
pub(crate) struct ResourceInlineOwner {
    completion: ResourceTaskCompletion,
    admitted: bool,
    registered: bool,
    released: bool,
}

impl ResourceInlineOwner {
    pub(crate) fn new() -> Self {
        let completion = ResourceTaskCompletion::new(|| {});
        lock(&completion.inner.state).inline = Some(InlineState {
            released: false,
            slots: 0,
        });
        Self {
            completion,
            admitted: false,
            registered: false,
            released: false,
        }
    }

    /// State→owner 下占原 observed 索引；commit 只能检查和移动框架元数据。
    /// 返回错误时不登记 owner，不析构用户槽；self 留在锁外调用帧中。
    pub(crate) fn reserve<T>(
        &mut self,
        registrar: &ResourceRegistrar,
        commit: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        if self.admitted || self.released {
            return Err(invalid_state());
        }
        let value = registrar.reserve_observed(&self.completion, commit)?;
        self.admitted = true;
        Ok(value)
    }

    /// 仅在释放 State 和 owner 锁后调用；期间 owner seal 不撤销已准入责任。
    /// 开始新用户阶段仍须由 Resolver 在同一 owner 闸门下独立认领。
    pub(crate) fn register(&mut self) -> Result<()> {
        if !self.admitted || self.registered || self.released {
            return Err(invalid_state());
        }
        self.registered = true;
        self.completion.mark(true, false);
        Ok(())
    }

    /// 完成证据可借用/克隆，不能从此取得结束内联责任的资格。
    pub(crate) fn completion(&self) -> ResourceTaskCompletion {
        self.completion.clone()
    }

    /// 每个独立 Future、参数或中间值分别占一槽，禁止把多个恶意 Drop 值聚合成一槽。
    /// 接收阶段已认领的结果不再次检查 owner open；seal 不能丢弃该真实退役责任。
    pub(crate) fn slot<T>(&self, value: T) -> Result<ResourceInlineSlot<T>> {
        let accepted = if self.registered && !self.released {
            let mut state = lock(&self.completion.inner.state);
            if let Some(inline) = state.inline.as_mut() {
                inline
                    .slots
                    .checked_add(1)
                    .map(|slots| inline.slots = slots)
            } else {
                None
            }
        } else {
            None
        };
        if accepted.is_none() {
            let primary = invalid_state();
            let report = disposal::dispose(value, Some(self.credential()));
            if report.seen {
                self.completion.note_diagnostic();
                return Err(super::ResourceTaskFailure::error(primary, interrupted()));
            }
            return Err(primary);
        }
        Ok(ResourceInlineSlot {
            value: Some(value),
            completion: Some(self.completion.clone()),
        })
    }

    /// 同步作用域只包实际 callback 或原 Future 的一次 poll；不得捕获独立用户槽。
    /// Future 自身留在 slot，poll panic 的有限 payload 清完后才能另行清 Future。
    pub(crate) fn run<T>(&self, action: impl FnOnce() -> T) -> Result<T> {
        if !self.registered || self.released {
            let primary = invalid_state();
            let report = disposal::dispose(action, Some(self.credential()));
            if report.seen {
                self.completion.note_diagnostic();
                return Err(super::ResourceTaskFailure::error(primary, interrupted()));
            }
            return Err(primary);
        }
        let (output, report) = disposal::scope(Some(self.credential()), || {
            let output = catch_isolated(action);
            if output.is_err() {
                disposal::failure(None);
            }
            output
        });
        if report.seen {
            self.completion.note_diagnostic();
        }
        output.map_err(|()| interrupted())
    }

    /// 只检查已观察到的清理诊断；不是最终交付核验，不等待自己的 completion。
    pub(crate) fn check_cleanup(&self) -> Result<()> {
        if lock(&self.completion.inner.state).interrupted {
            Err(interrupted())
        } else {
            Ok(())
        }
    }

    /// Resolver 已清完 Attempt/等待边后，锁外放弃唯一完成资格。
    /// 即使仍有独立槽，finish 也不等待：最后一槽真实清完才通知原栅栏。
    pub(crate) fn finish(mut self) {
        self.release();
    }

    fn credential(&self) -> disposal::Credential {
        disposal::Credential::Completion(self.completion.clone())
    }

    fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        {
            let mut state = lock(&self.completion.inner.state);
            if let Some(inline) = state.inline.as_mut() {
                inline.released = true;
            }
        }
        // reserve 后、显式 register 前取消也必须结束真实的已占观察。
        if !self.registered {
            self.registered = true;
            self.completion.mark(true, false);
        }
        self.completion.mark(false, false);
    }
}

impl Drop for ResourceInlineOwner {
    fn drop(&mut self) {
        self.release();
    }
}

/// 一个实际独立持有物；不 Clone，不向 owner 注册第二次观察。
/// slot 不持有 registrar 或 owner 强引用；其最后 Drop 在原 completion 下逐项处置。
pub(crate) struct ResourceInlineSlot<T> {
    value: Option<T>,
    completion: Option<ResourceTaskCompletion>,
}

impl<T> ResourceInlineSlot<T> {
    pub(crate) fn get_mut(&mut self) -> Option<&mut T> {
        self.value.as_mut()
    }

    /// 仅用于已认领的实际移交；未交付值须立刻转入另一受控槽，不能裸跨 await。
    /// 不在此释放槽资格，调用方仍需在锁外 clear/Drop 该空槽。
    pub(crate) fn take(&mut self) -> Option<T> {
        self.value.take()
    }

    /// 锁外清理单槽及全部有限 panic payload，结束后才释放栅栏持有计数。
    pub(crate) fn clear(&mut self) {
        let Some(completion) = self.completion.take() else {
            return;
        };
        let report = disposal::dispose(
            self.value.take(),
            Some(disposal::Credential::Completion(completion.clone())),
        );
        if report.seen {
            completion.note_diagnostic();
        }
        {
            let mut state = lock(&completion.inner.state);
            if let Some(inline) = state.inline.as_mut() {
                inline.slots -= 1;
            }
        }
        // Guard 尚未退役时 inline.released 继续阻止完成；完成通知不留在任何锁内。
        completion.mark(false, false);
    }
}

impl<T> Drop for ResourceInlineSlot<T> {
    fn drop(&mut self) {
        self.clear();
    }
}

fn invalid_state() -> Error {
    Error::custom(
        "resource_inline_state_invalid",
        "内联观察尚未准入或已经退役",
    )
}

fn interrupted() -> Error {
    Error::custom(
        "resource_task_interrupted",
        "内联调用或独立持有槽的清理异常",
    )
}

#[cfg(test)]
#[path = "inline_owner/tests.rs"]
mod tests;
