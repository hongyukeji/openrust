# 并发

## 运行并发任务 {#running-concurrent-tasks}

在应用内使用 `openrust::support::facades` 中的 `Concurrency`。在应用 Provider 中注册类型化任务，用 `Task::new(name, input)` 提交。默认 `process` 驱动为每个任务通过隐藏 Artisan 命令启动应用可执行文件。HTTP 和 Artisan 入口需要装配同一个任务 Provider。

`run` 按提交顺序返回。`runNamed` 接收 `IndexMap` 并保留键和顺序。进程驱动支持为每个子进程设置可选 `Duration` 超时。收齐子进程结果后返回任务错误。丢弃尚在运行的进程调用会取消其子进程。`sync` 驱动在当前进程顺序运行任务。可通过 `CONCURRENCY_DRIVER` 或 `config/concurrency.rs` 的 `default` 选择默认驱动。进程任务沿用应用根目录和 Artisan 的 `--env` 所选环境文件。OpenRust 用受管进程执行并行任务。Laravel 的 `fork` 驱动依赖 PHP CLI 和 `spatie/fork`。

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

调用 `Concurrency::defer(tasks)?` 可登记无需读取返回值的任务。应用在当前生命周期的结束阶段执行这些任务。

- **HTTP** 在发送状态码低于 400 的响应后执行。
- **Artisan** 在命令以状态码 0 返回后执行。
- **队列 worker** 在 `JobAttempted` 派发后执行，条件是本次尝试未失败且没有未处理的异常。任务主动释放且没有异常时，本次尝试也视为成功。`sync` 和 `deferred` 队列连接中的任务沿用外层 HTTP 或 Artisan 生命周期。

调用返回句柄的 `always()` 可覆盖错误响应、非零退出及已到达结束阶段的失败队列尝试。如果任务子进程在交回结果前被终止，该进程内登记的任务随之丢弃。硬超时队列任务交回结果后，由父 worker 执行延后任务，执行时间独立于原任务的超时预算。

延后任务携带登记时的公开和隐藏 Context。应用通过异常处理器报告执行失败，继续执行后续回调，并回收受管子进程。

在 Windows ARM64 上，受管子进程通过 stdin 接收任务输入和 Context。HTTP 可以在任务仍运行时交付响应。Artisan 会等待延后子进程退出，然后按原始退出码退出并关闭输出管道。
