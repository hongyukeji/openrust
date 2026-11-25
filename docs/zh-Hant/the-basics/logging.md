# 日誌

Slack 管道的預設顯示名稱為應用程式名稱加 ` Log`，例如 `ExampleStore Log`。可在 `.env` 中設定 `LOG_SLACK_USERNAME` 覆寫。

## 設定

透過 `LOG_CHANNEL` 選擇預設管道，`LOG_STACK` 設定預設堆疊的成員順序，`LOG_LEVEL` 設定最低層級。預設 `stack` 包含 `single`，將記錄附加至 `storage/logs/app.log`。相對路徑以應用程式根目錄為基準。管道可用 `name` 自訂記錄標籤，預設標籤取自應用程式環境。

支援 `single`、`daily`、`monthly`、`stderr`、`syslog`、`errorlog`、`slack`、`papertrail`、`null`、`stack`、`monolog` 及註冊的 `custom` 工廠。記錄達到最低層級時才開啟檔案。`single` 支援 `locking`、`bubble` 和整數 `permission`，如 Rust 的 `0o600`。權限設定在 Unix 生效。記錄使用 UTC 時間。

## 記錄棄用警告 {#logging-deprecation-warnings}

`LOG_DEPRECATIONS_CHANNEL` 選擇受支援的執行期相容警告管道，預設 `null` 捨棄記錄。`LOG_DEPRECATIONS_TRACE=true` 附帶位置與回溯，透過 `logging.deprecations` 設定。已定義的 `logging.channels.deprecations` 具名管道始終優先於所選管道。`APP_ENV=testing` 時預設跳過警告，`LOG_DEPRECATIONS_WHILE_TESTING=true` 可啟用。日誌失敗不會使棄用警告變成失敗回應。

此處處理樣板求值中有損數值轉換等執行期相容警告。Rust 編譯器診斷仍屬建構輸出，不會全域攔截所有依賴函式庫的警告。應用程式自身的執行期警告可明確寫入設定的 deprecations 管道。

## 建立日誌堆疊

`channels` 支援清單或逗號分隔字串。成員依序執行。`bubble: false` 的處理器接受記錄後停止後續處理。`null` 捨棄達到門檻的記錄並停止後續處理。設定 `ignore_exceptions: true` 後，群組會嘗試每個處理器並抑制寫入錯誤。

`Log::stack(&["single", "stderr"], None)` 建立臨時日誌堆疊。具名管道依應用程式延遲解析和快取。更新具名管道設定後，可用 `forget_channel(Some(name))` 重新解析。

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

使用 `daily` 或 `monthly` 產生如 `app-2026-11-11.log` 和 `app-2026-11.log` 的檔案。每個 UTC 週期內，第一筆達到門檻的記錄開啟當期檔案。`max_files` 包含目前的檔案，設為 0 時不限保留數量。應用程式預設保留 14 個日檔案和 3 個月檔案。清理僅處理基本檔名、副檔名與週期格式皆相符的一般檔案。

## 臨時管道與自訂管道

`Log::build(&settings)?` 建立臨時日誌管道。使用 `Log::stack_channels` 與 `StackChannel::Named`、`StackChannel::Logger` 可組合設定管道和臨時管道。組合後的日誌堆疊有自己的標籤。

設定 `driver: "custom"` 時，將 `via` 指向透過 `LogManager::register_factory` 登記的工廠。使用 `LogManager::extend` 可註冊具名驅動。工廠是編譯後的 Rust 閉包，接收管理器和管道設定，回傳實作日誌契約的物件。首次並行解析時可能同時呼叫工廠，初始化應可安全重複。成功解析的具名管道共用已發布的實例。工廠失敗時使用緊急日誌，後續呼叫可重試。

