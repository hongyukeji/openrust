//! 仅本线程同步Drop栈的诊断/归责；guard不离开私有闭包helper。
use super::super::resources::{ResourceDisposalTicket, ResourceTaskCompletion};
use std::cell::RefCell;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct AccountState {
    active: bool,
    unknown: bool,
}
/// 仅实际票据持有者可结束的框架元数据，不保存owner或用户值。
#[derive(Clone)]
pub(crate) struct Account(Arc<Mutex<AccountState>>);
impl Account {
    pub(crate) fn admitted() -> Self {
        Self(Arc::new(Mutex::new(AccountState {
            active: true,
            unknown: false,
        })))
    }
    pub(crate) fn record(&self) -> bool {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.active {
            return false;
        }
        state.unknown = true;
        true
    }
    pub(crate) fn finish(&self) -> bool {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.active {
            return false;
        }
        state.active = false;
        state.unknown
    }
}
/// 只由已经准入任务的ControlledTask持有，非Owner open标志。
#[derive(Clone)]
pub(crate) struct TaskTicket(Account);
impl TaskTicket {
    pub(super) fn admitted() -> Self {
        Self(Account::admitted())
    }
    pub(super) fn finish(&self) -> bool {
        self.0.finish()
    }
}
#[derive(Clone)]
pub(crate) enum Credential {
    Completion(ResourceTaskCompletion),
    Cleanup(ResourceDisposalTicket),
    Task(TaskTicket),
}
impl Credential {
    fn record(&self) -> bool {
        match self {
            Self::Completion(completion) => completion.try_note_unknown(),
            Self::Cleanup(ticket) => ticket.record(),
            Self::Task(ticket) => ticket.0.record(),
        }
    }
}
#[derive(Default)]
struct Diagnostic {
    seen: AtomicBool,
    owned: AtomicBool,
}
struct Context {
    local: Arc<Diagnostic>,
    own: Option<Credential>,
    parent: Option<Arc<Self>>,
}
std::thread_local! {static CURRENT:RefCell<Option<Arc<Context>>>=const{RefCell::new(None)};}
/// 只描述本次同步栈实际收到的事件，不读取其他Arc持有者的共享终态。
#[derive(Clone, Copy, Default)]
pub(crate) struct Report {
    pub(crate) seen: bool,
    pub(crate) owned: bool,
}

/// 已知holder真末Drop才调用；原活跃责任优先，无票据内层继承有效外层。
pub(crate) fn failure(original: Option<&Credential>) {
    let current = CURRENT.with_borrow(Clone::clone);
    let accepted = original.is_some_and(Credential::record);
    let mut context = current;
    let mut charged = accepted;
    let mut seen = false;
    let mut pending = Vec::new();
    while let Some(scope) = context.take() {
        seen = true;
        scope.local.seen.store(true, Ordering::Release);
        if !charged {
            pending.push(Arc::clone(&scope.local));
            if scope.own.as_ref().is_some_and(Credential::record) {
                for local in pending.drain(..) {
                    local.owned.store(true, Ordering::Release);
                }
                charged = true;
            }
        }
        context.clone_from(&scope.parent);
    }
    if !charged {
        tracing::error!(
            code = "resource_task_interrupted",
            diagnostic_context = seen,
            "同步析构异常没有尚有效的完成票据；既有历史不追溯修改"
        );
    }
}
struct Restore {
    previous: Option<Arc<Context>>,
}
impl Drop for Restore {
    fn drop(&mut self) {
        let ended =
            CURRENT.with_borrow_mut(|current| std::mem::replace(current, self.previous.take()));
        // TLS借用已结束且外层已恢复，才释放本次context/票据保活引用。
        drop(ended);
    }
}
/// 仅同步闭包；不返回guard，不能把TLS作用域带入await/别的线程。
pub(crate) fn scope<T>(own: Option<Credential>, action: impl FnOnce() -> T) -> (T, Report) {
    let parent = CURRENT.with_borrow(Clone::clone);
    let local = Arc::new(Diagnostic::default());
    let current = Arc::new(Context {
        local: Arc::clone(&local),
        own,
        parent: parent.clone(),
    });
    let previous = CURRENT.replace(Some(current));
    let restore = Restore { previous };
    let output = action();
    drop(restore);
    let report = Report {
        seen: local.seen.load(Ordering::Acquire),
        owned: local.owned.load(Ordering::Acquire),
    };
    (output, report)
}
/// 值和完整有限panic payload在同一同步作用域清完，恢复后才返回观察。
pub(crate) fn dispose<T>(value: T, own: Option<Credential>) -> Report {
    scope(own, || {
        if super::drop_isolated(value) {
            failure(None);
        }
    })
    .1
}
/// 当前作用域的必要诊断；不是owner准入或归账资格。
pub(crate) fn seen() -> bool {
    CURRENT.with_borrow(|scope| {
        scope
            .as_ref()
            .is_some_and(|scope| scope.local.seen.load(Ordering::Acquire))
    })
}
