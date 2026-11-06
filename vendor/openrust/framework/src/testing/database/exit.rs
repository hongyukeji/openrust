//! 正常进程退出前有界等待自有数据库操作线程；不接触数据库或 Tokio。

use std::{
    sync::{Condvar, Mutex, OnceLock},
    time::{Duration, Instant},
};

use crate::{Error, Result};

const EXIT_WAIT_LIMIT: Duration = Duration::from_secs(30);
static HOOK_REGISTERED: OnceLock<bool> = OnceLock::new();
static OPERATIONS: Mutex<State> = Mutex::new(State::new());
static FINISHED: Condvar = Condvar::new();

#[derive(Clone, Copy)]
enum OperationKind {
    Create,
    Cleanup,
}

struct State {
    active: usize,
    exiting: bool,
    sealed: bool,
}

impl State {
    const fn new() -> Self {
        Self {
            active: 0,
            exiting: false,
            sealed: false,
        }
    }

    fn register(&mut self, kind: OperationKind) -> Result<()> {
        if self.sealed
            || (self.exiting && (self.active == 0 || matches!(kind, OperationKind::Create)))
        {
            return Err(Error::custom(
                "test_database_exit_started",
                "进程退出期间不再接纳此类数据库操作",
            ));
        }
        self.active = self.active.checked_add(1).ok_or_else(|| {
            Error::custom(
                "test_database_exit_count_overflow",
                "数据库退出等待操作计数超出范围",
            )
        })?;
        Ok(())
    }

    fn complete(&mut self) -> bool {
        let Some(active) = self.active.checked_sub(1) else {
            return false;
        };
        self.active = active;
        true
    }

    fn begin_exit(&mut self) {
        self.exiting = true;
    }

    fn seal(&mut self) {
        self.exiting = true;
        self.sealed = true;
    }
}

/// 仅计数的数据库操作票据；移入自有线程闭包后由 RAII 完成登记。
///
/// 不持数据库、运行时或线程句柄，不能克隆；线程创建失败时闭包释放同样归还计数。
pub(super) struct ExitTicket;

impl Drop for ExitTicket {
    fn drop(&mut self) {
        if let Ok(mut state) = OPERATIONS.lock()
            && state.complete()
        {
            FINISHED.notify_all();
        }
    }
}

/// 在顶层网络数据库创建派发前登记；进程一旦开始退出就拒绝新建库。
pub(super) fn register_create_operation() -> Result<ExitTicket> {
    register_operation(OperationKind::Create)
}

/// 登记已有责任操作派生的清理；退出排空期间必须尚有活跃操作且未封闭。
///
/// 调用方必须先登记本票据再移交并释放原票据，避免责任链短暂归零。
pub(super) fn register_cleanup_operation() -> Result<ExitTicket> {
    register_operation(OperationKind::Cleanup)
}

fn register_operation(kind: OperationKind) -> Result<ExitTicket> {
    if !*HOOK_REGISTERED.get_or_init(|| shutdown_hooks::add_shutdown_hook(on_exit)) {
        return Err(Error::custom(
            "test_database_exit_hook_unavailable",
            "无法登记数据库操作退出等待器",
        ));
    }
    OPERATIONS
        .lock()
        .map_err(|_| {
            Error::custom(
                "test_database_exit_state_poisoned",
                "数据库退出等待状态不可用",
            )
        })?
        .register(kind)?;
    Ok(ExitTicket)
}

extern "C" fn on_exit() {
    // 任何展开均不得越过 C ABI；正常路径无分配、日志或数据库操作。
    let _ = std::panic::catch_unwind(|| {
        if std::panic::catch_unwind(wait_for_operations).is_err() {
            seal_operations();
        }
    });
}

fn seal_operations() {
    match OPERATIONS.lock() {
        Ok(mut state) => state.seal(),
        Err(poisoned) => poisoned.into_inner().seal(),
    }
}

fn wait_for_operations() {
    let started = Instant::now();
    let mut state = match OPERATIONS.lock() {
        Ok(state) => state,
        Err(poisoned) => {
            poisoned.into_inner().seal();
            return;
        }
    };
    state.begin_exit();
    let Some(deadline) = started.checked_add(EXIT_WAIT_LIMIT) else {
        state.seal();
        return;
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        state.seal();
        return;
    }
    // 同一剩余总期限内释放锁等待；任何返回路径均在同锁下封闭新登记。
    match FINISHED.wait_timeout_while(state, remaining, |state| state.active != 0) {
        Ok((mut state, _)) => state.seal(),
        Err(poisoned) => poisoned.into_inner().0.seal(),
    }
}

#[cfg(test)]
mod tests {
    use super::{OperationKind, State};
    use crate::Result;

    #[test]
    fn normal_operations_can_register_again_after_all_tickets_finish() -> Result<()> {
        let mut state = State::new();
        state.register(OperationKind::Create)?;
        state.register(OperationKind::Create)?;
        assert_eq!(state.active, 2);
        assert!(state.complete());
        assert!(state.complete());
        assert_eq!(state.active, 0);
        state.register(OperationKind::Create)?;
        assert_eq!(state.active, 1);
        Ok(())
    }

    #[test]
    fn exit_without_active_operations_closes_registration() {
        let mut state = State::new();
        state.begin_exit();
        assert!(
            matches!(state.register(OperationKind::Create), Err(error) if error.code() == "test_database_exit_started")
        );
        assert_eq!(state.active, 0);
    }

    #[test]
    fn active_exit_worker_can_handoff_cleanup_before_final_zero() -> Result<()> {
        let mut state = State::new();
        state.register(OperationKind::Create)?;
        state.begin_exit();
        assert!(state.register(OperationKind::Create).is_err());
        assert_eq!(state.active, 1);
        state.register(OperationKind::Cleanup)?;
        assert_eq!(state.active, 2);
        assert!(state.complete());
        assert_eq!(state.active, 1);
        assert!(state.complete());
        assert_eq!(state.active, 0);
        assert!(
            matches!(state.register(OperationKind::Cleanup), Err(error) if error.code() == "test_database_exit_started")
        );
        Ok(())
    }

    #[test]
    fn completion_at_zero_does_not_wrap_counter() {
        let mut state = State::new();
        assert!(!state.complete());
        assert_eq!(state.active, 0);
    }

    #[test]
    fn overflow_rejection_preserves_active_counter() {
        let mut state = State {
            active: usize::MAX,
            exiting: false,
            sealed: false,
        };
        assert!(
            matches!(state.register(OperationKind::Create), Err(error) if error.code() == "test_database_exit_count_overflow")
        );
        assert_eq!(state.active, usize::MAX);
    }

    #[test]
    fn sealed_state_rejects_create_and_cleanup_even_with_active_workers() -> Result<()> {
        let mut state = State::new();
        state.register(OperationKind::Create)?;
        state.begin_exit();
        state.register(OperationKind::Cleanup)?;
        state.seal();
        for kind in [OperationKind::Create, OperationKind::Cleanup] {
            assert!(
                matches!(state.register(kind), Err(error) if error.code() == "test_database_exit_started")
            );
            assert_eq!(state.active, 2);
        }
        assert!(state.complete());
        assert!(state.complete());
        assert!(state.register(OperationKind::Cleanup).is_err());
        Ok(())
    }

    #[test]
    fn cleanup_can_start_normally_without_an_active_create() -> Result<()> {
        let mut state = State::new();
        state.register(OperationKind::Cleanup)?;
        assert_eq!(state.active, 1);
        assert!(state.complete());
        Ok(())
    }
}
