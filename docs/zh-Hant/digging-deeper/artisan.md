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

## 命令簽章與閉包輸入 {#command-signatures}

位置參數支援必填 `{user}`、可選 `{user?}`、預設 `{user=Taylor}`、必填陣列 `{user*}` 和可選陣列 `{user?*}`。選項支援 `{--force}`、`{--queue=}`、`{--queue=default}`、`{--id=*}`；`{--Q|queue=}` 宣告縮寫。陣列預設值寫作 `{user=*Taylor,Dayle}` 或 `{--id=*1,2}`。以 ` : ` 分隔說明，簽章可以換行。

```rust
use openrust::{
    console::{ConsoleError, Input, Output},
    support::facades::Artisan,
};

fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "mail:send {user : User ID} {--Q|queue=default} {--id=*} {--force}",
        |input: &Input, output: &mut Output| {
            let user = input.argument("user").unwrap();
            let queue = input.option("queue").unwrap();
            output.comment(&format!("{user}: {queue}"));
            Ok(0)
        },
    )?.purpose("Send a marketing email")
}
```

型別命令透過 `CommandDefinition::from_signature` 建立相同的定義。`Input::argument` 和 `option` 回傳 JSON 純量／陣列，未知或未設定的名稱回傳 `None`；`arguments` 和 `options` 回傳各組輸入。未設定的可選純量在整組中為 JSON null，陣列保持陣列。自訂 Clap 值解析器透過 `matches()` 讀取其具體型別；這些 JSON 入口支援字串、布林、數值和 UTF-8 OS 字串。

原有 `Artisan::command(signature, |output| …)` 繼續可用，正常退出碼為 0。`command_with` 接受明確型別的輸入／輸出閉包並回傳 `Result<i32, ConsoleError>`，也接受非同步回呼 `fn(&Application, &Input, &mut Output) -> ConsoleFuture<Result<i32, ConsoleError>>`。在回呼中用 `app.make::<Service>().await` 解析服務，讀取所屬應用，在命令執行時解析而非註冊時。明確的回呼入口避免破壞既有 Rust 閉包型別推斷；不提供 PHP 參數反射或未宣告 Rust 型別的自動注入。

首個參數為 `Arc<Service>` 的非同步回呼會在執行時自動從目前應用解析服務。透過 Provider 或既有容器型別描述註冊該服務；建構繼續使用同一容器，查看清單不會提前執行建構。

```rust
use std::sync::Arc;
use openrust::{console::{ConsoleError, ConsoleFuture, Input, Output}, support::facades::Artisan};

struct Mailer;
impl Mailer {
    fn send(&self, user: &str) -> String { format!("Sent to {user}") }
}
fn send<'a>(mailer: Arc<Mailer>, input: &'a Input, output: &'a mut Output)
    -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
    Box::pin(async move {
        output.comment(&mailer.send(input.argument("user").unwrap().as_str().unwrap()));
        Ok(0)
    })
}
fn register_injected() -> Result<(), ConsoleError> {
    Artisan::command_with("mail:send-injected {user}", send)?.purpose("Send with an injected service")
}
```

## 型別化命令輸入 {#typed-command-input}

`Input::input()` 回傳 `CommandInput` 快照，`input_value(key, default)` 讀取單個值。支援 `string`／`str`、`integer`、`float`、`boolean` 及其 `_or`／`_strict`、`clamp`、`date`、`interval`、backed `r#enum`／`enums`、`array` 和 `collect`，重用請求輸入的轉換規則。未指定日期時區時使用應用設定；無效日期回傳錯誤，`ConsoleError::source` 保留原始錯誤來源。

```rust
use openrust::{console::{ConsoleError, Input, Output}, support::facades::Artisan};

fn register_typed() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "reports:send {--from=} {--limit=10} {--active}",
        |input: &Input, output: &mut Output| {
            let data = input.input();
            let date = data.date("from", Some("!Y-m-d"), Some("UTC"))?;
            let limit = data.integer_strict("limit", 10);
            output.comment(&format!("limit={limit}, active={}, date={}", data.boolean("active"), date.is_some()));
            Ok(0)
        },
    )?.purpose("Send a typed report")
}
```

`arguments()` 和 `options()` 保留各組。直接建構的 `CommandInput` 若有同名鍵，固定實作先合併選項再合併位置參數，位置參數優先。`all`／`to_array` 回傳合併值；`all_keys` 將缺失的選中鍵保留為 null，空選擇回傳整組，而 `only` 忽略缺失路徑。`get`、`has`／`exists`、`has_all`、`has_any`、`missing`、`filled`、`is_not_filled`、`any_filled` 和 `except` 使用既有點路徑規則。false、零和空陣列屬於已填寫，空字串與 null 不屬於。

