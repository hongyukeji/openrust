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

執行中的監控程序會重試短時 Redis 中繼資料中斷。無法核對所有權時，受管 Worker 等候領取。重新連線後 Horizon 驗證既有所有權權杖，驗證通過才恢復。權杖缺失或變更時，舊程序樹會關閉。受管 Worker 在 Redis 保留工作後再次核對所有權。未能通過核對時，只要保留仍有效，就把工作放回佇列首位並恢復先前的嘗試次數。主從切換期間由設定的 Redis 端點負責選定主節點。

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

設定 `HORIZON_NAME` 可在面板的頁面標題和主標題顯示部署名稱。

從 `openrust_horizon` 匯入事件類型，為 `JobPending::NAME`、`JobPushed::NAME` 和 `JobReserved::NAME` 註冊同步監聽器。三個事件分別在 Redis 寫入前、成功寫入後和保留工作後派發，提供連線、佇列、投遞 ID 和原生承載資料。`JobReserved` 的 `job` 快照也提供嘗試次數。使用承載資料的 UUID 關聯同一工作的不同投遞。

同一 Redis 連線上的工作鏈推進時，先為後續工作派發 `JobPending`。Redis 在一次原子操作中將後續工作加入佇列並刪除前一個保留，確認後依序派發後續工作的 `JobPushed` 和前一工作的 `JobDeleted`。保留失效可能在 `JobPending` 後阻止推進，使用 `JobPushed` 確認新的投遞。

Redis 將保留工作放回延遲佇列後派發 `JobReleased::NAME`。`delay` 是 Rust 的 `Duration`，工作詳情以 `release_delay_ms` 提供該值。Redis 確認刪除保留工作後派發 `JobDeleted::NAME`，工作鏈推進時也會派發。它提供工作快照、失敗標記和保留後的實際時間。

每批 Redis 遷移完成後、Worker 保留工作前，Horizon 派發 `JobsMigrated::NAME`。每次輪詢先處理到期的延遲工作，再處理過期的保留工作。監聽器收到連線、佇列和工作快照，空批次的工作清單為空。

`JobFailed::NAME` 提供失敗 Redis 投遞的連線、保留快照和例外文字。Worker 失敗時，事件在 Redis 確認和失敗回呼執行後派發，監聽器可讀取失敗工作詳情和標籤。直接呼叫 `Job::fail` 時，也會在 Redis 確認後派發。

原生 Redis 投遞延後至資料庫交易提交時，具有延遲的工作會在發起投遞時產生 Horizon 中繼資料，即時工作則在提交回呼中產生。兩者的 `JobPending` 和 `JobPushed` 均在提交回呼中於寫入 Redis 的前後派發。交易復原時，佇列和面板都不會出現新投遞。

在交易內透過 failover 投遞時，由首個接受工作的連線處理其交易設定。接受前發生錯誤時可以嘗試下一個連線。一旦所選連線登記了提交回呼，該回呼的錯誤就交給交易呼叫端處理。

透過 `JobPayload::from_payload` 讀取 Redis 工作上的 Horizon 中繼資料。Horizon 在寫入前計算標籤和靜默狀態，將結果、以十進位秒表示的投遞時間，以及加入佇列時確定的 `job_type()` 一併儲存。類型序列化為 `job`、`broadcast`、`event`、`mail` 或 `notification`。Horizon 重試沿用已儲存的標籤，清除靜默標誌，透過 `retry_of()` 提供原投遞 ID，新投遞的觀測類型為 `job`。原投遞包含重試截止時間和投遞時間時，新投遞沿用相同的重試時長。舊承載資料可能缺少 Horizon 中繼資料，也可能傳回預設類型 `job`。

