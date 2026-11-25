# Telescope 應用除錯

## 安裝與設定 {#configuration}

Telescope 將應用診斷資訊寫入獨立資料庫表，並提供需要授權的診斷面板。在應用 `Cargo.toml` 的 `[features].default` 中加入 `telescope`，保留既有功能，然後執行：

```sh
./artisan telescope:install
# 重新建置應用，載入發布的設定與 Provider。
./artisan migrate
```

安裝器發布 `config/telescope.rs`、`app/providers/telescope_service_provider.rs`，登記模組與 Provider。重複安裝保留現有檔案。套件探索負責註冊套件服務與遷移；Provider 啟動不會查詢 Telescope 資料表。

`TELESCOPE_ENABLED` 預設為 true；`TELESCOPE_PATH` 預設為 `telescope`；`TELESCOPE_DOMAIN` 可限制路由網域。`TELESCOPE_DRIVER` 預設為 `database`。儲存使用 `storage.database.connection` 與 `storage.database.chunk`（預設 1000）。套件遷移登記在預設連線；使用其他儲存連線時，須先在該連線登記 `TelescopeServiceProvider::migrations()` 再遷移。設定快取使用已儲存的快照；修改設定後重新啟動常駐程序。

每個 `watchers.<Name>Watcher.enabled` 開關對應 `TELESCOPE_<NAME>_WATCHER` 環境變數；複合名稱使用底線，例如 `TELESCOPE_CLIENT_REQUEST_WATCHER`。發布的設定包含各 Watcher 選項。`only_paths`、`ignore_paths`、`ignore_commands` 限制蒐集範圍，路徑比對不含開頭斜線。暫停狀態與監控標籤使用應用設定的快取。

`init` 及其內部命令 `init:resources` 預設不蒐集，避免在資料庫與套件資源準備完成前存取診斷儲存。

## 授權與過濾 {#authorization}

存取 `/telescope` 或自訂路徑。安裝的應用 Provider 預設允許本機環境，其他環境在自訂授權回呼前拒絕存取。回呼應使用應用已驗證身分與權限。面板及所有 API 路由保留 `web` 與 Telescope `Authorize` 中介軟體；寫入操作使用框架請求偽造防護，接受同源 Fetch 中繼資料或有效工作階段 CSRF 權杖。

安裝的 Provider 在本機記錄全部診斷，其他環境預設保留可回報例外、失敗請求與工作、排程工作，以及包含監控標籤的紀錄。在應用 Provider 中使用 `Telescope::filter`、`filter_batch`、`tag` 自訂策略。回呼設定跨執行作用域共用；待寫紀錄按 HTTP 請求、Artisan 命令與佇列嘗試隔離。`without_recording(future).await` 抑制巢狀記錄，並在正常完成或取消後恢復原狀態。

請求密碼與授權標頭預設遮罩；安裝的 Provider 在非本機環境額外隱藏 CSRF 與 Cookie。`hide_request_parameters`、`hide_request_headers`、`hide_response_parameters` 可增加名稱，巢狀參數使用點路徑。啟用敏感業務蒐集前，應審查事件酬載、郵件與日誌內容。

## Watcher 與記錄 {#watchers}

Watcher 涵蓋請求、查詢、例外、日誌、工作、批次、快取、HTTP 用戶端、命令、排程工作、郵件、通知、模型、事件、Gate、檢視、Redis 命令及 Dump。原生生命週期事件提供實際資料，待寫紀錄透過框架延遲回呼寫入；強制終止程序無法保證寫入。

`openrust_telescope::watchers` 下的 18 類公開結構均實作 `Watcher`。應用 Provider 可使用 `new(options)` 建構並呼叫 `register(app).await?`；獨立註冊前先關閉對應預設設定項，避免重複監聽。套件 Provider 也使用同一入口。`QueryWatcher` 支援 `slow`，`EventWatcher` 支援 `ignore` 模式。預設忽略框架事件；在解析的服務上呼叫 `telescope.ignore_framework_events.store(false, std::sync::atomic::Ordering::Release)` 可開啟，設定僅在目前應用內共用。

應用事件可實作 `Event::properties()` 提供 JSON 診斷快照；Rust 不會反射任意結構欄位。事件監聽器名稱來自實際的派送器註冊。模型載入計數使用原生 Eloquent 生命週期。郵件保留原始 mailable 與佇列旗標；預覽使用瀏覽器沙箱，下載提供擷取的 MIME 郵件。

