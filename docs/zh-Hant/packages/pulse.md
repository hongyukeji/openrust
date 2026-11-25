# Pulse

Pulse 收集應用程式效能指標，提供慢請求、慢 SQL、慢工作、外部 HTTP、例外、佇列、快取和使用者用量面板。

## 安裝 {#installation}

在應用程式 `Cargo.toml` 中啟用 `pulse` feature。透過 `./artisan` 執行時，將 `"pulse"` 加入 `[features].default` 並保留既有功能。套件探索自動載入 `PulseServiceProvider`；預設應用程式不需額外註冊擴充 Provider。

```sh
./artisan migrate
./artisan vendor:publish --tag=pulse-config
```

支援 PostgreSQL、MySQL、MariaDB 與 SQLite。Provider 註冊三張指標表的遷移，啟動時不查詢這些資料表。設定與範本歸屬套件；發佈的 Rust 設定須接入應用程式編譯型載入器並重新建置。既有檔案預設保留；命中設定快取後不重讀套件環境變數。

## 設定 {#configuration}

`PULSE_ENABLED` 控制收集。預設直接寫入資料庫；`PULSE_DB_CONNECTION` 選擇指標連線，`PULSE_STORAGE_KEEP` 預設為 `7 days`。`PULSE_INGEST_BUFFER` 預設 5000，超過時嘗試寫入；一般 HTTP、Artisan 與 Worker 生命週期結束時也會寫入。程序被強制終止或工作取消不保證寫入。

需要分離收集與持久化時設定 `PULSE_INGEST_DRIVER=redis`，透過 `PULSE_REDIS_CONNECTION` 選擇原生 Redis 連線，再執行消費命令。`PULSE_INGEST_KEEP` 接受保留時間，編譯型設定也可傳入筆數。Redis 儲存失敗保留串流項目供重試；多消費者和寫入成功後刪除串流失敗不保證嚴格一次處理。

`PULSE_CACHE_DRIVER` 選擇原生快取 store，用於伺服器節流、檢查鎖及重新啟動通知。多個程序應共用快取連線與前綴；file 快取僅在共用相同檔案系統時跨程序生效。

## 面板與授權 {#dashboard}

預設存取 `/pulse`。`PULSE_PATH` 與 `PULSE_DOMAIN` 自訂入口。本機環境允許存取，其他環境需要原生 `viewPulse` Gate 授權；同時設定應用程式認證與工作階段中介層。JSON 指標入口為面板路徑下的 `/api/metrics`，接受 `period=1h`、`6h`、`24h` 或 `7d`，使用相同授權。

```rust
let gate = app.make::<openrust::auth::access::GateRegistry>().await?;
gate.define_guest("viewPulse", |user, _| Box::pin(async move {
    Ok(if user.is_some() {
        openrust::auth::access::Response::allow()
    } else {
        openrust::auth::access::Response::deny("Forbidden")
    })
}));
```

面板使用原生 HTML、JavaScript 與 JSON 介面。透過 `./artisan vendor:publish --tag=pulse-dashboard` 發佈範本；自訂範本保留 `<!-- PULSE_STYLES -->` 和 `<!-- PULSE_SCRIPTS -->` 可注入套件資源。`Pulse::with_css` 接受可信任的應用程式 CSS 路徑或 HTML，去重並追加；`css` 與 `js` 回傳資源 HTML。Rust 將上游多載的 CSS 設定入口表達為 `with_css`。

## 記錄器 {#recorders}

`pulse.recorders` 按型別名稱設定 `CacheInteractions`、`Exceptions`、`Queues`、`SlowJobs`、`SlowOutgoingRequests`、`SlowQueries`、`SlowRequests`、`UserJobs`、`UserRequests` 和 `Servers`。`false` 或 `enabled=false` 在啟動時關閉該記錄器。`sample_rate` 為 0–1，慢記錄的 `threshold` 單位為毫秒；`ignore` 接受 PHP 分隔符形式的 PCRE 模式。

對於路由名稱以 `livewire.update` 結尾的請求，`SlowRequests` 和 `UserRequests` 使用已解碼的 `components[0].snapshot` JSON 中的 `memo.path`。慢請求記錄的 `via` 欄位保留處理路由。Pulse 使用 HTTP 核心已有的請求快照，保持不透明請求本文的未讀取狀態。