加入佇列前可用 `JobPayload::with_tags(payload, tags)` 設定預設標籤。Horizon 將應用程式回呼產生的標籤附加到預設標籤之後，保留預設標籤內部和兩組標籤之間的重複項目。讀取或編輯原始工作 JSON 時使用 `JobPayload::new`。`offset_set` 和 `offset_unset` 只修改 `decoded()`，`set` 合併欄位並更新 `value()` 中的完整 JSON。`JobPayload::from_payload` 傳回原生 Redis 的型別化快照 `JobPayloadMetadata`。

`value_mut()` 和 `decoded_mut()` 可分別修改原始文字和解碼資料。`JobPayload::new` 接受 JSON 物件或陣列。`set` 依合併順序重新編號整數鍵，將連續且依序排列的整數鍵編碼為 JSON 陣列。

在 `horizon.silenced` 中設定廣播事件、監聽器、郵件或通知的底層 Rust 完整類型名稱。一般工作也可使用已註冊的佇列工作名稱。透過程式碼設定靜默時，為類型實作 `openrust_horizon::Silenced`，並在應用程式啟動時呼叫 `Horizon::silence::<YourType>()`。註冊只在目前的應用程式生效。

監督器佇列名稱遵循原生 Redis 傳輸的 255 位元組上限，保留空格、Unicode 字元和冒號。監督器佇列清單以逗號分隔。以 `:delayed`、`:reserved` 或 `:notify` 結尾的名稱由傳輸保留。

長等待警示在郵件主旨、簡訊內文和 Slack 詳情中顯示部署名稱。`Horizon::route_slack_notifications_to` 接受傳入 Webhook 網址或頻道 ID。頻道 ID 使用 Slack Bot API，需設定 `services.slack.notifications.bot_user_oauth_token`。

使用 `Horizon::route_slack_webhook_notifications_to(url, channel)` 可在傳入 Webhook 的承載資料中指定頻道。Slack 應用程式的傳入 Webhook 固定投遞至安裝時選定的頻道。需要依請求指定目標頻道時，請使用 Bot API 頻道 ID。

如需自訂長等待警示，在應用程式服務提供者中繫結 `dyn LongWaitNotificationFactory`。工廠接收 `LongWaitDetected` 事件並回傳 `LongWaitDetectedNotification`。回傳值的 `signature()` 決定五分鐘去重視窗的歸屬。

為 `SupervisorLooped::NAME` 註冊同步監聽器，可在 Supervisor 持久化心跳後觀察其狀態。`SupervisorLooped` 提供 Supervisor 名稱和暫停狀態，Horizon 為該事件註冊 `MonitorWaitTimes`。

Horizon 在替補 Worker 啟動前派發 `WorkerProcessRestarting`，提供監督器、佇列、可用的原程序 PID 與結束碼。

Master 與各監督器發布初始狀態後等待一秒，再開始首次輪詢。後續每輪在上一輪結束一秒後開始。

每個 Worker 席位首次嘗試啟動後，非零結束和程序建立失敗受一秒冷卻限制。已結束的 Worker 在冷卻到期後替補時，Horizon 在 `WorkerProcessRestarting` 之後派發 `UnableToLaunchProcess`，並開始一分鐘冷卻。正常結束可在冷卻期間替補。作業系統無法建立程序時，原監督器保留席位，後續 `UnableToLaunchProcess` 的 PID 和結束碼為空。 監督器透過應用程式的例外處理器報告建立失敗，並在下一輪輪詢繼續檢查其餘席位。

符合的環境部署計畫準備完成後，Horizon 派發 `MasterSupervisorDeployed`，提供 Master 名稱、環境與待啟動的監督器。

應用程式可明確派發攜帶 Master 名稱的 `MasterSupervisorReviving`，或攜帶共享 `SupervisorProcess` 的 `SupervisorProcessRestarting`。

Master 持久化心跳後也會派發 `MasterSupervisorLooped::NAME`，事件提供 Master 名稱和暫停狀態。Horizon 註冊 `ExpireSupervisors`，清理兩個程序索引中的過期項目。