```rust
use openrust::{config::Repository, log::{LogContext, LogManager, StackChannel}};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let manager = LogManager::new(Arc::new(Repository::new(Default::default())), root);
    manager.register_factory("audit", |manager, config| {
        let logger = manager.build(&json!({"driver":"monthly", "path":config["path"]}))?;
        Ok(logger)
    });
    let audit = manager.build(&json!({"driver":"custom", "via":"audit", "path":"audit.log"}))?;
    let stack = manager.stack_channels(
        &[StackChannel::Named("single"), StackChannel::Logger(&audit)],
        Some("orders"),
    )?;
    stack.info("Audit ready", &LogContext::new())?;
    Ok(())
}
```

## 自訂日誌記錄 {#customizing-channel-records}

在服務 Provider 中用 `LogManager::register_formatter`、`register_processor` 和 `register_tap` 註冊名稱。透過 `formatter` 選擇格式化器，用 `formatter_with` 傳入選項。將 `formatter` 設為 `"default"` 保留處理器自己的格式器；省略時採用管道預設格式器。檔案、輪替檔案與標準錯誤處理器依格式化器回傳的位元組寫入，記錄分隔符號也由格式化器提供。自訂日誌工廠管理自己的輸出。

`processors` 的成員可以是名稱，也可以是帶有 `processor` 和 `with` 的物件。處理器依設定順序在格式化前執行，可透過 `LogRecord` 修改訊息、記錄上下文和公開作用域中繼資料。日誌堆疊依成員順序合併處理器，各成員沿用自己的格式化器。

