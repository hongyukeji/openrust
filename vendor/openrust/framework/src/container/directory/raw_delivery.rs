//! raw内部交付帧：通知不携用户值，owner可退役尚未交给caller的引用。
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::Notify;

use super::super::{Factory, FactoryParameters, ParameterOverrides, Value, container_error, lock};
use crate::foundation::lifecycle::{
    FutureFailure,
    disposal::{self, Credential},
};
use crate::foundation::resources::{ResourceCleanup, ResourceRegistrar, ResourceTaskCompletion};
use crate::{Error, Result};

pub(super) struct RawInputs {
    pub(super) factory: Factory,
    pub(super) parameters: FactoryParameters,
}
struct FrameState {
    inputs: Option<RawInputs>,
    result: Option<Result<Value>>,
    cleanup: Option<ResourceCleanup>,
    disposal_ticket: Option<crate::foundation::resources::ResourceDisposalTicket>,
    registration: Registration,
    retirement: Retirement,
    startup_done: bool,
}

#[derive(PartialEq, Eq)]
enum Registration {
    Pending,
    Open,
    Sealed,
    Historical,
}

#[derive(PartialEq, Eq)]
enum Retirement {
    Active,
    Disposing,
    Disposed,
    Retired,
    Delivering,
    Delivered,
}

pub(super) struct CallFrame {
    state: Mutex<FrameState>,
    failed: Arc<AtomicBool>,
    changed: Notify,
}
impl CallFrame {
    pub(super) fn new(
        factory: Factory,
        parameters: ParameterOverrides,
        owner: Option<ResourceRegistrar>,
    ) -> Arc<Self> {
        let failed = Arc::new(AtomicBool::new(false));
        let frame = Arc::new(Self {
            state: Mutex::new(FrameState {
                inputs: Some(RawInputs {
                    factory,
                    parameters: FactoryParameters {
                        overrides: parameters,
                        owner: None,
                        completion: None,
                    },
                }),
                result: None,
                cleanup: None,
                disposal_ticket: None,
                registration: Registration::Pending,
                retirement: Retirement::Active,
                startup_done: false,
            }),
            failed,
            changed: Notify::new(),
        });
        let weak = Arc::downgrade(&frame);
        let cleanup = owner.and_then(|owner| {
            owner.begin_reclaim(move |ticket| {
                if let Some(frame) = weak.upgrade() {
                    frame.retire_registered(ticket);
                }
            })
        });
        #[cfg(test)]
        super::super::directory_tests_v8::after_reclaim_registration();
        {
            let mut state = lock(&frame.state);
            let (cleanup, open) = match cleanup {
                Some((cleanup, open)) => (Some(cleanup), open),
                None => (None, false),
            };
            if let Some(cleanup) = &cleanup {
                state.disposal_ticket = Some(cleanup.disposal_ticket());
            }
            state.registration = if cleanup.is_none() {
                Registration::Historical
            } else if open {
                Registration::Open
            } else {
                Registration::Sealed
            };
            // startup尚未结束，retirer也不能先完成票据；构造相位在旧refs清完后收口。
            state.cleanup = cleanup;
        }
        frame
    }
    pub(super) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
    pub(super) fn complete_startup(&self, unknown: bool) {
        if unknown {
            self.failed.store(true, Ordering::Release);
        }
        let (cleanup, finish) = {
            let mut state = lock(&self.state);
            state.startup_done = true;
            if state.retirement == Retirement::Disposed
                && state.registration != Registration::Pending
            {
                (state.cleanup.take(), true)
            } else {
                (None, false)
            }
        };
        if let Some(mut cleanup) = cleanup {
            cleanup.finish(0);
        }
        if finish {
            self.mark_retirement_done();
        }
    }
    pub(super) fn disposal_credential(&self) -> Option<Credential> {
        lock(&self.state)
            .disposal_ticket
            .clone()
            .map(Credential::Cleanup)
    }
    fn retire_registered(&self, ticket: crate::foundation::resources::ResourceDisposalTicket) {
        lock(&self.state).disposal_ticket = Some(ticket);
        self.retire();
    }
    pub(super) fn take_inputs(&self) -> Result<RawInputs> {
        let mut state = lock(&self.state);
        if state.retirement != Retirement::Active
            || state.registration != Registration::Open
            || !state.startup_done
        {
            return Err(container_error(
                "resources_closed",
                "原工厂交付责任未取得准入",
            ));
        }
        state
            .inputs
            .take()
            .ok_or_else(|| container_error("factory_inputs_missing", "原工厂输入已转移"))
    }
    pub(super) fn store(&self, mut result: RawResult) {
        let mut state = lock(&self.state);
        if state.retirement == Retirement::Active {
            std::mem::swap(&mut state.result, &mut result.result);
        }
        drop(state);
        // retired的迟到结果仍归未完producer fence；绝不由channel槽隐式Drop。
        drop(result);
    }
    pub(super) fn retire(&self) {
        let (inputs, result) = {
            let mut state = lock(&self.state);
            if state.retirement != Retirement::Active {
                return;
            }
            state.retirement = Retirement::Disposing;
            (state.inputs.take(), state.result.take())
        };
        let mut retirement = Delivery {
            result: RawResult {
                result,
                failure: self.failure(),
                disposal: None,
            },
            cleanup: None,
            failed: Arc::clone(&self.failed),
            disposal: self.disposal_credential(),
            on_finish: None,
        };
        let ((), report) = disposal::scope(self.disposal_credential(), || {
            if let Some(inputs) = inputs {
                disposal::dispose(inputs.factory, None);
                disposal::dispose(inputs.parameters, None);
            }
            retirement.result.clear();
        });
        if report.seen {
            self.failed.store(true, Ordering::Release);
        }
        let finish = {
            let mut state = lock(&self.state);
            state.retirement = Retirement::Disposed;
            if state.registration != Registration::Pending && state.startup_done {
                retirement.cleanup = state.cleanup.take();
                true
            } else {
                false
            }
        };
        retirement.finish_cleanup();
        if finish {
            self.mark_retirement_done();
        }
    }
    fn mark_retirement_done(&self) {
        let mut state = lock(&self.state);
        state.retirement = if matches!(
            state.retirement,
            Retirement::Delivering | Retirement::Delivered
        ) {
            Retirement::Delivered
        } else {
            Retirement::Retired
        };
        drop(state);
        self.changed.notify_waiters();
    }
    async fn wait_retirement_done(&self) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let done = matches!(
                lock(&self.state).retirement,
                Retirement::Retired | Retirement::Delivered
            );
            if done {
                return;
            }
            notified.await;
        }
    }
    fn failure(&self) -> FutureFailure {
        let failed = Arc::clone(&self.failed);
        FutureFailure::new(move || {
            failed.store(true, Ordering::Release);
        })
    }
    pub(super) fn take_delivery(self: &Arc<Self>) -> Result<Delivery> {
        let mut state = lock(&self.state);
        if state.retirement != Retirement::Active {
            return Err(container_error("resources_closed", "原工厂交付已放弃"));
        }
        if state.inputs.is_some() {
            return Err(container_error(
                "factory_inputs_missing",
                "原工厂尚未准入不能交付",
            ));
        }
        state.retirement = Retirement::Delivering;
        Ok(Delivery {
            result: RawResult {
                result: state.result.take(),
                failure: self.failure(),
                disposal: None,
            },
            cleanup: state.cleanup.take(),
            failed: Arc::clone(&self.failed),
            disposal: state.disposal_ticket.clone().map(Credential::Cleanup),
            on_finish: Some(Arc::downgrade(self)),
        })
    }
}
impl Drop for CallFrame {
    fn drop(&mut self) {
        self.retire();
    }
}

