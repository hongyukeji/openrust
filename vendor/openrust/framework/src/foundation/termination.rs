//! 应用终止回调的登记与遍历。

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::ApplicationState;
use super::lifecycle::disposal;
use crate::{BoxFuture, Result};

pub(crate) type TerminationHook =
    dyn Fn(ApplicationState) -> BoxFuture<'static, Result<()>> + Send + Sync;

#[derive(Default)]
struct Queue {
    generation: Arc<()>,
    callbacks: Vec<Arc<TerminationHook>>,
}

impl Drop for Queue {
    fn drop(&mut self) {
        for callback in std::mem::take(&mut self.callbacks) {
            if disposal::dispose(callback, None).seen {
                tracing::error!("application terminating callback disposal failed");
            }
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct TerminationCallbacks(Arc<Mutex<Queue>>);

impl TerminationCallbacks {
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn push(&self, callback: Arc<TerminationHook>) {
        self.queue().callbacks.push(callback);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.queue().callbacks.is_empty()
    }

    pub(crate) fn take(&self) -> Vec<Arc<TerminationHook>> {
        let mut queue = self.queue();
        queue.generation = Arc::new(());
        std::mem::take(&mut queue.callbacks)
    }

    pub(crate) async fn run(&self, state: &ApplicationState) -> Result<()> {
        let generation = Arc::clone(&self.queue().generation);
        let mut index = 0;
        loop {
            let callback = {
                let queue = self.queue();
                if !Arc::ptr_eq(&generation, &queue.generation) {
                    return Ok(());
                }
                queue.callbacks.get(index).cloned()
            };
            let Some(callback) = callback else {
                return Ok(());
            };
            callback(state.clone()).await?;
            index += 1;
        }
    }
}
