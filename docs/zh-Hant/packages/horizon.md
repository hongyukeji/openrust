# Horizon

## 安裝 {#installation}

啟用根應用的 `horizon` feature，即可使用可選 Redis 佇列監督擴充 `openrust-horizon` 與授權維運面板。`HorizonServiceProvider` 透過包發現自動註冊；獨立使用包的應用明確註冊此 Provider。載入 Provider 不啟動 Worker，也不連線 Redis。

在應用程式 `Cargo.toml` 的 `[features].default` 清單中加入 `horizon`，保留已有功能，然後使用原始碼入口：

```sh
./artisan vendor:publish --tag=horizon-config
./artisan horizon
```

發佈設定後重新編譯應用使其生效。重複發佈保留已有檔案，`--force` 才覆寫。設定快取見[設定](../getting-started/configuration.md)。

## 設定 {#configuration}

重用 `database.redis` 中的單機 Redis 連線和 `queue.connections` 中的 Redis 佇列連線。`horizon.use` 選擇元資料連線，預設 `default`；`horizon.prefix` 讀取 `HORIZON_PREFIX`，預設將 `APP_NAME` 轉為底線 slug 後附加 `_horizon:`；共用 Redis 的不同環境應使用獨立前綴。不支援 Redis Cluster。原生[佇列](../digging-deeper/queues.md)仍按自身契約支援其他傳輸。

既有部署若使用先前的 `horizon:` 中繼資料前綴，應明確設定 `HORIZON_PREFIX=horizon:`，讓相關 Worker 繼續使用相同前綴。排空舊 Master 後再切換前綴；新前綴具有獨立的控制狀態與歷史紀錄。

`horizon.defaults` 按監督器名稱設定；`horizon.environments` 選擇第一條符合 `app.env` 的模式，支援 `*`。未符合時不啟動監督器，`max_processes: 0` 停用對應監督器。環境選項與預設值遞迴合併，陣列按索引覆寫。預設範本在 `production` 使用 10 個程序預算，在 `local` 使用 3 個。

監督器支援 `connection`、`queue`（字串或字串陣列）、`balance`、`auto_scaling_strategy`、`min_processes`、`max_processes`、`balance_max_shift`、`balance_cooldown`、`tries`、`timeout`、`sleep`、`memory`、`max_jobs`、`max_time`、`workers_name`、`backoff`、`force`、`rest`、`nice` 和 `directory`。`backoff` 接受整數、逗號分隔字串或整數陣列。`tries: 0` 表示不限嘗試次數，`sleep: 0` 不等待，`timeout: 0` 關閉工作超時。`nice` 非零時使用 Unix 行程優先級介面，工作行程繼承監督器優先級；其他平台明確拒絕此選項。設定支援 snake_case 與 `processes` 別名；Rust 的選項結構使用 `max_tries`，設定仍可使用 `tries`。

`balance: "auto"` 按預計清空耗時（`time`）、待處理數量（`size`）或 `log1p` 數量比例（`log`）計算整個 `max_processes` 預算中的浮點目標。按目標從小到大調整程序池，目標向上取整，並套用 `min_processes`、剩餘預算與 `balance_max_shift`。未知耗時按零處理，非空但總耗時為零的佇列以最大預算為目標。`simple` 均分預算；`false` 或 `"off"` 使用一個優先佇列池，按待處理數量在最小與最大值間縮放。`balance_cooldown` 控制間隔；計算時包含結束中的程序，各 Master 獨立管理預算。

Worker 的 `timeout` 必須短於佇列的 `retry_after`，預設分別為 60 和 90 秒。Worker 使用應用自身可執行檔案、基礎目錄、環境、處理器和設定。`max_jobs` 與 `max_time` 觸發程序輪替，異常結束後自動補位。使用作業系統程序管理器託管 Master。SIGTERM／SIGINT 及終止請求等待目前工作完成，寬限期為設定的超時加五秒，之後強制結束。

程序監督器面向 Unix 部署。基於訊號的監督器控制和孤兒程序檢查需要 Unix 程序工具。在其他平台上使用受支援的中繼資料命令，並在執行 Worker 前驗證部署行為。

## 面板授權 {#authorization}

套件 Provider 自動註冊面板路由。在應用 Provider 或路由組裝中註冊授權回調。每次請求（包括指標介面）都須透過回調，否則拒絕存取。回調使用應用已有的登入工作階段與授權策略。

```rust
use openrust::http::{HttpError, HttpFuture, Request};
use openrust_horizon::Horizon;

fn view_horizon(request: &mut Request) -> HttpFuture<'_, Result<bool, HttpError>> {
    // 替換為應用已認證維運人員的授權策略。
    let _ = request;
    Box::pin(async { Ok(false) })
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Horizon::auth(view_horizon).expect("Horizon provider registered");
    Horizon::routes()
}
```

`HORIZON_PATH` 控制 `horizon.path`（預設 `horizon`），`HORIZON_DOMAIN` 控制可選的 `horizon.domain`；`horizon.middleware` 預設 `["web"]`。設定 `horizon.routes=false` 停用自動註冊。`Horizon::routes()` 使用這些設定；明確呼叫 `routes_at("/operations/horizon")` 優先於自動註冊，每個應用只註冊一次。現有路由群組繼續應用自身前綴與中間件。面板每五秒刷新，設定嚴格的內容安全策略並停用回應快取，支援佇列負載、Master 與監督器、待處理／完成／靜默／失敗工作、工作承載資料與異常、監控標籤、指標快照和批次。失敗工作及批次失敗工作透過已設定的原生失敗工作提供器重試。