pub(super) struct CallerFrame(pub(super) Arc<CallFrame>);
impl CallerFrame {
    pub(super) async fn finish<T>(&self, result: Result<T>) -> Result<T> {
        let mut output = FinalOutput {
            result: Some(result),
            failed: Arc::clone(&self.0.failed),
            disposal: self.0.disposal_credential(),
        };
        self.0.retire();
        // 认领不等于完成；已由owner清理时，活跃caller等同一有限退役责任。
        self.0.wait_retirement_done().await;
        let delivered = matches!(
            lock(&self.0.state).retirement,
            Retirement::Delivering | Retirement::Delivered
        );
        if self.0.failed() && !delivered {
            output.fail(interrupted());
        }
        output.result.take().unwrap_or_else(|| Err(interrupted()))
    }
}
impl Drop for CallerFrame {
    fn drop(&mut self) {
        self.0.retire();
        let historical = lock(&self.0.state).registration == Registration::Historical;
        if historical && self.0.failed() {
            tracing::error!(
                code = "resource_task_interrupted",
                "未准入原工厂调用的内部引用回收异常；既有关闭历史保持不变"
            );
        }
    }
}

pub(super) struct RawResult {
    pub(super) result: Option<Result<Value>>,
    failure: FutureFailure,
    disposal: Option<Credential>,
}
impl RawResult {
    pub(super) fn producer(result: Result<Value>, completion: ResourceTaskCompletion) -> Self {
        Self {
            result: Some(result),
            failure: FutureFailure::new({
                let completion = completion.clone();
                move || completion.note_diagnostic()
            }),
            disposal: Some(Credential::Completion(completion)),
        }
    }
    pub(super) fn reject(&mut self, primary: Error) {
        self.clear();
        self.result = Some(Err(primary));
    }
    pub(super) fn reject_lifecycle(&mut self, error: Error) {
        let execution_primary = matches!(&self.result,Some(Err(primary)) if matches!(primary.code(),"execution_cancelled"|"execution_deadline_exceeded"));
        if !execution_primary {
            self.reject(error);
        }
    }
    pub(super) fn suppress_success(&mut self) {
        if matches!(self.result, Some(Ok(_))) {
            self.reject(interrupted());
        }
    }
    fn clear(&mut self) {
        if disposal::dispose(self.result.take(), self.disposal.clone()).seen {
            self.failure.note_diagnostic();
        }
    }
}
impl Drop for RawResult {
    fn drop(&mut self) {
        self.clear();
    }
}
fn interrupted() -> Error {
    container_error("resource_task_interrupted", "原工厂交付或捕获清理异常")
}

