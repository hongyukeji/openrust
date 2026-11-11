# 日誌

## 設定

透過 `LOG_CHANNEL` 選擇預設管道，`LOG_STACK` 設定預設堆疊的成員順序，`LOG_LEVEL` 設定最低層級。預設 `stack` 包含 `single`，將記錄附加至 `storage/logs/openrust.log`。相對路徑以應用程式根目錄為基準。管道可用 `name` 自訂記錄標籤，預設標籤取自應用程式環境。

目前驅動包括檔案 `single`、標準錯誤 `stderr`、捨棄記錄的 `null` 和組合管道的 `stack`。記錄達到最低層級時才開啟檔案。`single` 支援 `locking`、`bubble` 和整數 `permission`，如 Rust 的 `0o600`。權限設定在 Unix 生效。記錄使用 UTC 時間。

## 建立日誌堆疊

`channels` 支援清單或逗號分隔字串。成員依序執行。`bubble: false` 的處理器接受記錄後停止後續處理。`null` 捨棄達到門檻的記錄並停止後續處理。設定 `ignore_exceptions: true` 後，群組會嘗試每個處理器並抑制寫入錯誤。

`Log::stack(&["single", "stderr"], None)` 建立臨時日誌堆疊。具名管道依應用程式延遲解析和快取。更新具名管道設定後，可用 `forgetChannel(Some(name))` 重新解析。

## 寫入日誌

在應用程式生命週期中呼叫 `Log::info("Order {id} created", &context)`。`Log::channel(Some("stderr"))?` 可選擇具名管道。上下文使用結構化 JSON，`replace_placeholders` 啟用預留位置替換。Rust 日誌介面同步寫入。

無效管道會回退到 `logging.channels.emergency.path`。寫入失敗時，緊急管道寫入診斷和原始記錄。緊急檔案也無法寫入時，改用標準錯誤。

```rust
use openrust::{config::Repository, log::{LogContext, LogManager}};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let logger = LogManager::new(Arc::new(Repository::new(Default::default())), root);
    let context = LogContext::from_iter([("id".into(), 42.into())]);
    logger.info("Order {id} created", &context)?;
    Ok(())
}
```
