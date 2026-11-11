# 並行

## 執行並行工作 {#running-concurrent-tasks}

在應用程式內使用 `openrust::support::facades` 中的 `Concurrency`。在應用程式 Provider 註冊型別化工作，再用 `Task::new(name, input)` 提交。預設 `process` 驅動會為各項工作透過隱藏的 Artisan 命令啟動應用程式執行檔。HTTP 與 Artisan 入口需裝配同一個工作 Provider。

`run` 依提交順序回傳。`runNamed` 接收 `IndexMap` 並保留鍵與順序。程序驅動可為每個子程序設定選用的 `Duration` 逾時。收齊子程序結果後回傳工作錯誤。捨棄仍在執行的程序呼叫會取消其子程序。`sync` 驅動在目前程序中依序執行工作。可透過 `concurrency.default` 選擇預設驅動。

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

## 延後執行並行工作 {#deferring-concurrent-tasks}

在 HTTP 處理器呼叫 `Concurrency::defer(tasks)?` 可將工作排在回應之後。狀態碼低於 400 時執行工作。若錯誤回應也需執行，呼叫傳回控制代碼的 `always()`。延後的工作會攜帶請求 Context，執行失敗由應用程式的例外處理器回報。
