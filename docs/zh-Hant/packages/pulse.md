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

慢操作卡片預設按最慢耗時排列，可切換為按次數排序。例外預設按次數排列，可切換為按最近發生時間排序。用量顯示請求、慢請求或工作的前十名使用者。面板選擇儲存在頁面 URL。指標入口接受 `slow-requests=count`、`slow-queries=count`、`slow-jobs=count`、`slow-outgoing-requests=count`、`exceptions=latest` 和 `usage=requests|slow_requests|jobs`。

快取卡片顯示所選時段的命中、未命中和命中率，彙總包括 100 列明細以外的鍵。啟用取樣時，面板用 `~` 標記估算次數。JSON 指標的 `cache_totals` 傳回原始總量和取樣率。

伺服器卡片同時顯示最近一次快照和所選時段內的 CPU、記憶體平均歷史。距回報不足 30 秒時標為最近回報。可為 `pulse.dashboard.servers.ignore_after` 設定秒數或 `15 seconds`、`1 minute and 15 seconds` 等相對時長來隱藏過期快照，預設 `null` 會保留。預設按主機名稱升冪。`sort_direction` 可設為 `asc` 或 `desc`。`sort_by` 支援 `name`、`cpu_current`、`memory_current`、`memory_total`、`updated_at`、`recently_reported`、`cpu`、`memory` 和 `storage`。用 `storage.0.used` 等巢狀路徑可選擇特定讀數。數值主機名稱依數值排序。

## 記錄器 {#recorders}

`pulse.recorders` 按型別名稱設定 `CacheInteractions`、`Exceptions`、`Queues`、`SlowJobs`、`SlowOutgoingRequests`、`SlowQueries`、`SlowRequests`、`UserJobs`、`UserRequests` 和 `Servers`。`false` 或 `enabled=false` 在啟動時關閉該記錄器。`sample_rate` 為 0–1，慢記錄的 `threshold` 單位為毫秒；`ignore` 接受 PHP 分隔符形式的 PCRE 模式。

`Queues`、`UserJobs` 和 `SlowJobs` 按工作顯示名稱比對 `ignore`。例如，透過 `Artisan::queue` 派送的工作會以命令名稱比對。`SlowJobs` 也用工作顯示名稱比對閾值規則及彙總慢工作。每次嘗試從 `JobProcessing` 開始計時，到第一個成功、失敗或例外後釋放事件為止。`sync` 連線不計入。

對於路由名稱以 `livewire.update` 結尾的請求，`SlowRequests` 和 `UserRequests` 使用已解碼的 `components[0].snapshot` JSON 中的 `memo.path`。慢請求記錄的 `via` 欄位保留處理路由。Pulse 使用 HTTP 核心已有的請求快照，保持不透明請求本文的未讀取狀態。

`SlowOutgoingRequests` 記錄原生 HTTP 用戶端的每次嘗試，包括失敗及重試。忽略與分組規則在移除 URL 使用者憑證之後執行。自訂傳輸可向 `SlowOutgoingRequests::record_outgoing` 提供絕對 URL 和毫秒耗時，沿用相同規則。

慢請求耗時包含回應終止中介層及延後執行的工作，錯誤回應也會計入。使用者歸因在終止前的請求作用域中取得。

`groups` 與按模式設定的 `threshold` 使用有序的 `[[pattern, value], ...]` 表達首次符合規則。例如 `[["/^orders:\\d+$/", "orders:*"]]`；物件形式按 JSON 物件的鍵順序比對。工作取樣以工作 UUID 保持生命週期一致。請求指標使用符合的路由，避免為每個動態 ID 建立指標鍵；外部 HTTP 移除 URL 使用者名稱與密碼。`u` 啟用 Unicode 字元類別。`r` 修飾符需要 PCRE2 10.43 或更新版本。

快取分組的位元組模式可能產生非法 UTF-8。Pulse 在 Redis 和 SQLite 中保留原始位元組，Cache 卡片僅在顯示時使用替代字元。PostgreSQL 和嚴格模式 MySQL 在儲存時拒絕非法鍵，非嚴格模式 MySQL 按資料庫的文字規則轉換。舊版 Redis 消費程序會將位元組鍵負載保留在佇列中，啟用可能產生此類鍵的規則前請先完成消費程序升級。

用 `Pulse::graph_bytes` 按原始位元組讀取圖表序列，即使顯示標籤相同也能分辨不同序列。文字版 `Pulse::graph` 要求鍵為有效 UTF-8。

用 `Pulse::set_bytes` 寫入帶原始位元組鍵的儀表值，用 `Pulse::values_bytes` 按原始位元組篩選。SQLite 保留無效 UTF-8 鍵並以資料庫 `Value::Bytes` 傳回。PostgreSQL 和 MySQL 依照欄位編碼與 SQL 模式處理。透過 Redis 傳送帶位元組鍵的儀表值前，請先更新所有 `pulse:work` 消費者。窮舉比對 `Item` 的自訂篩選器與儲存需要處理 `Item::ByteValue`。

慢查詢 `location` 使用資料庫事件擷取的呼叫位置，Release 建置也可記錄。沒有可用位置時寫入空字串，關閉 `location` 時寫入空值。`Exceptions` 採集框架例外處理器回報的例外（含回報的工作失敗），也採集直接向 Pulse 回報的例外。呼叫 `Pulse::report_exception(error).await?` 可向 Pulse 提交 Rust 錯誤。在錯誤發生處使用 `SharedException::located(error)` 可擷取原始碼檔案與行號。監聽器可訂閱 `NativeExceptionReported::NAME` 讀取原始錯誤。已有型別名稱與位置時，可呼叫 `Pulse::report(class, location)`。僅將錯誤附加到應用程式日誌不會向 Pulse 回報。

`Servers` 預設每個實例 15 秒取樣一次，儲存 CPU、記憶體與磁碟快照；載入設定時預設讀取作業系統主機名稱，建立設定快取時會儲存該值。`PULSE_SERVER_NAME` 可覆寫主機名稱，`PULSE_SERVER_DIRECTORIES` 設定以冒號分隔的目錄。`Servers::detect_cpu_using`、`detect_memory_using` 可替換偵測器。

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

`pulse:check` 將下一次檢查安排在本次檢查開始一秒後，伺服器記錄器按取樣週期工作。`VAPOR_SSM_PATH` 啟用 Vapor 時只檢查一次，`SharedBeat.instance` 為 `vapor`。其他環境下，每次執行命令使用一個 16 位英數字元執行個體識別碼，可加 `--once` 只檢查一次。`pulse:work` 消費 Redis 串流並定期清理舊指標，每輪處理完成後等待一秒。直接儲存模式無須執行該程序。`pulse:restart` 通知長期執行的兩類命令結束，由程序管理器重新啟動。`pulse:clear` 與 `pulse:purge` 清理指定型別或全部指標，正式環境需要確認或 `--force`。部署後使用 `./main artisan`。