`groups` 與按模式設定的 `threshold` 使用有序的 `[[pattern, value], ...]` 表達首次符合規則。例如 `[["/^orders:\\d+$/", "orders:*"]]`；物件形式按 JSON 物件的鍵順序比對。工作取樣以工作 UUID 保持生命週期一致。請求指標使用符合的路由，避免為每個動態 ID 建立指標鍵；外部 HTTP 移除 URL 使用者名稱與密碼。

慢查詢 `location` 嘗試還原應用程式原始碼位置；不含除錯符號的建置可能回傳空位置。`Exceptions` 接收原生帶型別的例外日誌、工作例外與 `Pulse::report`，一般 JSON 日誌內容不視為例外。Rust 錯誤未必帶有呼叫堆疊，需要時明確傳入位置。

`Servers` 預設每個實例 15 秒取樣一次，儲存 CPU、記憶體與磁碟快照；`PULSE_SERVER_NAME` 與 `PULSE_SERVER_DIRECTORIES` 自訂名稱及冒號分隔的目錄。`Servers::detect_cpu_using`、`detect_memory_using` 可替換偵測器。

## 自訂指標 {#custom-metrics}

在已啟動的請求、命令或工作中使用 Facade。指標在目前生命週期緩衝，設定與篩選器在應用程式內共用。

```rust
use openrust_pulse::facades::Pulse;

Pulse::record("orders", "completed", Some(120), None).await?
    .count().sum().max().avg();
Pulse::set("deployment", "version", "v1", None).await?;
Pulse::ingest().await?;
let totals = Pulse::aggregate_total(&["orders".into()], "sum", 3600).await?;
```

`record` 回傳 Entry，可選擇 `count`、`min`、`max`、`sum`、`avg`；`only_buckets` 僅寫入彙總。回傳 Entry 與緩衝項目共用彙總旗標，其他欄位遵循 Rust 值所有權，修改副本不會改寫緩衝項目。`set` 為相同型別與鍵保留最後傳入的值。四種時間範圍為 1 小時、6 小時、24 小時和 7 天，圖表填入 60 個時間點。

`filter` 篩選寫入項目；`ignore(future)` 暫時忽略收集並在取消時恢復；`lazy` 延遲產生指標；`handle_exceptions_using` 接收寫入與記錄器錯誤。`flush` 捨棄目前緩衝，`ingest` 立即提交。查詢介面包含 `values`、`graph`、`aggregate`、`aggregate_types` 和 `aggregate_total`。後者接收型別切片並回傳各型別結果，空集合結果為 `None`。

實作 `recorders::Recorder` 的 `name`、`listens` 與非同步 `record`，在套件啟動後的應用程式 Provider 呼叫 `pulse.register(vec![recorder])`，即可監聽原生事件。使用 `recorders()` 檢視註冊實例。Rust 透過編譯型型別與明確事件名稱取代 PHP 類別反射；自訂 HTML 卡片透過發佈範本實現。

## 使用者 {#users}

用量記錄讀取原生認證已解析的身分。先執行認證中介層或 `Auth::user(request).await?`；投遞佇列工作時可用 `remember_user` 明確關聯操作者。預設面板透過認證 Provider 讀取姓名、電子郵件和頭像，找不到時顯示 ID。`Pulse::user` 自訂欄位，或在擴充啟動前綁定 `ResolvesUsers` 自訂使用者鍵與載入行為。預設通用認證 Provider 按 ID 載入；需要批次最佳化時實作該契約。

## 命令 {#commands}

```sh
./artisan pulse:check
./artisan pulse:check --once
./artisan pulse:work
./artisan pulse:work --stop-when-empty
./artisan pulse:restart
./artisan pulse:clear --type=slow_request --type=slow_query --force
./artisan pulse:purge --force
```

`pulse:check` 每秒發送檢查事件，伺服器記錄器按取樣週期工作；Vapor 環境或 `--once` 執行一次。`pulse:work` 消費 Redis 串流並定期清理舊指標；直接儲存模式不需要該程序。`pulse:restart` 通知長期執行的兩類命令結束，由程序管理器重新啟動。`pulse:clear` 與 `pulse:purge` 清理指定型別或全部指標，正式環境需要確認或 `--force`。部署後使用 `./main artisan`。
