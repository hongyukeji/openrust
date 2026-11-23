# Artisan 命令列

Console Kernel [引導應用程式](../architecture-concepts/lifecycle.md)後分派 Artisan 命令。應用程式骨架的命令清單和說明無需 HTTP 監聽或資料庫連線即可執行。

```sh
./artisan list
./artisan list cache --format=json --short
./artisan help list
./artisan list --help
./artisan inspire
```

預設控制台路由登記了 `inspire`。執行 `./artisan inspire` 會隨機輸出一則引言及其作者，結束代碼為 0。

名言庫與固定版框架一致，共 41 則。支援色彩的終端機會將正文顯示為粗體、署名顯示為灰色。重新導向時預設輸出純文字。可以用 `--ansi` 強制開啟樣式，或用 `--no-ansi` 關閉。

Windows 上使用 `artisan.bat`。編譯後的應用程式透過 `main artisan` 接收相同的命令。

清單和說明成功時傳回結束代碼 0。未知命令和無效引數傳回 1。標示為 `Planned` 的項目可以檢視說明，執行時傳回 1。

透過 `with_commands` 接入 `app/console/commands/mod.rs` 中的命令型別。在 `bootstrap/app.rs` 的 `with_routing` 中宣告 `routes/console.rs` 的控制台路由。應用程式命令可以取代同名預留項目。

`routes/console.rs` 匯入 `openrust::support::facades::Artisan`，透過 `Artisan::command("inspire", …)` 向 `register()` 執行期間繫結的主控台註冊閉包，透過 `purpose()` 設定說明和命令清單中的描述。閉包接收命令輸出物件，正常傳回時結束代碼為 0。

`Application::handle_command` 呼叫 Console Kernel 的 `handle` 和 `terminate`。Kernel 完成引導後載入延遲 Provider，再登記命令清單、框架內建命令和應用程式命令，最後分派。


## 原始碼與發布入口 {#source-and-release-entrypoints}

原始碼 `artisan` 腳本先進入應用目錄，再將參數原樣交給 Cargo，保留空格、空參數和非 ASCII 文字。腳本回傳命令的結束碼。

獨立建置使用 `./main artisan list`、`./main artisan help inspire` 或 `./main artisan inspire`，無需 Cargo。設定相對於可執行檔解析，從其他工作目錄啟動也可執行。參閱[部署](../getting-started/deployment.md)。