程序超過記憶體上限時，會在以結束碼 `12` 關閉前派發 `MasterSupervisorOutOfMemory` 或 `SupervisorOutOfMemory`。事件提供程序名稱、實測常駐記憶體及上限，記憶體單位為 MiB。

Horizon 在對應循環事件上註冊 `MonitorMasterSupervisorMemory` 和 `MonitorSupervisorMemory`。每輪共用一次檢查，即使其他同步監聽器提前終止派發，記憶體保護仍會執行。

Master 也會清理過期的工作索引。最近、待處理、已完成、靜默和最近失敗索引每分鐘檢查一次。失敗和監控索引各自按設定的保留分鐘數除以 12 後向下取整，最短間隔為一分鐘。

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

`horizon:status` 根據所有活躍 Master 的心跳傳回狀態。執行時傳回 0，任一 Master 暫停時傳回 1，無存活 Master 時傳回 2。請在每台需要控制的主機上執行 `horizon:pause`、`horizon:continue` 和 `horizon:terminate`。暫停阻止領取新工作，已執行工作可以完成。繼續恢復領取。終止關閉本機 Master，並透過應用程式快取發佈原生佇列重新啟動通知。啟用 `fast_termination` 時，可傳入 `--wait` 等待 Worker 排空。具名監督器命令也只選目前主機。升級後重新啟動舊 Master 和監督器，以便在 Redis 發佈主機資訊。

如果舊版本在 Redis 留下共用暫停狀態，可在已授權的面板選擇「繼續」，解除該元資料前綴上的暫停。

在 Windows MSVC 上，`horizon:terminate` 會要求本機程序樹正常關閉。應用程式的可執行檔在建置時預留 16 MiB 堆疊空間。使用自訂 Windows MSVC 應用程式入口執行 Horizon 時，請在連結時使用 `/STACK:16777216`。

Master 名稱由主機名稱轉換出的短名和程序內固定的四字元權杖組成。若要自訂短名，可在應用程式 Provider 的 `boot` 方法呼叫 `openrust_horizon::MasterSupervisor::determine_name_using(|| "orders.primary".into())?`。啟動 Horizon 和執行控制命令時使用同一短名。自訂短名可以為空，也可以包含 Unicode 文字、空格和標點符號。短名不能包含控制字元。在 shell 中將含空格的監督器全名傳給 Artisan 時，請使用引號。

若要向執行中的 Master 新增監督器，請從應用程式容器解析 `RedisHorizonCommandQueue`。透過 `MasterSupervisor::command_queue_for(Some(master_name))` 取得目標佇列，將 `AddSupervisor::NAME` 和 `SupervisorOptions::to_array()` 產生的選項推入佇列。監督器名稱由 Master 全名、`:` 和監督器名稱組成。暫停中的 Master 會在繼續執行後啟動新監督器。

`ProvisioningPlan::get(master_name)` 讀取應用程式的部署計畫。部署按設定順序選擇第一個符合的環境，將監督器選項與預設值合併，並略過 `max_processes` 為零的監督器。Artisan 啟動時會套用該計畫。若要將計畫套用到執行中的 Master，可呼叫 `plan.deploy(environment, &app).await`。

若要從應用程式程式碼控制執行中的監督器，透過 `RedisHorizonCommandQueue` 將 `openrust_horizon::supervisor_commands` 中的命令送往監督器全名。`SCALE` 的 `scale` 參數指定 Worker 總數，`BALANCE` 接收各佇列的 Worker 數量，`TERMINATE` 可透過 `status` 指定結束碼。`RESTART` 會更換 Worker 並恢復暫停的監督器。

在應用程式 Provider 中，可用 `instance_named` 依佇列中的命令名稱繫結 `dyn MasterSupervisorCommandHandler` 或 `dyn SupervisorCommandHandler`。繫結可處理自訂命令，也可覆寫同名的內建命令。處理器接收目前程序的監控執行個體和佇列傳入的 JSON 選項。Supervisor 的處理器回傳 `Some(status)` 時，目前的監督器會排空並以該狀態碼結束。

