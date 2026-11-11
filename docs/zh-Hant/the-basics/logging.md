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

## 輪替檔案管道

使用 `daily` 或 `monthly` 產生如 `openrust-2026-11-11.log` 和 `openrust-2026-11.log` 的檔案。每個 UTC 週期內，第一筆達到門檻的記錄開啟當期檔案。`max_files` 包含目前的檔案，設為 0 時不限保留數量。應用程式預設保留 14 個日檔案和 3 個月檔案。清理僅處理基本檔名、副檔名與週期格式皆相符的一般檔案。

## 臨時管道與自訂管道

`Log::build(&settings)?` 建立臨時日誌管道。使用 `Log::stackChannels` 與 `StackChannel::Named`、`StackChannel::Logger` 可組合設定管道和臨時管道。組合後的日誌堆疊有自己的標籤。

設定 `driver: "custom"` 時，將 `via` 指向透過 `LogManager::registerFactory` 登記的工廠。使用 `LogManager::extend` 可註冊具名驅動。工廠是編譯後的 Rust 閉包，接收管理器和管道設定，回傳實作日誌契約的物件。首次並行解析時可能同時呼叫工廠，初始化應可安全重複。成功解析的具名管道共用已發布的實例。工廠失敗時使用緊急日誌，後續呼叫可重試。

```rust
use openrust::{config::Repository, log::{LogContext, LogManager, StackChannel}};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let manager = LogManager::new(Arc::new(Repository::new(Default::default())), root);
    manager.registerFactory("audit", |manager, config| {
        let logger = manager.build(&json!({"driver":"monthly", "path":config["path"]}))?;
        Ok(logger)
    });
    let audit = manager.build(&json!({"driver":"custom", "via":"audit", "path":"audit.log"}))?;
    let stack = manager.stackChannels(
        &[StackChannel::Named("single"), StackChannel::Logger(&audit)],
        Some("orders"),
    )?;
    stack.info("Audit ready", &LogContext::new())?;
    Ok(())
}
```

## 上下文資訊

[Context Facade](../digging-deeper/context.md) 擷取的公開欄位以獨立 JSON 中繼資料寫在單筆日誌欄位之後。自訂寫入端可實作 `logWithContext` 接收兩組欄位。隱藏欄位保存在上下文儲存庫中。
