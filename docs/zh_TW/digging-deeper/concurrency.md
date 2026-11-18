# 並行

## 執行並行工作 {#running-concurrent-tasks}

在應用程式內使用 `openrust::support::facades` 中的 `Concurrency`。在應用程式 Provider 註冊型別化工作，再用 `Task::new(name, input)` 提交。預設 `process` 驅動會為各項工作透過隱藏的 Artisan 命令啟動應用程式執行檔。HTTP 與 Artisan 入口需裝配同一個工作 Provider。

`run` 依提交順序回傳。`runNamed` 接收 `IndexMap` 並保留鍵與順序。程序驅動可為每個子程序設定選用的 `Duration` 逾時。收齊子程序結果後回傳工作錯誤。捨棄仍在執行的程序呼叫會取消其子程序。`sync` 驅動在目前程序中依序執行工作。可透過 `CONCURRENCY_DRIVER` 或 `config/concurrency.rs` 的 `default` 選擇預設驅動。程序工作沿用應用程式根目錄及 Artisan 的 `--env` 所選環境檔案。OpenRust 透過受管理的程序執行並行工作。Laravel 的 `fork` 驅動依賴 PHP CLI 和 `spatie/fork`。

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

呼叫 `Concurrency::defer(tasks)?` 可登記無需讀取回傳值的工作。應用程式在目前生命週期的結束階段執行這些工作。

- **HTTP** 在送出狀態碼低於 400 的回應後執行。
- **Artisan** 在命令以狀態碼 0 傳回後執行。
- **佇列 worker** 在 `JobAttempted` 送出後執行，條件是本次嘗試未失敗且沒有未處理的例外。工作主動釋放且沒有例外時，本次嘗試也視為成功。`sync` 和 `deferred` 佇列連線中的工作沿用外層 HTTP 或 Artisan 生命週期。

呼叫傳回控制代碼的 `always()` 可涵蓋錯誤回應、非零結束狀態，以及已到達結束階段的失敗佇列嘗試。工作子程序在交回結果前若遭終止，該程序內登記的工作會一併捨棄。硬逾時佇列工作交回結果後，由父 worker 執行延後工作，執行時間獨立於原工作的逾時預算。

在生命週期結束時，`process` 驅動為每項任務啟動獨立的 Artisan 子程序。子程序啟動後，發起程序即可結束，任務仍可繼續執行。子程序透過 stdin 接收輸入。發起程序回報啟動失敗，子程序透過應用程式的例外處理器回報各自的任務失敗。如果需要結果或要等待任務完成，請使用 `run`。

`sync` 驅動在生命週期結束前，在目前程序按登記順序完成任務。兩種驅動都會保留登記任務時擷取的公開和隱藏 Context。