若要替換結構化命令佇列，請在應用程式容器繫結 `dyn HorizonCommandQueue`，並讓所有 Horizon 程序載入相同設定。發送命令時，透過 Horizon `Repository::command_target(name).await` 解析目標，再呼叫佇列的 `push`。後端須同時使用 `target.name()` 和 `target.scope()` 區分佇列，並按入列順序一次領取一批。命令目標對應目前程序的所有權，每次發送前應重新解析。獨立執行的 Supervisor 不帶所有權代次。

若要更換執行中 Master 的所有監督器及其 Worker，將空選項的 `RESTART` 送往 `MasterSupervisor::command_queue_for(Some(master_name))`。`MasterSupervisor::restart().await` 依執行個體的名稱提交同一請求。Master 從暫停恢復。依預設，各監督器會等候執行中的工作完成後再更換。

`horizon` 會即時將受管程序的輸出寫到 Artisan。從應用程式程式碼執行 `MasterSupervisor` 或 `Supervisor` 時，可用 `handle_output_using` 接收直接子程序的輸出。回呼接收 `openrust::process::OutputStream` 與位元組片段，可區分該子程序的 stdout 與 stderr。`horizon:supervisor` 會將 Worker 的兩個資料流都寫入自己的 stdout，因此 Master 收到合併後的輸出。

直接使用 `WorkerProcess` 或 `SupervisorProcess` 時，透過 `handle_output_using` 註冊回呼，將已設定的 `PendingProcess` 傳給 `start`。透過 `take_process` 取得目前的控制代碼，再等候或停止程序。後續啟動會繼續使用已註冊的回呼，直到呼叫端替換它。

`WorkerProcess::monitor` 檢查目前程序，並在冷卻期允許時啟動傳入的命令。兩個包裝器都提供 `terminate` 請求關停，以及 `stop` 在寬限期內等候結束。獨立包裝器的 `pause` 與 `continue_working` 使用 Unix 訊號。Windows 受管 Horizon 命令透過共用程序狀態暫停和繼續。自訂 Windows 子程序需要監聽關停管道，才能正常排空。使用 `SupervisorProcess::terminate_with_status` 可向設定的命令佇列發送帶結束碼的終止請求。

獨立管理包裝器時，呼叫 `SupervisorProcess::monitor` 並傳入目標 Master 的完整名稱。透過 `dont_restart_on` 可指定冷卻期結束後終止監控的結束碼。預設清單為 `[0, 2, 13]`，結束碼 13 一律立即結束監控。其他結束碼在冷卻期結束後檢查 Supervisor 心跳，心跳缺失或仍屬於已結束程序時，按原選項發送 `AddSupervisor` 命令。

Horizon 工作 ID 識別獨立的 Redis 投遞，由佇列推送和面板回傳。`horizon:retry` 與 `horizon:forget` 使用此 ID。重試取得新 ID，原失敗監控記錄保留並記錄重試結果。原生 `queue:retry` 和失敗工作儲存使用承載資料 UUID，由 Horizon 內部轉換；批次重試也會將這些 UUID 解析為目前失敗投遞。

按[佇列](../digging-deeper/queues.md)設定持久失敗儲存，重試重用原生領取與重新派發機制。`horizon:forget` 刪除監控失敗記錄與原生失敗記錄，`--all` 分頁處理全部 Horizon 失敗記錄。`horizon:clear` 清空指定佇列和待處理監控記錄，`horizon:purge` 處理孤立程序。透過[工作排程](../digging-deeper/scheduling.md)定期執行 `horizon:snapshot` 保存指標歷史；`horizon:clear-metrics` 清理指標並保留工作記錄。

`horizon:listen` 是開發命令，每 500 毫秒輪詢一次 `horizon.watch` 檔案，即使省略 `--poll` 也是如此。檔案變化後重新啟動 Horizon；原始碼應用包含 `Cargo.toml` 時透過 Cargo 啟動，以重新編譯 Rust 原始碼和設定變更。監控時略過符號連結。