暫停、繼續、終止、監控變更和重試均須逐次通過授權回調。瀏覽器對寫入操作傳送 `X-Horizon-Request: 1`，直接呼叫介面也須攜帶該標頭；拒絕跨站瀏覽器寫入操作。為整個路由群組套用既有的認證與中間件。面板授權包含檢視工作資料和執行維運控制的權限。

## 命令 {#commands}

```sh
./artisan horizon:status
./artisan horizon:status --json
./artisan horizon:pause
./artisan horizon:continue
./artisan horizon:terminate
./artisan horizon:failed
./artisan horizon:retry JOB_ID
./artisan horizon:forget JOB_ID
./artisan horizon:forget --all
./artisan horizon:list
./artisan horizon:supervisors
./artisan horizon:supervisor-status supervisor-1
./artisan horizon:pause-supervisor supervisor-1
./artisan horizon:continue-supervisor supervisor-1
./artisan horizon:snapshot
./artisan horizon:clear-metrics
./artisan horizon:clear redis --queue=default
./artisan horizon:purge
```

`horizon:status` 在執行時傳回 0、暫停時傳回 1、無存活 Master 時傳回 2。暫停阻止領取新工作，已執行工作可完成；繼續恢復領取；終止使目前 Master 優雅結束，新啟動的 Master 使用新代次。監督器命令操作具名監督器，控制命令作用於共用元資料前綴。

Horizon 工作 ID 識別獨立的 Redis 投遞，由佇列推送和面板回傳。`horizon:retry` 與 `horizon:forget` 使用此 ID。重試取得新 ID，原失敗監控記錄保留並記錄重試結果。原生 `queue:retry` 和失敗工作儲存使用承載資料 UUID，由 Horizon 內部轉換；批次重試也會將這些 UUID 解析為目前失敗投遞。

按[佇列](../digging-deeper/queues.md)設定持久失敗儲存，重試重用原生領取與重新派發機制。`horizon:forget` 刪除監控失敗記錄與原生失敗記錄，`--all` 分頁處理全部 Horizon 失敗記錄。`horizon:clear` 清空指定佇列和待處理監控記錄，`horizon:purge` 處理孤立程序。透過[工作排程](../digging-deeper/scheduling.md)定期執行 `horizon:snapshot` 保存指標歷史；`horizon:clear-metrics` 清理指標並保留工作記錄。

`horizon:listen` 是開發命令，每 500 毫秒輪詢一次 `horizon.watch` 檔案，即使省略 `--poll` 也是如此。檔案變化後重新啟動 Horizon；原始碼應用包含 `Cargo.toml` 時透過 Cargo 啟動，以重新編譯 Rust 原始碼和設定變更。監控時略過符號連結。

## 工作標籤 {#tags}

在應用 Provider 中、Horizon 註冊後呼叫 `Horizon::tag(fn(&Payload) -> Vec<String>)`。回呼對每個被觀察的承載資料產生標籤，忽略空字串，重複標籤保留首次出現順序。Rust 工作透過這些回呼明確提供標籤，不透過反射自動探索承載資料中的模型欄位。

`Tags::for_model(&model)` 將 Rust 模型型別與整數、無符號整數或字串主鍵組合為標籤；缺少主鍵或主鍵型別不支援時回傳錯誤。工作攜帶模型身分時，可將回傳的字串加入回呼產生的標籤。各應用註冊的標籤回呼彼此隔離。面板的監控操作決定哪些標籤保留監控工作。

## 指標與限制 {#metrics}

`Repository::recent(1..=100)`、`metrics(connection, queue)`、`masters()`、`pause()`、`r#continue()` 和 `terminate()` 提供相同的儲存操作。`SupervisorOptions::from_config` 驗證設定，`AutoScaler::number_of_workers_per_queue` 計算浮點目標，`scale_pool` 計算逐池調整，`MasterSupervisor` 管理子程序生命週期。

Redis 佇列事件記錄 ID、工作名、連線、佇列、嘗試次數、狀態、耗時與 Redis 伺服器端時間。授權後的工作詳情還提供序列化承載資料、標籤和異常文字。`Repository::recent(1..=100)` 保持純元資料投影，近期索引最多 1000 個工作身份；`retention`（60–86400 秒，預設 3600）控制近期元資料和滾動 60 分鐘嘗試指標。

`horizon.trim` 按分鐘控制工作清單保留時間：`recent`、`pending`、`completed` 預設為 60，`recent_failed`、`failed`、`monitored` 預設為 10080。工作清單每頁傳回 50 筆。`horizon.metrics.trim_snapshots.job` 與 `.queue` 預設各保留 24 個快照。平均耗時和吞吐量只計算成功完成的工作，失敗、釋放和等待重試不參與計算。重複結束事件在下一次處理事件前忽略；手動重試重設嘗試次數時開始新的觀測。

`Repository::jobs()`、`tags()`、`metrics_repository()`、`master_supervisors()`、`supervisor_repository()`、`processes()` 與 `workload()` 提供獨立儲存庫操作。監控標籤按元資料前綴保存。每個指標的快照寫入與目前計數重設原子執行。Master 心跳 15 秒後到期，監督器心跳 30 秒後到期。元資料服務錯誤會暫停領取新工作並結束 Master，由作業系統程序管理器恢復。監控記錄會到期，不作為持久執行帳本。