pub(super) struct Delivery {
    result: RawResult,
    cleanup: Option<ResourceCleanup>,
    failed: Arc<AtomicBool>,
    disposal: Option<Credential>,
    on_finish: Option<Weak<CallFrame>>,
}
impl Delivery {
    fn finish_cleanup(&mut self) {
        let ((), report) = disposal::scope(self.disposal.clone(), || self.result.clear());
        if report.seen {
            self.failed.store(true, Ordering::Release);
        }
        // 当前上下文已经恢复；只消费真实票据积累的owning未知，不用诊断bool计错。
        if let Some(mut cleanup) = self.cleanup.take() {
            cleanup.finish(0);
        }
        if let Some(frame) = self.on_finish.take().and_then(|frame| frame.upgrade()) {
            frame.mark_retirement_done();
        }
    }
    pub(super) fn finish<T: ?Sized + Send + Sync + 'static>(
        mut self,
        cleanup: Option<Error>,
    ) -> Result<Arc<T>> {
        let (mut output, report) = disposal::scope(self.disposal.clone(), || {
            let output = match &self.result.result {
                Some(Ok(value)) => value.downcast_ref::<Arc<T>>().cloned().ok_or_else(|| {
                    container_error("service_type_mismatch", "原工厂实际输出与声明不同")
                }),
                Some(Err(_)) => match self.result.result.take() {
                    Some(Err(error)) => Err(error),
                    _ => Err(container_error(
                        "factory_result_missing",
                        "原工厂结果已转移",
                    )),
                },
                None => Err(container_error(
                    "service_initialization_cancelled",
                    "原工厂没有交付结果",
                )),
            };
            let mut output = FinalOutput {
                result: Some(output),
                failed: Arc::clone(&self.failed),
                disposal: self.disposal.clone(),
            };
            self.result.clear();
            if let Some(cleanup) = cleanup {
                output.fail(cleanup);
            } else if self.failed.load(Ordering::Acquire) || disposal::seen() {
                output.fail(interrupted());
            }
            output
        });
        if report.seen {
            self.failed.store(true, Ordering::Release);
        }
        self.finish_cleanup();
        // 其后只有空guard/框架元数据析构，无await；本poll返回Ready即外部Arc接管。
        output.result.take().unwrap_or_else(|| Err(interrupted()))
    }
}
impl Drop for Delivery {
    fn drop(&mut self) {
        self.finish_cleanup();
    }
}
struct FinalOutput<T> {
    result: Option<Result<T>>,
    failed: Arc<AtomicBool>,
    disposal: Option<Credential>,
}
impl<T> FinalOutput<T> {
    fn fail(&mut self, cleanup: Error) {
        self.result = Some(match self.result.take() {
            Some(Err(primary)) => Err(crate::foundation::resources::ResourceTaskFailure::error(
                primary, cleanup,
            )),
            Some(Ok(value)) => {
                if disposal::dispose(value, self.disposal.clone()).seen {
                    self.failed.store(true, Ordering::Release);
                }
                Err(cleanup)
            }
            None => Err(cleanup),
        });
    }
}
impl<T> Drop for FinalOutput<T> {
    fn drop(&mut self) {
        if disposal::dispose(self.result.take(), self.disposal.clone()).seen {
            self.failed.store(true, Ordering::Release);
        }
    }
}
