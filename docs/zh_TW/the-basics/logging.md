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

## 自訂日誌記錄 {#customizing-channel-records}

在服務 Provider 中用 `LogManager::registerFormatter`、`registerProcessor` 和 `registerTap` 註冊名稱。透過 `formatter` 選擇格式化器，用 `formatter_with` 傳入選項。將 `formatter` 設為 `"default"` 使用內建格式。檔案、輪替檔案與標準錯誤處理器依格式化器回傳的位元組寫入，記錄分隔符號也由格式化器提供。自訂日誌工廠管理自己的輸出。

`processors` 的成員可以是名稱，也可以是帶有 `processor` 和 `with` 的物件。處理器依設定順序在格式化前執行，可透過 `LogRecord` 修改訊息、記錄上下文和公開作用域中繼資料。日誌堆疊依成員順序合併處理器，各成員沿用自己的格式化器。

`"audit:orders,prod"` 這樣的 tap 會收到日誌器和兩個參數。tap 在快取通道前依清單順序執行。用 `setFormatter` 取代通道格式化器，用 `pushProcessor` 在最前面插入處理器。

```rust
use openrust::{
    config::Repository,
    log::{LogContext, LogManager},
};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = LogManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    manager.registerFormatter("message", |_| {
        Ok(Arc::new(|record| Ok(format!("{}\n", record.message()))))
    });
    let channel = manager.build(&json!({
        "driver": "single",
        "path": "storage/logs/audit.log",
        "formatter": "message",
    }))?;
    channel.info("Order recorded", &LogContext::new())?;
    Ok(())
}
```

## 處理器通道 {#handler-channels}

透過 `driver: "monolog"` 指定具名 Rust 處理器。內建 `stream` 處理器接受檔案路徑、`stderr` 或 `stdout`。可用 `LogManager::registerHandler` 註冊其他處理器。工廠收到合併後的設定，回傳接收結構化記錄與格式化文字的回呼。設定依通道 `level`、`with`、`handler_with` 的順序合併，後者覆蓋前者。

設定 `action_level` 後，通道累積記錄，到達指定等級時寫出整批。底層處理器仍依最低 `level` 逐筆篩選。第一次寫出後，後續記錄會直接進入底層處理器。設定 `stop_buffering: false` 可在每次寫出後開始累積下一批。緩衝狀態屬於解析後的通道，釋放通道時會捨棄尚未觸發的批次。

```rust
use openrust::{config::Repository, log::{LogContext, LogManager}};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = LogManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    let audit = manager.build(&json!({
        "driver": "monolog",
        "handler": "stream",
        "with": {"stream": "storage/logs/audit.log"},
        "handler_with": {"level": "info"},
        "action_level": "error",
        "stop_buffering": false,
    }))?;
    audit.info("Import started", &LogContext::new())?;
    audit.error("Import failed", &LogContext::new())?;
    Ok(())
}
```

## Syslog 通道 {#syslog-channel}

設定 `driver: "syslog"` 後，將記錄傳送到本機系統日誌。在 Unix 上使用本機資料包通訊端。macOS 預設使用 `/var/run/syslog`，Linux 預設使用 `/dev/log`。可透過 `socket` 指定接收端，相對路徑從應用程式根目錄解析。傳送失敗時寫入緊急日誌，同一通道在下一筆記錄到達時可重新傳送。

`facility` 預設為 `user`，可填寫 `local4` 等名稱或對應的 syslog 數值常數。資料包的優先等級由 facility 和記錄等級組成，標籤取自應用程式名稱與處理程序 ID。省略格式化器時，訊息本文採用框架行格式。設定 `formatter: "default"` 可省略本文的時間戳記，也可以指定已註冊的格式化器。缺少 Unix 資料包支援的平台使用緊急日誌。

```rust
use openrust::{config::Repository, log::{LogContext, LogManager}};
use serde_json::json;
use std::{os::unix::net::UnixDatagram, sync::Arc, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let receiver = UnixDatagram::bind("audit.sock")?;
    receiver.set_read_timeout(Some(Duration::from_secs(2)))?;
    let manager = LogManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    let audit = manager.build(&json!({
        "driver": "syslog",
        "socket": "audit.sock",
        "facility": "local4",
        "formatter": "default",
    }))?;
    audit.notice("Import complete", &LogContext::new())?;
    let mut bytes = [0; 1024];
    let received = receiver.recv(&mut bytes)?;
    let packet = std::str::from_utf8(&bytes[..received])?;
    assert!(packet.starts_with("<165>"));
    assert!(packet.ends_with("Import complete"));
    Ok(())
}
```
在 Windows 上，通道寫入 Application 事件日誌，Provider 名稱取自應用程式名稱。事件詳細資料中的兩個插入字串分別保留含處理程序 ID 的應用程式標籤與格式化訊息，類別保留 syslog 等級編號。與 PHP 的 Windows 接線一致，`alert` 對應 Error，`info` 對應 Information，其餘等級對應 Warning。Windows 會驗證 `facility`，事件本身沒有 facility 欄位。`socket` 僅適用於 Unix，Windows 設定應省略此項。

在 PowerShell 中將 `my-app` 換成從應用程式名稱衍生的 Provider 名稱，即可讀取原始訊息。

```powershell
$filter = "*[System[Provider[@Name='my-app']]]"
Get-WinEvent -LogName Application -FilterXPath $filter -MaxEvents 1 |
    ForEach-Object { $_.Properties[1].Value }
```


## JSON 例外日誌 {#json-exception-logs}

設定 `formatter: "json"` 可輸出結構化記錄。可透過 `formatter_with` 設定 `append_newline`、`ignore_empty_context_and_extra` 和 `max_normalize_depth`（預設 9）。