使用 `openrust::support::dump(&value).await?` 記錄 Debug 傾印。蒐集需要啟用面板傾印監看或設定 `TELESCOPE_DUMP_WATCHER_ALWAYS=true`。此方法不攔截 `dbg!`、`println!` 或同步集合傾印方法。請求輸入記錄使用已解碼資料，不讀取尚未消費的本文串流。Redis 觀察涵蓋透過 `Connection::query_command` 成功執行的命令；管線與專用阻塞傳輸不在此觀察路徑內。

執行環境未提供可靠位置時，來源位置與堆疊影格保持空值。Gate 項目使用 Rust 授權入口提供的呼叫位置。`ignore_packages` 排除相依套件來源，`ignore_paths` 依路徑子字串篩選。來源遭篩除或無法取得時，授權結果仍會記錄，檔案和行號留空。Query 項目會使用已擷取的最近一處合格資料庫呼叫位置。一次分頁呼叫觸發的計數、頁面資料和預先載入查詢都可指向同一行應用程式碼。Eloquent 寫入也可記錄應用程式的呼叫位置，包括靜默寫入和交易內寫入。直接關聯查找、批次讀取和游標也可記錄應用程式的呼叫位置。一次多對多中介表操作產生的讀取和寫入可歸到同一行。Schema 的建表、改表和目錄查詢也可歸到應用程式呼叫位置。透過外鍵限制包裝器執行的回呼查詢會保留回呼內的呼叫行。已擷取的應用呼叫位置全部遭篩除時，查詢不會寫入。入口尚未提供應用呼叫位置的查詢仍可記錄，檔案和行號留空。不透明 Rust 回呼無法提供檢視 composer 名稱。伺服器記錄已緩衝的 JSON／文字回應，套用回應參數遮罩及字元長度限制；不讀取不透明本文或串流，標示為 `Not Captured`。處理器／控制器名稱與實際路由中介軟體型別來自路由器。HTTP 用戶端會記錄傳給 `as_multipart()` 的欄位，以及透過 `attach_body()` 附加的檔名、十進位 KB 大小和標頭。附件串流由傳輸層讀取，未知長度顯示為 `null`。原生 `Part` 仍可透過 `attach()` 傳送，同名欄位的最後一項為不透明部件時，Telescope 省略該欄位。面板提供清單、過濾、詳情、監控標籤與記錄控制，未重現上游 Vue 介面或頭像功能。

出站診斷會記錄 JSON 或表單欄位，也可記錄 GET 網址中傳入的查詢參數。表單欄位保留原始純量型別，同名欄位在診斷中顯示最後一項。建構器的查詢選項仍用於實際傳送的 URL。原始本文僅在內容類型為支援的 JSON 或表單格式時才會解碼。

發生重新導向後，回應診斷顯示最終網址，並保留首次呼叫的 GET、JSON 或表單輸入。連線失敗會使用例外攜帶的請求，只解碼媒體類型受支援的緩衝本文。

## 查詢與命令 {#commands}

```sh
./artisan telescope:list request --limit=20
./artisan telescope:list exception --tag=production --json
./artisan telescope:show latest:exception --full
./artisan telescope:show latest --type=query,log --json
./artisan telescope:pause
./artisan telescope:resume
./artisan telescope:prune --hours=48 --keep-exceptions
./artisan telescope:clear
```

`list` 接受 `--tag`、`--batch`、`--family` 與 `--before` 序號游標。`show` 接受 UUID、無歧義 UUID 前綴、`latest` 或 `latest:{type}`。一般輸出截斷長內容，`--full` 保留完整值；JSON 始終保留擷取內容。主控台使用一般表格與時間戳記，未採用 PHP 主控台的樣式與相對時間描述。

`prune` 的小時數必須為非負整數；`clear` 刪除全部 Telescope 紀錄、標籤與監控列。保留策略由應用排程器執行。`telescope:publish` 發布設定，`--force` 覆寫該檔案。獨立程式碼可用 `EntriesRepository` 實作建立 `Telescope` 並明確呼叫 `store().await`；應用 Provider 可在首次解析 Telescope 前替換此契約。