直接執行編譯後的 `main` 會依設定[啟動應用程式服務](../getting-started/deployment.md#一起執行服務)。用 `main artisan` 單獨執行主控台命令，這些服務不會啟動。無法辨識的首個引數會在建立應用程式前顯示程式用法並回傳結束代碼 2。Artisan 分派錯誤回傳 1。

在 CMD 中執行 `artisan.bat inspire`。在 PowerShell 中使用 `.\artisan.bat help inspire --format=json`。原始碼啟動器需要在 `PATH` 中找到 `cargo.exe`，執行前會切換到應用程式目錄。在 CMD 中用 `%ERRORLEVEL%` 讀取結束代碼，在 PowerShell 中用 `$LASTEXITCODE`。[Windows 獨立部署](../getting-started/deployment.md#windows-standalone)使用 `.\main.exe artisan inspire`。

### 開發 HTTP 服務 {#development-http-server}

用 `./artisan serve` 單獨啟動 HTTP，預設監聽 `127.0.0.1:8000`。可用 `SERVER_HOST` 和 `SERVER_PORT` 更改預設值，也可在本次執行時傳入 `--host` 和 `--port`。未明確指定連接埠時，如果連接埠被占用，`--tries` 限制繼續嘗試的連接埠數，預設額外嘗試 10 個。

```sh
./artisan serve
./artisan serve --host=127.0.0.1 --port=8080 --no-reload
```

選定的環境檔案變更時，服務會在原監聽連接埠重新啟動。用 `--env=local` 選擇環境，用 `--no-reload` 保持目前設定直到停止。按 Ctrl+C 停止服務。執行編譯後的 `main` 時省略模式引數，即可[一起啟動已設定的服務](../getting-started/deployment.md#一起執行服務)。

## 開發程序 {#the-dev-command}

在應用程式目錄同時執行已登記的開發程序。預設的開發伺服器和佇列監聽器直接使用 Artisan 目前的執行檔。Vite 等 Node 程序依需要使用 Node。

```sh
./artisan dev
./artisan dev --tabs
./artisan dev --stream
./artisan dev --inline --timestamps --no-restart
./artisan dev --json
```

輸出依程序加上標籤，保留各自的 stdout 和 stderr。`--timestamps` 為內嵌輸出加上 UTC 時間。`--json` 每行輸出一筆有版本號的事件，包括程序啟動、輸出、結束、重試和最後的 `done`。

執行滿一秒後失敗的程序會每隔一秒重試，最多五次。成功結束的程序不再執行，一秒內失敗則立即回報。用 `--no-restart` 關閉這次重試，或在應用程式用 `DevCommands::disable_auto_restart()` 關閉。用 `DevCommands::with_timestamps()` 預設開啟時間戳記。第一個永久失敗決定最後的結束代碼，其他程序可以繼續執行。

Unix 上的可用終端機預設開啟分頁檢視，也可用 `--stream` 直接開啟合併檢視。重新導向輸出、尺寸不足的終端機、Windows、`--inline` 和 `--json` 會走內嵌輸出路徑。互動工作階段正常離開時回傳 0，內嵌和 JSON 模式則回報第一個永久失敗的結束代碼。

按 `1`–`9` 選擇分頁，也可用 Tab 聚焦側欄後按方向鍵選擇。按 `s` 切至串流、`t` 回到分頁、`f` 篩選串流、`r` 重新啟動目前分頁的程序、`c` 清除目前檢視。用 `/` 搜尋，再按 `n` 或 `N` 切換符合的項目。用方向鍵或 Page Up、Page Down 捲動日誌。

`--buffer-size` 和 `--stream-buffer-size` 控制各分頁與串流保留的行數，預設分別是 2,000 和 10,000 行。應用程式可用 `DevCommands::buffer_size()`、`stream_buffer_size()` 設定，並用 `tabs()`、`stream()` 或 `inline()` 選擇預設模式。

按 `q` 或 Ctrl+C 離開。Unix 終止訊號也會要求關閉服務，最多等待子程序結束六秒。Artisan 恢復終端機，並將保留的串流日誌印到對應的輸出串流。可選 [Pail 命令](../the-basics/logging.md#tailing-log-messages-using-pail)也可獨立執行。

在 Windows 主控台按 Ctrl+C 要求關閉服務。每條開發命令都在自己的 Windows 作業中執行。Artisan 對該命令的主控台程序群組送出 Ctrl+Break，並在寬限結束後回收仍在執行的子孫程序。啟用 `--no-restart` 時，一條命令失敗也會關閉其他開發命令並回傳失敗代碼。

### 查看開發程序

用 `dev:list` 查看目前應用程式登記的開發程序。預設列出 [HTTP 開發伺服器](#development-http-server)和佇列監聽器。應用程式含有 `package.json` 時，也會依鎖定檔選擇 Bun、pnpm、Yarn 或 npm 並列出 Vite。查詢不會啟動程序。載入可選 Pail Provider 後，會在 Vite 之前加入 `logs`，執行 `pail --timeout=0`。它屬於框架預設程序，排除 vendor 不會移除它，`without_default_commands()` 會移除它。套件或應用程式可以用同名 `logs` 項目覆寫。

```sh
./artisan dev:list
./artisan dev:list --json
./artisan dev:list --no-interaction --filter=queue
```

非互動環境也會輸出 JSON。`--filter` 依名稱或命令的子字串篩選，`--except-vendor` 和 `--only-vendor` 篩選套件登記的程序。篩選沒有結果時結束碼為 1。

在應用程式服務提供者的 `boot` 方法中，透過 `openrust::foundation::DevCommands` 登記程序。同名程序會覆蓋預設項目，應用程式登記優先於套件登記。

```rust
DevCommands::artisan("serve --port=9000", Some("server"))?.green();
```

Shell 命令用 `register` 登記，套件管理器命令用 `node` 或 `node_exec`。名稱傳 `None` 時使用命令的第一個單字。用 `only`、`except`、`order`、`without_vendor_commands` 和 `without_default_commands` 調整應用程式清單。框架依 Rust 呼叫者的原始碼路徑辨識套件登記。

## 命令說明和輸出格式 {#command-help-and-output-formats}

`./artisan help migrate` 與 `./artisan migrate --help` 從同一命令定義讀取引數、選項和說明。檢視說明時不會執行目標命令。精確的命令名稱和別名優先，可依完整名稱或冒號分隔的各段使用唯一縮寫。

```sh
./artisan list cache --format=json --short
./artisan help migrate --format=json
./artisan help migrate --format=xml
./artisan help migrate --format=md
```

`list` 和 `help` 支援 `txt`、`json`、`xml` 和 `md`。一般文字清單會先顯示全域選項，再列出命令。指定命令群組時也會顯示全域選項。`list --raw` 只輸出平鋪清單。`help --raw` 輸出完整的純文字說明，啟用 ANSI 時也會移除樣式。`list --short` 保留名稱、別名和實作狀態，省略命令詳細資料。各語言的結構化輸出使用相同的命令和選項識別碼。說明和清單成功時結束碼為 0。引數錯誤、歧義和未知命令的結束碼為 1。

`--quiet` 隱藏一般輸出。`--silent` 同時隱藏主控台錯誤。命令定義也可使用全域 `--no-interaction`、`--ansi`、`--no-ansi`、`--version` 和詳細程度選項。

語言選擇與覆寫見 [Artisan 翻譯](./localization.md#artisan-translations)。

## 在程式中執行命令 {#programmatically-executing-commands}

在 HTTP 處理器或另一個 Artisan 命令中，用 `Artisan::call(name, arguments, &mut output).await` 執行已登記的命令。命令在目前應用程式內執行並回傳結束代碼。未知命令和無效引數回傳 `ConsoleError`。每次呼叫用獨立的 `Output` 收集標準輸出與標準錯誤。

```rust
use std::ffi::OsString;

use openrust::{
    console::{ConsoleError, Output},
    support::facades::Artisan,
};

async fn inspire_help() -> Result<(i32, String), ConsoleError> {
    let mut output = Output::default();
    let arguments = ["inspire", "--format=json"].map(OsString::from);
    let status = Artisan::call("help", &arguments, &mut output).await?;
    Ok((status, output.stdout))
}
```

每個位置引數或選項各傳一個 `OsString`。含空格的值放在同一個引數中。布林開關為真時傳入開關選項，多值選項可重複傳入。用 `Artisan::call_silently(name, arguments).await` 捨棄兩個輸出串流並保留結束代碼。

### 將 Artisan 命令加入佇列 {#queueing-artisan-commands}

用 `Artisan::queue` 準備命令。呼叫 `dispatch().await` 提交工作並取得派發結果。用 `on_connection` 和 `on_queue` 指定目標，也可以使用預設連線及其預設佇列。

```rust
use openrust::{
    queue::{DispatchResult, QueueResult},
    support::facades::Artisan,
};

async fn queue_inspire() -> QueueResult<DispatchResult> {
    Artisan::queue("inspire", &[])
        .on_connection("redis")
        .on_queue("commands")
        .dispatch()
        .await
}
```

用 `./artisan queue:work redis --queue=commands` 啟動對應的 Worker。工作資料以 JSON 儲存命令名稱和 UTF-8 引數。Worker 在自身應用程式內解析命令，捨棄主控台輸出，並確認正常傳回的工作，即使結束代碼非零。需要使用佇列重試和失敗記錄時，在命令中回傳 `ConsoleError`。

## 按需安裝路由 {#optional-route-installers}

在原始碼目錄執行下列命令，產生路由檔案並更新模組宣告與啟動設定，然後重新建置。既有路由檔案會保留。

```sh
./artisan install:broadcasting
./artisan install:grpc
```

使用 `./artisan install:api` 建立並註冊 API 路由。預設保留既有路由。功能開關與遷移選項請參閱 [Sanctum 安裝](../packages/sanctum.md#installation)。 廣播接入後會註冊 `/broadcasting/auth`，授權需要廣播驅動程式和頻道規則。gRPC 須註冊 Tonic 服務，並設定 `GRPC_HOST` 和 `GRPC_PORT` 才會開啟監聽。

## 應用程式初始化 {#complete-installation}

在原始碼專案執行 `./artisan init`，Windows 使用 `artisan.bat init`。精靈讀取既有值，引導應用程式、資料庫、快取、Session 和佇列設定，密碼隱藏輸入，連線檢查通過後儲存。非互動模式讀取專案環境。目前初始化支援 SQLite、MySQL／MariaDB 和 PostgreSQL，以及本機／資料庫／Redis 服務和日誌／SMTP 郵件和內建簡訊供應商。API 使用 Sanctum，同時安裝頻道和 gRPC 註冊。Reverb 套件自動安裝，是否隨主應用程式啟動由 REVERB_ENABLED 控制；僅產生缺少的 Reverb 憑證，保留既有值。初始化預設安裝全部已支援擴充套件，應用程式骨架本身仍保持輕量。

```sh
./artisan init --dry-run
./artisan init
./artisan init --no-interaction
./artisan init --extensions=api --no-interaction
```

`--extensions=all` 是預設集合和精靈推薦項，也可選擇 `api,broadcasting,reverb,grpc,passport,fortify,passkeys,socialite,precognition,oidc,sso`。`api` 安裝 Sanctum；`passport` 自動補選 `api`，`passkeys` 補選 Fortify，`sso` 補選 OIDC 和 Fortify，`reverb` 補選廣播頻道。後續執行可補裝套件，已設定 Reverb 時須納入選擇。API `/user` 路由保留 `auth:sanctum`，Passport 使用獨立 `api` 守衛；既有不相容守衛會保留並回報衝突。

Fortify 安裝驗證動作、應用程式 Provider、設定和遷移；初始化新產生的設定使用 `views=false` 並啟用郵件驗證，既有自訂設定保留。請透過 `ResetPassword::create_url_using` 設定前端密碼重設位址，或提供名為 `password.reset` 的路由；無介面初始化不產生前端頁面。依選擇啟用 Passkeys。新設定須將 APP_URL 設為含網域、無路徑的精確 HTTPS Origin；開發環境允許 HTTP localhost，例如 `APP_URL=http://localhost:8000`。HTTP IP 位址會在寫入前被預檢拒絕。Socialite、OIDC 啟用對應套件，供應商憑證、獲准回呼、本機帳戶明確綁定及應用程式路由仍由應用程式設定；選擇 SSO 不會建立身分提供方或設定全域登出。Precognition 已內建，在選定路由新增 `precognitive`。安裝狀態與這些外部前置分開回報。

缺少 `.env` 時從範例建立，保留既有金鑰和憑證；缺少應用程式金鑰時呼叫 `key:generate`。共用安裝服務在原始碼鎖內發布所選資源；目前程式未編入安裝器時，集中建置一次載入安裝服務，隨後統一建置全部產生模組並執行一次原遷移入口，不遞迴執行個別安裝命令。正式環境需要確認或 `--force`。初始化成功後執行 `cargo run`，命令不自動啟動長期服務。`--without-migration` 回報準備完成、遷移待執行。`--force` 覆寫可發布設定和骨架並授權正式遷移，保留 APP_KEY、Passport 金鑰、無關應用程式模組及全部歷史遷移。`--dry-run` 不連線、不寫入、不加鎖、不建置、不遷移。必要步驟失敗傳回非零，停止相依步驟並保留完成內容供重試；並行原始碼安裝立即回報衝突。

選擇 SMTP 後複用既有傳輸層檢查連線、TLS 和驗證，不寄送測試郵件。精靈收集內建簡訊供應商憑證；傳送身分和業務範本仍依各供應商設定，不自動傳送簡訊，也不宣稱實際投遞通過。啟用 Reverb 時引導監聽及發布位址，預設同處理程序執行；既有設定始終作為精靈預設值。

使用 `--env=acceptance` 初始化 `.env.acceptance`；清除設定、產生金鑰和遷移步驟使用同一環境，不修改 `.env`。

## 產生命令 {#generating-commands}

破壞性命令可呼叫 `openrust::console::confirm_to_proceed(production, input.matches())?`，僅在回傳 `true` 時繼續。`production` 由應用程式環境或命令確認條件決定。共用的多語提示預設拒絕，非互動輸入拒絕；命令宣告的 `--force` 可略過確認。

在原始碼目錄產生命令。產生的型別實作 Artisan 命令契約。

```sh
./artisan make:command SendReport
./artisan make:command Reports/ArchiveReport --command=reports:archive
```

第一條命令寫入 `app/console/commands/send_report.rs`，命令簽章為 `app:send-report`。第二條在 `reports` 模組下產生命令並指定簽章。使用 `-f` 或 `--force` 覆寫既有檔案。

在 `app/console/commands/mod.rs` 宣告新模組並註冊命令。以 `SendReport` 為例，加入 `mod send_report;`，在 `register` 內呼叫 `artisan.add_command(Box::new(send_report::SendReport::default()))?;`。重新建置後執行 `./artisan app:send-report`。

在應用程式根目錄放置 `stubs/console.rs.stub` 可自訂產生內容。產生器替換 `{{class}}` 和 `{{command}}`。

### 產生配套測試 {#generating-an-accompanying-test}

加入 `--test` 可在產生命令時同時產生一個功能測試。

```sh
./artisan make:command SendReport --test
```

測試寫入 `tests/feature/console/commands/send_report_test.rs`，巢狀命令保留原有子目錄。依照[測試接線](../testing/getting-started.md#creating-tests)宣告模組後，以 Cargo 執行。樣板沿用 `make:test` 的健康檢查，可在產生的測試中加入命令斷言。

`--force` 允許取代兩個產生的檔案。命令產生後若測試產生失敗，Artisan 以狀態碼 1 結束並保留命令檔案。解決錯誤後，執行 `./artisan make:test Console/Commands/SendReportTest` 單獨產生測試。

## 自訂產生樣板 {#stub-customization}

將產生器樣板發佈到應用程式的 `stubs/` 目錄。命令會發佈結構體、trait、列舉、命令、工廠、遷移、模型、策略、Seeder、功能測試和單元測試樣板。編輯已發佈的樣板即可修改對應產生器下一次產生的內容。

```sh
./artisan stub:publish
./artisan stub:publish --existing
./artisan stub:publish --force
```

預設保留既有樣板並補齊缺少的檔案。`--existing` 只覆寫已存在的樣板，與 `--force` 同用時也維持這個範圍。單獨使用 `--force` 會覆寫既有樣板並補齊缺少的檔案。

同步的 `foundation.publishing_stubs` 監聽器可透過 `PublishingStubs` 事件加入來源檔案、插入編譯進產物的樣板或移除樣板。發佈目標使用應用程式樣板目錄內的檔名。

## 產生 Rust 型別 {#generating-rust-types}

Artisan 可產生 Rust 結構體、trait 和列舉。使用產生的型別前請宣告對應模組。

```sh
./artisan make:class Report
./artisan make:class SendReport --invokable
./artisan make:interface ReportStore
./artisan make:trait ReportFormatting
./artisan make:enum ReportState
./artisan make:enum ReportCode --int
./artisan make:enum ReportKind --string
```

`make:interface` 和 `make:trait` 均產生 Rust trait。介面依序尋找 `app/contracts`、`app/interfaces`，trait 依序尋找 `app/concerns`、`app/traits`，列舉依序尋找 `app/enums`、`app/enumerations`。目錄均不存在時寫入 `app/`。名稱以 `App/` 開頭時，從 `app/` 開始解析。

可呼叫結構體提供明確的 `invoke()` 方法。整數與字串後備列舉透過 `value()` 分別回傳 `i64` 和靜態字串。使用 `-f` 或 `--force` 取代已產生的檔案。可透過 [`stub:publish`](#stub-customization) 發佈可自訂樣板。介面命令使用內建樣板。