`"audit:orders,prod"` 這樣的 tap 會收到日誌器和兩個參數。tap 在快取通道前依清單順序執行。用 `set_formatter` 取代通道格式化器，用 `push_processor` 在最前面插入處理器。

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
    manager.register_formatter("message", |_| {
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

透過 `driver: "monolog"` 指定具名 Rust 處理器。內建 `stream` 處理器接受檔案路徑、`stderr` 或 `stdout`。可用 `LogManager::register_handler` 註冊其他處理器。工廠收到合併後的設定，回傳接收結構化記錄與格式化文字的回呼。設定依通道 `level`、`with`、`handler_with` 的順序合併，後者覆蓋前者。

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
    assert!(packet.ends_with("Import complete [] []"));
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

使用 `Log::log_exception(level, message, &error, &context)` 將錯誤寫入 `context.exception`，來源鏈記錄在 `previous` 中。在 `with_exceptions` 中註冊 `exception_context::<E>`，為相符的錯誤型別新增欄位，來源錯誤也適用。現有 `context` 回呼會補充直接記錄的根錯誤。應用程式透過報告機制記錄時，根錯誤欄位只出現在記錄的上下文中，來源錯誤欄位仍保留在對應節點。Rust 會保留直接傳入與已註冊錯誤的型別名稱，型別被抹除的來源可能顯示為 `Error`。標準錯誤介面不提供原始檔案或呼叫堆疊。

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
    logger.log_exception(Level::Error, "Import failed", &error, &LogContext::new())?;
    Ok(())
}
```

## 上下文資訊

[Context Facade](../digging-deeper/context.md) 擷取的公開欄位以獨立 JSON 中繼資料寫在單筆日誌欄位之後。自訂寫入端可實作 `log_with_context` 接收兩組欄位。隱藏欄位保存在上下文儲存庫中。

## 監聽日誌訊息

在服務提供者的 `boot` 方法中註冊 `Log::listen`。管道接受並寫入記錄後，同步執行監聽閉包，閉包回傳 `LogResult<()>`。`LogManager::listen` 和 `Logger::listen` 共用目前應用程式的監聽器，涵蓋具名管道、日誌堆疊、臨時管道及重新命名的 Logger。

`MessageLogged` 包含原始訊息、層級及 context。佔位符插值與管道處理器套用於寫出的記錄。`log_exception` 也提供正規化的 `context["exception"]`。低於所有處理器門檻的記錄跳過通知。緊急回退記錄及診斷也會通知監聽器。

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

使用 `cargo build --features pail` 啟用選用的 `openrust-pail` 套件，現有套件探索會裝配 Provider。它將任意日誌驅動接受的訊息複製到目前應用的 `storage/pail/*.pail` 活躍工作階段。沒有工作階段檔案時不寫擷取檔案。實際命令和篩選介面見[下文](#tailing-log-messages-using-pail)。

套件公開 `File`、`Files`、`Handler` 和 `PailServiceProvider`。`File::create()` 建立工作階段，保留既有檔案內容；`destroy()` 刪除工作階段。跨處理程序檔案鎖保護 JSON 行寫入，保留等級和結構化例外。超過一小時未寫入的工作階段會被清除。日誌不會重建已刪除的工作階段，檔案錯誤沿原日誌結果傳遞。

`context.__pail.origin` 包含 HTTP 方法與路徑、命令名稱或佇列與任務來源。公開 Context 覆蓋訊息 context，隱藏資料不進入擷取。Rust 以綁定應用的非同步作用域隔離同處理程序並行請求、巢狀命令與並行任務；沒有日誌監聽器時略過來源收集。

### 日誌呼叫時的請求驗證 {#request-log-authentication}

同步 `Log::info` 等既有方法擷取目前 guard 已解析的身分，未解析使用者為 null。在非同步 HTTP 處理器中，需要本次日誌解析驗證時，使用請求日誌入口：

```rust
Log::log_with_request(request, openrust::log::Level::Info, "Order placed", &LogContext::new()).await?;
```

`LogManager` 和已解析的 `Logger` 通道也提供同名方法。通道先寫日誌，再按註冊順序通知監聽器。存在活躍 Pail 工作階段時，Pail 等待目前選取的 guard，在本次呼叫返回前擷取 ID 和信箱。已快取使用者直接重用；登入、登出和 guard 切換影響後續日誌。沒有活躍工作階段、層級未被通道接受或被抑制的棄用警告不會觸發驗證。內建 HTTP 例外報告也使用請求日誌入口。

顯式 await 對應 PHP 在同步日誌監聽器中解析 guard 的能力。容器回呼等同步程式碼繼續使用身分快照。`LogManager::listen_for_request` 註冊同步監聽器及其非同步請求版本，普通 `listen` 回呼仍同步執行。執行回呼時不持有監聽器註冊表鎖。guard 解析期間產生的日誌讀取已有快照，不再遞迴解析驗證。

驗證或監聽器錯誤在普通通道完成寫入後沿日誌結果返回。取消 future 因而可能留下已有通道記錄而沒有 Pail 擷取。取消會釋放請求借用與解析作用域，後續呼叫可以重試。請求和應用程式分別儲存驗證狀態；HTTP 中程式化呼叫 Artisan 保留該 HTTP 來源，獨立 Artisan 使用命令來源。

認證回呼本身可以寫日誌。同一請求、同名 Guard 的巢狀讀取只回傳已知身分，不重新啟動尚未完成的認證。完成、錯誤和取消均釋放解析標記；外層認證錯誤仍正常回傳，後續請求或重試可重新解析。

### 讀取與篩選擷取記錄

`openrust_pail::value_objects::MessageLogged::from_json` 讀取一行擷取資料，可取得訊息、等級、日期／時間、例外類別與檔案、trace，以及具型別的 HTTP、命令或佇列來源。解析、日期轉換與來源解碼的失敗透過 `Result` 傳回。`context()` 排除保留欄位 `__pail`、`exception` 和 `user_id`。

`Options::accepts` 要求所有已設定條件同時符合。等級比較與子字串搜尋使用 ASCII 大小寫折疊。message 條件只搜尋訊息文字；通用 filter 搜尋包含來源與 context 的四欄位 JSON 表示。該表示遵循 PHP 的 Unicode 跳脫、物件順序及浮點格式，因此通用 Unicode 搜尋可能需要使用 `\u` 表示。驗證 ID 按字串精確比較，`12` 與 `012` 不同。`from_command(&Input)` 保留 auth 優先於 user 的規則，包括明確空值，以及 PHP 的 timeout 轉換；Rust 的命令輸入物件與命令實作分開傳入。

```rust
use openrust_pail::{Options, value_objects::MessageLogged};

fn select(line: &str) -> Result<bool, serde_json::Error> {
    let message = MessageLogged::from_json(line)?;
    let options = Options::new(3600, None, Some("ERROR".into()), None, Some("payment".into()));
    Ok(options.accepts(&message))
}
```

以上解析與篩選介面也用於[即時 Pail 命令](#tailing-log-messages-using-pail)。請求驗證遵循上文的顯式 await 與快照規則。

### Provider 啟用條件

停用擷取時仍註冊 Files 和 Handler 綁定。啟動時，若容器環境名稱精確為 `testing`，或 `VAPOR_SSM_PATH` 為非空且非 `0` 的字串，就略過擷取監聽。`PAIL_TESTS=true` 用於 Pail 自身驗收，可以覆蓋這兩個條件。該旗標按框架Env 規則處理 false、empty、null 和引號值；Vapor 標記按原始 PHP 字串真假判斷。

旗標來自目前應用不可變的環境快照，處理程序變數優先於選定的環境檔案。設定快取生效時不載入環境檔案，因此檔案中的 `PAIL_TESTS` 不生效。Handler 在第一條被接受的日誌或佇列生命週期事件到來時才解析；建構失敗在該時機傳遞，原日誌驅動已接受的寫入仍保留。

Pail 透過共用路徑服務解析 `Application::storage_path("pail")`。在首次擷取前設定 `use_storage_path` 或 `APP_STORAGE_PATH`，參見[應用儲存路徑](../getting-started/configuration.md#application-storage-path)。

預設檔案日誌頻道和緊急回退使用應用儲存路徑。應在載入設定或解析頻道前設定路徑；已載入的頻道路徑保留其設定值，參見[應用儲存路徑](../getting-started/configuration.md#application-storage-path)。

## Pail CLI 列印器 {#pail-cli-printer}

可選 Pail 套件提供 `openrust_pail::contracts::Printer` 和 `printers::CliPrinter`。在 Artisan 命令中建構 `CliPrinter::new(output, base_path)`，呼叫 `print(&message_logged)?`。列印器讀取目前輸出的詳細程度與色彩狀態。一般模式顯示時間並縮略類型、檔案和訊息；`-v` 顯示完整日期與本文，`-vv` 或 `-vvv` 再顯示擷取的堆疊框架。

底部顯示 HTTP 方法／路徑和驗證資訊，或佇列／工作／命令，以及公開 Context。數字鍵與 null、PHP 風格值匯出、HTML 文字和多行訊息依固定 Pail 規則處理。邊框寬度上限為 150 個終端欄；原始訊息和長底部資訊可能像上游一樣溢出。Rust 的 `::` 類型命名空間及 Windows 本機路徑分隔符適配到縮略和相對來源顯示。

Rust JSON 無符號整數保留原始精度；超過 PHP 有號整數範圍的值，其顯示可能與 PHP 的浮點近似不同。Rust 直接產生終端訊息框，透過 `Output::write_raw` 寫入已轉譯文字。真實 Artisan 輸出立即寫出，擷取輸出保留在緩衝區。`--ansi`、`--no-ansi`、`--quiet` 和 `--silent` 生效，寫入錯誤沿呼叫鏈傳遞；格式錯誤在寫入該訊息框前傳回。輸出詳細程度隨呼叫作用域儲存和還原。`PAIL_TESTS` 用於擷取測試，顯示的日期、檔案和堆疊保留實際紀錄值，不採用 PHP 測試夾具中的替換值。

[即時 Pail 命令](#tailing-log-messages-using-pail)使用此列印器，載入 Provider 後也會向 [Artisan dev](../digging-deeper/artisan.md#the-dev-command)加入預設 `logs` 程序。

## 使用 Pail 追蹤日誌 {#tailing-log-messages-using-pail}

在應用程式 `Cargo.toml` 的 `[features].default` 清單中加入 `pail`，保留已有功能，然後使用原始碼入口：

```sh
./artisan pail --timeout=1
./artisan pail --timeout=0 --level=error --message=payment -vv
```

`pail` 跟隨應用程式日誌事件，不依賴日誌通道的儲存後端。每次呼叫在目前應用程式儲存路徑的 `pail/` 目錄建立獨立工作階段，並行呼叫分別接收日誌。`--filter`、`--message`、`--level`、`--auth` 和 `--user` 使用上文的篩選規則，`--auth` 優先於別名 `--user`。執行逾時預設 3600 秒，零表示停用逾時；逾時印出「Maximum execution time exceeded.」後成功退出。Ctrl+C 或 SIGTERM 停止跟隨並刪除本次工作階段。無效記錄或輸出錯誤令命令失敗，同樣清理工作階段；`--quiet` 和 `--silent` 抑制輸出。

Rust 原生跟隨檔案，支援分段 UTF-8 行、截斷及依名稱重新開啟輪換檔案，無需 PHP、PCNTL 或 `tail` 子程序。初次讀取已有內容遵循末十行規則。偵測到截斷或替換時會捨棄未完成行，避免串接不同檔案世代；完整 JSON 記錄立即篩選並印出。Windows 上使用 `.\artisan.bat pail --timeout=0`。原生主控台 Ctrl+C 和正常逾時均會停止命令並刪除本次擷取工作階段。

Provider 透過所屬應用程式的 ConsoleKernel 註冊命令，沿用擷取的 testing／Vapor 門禁。Rust 沒有 PHP 全域 SAPI 旗標：僅啟動 HTTP 不執行命令註冊回呼，獨立與程式化 Artisan 使用同一回呼。取消擷取呼叫會清理工作階段並恢復呼叫端輸出狀態。本實作不增加心跳：與上游一樣，即使 `--timeout=0`，超過一小時沒有寫入的工作階段也可能被下一次日誌生產清理。清理保留 `.gitignore`。載入 Provider 後也會向 [Artisan dev](../digging-deeper/artisan.md#the-dev-command)加入預設 `logs` 程序。

### 開發程序註冊

啟用 `pail` 後，套件探索載入 Provider，`./artisan dev:list --json` 會列出執行 `pail --timeout=0` 的 `logs`。用 `./artisan dev --inline` 啟動，監督器沿用同一執行檔，不呼叫 Cargo，也不要求 PHP／PCNTL。未編譯 Pail，或應用程式未載入其 Provider 時，沒有預設 `logs` 項目。查詢清單不會建立擷取工作階段。

註冊依 Provider 身分判斷，獨立於擷取門禁。在 testing 或 Vapor 環境中，已載入的 Provider 仍提供預設 `logs`，但 `pail` 命令被門禁停用，除非設定 PAIL_TESTS 例外。此類應用程式可用 `DevCommands::except(&["logs"])?` 排除它。Windows 監督器向預設 `logs` 程序傳送主控台關停訊號，並透過 Windows 作業回收程序樹。正常 Ctrl+C 關停會刪除本次 Pail 工作階段。

每次解析程序清單都會檢查 Provider 的載入狀態，因此在 Pail 註冊之前查詢清單，不會導致後續永久遺漏 `logs`。
## 頻道上下文

使用 `with_context`、`without_context` 設定或移除頻道上下文。`share_context` 合併到已解析及未來的頻道，每則日誌的同名欄位優先。`shared_context` 讀取未來頻道的預設值。`flush_shared_context` 只清空這些預設值，不刪除已合併到頻道的欄位；移除頻道欄位使用 `without_context`。啟動階段預設值由請求或佇列工作繼承，動態上下文按作用域隔離，重用頻道控制代碼也不跨請求共用。


## 讀取已解析通道

`LogManager::get_channels()` 與 `Log::get_channels()` 傳回已解析通道名稱到 `Arc<Logger>` 控制代碼的快照，不建立尚未使用的通道。`forget_channel` 移除通道後，影響後續解析；先前傳回的快照和日誌控制代碼仍可使用。

## 存取原生日誌器

`Logger::get_logger()` 傳回通道實際使用的 `Arc<NativeLogger>` 控制代碼。`LogManager::get_logger()` 與 `Log::get_logger()` 解析預設通道，透過 `Result` 傳回同一個原生實例。讀取已解析控制代碼不會建立其他通道；`forget_channel` 後，已持有的控制代碼仍可使用。

```rust
use openrust::{log::LogContext, support::facades::Log};

let channel = Log::channel(None)?;
let native = channel.get_logger();
native.info("Worker started", &LogContext::new())?;
let default_native = Log::get_logger()?;
```

原生寫入複用設定的級別篩選、處理器、格式器與輸出目標，只接收呼叫時傳入的上下文，略過應用程式頻道上下文、Context 中繼資料、MessageLogged 監聽器及 Pail 擷取。原生寫入或處理器失敗會直接傳回錯誤。需要應用程式上下文、事件及緊急復原時，使用通道或 Log Facade 的級別寫入方法。原生控制代碼實作既有 Rust 日誌契約；共用通道的處理器與格式器繼續透過既有註冊與 tap 介面設定。

## 程序錯誤日誌 {#process-error-log}

使用 `driver: "errorlog"` 將記錄寫入程序錯誤串流。支援 `type: 0`（預設）與 `type: 4`；原生可執行程式沒有 PHP SAPI，兩者均寫標準錯誤。目標額外附加一個換行，即使格式化文字已有結尾換行。等級篩選、預留位置、處理器與格式化沿用一般通道規則。此驅動依建構子契約始終繼續下一個堆疊成員，`bubble` 設定不改變此行為；不支援的類型進入既有應急回退。

## 遠端日誌通道 {#remote-log-channels}

`slack` 驅動將 JSON 傳送到設定的 incoming Webhook。設定 `LOG_SLACK_WEBHOOK_URL`，應用設定以 `LOG_LEVEL` 或 `critical` 作為最低等級。透過 `url`、`username`、`channel`、`emoji`、`attachment`（預設 `true`）、`short`（預設 `false`）、`context`（預設 `true`）和 `exclude_fields` 設定訊息。附件保留結構化欄位；`attachment: false` 傳送格式化文字。預留位置替換與處理器先於傳送執行。

`exclude_fields` 沿用外部處理器的點分路徑規則：空值保留，路徑中末段重複出現時停在第一個同名片段。需要不同去識別策略時使用處理器；不要在日誌上下文中放入不應傳送到設定目標的金鑰。

原生同步日誌會等待傳輸結果。通道內部執行緒持有既有 HTTP 用戶端及執行期，在非同步請求中記錄日誌亦不會巢狀建立執行期。`timeout` 為每次嘗試的秒數（預設 `10`）；連線、逾時與本文傳輸失敗最多嘗試五次，不跟隨重新導向。與外部處理器相同，單憑 HTTP 狀態碼不判定傳輸錯誤，收到回應不表示遠端服務已接受訊息。傳輸失敗走既有應急回退，通道恢復後可繼續傳送。

`papertrail` 通道讀取 `PAPERTRAIL_URL` 和 `PAPERTRAIL_PORT`，傳送遠端 Syslog 資料報。保留現有 `driver: "papertrail"` 原生別名，通用處理器形式為：

```rust
let settings = serde_json::json!({
    "driver": "monolog",
    "handler": "syslog_udp",
    "handler_with": {"host": "127.0.0.1", "port": "10000", "ident": "orders"},
    "replace_placeholders": true,
});
```

處理器接受整數或數字字串連接埠，以及 `facility`、`ident`、`rfc`：`0` 為 RFC 3164，`1`（預設）為 RFC 5424，`2` 帶毫秒精度。主機名稱取自系統，程式識別預設使用應用名稱。自訂格式化輸出多行時逐行傳送資料報；含標頭的名義長度上限為 65,023 位元組，同時受平台 Socket 限制。Unix 下連接埠 `0` 選擇本機資料報 Socket，相對路徑從應用根目錄解析。UDP 不提供送達確認；通道繼續沿用一般堆疊篩選、格式化及應急處理。

## 內建格式器與處理器 {#native-log-components}

`openrust::log::formatters::HtmlFormatter` 與 `openrust::log::processors::{MemoryUsageProcessor, PsrLogMessageProcessor}` 是可直接呼叫的公開型別，也可透過管道設定選擇：

```json
{"driver":"single","path":"storage/logs/formatted.html","formatter":"html","formatter_with":{"dateFormat":"Y-m-d H:i:s"},"processors":[{"processor":"memory_usage","with":{"realUsage":true,"useFormatting":false}},{"processor":"psr_log_message","with":{"removeUsedContextFields":true}}]}
```

`html` 跳脫訊息和欄位，`dateFormat` 使用既有的 PHP 風格日期格式。`psr_log_message` 替換 JSON 上下文中的預留位置，可移除已使用欄位；JSON 上下文不包含不透明 PHP 物件及其日期格式鉤子。`memory_usage` 將原生程序駐留記憶體寫入 `extra.memory_usage`，指標是作業系統記憶體，不是 PHP 配置器記憶體。`useFormatting` 預設 `true`，設為 `false` 保留數字位元組數。`realUsage: false` 會被拒絕，不回傳不同意義的替代指標。同名編譯註冊優先於內建工廠。

## 瀏覽器主控台輸出 {#browser-log-output}

開發時可設定管道：
```json
{"driver":"monolog","handler":"browser_console","level":"debug"}
```

`browser_console` 向 HTML 或 JavaScript 回應追加指令碼，保留層級、`[[文字]]{color: blue}` 樣式標記、管道標籤、上下文和附加欄位。JSON、HEAD、無本文狀態、已編碼內容、分段檔案和二進位下載保持原樣。CLI 不輸出瀏覽器指令碼。記錄依請求及應用程式隔離。對應公開型別為 `openrust::log::BrowserConsoleHandler`。

串流本文在 trailers 或 EOF 前追加主控台輸出。原始本文串流在請求作用域關閉後消費，應提前用 `Log::channel(None)?` 取得自有日誌控制代碼並移入串流，不延長請求級 Facade 生命週期。動態瀏覽器管道應在回傳回應前建立，或預先宣告在 `logging.channels`，以便安裝本文包裝。

## 從容器解析 Tap {#container-resolved-taps}

在服務提供者的 `register` 中呼叫以下函式，並將管道的 `tap` 設定為 `["customize:orders,prod"]`。首次建立管道時，註冊閉包才從所屬應用程式容器解析具體自訂類別及其建構依賴，不在啟動時提前建構。編譯註冊替代執行期類別名稱載入，依賴仍由容器提供。`forget_channel` 後重新建立管道及其 tap。

```rust
use openrust::{
    container::{Container, ClassDescriptor, ConstructorParam},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    log::{Formatter, LogError, LogManager, LogResult, Logger, formatters::LineFormatter},
    support::ProviderResult,
};
use std::sync::Arc;

struct FormatterLabel(String);
struct CustomizeFormatter { label: Arc<FormatterLabel> }
impl CustomizeFormatter {
    fn invoke(&self, logger: &mut Logger, arguments: &[&str]) -> LogResult<()> {
        let pattern = format!("{}:{}:%message%\n", self.label.0, arguments.join(","));
        let formatter = LineFormatter::new(Some(&pattern), None);
        let formatter: Arc<Formatter> = Arc::new(move |record| formatter.format(record));
        for mut handler in logger.get_handlers() {
            handler.set_formatter(formatter.clone());
        }
        Ok(())
    }
}
fn register_formatter_tap(app: &Application) -> ProviderResult {
    let container = app.container();
    container.bind::<FormatterLabel, _>(|container| {
        let config = container.get::<dyn openrust::contracts::config::Repository>("config")?;
        let label = config.get("app.name").and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "App".into());
        Ok(Arc::new(FormatterLabel(label)))
    })?;
    container.register_class(ClassDescriptor::concrete::<CustomizeFormatter, _>(
        vec![ConstructorParam::class::<FormatterLabel>("label")],
        |arguments| Ok(Arc::new(CustomizeFormatter { label: arguments.service(0)? })),
    )?)?;
    let owner = Arc::downgrade(&Container::get_instance());
    container.make::<LogManager>()?.register_tap("customize", move |logger, arguments| {
        owner.upgrade().ok_or(LogError::InvalidConfiguration)?
            .make::<CustomizeFormatter>()?.invoke(logger, arguments)
    });
    Ok(())
}
```

`openrust::log::formatters::LineFormatter` 與 `formatter: "line"` 支援 `%datetime%`、`%channel%`、`%level_name%`、`%level%`、`%message%`、`%context%`、`%extra%` 及單個 `%context.key%`／`%extra.key%`。`formatter_with` 接受 `format`、`dateFormat`、`allowInlineLineBreaks` 和 `ignoreEmptyContextAndExtra`，後兩項預設 `false`。公開 setter 與 `format_batch` 使用同一實作，回傳的分隔符原樣寫入。原生錯誤保留既有結構化快照，不虛構 PHP Throwable 堆疊或執行期類別反射。

在 tap 中，`get_handlers()` 透過 `&mut Logger` 借用實際處理鏈，每個控制代碼可呼叫 `set_formatter`。設定採用寫入時複製，其他已共享日誌器保留原有設定，一般寫入路徑不增加格式設定鎖。緊急回退不提供可設定的管道處理鏈。

Log 門面也提供 `driver`、`get_default_driver`、`set_default_driver`、`forget_channel` 與 `extend`。`driver(None)` 選擇目前預設管道，`forget_channel(None)` 清除該管道快取，已回傳的日誌器仍保留原有輸出目標。這些操作需在有效應用程式作用域內執行。

## 宣告處理器自帶格式器 {#formattable-handlers}

自訂文字處理器可實作 `FormattableHandler`，在 Provider 中透過 `LogManager::register_formattable_handler` 註冊，繼續使用既有處理器註冊表及寫入管線。

```rust
use openrust::log::{FormattableHandler, Formatter, LogManager, LogRecord, LogResult};
use std::{io::Write as _, sync::Arc};

struct PrefixHandler { prefix: String }
impl FormattableHandler for PrefixHandler {
    fn get_default_formatter(&self) -> LogResult<Arc<Formatter>> {
        let prefix = self.prefix.clone();
        Ok(Arc::new(move |record| Ok(format!("{prefix}:{}\n", record.message()))))
    }
    fn write(&self, _record: &LogRecord<'_>, formatted: &str) -> LogResult<()> {
        std::io::stderr().lock().write_all(formatted.as_bytes())?;
        Ok(())
    }
}
fn register_own_formatter(manager: &LogManager) {
    manager.register_formattable_handler("own_text", |arguments| {
        let prefix = arguments.get("prefix").and_then(|v| v.as_str())
            .unwrap_or("APP").to_owned();
        Ok(Arc::new(PrefixHandler { prefix }))
    });
}
```

設定 `driver: "monolog"`、`handler: "own_text"`、`handler_with: {"prefix":"APP"}` 與 `formatter: "default"` 使用自帶格式。省略 `formatter` 採用框架格式，指定 `line`、`html` 或註冊名稱則覆蓋自帶格式。一般 `register_handler` 回呼保持可用，選 `default` 時採用標準行格式器。

預設 getter 僅在第一條通過層級篩選的記錄需要時執行。失敗後可在同一管道重試，成功格式器會快取。原生呼叫回傳原錯誤鏈，框架寫入保留緊急回退。執行使用者程式碼時不持註冊鎖。並行首次解析可能多次呼叫 getter，最終保留同一個成功格式器，因此 getter 應保持冪等。此能力輸出文字，不代表任意 PHP 擴充或結構化 APM 格式器型別均已支援。

選用的 [New Relic APM 擴充](../packages/new-relic.md)會匯出應用程式追蹤與指標。核心日誌通道使用各自的設定與處理器。
