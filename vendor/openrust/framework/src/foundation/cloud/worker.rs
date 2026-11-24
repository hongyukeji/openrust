use super::Queue;
use crate::{container::Container, contracts::queue::Queue as Contract};
use std::{
    future::Future,
    sync::{Arc, Weak},
};
struct Runtime {
    queue: Weak<Queue>,
    owner: Weak<Container>,
    selection: String,
    execution: Arc<super::telemetry::Execution>,
}
tokio::task_local! {static RUNTIME:Option<Runtime>;}
/// 仅真实 queue:work 执行域启用 Agent；任务并发和其他应用不共享该状态。
pub(crate) async fn scope<T>(
    owner: &Arc<Container>,
    queue: Arc<dyn Contract>,
    queues: &[&str],
    run: impl Future<Output = T>,
) -> T {
    let queue = queue.into_any().and_then(|q| q.downcast::<Queue>().ok());
    let runtime = queue.map(|queue| Runtime {
        selection: if queues.is_empty() {
            queue.default_queue().into()
        } else {
            queues.join(",")
        },
        owner: Arc::downgrade(owner),
        queue: Arc::downgrade(&queue),
        execution: Arc::default(),
    });
    RUNTIME.scope(runtime, run).await
}
pub(crate) fn selection(queue: &Queue) -> Option<String> {
    RUNTIME
        .try_with(|runtime| {
            runtime
                .as_ref()
                .filter(|runtime| {
                    runtime
                        .queue
                        .upgrade()
                        .is_some_and(|current| std::ptr::eq(current.as_ref(), queue))
                })
                .map(|runtime| runtime.selection.clone())
        })
        .ok()
        .flatten()
}
pub(crate) fn active(owner: &Arc<Container>) -> bool {
    RUNTIME
        .try_with(|r| {
            r.as_ref().is_some_and(|r| {
                r.owner
                    .upgrade()
                    .is_some_and(|current| Arc::ptr_eq(&current, owner))
            })
        })
        .unwrap_or(false)
}

/// 仅为当前托管 Worker 补充 Agent 故障识别，其他异常继续委托原检测器。
pub struct AgentAwareLostConnectionDetector(
    pub std::sync::Arc<dyn crate::contracts::database::LostConnectionDetector>,
);
impl crate::contracts::database::LostConnectionDetector for AgentAwareLostConnectionDetector {
    fn caused_by_lost_connection(&self, error: &(dyn std::error::Error + 'static)) -> bool {
        let mut current = Some(error);
        for _ in 0..32 {
            let Some(cause) = current else { break };
            if cause.is::<super::AgentUnreachable>() {
                return true;
            }
            current = cause.source();
        }
        self.0.caused_by_lost_connection(error)
    }
}

pub(crate) struct StopListener(pub Weak<Container>);
impl crate::contracts::events::Listener for StopListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn crate::contracts::events::Event,
        _context: &'a mut crate::events::EventContext<'_>,
    ) -> crate::events::EventFuture<
        'a,
        crate::events::EventResult<Option<crate::events::ListenerResponse>>,
    > {
        Box::pin(async move {
            let Some(event) = event.downcast_ref::<crate::queue::events::WorkerStopping>() else {
                return Ok(None);
            };
            let queue = RUNTIME
                .try_with(|r| {
                    r.as_ref()
                        .filter(|r| Weak::ptr_eq(&r.owner, &self.0))
                        .and_then(|r| r.queue.upgrade())
                })
                .ok()
                .flatten();
            if let Some(queue) = queue
                && event.connection_name == queue.get_connection_name()
            {
                let default = if event.reason == crate::queue::WorkerStopReason::TimedOut {
                    "released"
                } else {
                    "processed"
                };
                queue
                    .finish_processing_job(Some(default), None)
                    .await
                    .map_err(crate::events::EventError::Queue)?;
            }
            Ok(None)
        })
    }
}

pub(crate) fn execution(queue: &Queue) -> Option<Arc<super::telemetry::Execution>> {
    RUNTIME
        .try_with(|runtime| {
            runtime
                .as_ref()
                .filter(|r| {
                    r.queue
                        .upgrade()
                        .is_some_and(|current| std::ptr::eq(current.as_ref(), queue))
                })
                .map(|r| r.execution.clone())
        })
        .ok()
        .flatten()
}
