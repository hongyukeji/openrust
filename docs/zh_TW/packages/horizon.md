# Horizon

## 安裝 {#installation}

啟用根應用的 `horizon` feature，即可使用可選 Redis 佇列監督擴充 `openrust-horizon` 與唯讀面板。`HorizonServiceProvider` 透過包發現自動註冊；獨立使用包的應用明確註冊此 Provider。載入 Provider 不啟動 Worker，也不連線 Redis。

在應用程式 `Cargo.toml` 的 `[features].default` 清單中加入 `horizon`，保留已有功能，然後使用原始碼入口：

```sh
./artisan vendor:publish --tag=horizon-config
./artisan horizon
```

發佈設定後重新編譯應用使其生效。重複發佈保留已有檔案，`--force` 才覆寫。設定快取見[設定](../getting-started/configuration.md)。

## 設定 {#configuration}

重用 `database.redis` 中的單機 Redis 連線和 `queue.connections` 中的 Redis 佇列連線。`horizon.use` 選擇元資料連線，預設 `default`；`horizon.prefix` 預設 `horizon:`，須按應用和環境隔離。不支援 Redis Cluster。原生[佇列](../digging-deeper/queues.md)仍按自身契約支援其他傳輸。

`horizon.defaults` 按監督器名稱設定；`horizon.environments` 選擇第一條符合 `app.env` 的模式，支援 `*`。未符合時不啟動監督器，`maxProcesses: 0` 停用對應監督器。環境選項與預設值遞迴合併，陣列按索引覆寫。預設範本在 `production` 使用 10 個程序預算，在 `local` 使用 3 個。

監督器支援 `connection`、`queue`（字串或字串陣列）、`balance`、`autoScalingStrategy`、`minProcesses`、`maxProcesses`、`balanceMaxShift`、`balanceCooldown`、`tries`、`timeout`、`sleep`、`memory`、`maxJobs`、`maxTime`、`workersName`、`backoff`、`force`、`rest`、`nice` 和 `directory`。`backoff` 接受整數、逗號分隔字串或整數陣列。`tries: 0` 表示不限嘗試次數，`sleep: 0` 不等待，`timeout: 0` 關閉工作超時。`nice` 非零時需要 Unix 的 `nice` 命令；其他平台明確失敗。設定支援 snake_case 與 `processes` 別名；Rust 的選項結構使用 `maxTries`，設定仍可使用 `tries`。

`balance: "auto"` 按預計清空耗時（`time`）、待處理數量（`size`）或 `log1p` 數量比例（`log`）計算整個 `maxProcesses` 預算中的浮點目標。按目標從小到大調整程序池，目標向上取整，並套用 `minProcesses`、剩餘預算與 `balanceMaxShift`。未知耗時按零處理，非空但總耗時為零的佇列以最大預算為目標。`simple` 均分預算；`false` 或 `"off"` 使用一個優先佇列池，按待處理數量在最小與最大值間縮放。`balanceCooldown` 控制間隔；計算時包含結束中的程序，各 Master 獨立管理預算。

Worker 的 `timeout` 必須短於佇列的 `retry_after`，預設分別為 60 和 90 秒。Worker 使用應用自身可執行檔案、基礎目錄、環境、處理器和設定。`maxJobs` 與 `maxTime` 觸發程序輪替，異常結束後自動補位。使用作業系統程序管理器託管 Master。SIGTERM／SIGINT 及終止請求等待目前工作完成，寬限期為設定的超時加五秒，之後強制結束。

## 面板授權 {#authorization}

在應用路由組裝中註冊授權回調及路由。每次請求（包括指標介面）都須透過回調，否則拒絕存取。回調使用應用已有的登入工作階段與授權策略。

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

`Horizon::routes()` 註冊 `GET /horizon` 和 `GET /horizon/api/stats`；使用 `routesAt("/operations/horizon")` 指定其他靜態路徑。現有路由群組繼續應用自身前綴與中間件。面板每五秒刷新，設定嚴格的內容安全策略並禁用回應快取，顯示佇列積壓、監督器、近期工作和彙總指標。控制操作使用 Artisan。

## 命令 {#commands}

```sh
./artisan horizon:status
./artisan horizon:status --json
./artisan horizon:pause
./artisan horizon:continue
./artisan horizon:terminate
./artisan horizon:failed
./artisan horizon:retry JOB_UUID
./artisan horizon:forget JOB_UUID
```

沒有存活 Master 時，`horizon:status` 結束碼為 1。暫停阻止 Horizon Worker 領取新工作，已執行工作可完成；繼續恢復領取。終止請求使該請求前啟動的 Master 優雅結束，新啟動的 Master 使用新代次。控制命令作用於共用元資料前綴。

重試和刪除先檢查失敗工作屬於已設定的 Horizon 連線與佇列，再呼叫原生失敗工作提供器。按[佇列](../digging-deeper/queues.md)設定持久失敗儲存。近期失敗清單僅保留去識別化元資料，原生失敗記錄刪除後，該清單仍可能保留到期；不顯示承載資料或異常文字。

## 指標與限制 {#metrics}

`Repository::recent(1..=100)`、`metrics(connection, queue)`、`masters()`、`pause()`、`r#continue()` 和 `terminate()` 提供相同的儲存操作。`SupervisorOptions::fromConfig` 驗證設定，`AutoScaler::numberOfWorkersPerQueue` 計算浮點目標，`scalePool` 計算逐池調整，`MasterSupervisor` 管理子程序生命週期。

已設定 Redis 佇列的工作事件記錄 ID、正規化工作名、連線、佇列、嘗試次數、狀態、毫秒耗時與 Redis 伺服器端時間，不記錄承載資料、異常訊息及憑證。指標彙總最近 60 個分鐘桶，同時受 `retention` 限制（60–86400 秒，預設 3600）。近期索引最多保存 1000 個工作身份，每次最多讀取 100 個；同一工作顯示最近一次嘗試。重複結束事件在下一次處理事件前忽略；手動重試即使重置嘗試次數，也開始新的觀測。這些資料用於監控，不是持久執行帳本。

心跳在 15 秒後到期。元資料服務錯誤會暫停 Horizon 領取新工作並結束 Master，由程序管理器恢復。HTTP 授權由應用負責。本包不提供 Laravel Horizon 的標籤、通知傳送、長等待告警、快照命令或可寫 Web 控制。

平均耗時只計算成功完成的工作，失敗、釋放與等待重試的嘗試不參與計算。
