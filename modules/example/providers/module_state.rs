//! 模块持久状态与已加载快照的只读业务示例；不提供匿名管理写入端点。

use openrust::core::ApiResponse;
use openrust::events::{EventPropagation, EventsServiceProvider};
use openrust::foundation::RegisterContext;
use openrust::http::Inject;
use openrust::support::ServiceProvider;
use openrust::{BoxFuture, Error, Result};
use openrust_modules::{ModuleLifecycleEvent, ModuleRepository};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// 同一应用实例内观察到的模块生命周期事件，不另建事件派发器。
#[derive(Default)]
pub(crate) struct ModuleStateLog {
    events: Mutex<VecDeque<String>>,
}

impl ModuleStateLog {
    fn record(&self, name: String) -> Result<()> {
        let mut events = self.events.lock().map_err(|_| unavailable())?;
        // 示例只展示最近64项，不让长期运行应用的操作观察记录无界增长。
        if events.len() == 64 {
            events.pop_front();
        }
        events.push_back(name);
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<String>> {
        Ok(self
            .events
            .lock()
            .map_err(|_| unavailable())?
            .iter()
            .cloned()
            .collect())
    }
}

fn unavailable() -> Error {
    Error::custom(
        "example_module_state_unavailable",
        "示例模块状态观察记录不可用",
    )
}

/// 将只读状态端点和命名监听器共享的同一日志实例绑定到统一容器。
pub(crate) struct ModuleStateServiceProvider {
    log: Arc<ModuleStateLog>,
}

impl ModuleStateServiceProvider {
    pub(crate) fn new(log: Arc<ModuleStateLog>) -> Self {
        Self { log }
    }
}

impl ServiceProvider for ModuleStateServiceProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { app.instance(Arc::clone(&self.log)).await })
    }
}

/// 创建唯一模块事件 Provider；descriptor 随后向同一实例追加适用的文章监听器。
pub(crate) fn events_provider(log: Arc<ModuleStateLog>) -> EventsServiceProvider {
    let mut events = EventsServiceProvider::new();
    events.listen_named::<ModuleLifecycleEvent, _>("modules.example.*", move |event, _| {
        let log = Arc::clone(&log);
        Box::pin(async move {
            log.record(event.event_name())?;
            Ok(EventPropagation::Continue)
        })
    });
    events
}

/// 根应用已经加载的模块和激活器持久状态是两种不同的观察值。
#[derive(serde::Serialize)]
pub(crate) struct ModuleState {
    name: String,
    enabled: bool,
    loaded: bool,
    events: Vec<String>,
}

/// 通过公开 `ModuleRepository` 读取状态；管理写入保留在模块扩展命令。
pub(crate) async fn show(
    Inject(repository): Inject<ModuleRepository>,
    Inject(log): Inject<ModuleStateLog>,
) -> Result<ApiResponse<ModuleState>> {
    let module = repository.find_or_fail("example")?;
    Ok(ApiResponse::success(ModuleState {
        name: module.name().to_owned(),
        enabled: module.is_enabled()?,
        loaded: module.is_loaded(),
        events: log.snapshot()?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_log_keeps_only_recent_bounded_history() -> Result<()> {
        let log = ModuleStateLog::default();
        for value in 0..70 {
            log.record(value.to_string())?;
        }
        let history = log.snapshot()?;
        assert_eq!(history.len(), 64);
        assert_eq!(history.first().map(String::as_str), Some("6"));
        assert_eq!(history.last().map(String::as_str), Some("69"));
        Ok(())
    }
}