預設監控應用程式、啟動、設定、資料庫、路由和檢視目錄，以及 `public/` 和 `resources/` 下的 Rust 檔案和根目錄的 `main.rs`、`build.rs`、`Cargo.toml`、`Cargo.lock`、`.env`。

## 工作標籤 {#tags}

在應用 Provider 中，Horizon 註冊後可用 `Horizon::tag_for::<T>(callback)` 為底層工作、郵件、通知或事件類型指定標籤。用 `Horizon::tag_models_for::<T>(callback)` 註冊後備回呼，它會在該類型的明確標籤為空時執行。排入佇列的監聽器與其事件各自套用這套規則，先取監聽器標籤，再取事件標籤，重複標籤保留首次出現順序。兩個回呼都可以讀取承載資料中的序列化事件。

`Tags::for_model(&model)` 將 Rust 模型型別與純量主鍵組合為標籤。缺少主鍵、主鍵為 null 或 false 時後綴為空，true 的後綴為 `1`。結構化值或無效 UTF-8 位元組鍵會回傳錯誤。`Tags::for_models` 依框架保存的模型狀態去重。不同模型產生的同名標籤依原順序保留。應用程式自訂欄位或獨立載入的關聯影響等價判定時，使用 `Tags::for_models_by` 提供比較規則。Rust 應用程式透過回呼明確宣告承載資料中的模型欄位。

`Horizon::tag(callback)` 可為應用程式中的每筆承載資料補充標籤，並忽略空字串。各應用程式的回呼彼此隔離。面板的監控操作決定哪些標籤保留監控工作。

## 指標與限制 {#metrics}

`Repository::recent(1..=100)`、`metrics(connection, queue)`、`masters()`、`pause()`、`r#continue()` 和 `terminate()` 提供相同的儲存操作。`SupervisorOptions::from_config` 驗證設定，`AutoScaler::number_of_workers_per_queue` 計算浮點目標，`scale_pool` 計算逐池調整，`MasterSupervisor` 管理子程序生命週期。

Redis 佇列事件記錄 ID、工作名、連線、佇列、嘗試次數、狀態、耗時與 Redis 伺服器端時間。授權後的工作詳情還提供序列化承載資料、標籤和異常文字。`Repository::recent(1..=100)` 保持純元資料投影，近期索引最多 1000 個工作身份；`retention`（60–86400 秒，預設 3600）控制近期元資料和滾動 60 分鐘嘗試指標。

`horizon.trim` 按分鐘控制工作清單保留時間：`recent`、`pending`、`completed` 預設為 60，`recent_failed`、`failed`、`monitored` 預設為 10080。工作清單每頁傳回 50 筆。`horizon.metrics.trim_snapshots.job` 與 `.queue` 預設各保留 24 個快照。平均耗時和吞吐量只計算成功完成的工作，失敗、釋放和等待重試不參與計算。重複結束事件在下一次處理事件前忽略；手動重試重設嘗試次數時開始新的觀測。

`Repository::jobs()`、`tags()`、`metrics_repository()`、`master_supervisors()`、`supervisor_repository()`、`processes()` 與 `workload()` 提供獨立儲存庫操作。監控標籤按元資料前綴保存。每個指標的快照寫入與目前計數重設原子執行。Master 心跳 15 秒後到期，監督器心跳 30 秒後到期。元資料服務錯誤會暫停領取新工作並結束 Master，由作業系統程序管理器恢復。監控記錄會到期，不作為持久執行帳本。

負載介面（預設 `/horizon/api/workload`）對單一佇列回傳 `split_queues: null`。分組佇列的 `split_queues` 以佇列名稱為鍵。去重後的鍵依序為 `0`、`1` 等連續整數時，編碼為 JSON 陣列。重複佇列在積壓數和分項中只計一次，每次出現都計入整組等待時間估算。