使用 `Log::logException(level, message, &error, &context)` 將錯誤寫入 `context.exception`，來源鏈記錄在 `previous` 中。在 `withExceptions` 中註冊 `exceptionContext::<E>`，為相符的錯誤型別新增欄位，來源錯誤也適用。現有 `context` 回呼會補充直接記錄的根錯誤。應用程式透過報告機制記錄時，根錯誤欄位只出現在記錄的上下文中，來源錯誤欄位仍保留在對應節點。Rust 會保留直接傳入與已註冊錯誤的型別名稱，型別被抹除的來源可能顯示為 `Error`。標準錯誤介面不提供原始檔案或呼叫堆疊。

```rust
use openrust::{config::Repository, log::{Level, LogContext, LogManager}};
use serde_json::json;
use std::{io, sync::Arc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = LogManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    let logger = manager.build(&json!({
        "driver": "single",
        "path": "storage/logs/errors.jsonl",
        "formatter": "json",
    }))?;
    let error = io::Error::other("storage offline");
    logger.logException(Level::Error, "Import failed", &error, &LogContext::new())?;
    Ok(())
}
```

## 上下文資訊

[Context Facade](../digging-deeper/context.md) 擷取的公開欄位以獨立 JSON 中繼資料寫在單筆日誌欄位之後。自訂寫入端可實作 `logWithContext` 接收兩組欄位。隱藏欄位保存在上下文儲存庫中。

## 監聽日誌訊息

在服務提供者的 `boot` 方法中註冊 `Log::listen`。管道接受並寫入記錄後，同步執行監聽閉包，閉包回傳 `LogResult<()>`。`LogManager::listen` 和 `Logger::listen` 共用目前應用程式的監聽器，涵蓋具名管道、日誌堆疊、臨時管道及重新命名的 Logger。

`MessageLogged` 包含原始訊息、層級及 context。佔位符插值與管道處理器套用於寫出的記錄。`logException` 也提供正規化的 `context["exception"]`。低於所有處理器門檻的記錄跳過通知。緊急回退記錄及診斷也會通知監聽器。

閉包依註冊順序執行。回呼錯誤在日誌寫入後傳回呼叫者，並停止後續回呼。閉包可以再次寫入日誌或註冊監聽器，新增註冊於下一次通知生效。同步日誌監聽在呼叫者的應用程式作用域內執行，通用非同步事件分發採用顯式呼叫。

```rust
use openrust::{log::LogError, support::facades::Log};

fn register_log_listener() -> Result<(), LogError> {
    Log::listen(|event| {
        eprintln!("{}", event.message);
        Ok(())
    })
}
```

## Pail 工作階段擷取

使用 `cargo build --features pail` 啟用選用的 `openrust-pail` 套件，現有套件探索會裝配 Provider。它將任意日誌驅動接受的訊息複製到目前應用的 `storage/pail/*.pail` 活躍工作階段。沒有工作階段檔案時不寫擷取檔案。`pail` 命令及篩選介面仍待實作。

套件公開 `File`、`Files`、`Handler` 和 `PailServiceProvider`。`File::create()` 建立工作階段，保留既有檔案內容；`destroy()` 刪除工作階段。跨處理程序檔案鎖保護 JSON 行寫入，保留等級和結構化例外。超過一小時未寫入的工作階段會被清除。日誌不會重建已刪除的工作階段，檔案錯誤沿原日誌結果傳遞。

`context.__pail.origin` 包含 HTTP 方法與路徑、命令名稱或佇列與任務來源。公開 Context 覆蓋訊息 context，隱藏資料不進入擷取。Rust 以綁定應用的非同步作用域隔離同處理程序並行請求、巢狀命令與並行任務；沒有日誌監聽器時略過來源收集。

HTTP 身分讀取目前 guard 已解析的使用者，後續切換 guard 也會更新。非同步驗證尚未解析時身分為 null。PHP Pail 可以在同步日誌呼叫中解析驗證，這項差異另行追蹤；本節不代表 Pail 命令和驗證行為已經全部對齊。

### 讀取與篩選擷取記錄

`openrust_pail::value_objects::MessageLogged::fromJson` 讀取一行擷取資料，可取得訊息、等級、日期／時間、例外類別與檔案、trace，以及具型別的 HTTP、命令或佇列來源。解析、日期轉換與來源解碼的失敗透過 `Result` 傳回。`context()` 排除保留欄位 `__pail`、`exception` 和 `userId`。

`Options::accepts` 要求所有已設定條件同時符合。等級比較與子字串搜尋使用 ASCII 大小寫折疊。message 條件只搜尋訊息文字；通用 filter 搜尋包含來源與 context 的四欄位 JSON 表示。該表示遵循 PHP 的 Unicode 跳脫、物件順序及浮點格式，因此通用 Unicode 搜尋可能需要使用 `\u` 表示。驗證 ID 按字串精確比較，`12` 與 `012` 不同。`fromCommand(&Input)` 保留 auth 優先於 user 的規則，包括明確空值，以及 PHP 的 timeout 轉換；Rust 的命令輸入物件與命令實作分開傳入。

```rust
use openrust_pail::{Options, value_objects::MessageLogged};

fn select(line: &str) -> Result<bool, serde_json::Error> {
    let message = MessageLogged::fromJson(line)?;
    let options = Options::new(3600, None, Some("ERROR".into()), None, Some("payment".into()));
    Ok(options.accepts(&message))
}
```

以上解析與篩選介面已驗收。即時 `pail` 命令、終端列印與 dev 註冊仍待接入，驗證自動解析保留上文所述限制。