條件入口 `when_has`、`when_filled`、`when_missing` 和 `when_enum` 回傳 `Option<R>`（列舉入口還回傳 `Result`）。`None` 表示未執行回呼，以 `or_else` 提供回退；Rust 使用明確回傳型別，不隱式混合 PHP 按真值決定的輸入物件與回呼回傳值。動態屬性使用 `get`／`exists`，這些適配不會執行任意 PHP 運算式。

## 隔離命令 {#isolatable-commands}

命令實作 `contracts::console::Isolatable`，透過 `Artisan::add_isolatable`、`console::Application::add_isolatable_command` 或 Console Kernel 契約的 `register_isolatable_command` 註冊，自動提供 `--isolated[=code]`。Rust 的 `dyn Command` 擦除後無法自動發現另一 Trait，因此明確選擇帶 Trait 資訊的註冊入口。原有 `add` 和 `CommandDefinition::isolatable()` 保留；後者使用命令名及一小時鎖。

```rust
use openrust::{
    cache::Ttl,
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::console::Isolatable,
    foundation::Application,
    support::facades::Artisan,
};

struct SendReports(CommandDefinition);
impl Default for SendReports {
    fn default() -> Self {
        Self(CommandDefinition::from_signature("reports:isolated {user}").expect("valid signature"))
    }
}
impl Command for SendReports {
    fn definition(&self) -> &CommandDefinition { &self.0 }
    fn handle<'a>(&'a self, _app: &'a Application, input: &'a Input, output: &'a mut Output)
        -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            output.comment(&format!("Report for {}", input.argument("user").unwrap()));
            Ok(0)
        })
    }
}
impl Isolatable for SendReports {
    fn isolatable_id(&self, input: &Input) -> Result<Option<String>, ConsoleError> {
        Ok(input.argument("user").and_then(|value| value.as_str().map(str::to_owned)))
    }
    fn isolation_lock_expires_at(&self, _input: &Input) -> Result<Ttl, ConsoleError> {
        Ok(Ttl::Seconds(300))
    }
}
fn register_isolated() -> Result<(), ConsoleError> {
    Artisan::add_isolatable(Box::<SendReports>::default())
}
```

隔離按需啟用，普通呼叫不取得鎖。指定 `--isolated` 時，衝突略過處理器並回傳 0；`--isolated=12` 可指定衝突退出碼。Trait 預設方法同樣使用命令名和一小時期限。覆寫 `isolatable_id` 追加與輸入有關的後綴，覆寫 `isolation_lock_expires_at` 回傳 `Ttl::Seconds` 或 `Ttl::Until(SystemTime)`；空 ID 與未提供 ID 不同。

參與程序必須使用同一支援鎖的快取及命名空間。array 只在應用程序內協調，file 需要共享檔案系統，database／Redis 使用其設定後端。正常完成、錯誤和執行階段內取消會釋放所屬鎖；程序被終止時依靠期限恢復，過期的舊擁有者不會刪除新擁有者的鎖。資料庫整秒鎖採用 Unix 秒加租期，亞秒期限保留既定原生取整策略。

## 全域選項與經過驗證的輸入修改 {#editing-command-input}

分派後的 `Input` 助手包含呼叫時的 `command` 參數（保留別名）及繼承的全域選項：`verbose` 為布林值，`env` 為可空字串，`ansi` 為 null／true／false。`no-ansi` 合併到 `ansi`，不單列選項。`matches()` 保留原生 Clap 開關、環境值及詳細程度計數，既有 CLI 解析與輸出優先順序不變。單獨呼叫 `CommandDefinition::parse` 仍依其宣告範圍解析。`Input::clone` 保留註冊定義身分及關停權杖，修改後的副本仍可交給 Console Kernel。

```rust
use openrust::{console::{ConsoleError, Input, Output}, support::facades::Artisan};
use serde_json::json;

fn register_edited() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:edit {user} {--queue=default}", |input: &Input, output: &mut Output| {
        let mut edited = input.clone();
        edited.set_argument("user", ["42"])?;
        edited.set_option("queue", json!("priority"))?;
        let data = edited.input();
        output.comment(&format!("user={}, queue={}", data.string("user"), data.string("queue")));
        Ok(0)
    })?.purpose("Edit parsed command input")
}
```

`set_argument` 接收 CLI 參數 token；`set_option` 接收 JSON 字串、布林開關、計數、陣列，以及表示可選值未帶參數的 null。每次修改重用原 Clap 解析器及約束，同時更新 `matches()` 與型別化助手；驗證失敗不改原輸入，保留其他值、空字串及非 UTF-8 OS 參數。

這些入口依 CLI 定義重新驗證，並非任意 PHP 物件修改。定義無法表達的值、可選位置的空洞、清空非空預設陣列都會回傳錯誤，不悄悄重新繫結其他欄位或恢復預設值。讀取或修改全域輸入不會重新設定已建立的輸出串流。
