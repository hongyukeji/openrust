//! 显式执行会话的文件一次初始化；等待方不越过尚未完成的初始化。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use openrust::core::{Error, RegisterContext, Result};

use super::ModuleFile;

tokio::task_local! {
    static INITIALIZING: Vec<(usize, PathBuf)>;
}

#[derive(Clone, Copy)]
enum Status {
    Running,
    Ready,
    Failed,
    Cancelled,
}
struct FileState {
    status: Mutex<Status>,
    changed: Notify,
}

/// 对应一次显式执行会话的 `include_once` 集合；默认每个 App 独立。
///
/// 多个 App 可共享此句柄；并发调用等待当前初始化完成。异步失败或取消会封闭该
/// 文件，后续调用明确失败，避免把不完整副作用当成功；新执行会话才可重新尝试。
#[derive(Default)]
pub struct ModuleExecution {
    files: Mutex<HashMap<PathBuf, Arc<FileState>>>,
    pub(super) manifest: Mutex<Option<Arc<super::loading::ExecutionManifest>>>,
}

impl ModuleExecution {
    /// 建立独立执行边界，不共享 Container 或 Provider 实例。
    pub fn new() -> Self {
        Self::default()
    }

    pub(super) async fn initialize(
        &self,
        path: PathBuf,
        initializer: &dyn ModuleFile,
        app: &mut RegisterContext<'_>,
    ) -> Result<()> {
        let identity = std::ptr::from_ref(self) as usize;
        if INITIALIZING
            .try_with(|active| {
                active
                    .iter()
                    .any(|(execution, file)| *execution == identity && file == &path)
            })
            .unwrap_or(false)
        {
            return Err(Error::custom(
                "module_file_initialization_cycle",
                "同一异步调用链重入正在初始化的模块文件",
            ));
        }
        let (state, owner) = {
            let mut files = self.files.lock().map_err(|_| unavailable())?;
            if let Some(state) = files.get(&path) {
                (Arc::clone(state), false)
            } else {
                let state = Arc::new(FileState {
                    status: Mutex::new(Status::Running),
                    changed: Notify::new(),
                });
                files.insert(path.clone(), Arc::clone(&state));
                (state, true)
            }
        };
        if !owner {
            return state.wait().await;
        }
        let mut guard = InitializationGuard {
            state,
            finished: false,
        };
        let mut active = INITIALIZING.try_with(Clone::clone).unwrap_or_default();
        active.push((identity, path));
        let result = INITIALIZING
            .scope(active, initializer.initialize(app))
            .await;
        guard.finish(if result.is_ok() {
            Status::Ready
        } else {
            Status::Failed
        })?;
        result
    }

    pub(super) async fn wait_if_started(&self, path: &std::path::Path) -> Result<bool> {
        let state = self
            .files
            .lock()
            .map_err(|_| unavailable())?
            .get(path)
            .cloned();
        if let Some(state) = state {
            let identity = std::ptr::from_ref(self) as usize;
            if INITIALIZING
                .try_with(|active| {
                    active
                        .iter()
                        .any(|(execution, file)| *execution == identity && file == path)
                })
                .unwrap_or(false)
            {
                return Err(Error::custom(
                    "module_file_initialization_cycle",
                    "同一异步调用链重入正在初始化的模块文件",
                ));
            }
            state.wait().await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl FileState {
    async fn wait(&self) -> Result<()> {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            // 注册等待后再检查，避免完成通知落在检查与 await 之间。
            notified.as_mut().enable();
            let status = *self.status.lock().map_err(|_| unavailable())?;
            match status {
                Status::Ready => return Ok(()),
                Status::Failed => {
                    return Err(Error::custom(
                        "module_file_initialization_failed",
                        "共享执行会话中的模块文件初始化已失败",
                    ));
                }
                Status::Cancelled => {
                    return Err(Error::custom(
                        "module_file_initialization_cancelled",
                        "共享执行会话中的模块文件初始化已取消",
                    ));
                }
                Status::Running => notified.await,
            }
        }
    }
}
struct InitializationGuard {
    state: Arc<FileState>,
    finished: bool,
}
impl InitializationGuard {
    fn finish(&mut self, status: Status) -> Result<()> {
        *self.state.status.lock().map_err(|_| unavailable())? = status;
        self.finished = true;
        self.state.changed.notify_waiters();
        Ok(())
    }
}
impl Drop for InitializationGuard {
    fn drop(&mut self) {
        if !self.finished {
            // Drop 不能 await；发布取消并唤醒所有等待者，不保留永远 Running 的悬挂槽。
            let mut status = self
                .state
                .status
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *status = Status::Cancelled;
            drop(status);
            self.state.changed.notify_waiters();
        }
    }
}
fn unavailable() -> Error {
    Error::custom("module_execution_poisoned", "模块执行会话状态不可用")
}
