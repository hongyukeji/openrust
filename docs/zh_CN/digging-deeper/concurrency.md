# 并发

## 运行并发任务 {#running-concurrent-tasks}

在应用内使用 `openrust::support::facades` 中的 `Concurrency`。在应用 Provider 中注册类型化任务，用 `Task::new(name, input)` 提交。默认 `process` 驱动为每个任务通过隐藏 Artisan 命令启动应用可执行文件。HTTP 和 Artisan 入口需要装配同一个任务 Provider。

`run` 按提交顺序返回。`runNamed` 接收 `IndexMap` 并保留键和顺序。进程驱动支持为每个子进程设置可选 `Duration` 超时。收齐子进程结果后返回任务错误。丢弃尚在运行的进程调用会取消其子进程。`sync` 驱动在当前进程顺序运行任务。可通过 `concurrency.default` 选择默认驱动。

```rust
use openrust::{
    concurrency::{Manager, Registry, Task},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

struct Tasks;

impl ServiceProvider for Tasks {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().make::<Registry>()?.register("math:double", |value: u64| async move {
                Ok::<_, std::convert::Infallible>(value * 2)
            })?;
            Ok(())
        })
    }
}

let mut app = Application::configure(std::env::current_dir()?)
    .withProviders(vec![Box::new(Tasks)])
    .create()?;
app.boot().await?;

let manager = app.container().make::<Manager>()?;
let result = manager.driver(Some("sync"))?
    .run([Task::new("math:double", 21)?, Task::new("math:double", 6)?], None)
    .await?;
assert_eq!(result, vec![serde_json::json!(42), serde_json::json!(12)]);
```

## 延迟执行并发任务 {#deferring-concurrent-tasks}

在 HTTP 处理器中调用 `Concurrency::defer(tasks)?` 可将任务安排到响应之后。状态码低于 400 时执行任务。若错误响应也需执行，调用返回句柄的 `always()`。延迟任务会携带请求 Context，执行失败由应用异常处理器报告。
