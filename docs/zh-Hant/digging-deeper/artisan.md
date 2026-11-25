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

## 缺失必填參數的自動提示 {#prompting-for-missing-input}

實作 `PromptsForMissingInput`，註冊前用 `.prompting()` 保留該 Trait。必填參數按宣告順序收集，隨後執行最終驗證與隔離。已傳參數不重複詢問；`--no-interaction`、`--quiet` 和 `--silent` 跳過詢問，但仍拒絕缺失的必填參數。

```rust
use openrust::{
    console::{Command, CommandDefinition, CommandRegistration, ConsoleError,
        ConsoleFuture, Input, MissingInputPrompt, Output},
    contracts::console::PromptsForMissingInput,
    foundation::Application,
    support::facades::Artisan,
};
use std::collections::BTreeMap;

struct Deliver(CommandDefinition);
impl Command for Deliver {
    fn definition(&self) -> &CommandDefinition { &self.0 }
    fn handle<'a>(&'a self, _: &'a Application, input: &'a Input,
        output: &'a mut Output) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            output.stdout.push_str(&format!("user={}, queue={}",
                input.argument("user").unwrap().as_str().unwrap(),
                input.option("queue").unwrap()));
            Ok(0)
        })
    }
}
impl PromptsForMissingInput for Deliver {
    fn prompt_for_missing_arguments_using(&self) -> BTreeMap<String, MissingInputPrompt> {
        BTreeMap::from([("user".into(),
            MissingInputPrompt::text("Which user should receive the report?", "e.g. 123"))])
    }
}
fn register_prompted() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(CommandRegistration::new(Deliver(
        CommandDefinition::from_signature("reports:prompt {user} {--queue}")?
    )).prompting()))
}
```

預設問題使用參數說明或名稱。`MissingInputPrompt::text` 提供問題與佔位說明；`MissingInputPrompt::custom` 接收非同步的應用／可變輸入／輸出回呼，回傳 CLI token（`Vec<OsString>`），陣列參數可以回傳多個值。自訂回呼可以回傳 `"0"`；預設文字問題拒絕空答案或 `"0"`，保持必填文字驗證語意。每個缺失參數都會重新取得問題定義。

`after_prompting_for_missing_arguments` 只在至少收集一個缺失參數後執行，接收可變 `Input`，可呼叫 `set_option` 或 `set_argument`；命令執行前再做最終驗證。型別同時實作兩種 Trait 時，可在同一個 `CommandRegistration` 上組合 `.prompting().isolatable()`。適配器在型別擦除前保留 Trait，重用既有註冊、快取鎖與生命週期管線。

預設文字問題要求連接終端；HTTP／程式化擷取輸出不會讀取程序標準輸入。這些環境請明確傳參或提供自訂回呼。說明資訊不觸發互動。取消與回呼失敗沿既有錯誤處理傳播；等待輸入時不持有隔離鎖。

## 命令註冊的生命週期 {#command-lifecycle}

Provider 就緒後，每個應用裝配一次命令。後續 Facade 呼叫、HTTP 請求和受管 Runner 重用已註冊的命令實例及閉包擷取。不同應用實例、不同 CLI 程序使用各自的註冊表。下面的閉包在同一應用中連續呼叫兩次，依次回傳 `1`、`2`。

```rust
use openrust::{console::{ConsoleError, Input, Output}, support::facades::Artisan};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

fn register_counter() -> Result<(), ConsoleError> {
    let calls = Arc::new(AtomicUsize::new(0));
    Artisan::command_with("reports:count", move |_: &Input, output: &mut Output| {
        output.stdout.push_str(&(calls.fetch_add(1, Ordering::SeqCst) + 1).to_string());
        Ok(0)
    })?.purpose("Count calls within this application")
}
```

每次呼叫獨立持有輸入、輸出、詳細程度和生命週期作用域。命令執行失敗不會重建註冊表；註冊失敗不會公開半成品，後續可以重試。執行命令時不持有註冊表鎖，因此支援巢狀呼叫。

並行請求可能執行同一個命令物件。共享欄位須保證執行緒安全；請求輸入、使用者和請求層級依賴在 `handle` 或目前的 `command_with` 回呼中解析，不在命令註冊時擷取請求資料。

## 寫入命令輸出 {#writing-command-output}

透過 `input.io(output)` 借用目前命令上下文。`line`、`info`、`comment`、`question`、`warn`、`error`、`alert` 和 `new_line(count)` 使用同一輸出。`info` 輸出綠色訊息；`output_components()` 保留既有資訊／警告／錯誤元件配置。輸出失敗可透過 `ConsoleError::source` 讀取原始原因。

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError},
    support::facades::Artisan,
};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

fn register_output() -> Result<(), ConsoleError> {
    let calls = Arc::new(AtomicUsize::new(0));
    Artisan::command_with("reports:output", AsyncCommandCallback::new(move |_, input, output| {
        let calls = calls.clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            let mut io = input.io(output);
            io.info("Report ready")?;
            io.line("<options=bold>Summary</>")?;
            io.comment("Report generated")?;
            io.question("Review the report")?;
            io.warn("One record was skipped")?;
            io.error("A record needs correction")?;
            io.new_line(1)?;
            io.alert("Check complete")?;
            io.output_components().info("Saved");
            io.line(&format!("Calls: {}", calls.fetch_add(1, Ordering::SeqCst) + 1))?;
            Ok(0)
        })
    }))?.purpose("Display a report")
}
```

寫入前解析命名樣式、前景／背景色、樣式選項及超連結。未知標籤保持字面量，跳脫尖括號正常顯示。未閉合的樣式狀態屬於該輸出物件，不傳入另一輸出；標籤巢狀、顏色或選項非法時回傳 `ConsoleFormatError`。`--ansi`／`--no-ansi` 控制色彩，`--quiet`／`--silent` 抑制普通訊息。

一般 CLI 入口即時傳送這些訊息，並先傳送尚未寫出的元件；不會在 `stdout` 中保留無界終端歷史。程式化 `Output::default()` 將文字擷取到 `stdout`，可用於 HTTP 回應或斷言。原生命令事件的緩衝輸出欄位記錄待寫／擷取文字，不是原始終端輸出的日誌歸檔。

`AsyncCommandCallback::new` 為擷取狀態的非同步閉包明確 boxed-future 回傳型別，仍將同一個回呼交給既有 `command_with`，不增加第二個執行器或額外回呼層。具名非同步函式繼續可用。`has_argument`／`has_option` 檢查輸入定義，已宣告但未賦值的輸入仍存在；`input()` 提供型別化輸入，`input_value` 支援缺失時的回退值。

需要控制單則訊息的詳細程度或直接寫入樣式輸出時，使用下方介面。互動問答、表格和進度條不屬於這些訊息方法。

## 輸出樣式與詳細程度 {#output-style-verbosity}

`get_output()` 回傳真正的 `console::OutputStyle`，它的 `get_output()` 借用底層 `Output`。`write`、`writeln` 和 `new_line` 共用原格式器和輸出串流。訊息可傳字串或字串切片；`writeln` 為每則訊息追加換行。`OutputFormat::Raw` 保留標籤，`Plain` 先格式化再移除剩餘標記。需要組合格式與詳細程度時，傳入 `OutputOptions`。 `write_iter`／`writeln_iter` 接受字串迭代器，包括擁有的字串和單次遍歷迭代器，保留逐則寫入與計數規則。

預設訊息門檻為 `Normal`。使用 `_with` 方法指定門檻，原單參數方法維持預設行為；`line_with` 還接受可選樣式。`Verbose`、`VeryVerbose`、`Debug` 分別需要 `-v`、`-vv`、`-vvv`；`Quiet` 在 `--quiet` 下仍顯示，`--silent` 隱藏全部訊息。`OutputVerbosity::from_name` 映射 `v`、`vv`、`vvv`、`quiet`、`normal`，未知名稱按 `Normal` 處理。

`new_lines_written` 初始為一，`new_line_written` 初始為 false，記錄經此樣式物件寫入的換行；直接編輯緩衝或原始寫入不會更新它們。隱藏的 `write` 與 `writeln` 計數規則不同，應讀取計數，不能只從擷取文字推斷。`is_quiet`、`is_verbose`、`is_very_verbose`、`is_debug` 查詢目前輸出狀態。`set_input` 替換命令讀取的輸入，`set_output` 接受借用替代串流的真正 `OutputStyle`；Rust 借用保證替代輸入和輸出仍然存活。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputFormat, OutputVerbosity},
    support::facades::Artisan,
};

pub fn register_styled() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:styled", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        io.info_with("Details", OutputVerbosity::Verbose)?;
        io.warn_with("Essential", OutputVerbosity::Quiet)?;
        io.line_with("Summary", Some("info"), None)?;
        let style = io.get_output();
        style.write(&["<raw>", "text"], false, OutputFormat::Raw)?;
        style.new_line(1)?;
        Ok(0)
    })?;
    Ok(())
}
```

## 互動問答 {#interactive-questions}

透過非同步命令 I/O 讀取文字、確認、隱藏文字和補全。`confirm` 預設 false；`OutputStyle::confirm` 預設 true。

```rust
use openrust::{
    console::{AsyncCommandCallback, CompletionCallback, ConsoleError},
    support::facades::Artisan,
};

fn register_questions() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:questions", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let name = io.ask_with("Name", Some("Guest")).await?;
            let proceed = io.confirm("Continue").await?;
            let token = io.secret("Token").await?;
            let color = io.anticipate("Color", CompletionCallback::values(vec!["blue".into(), "green".into()]), Some("blue")).await?;
            io.line(&serde_json::json!({"name": name, "proceed": proceed, "token_present": token.is_some(), "color": color}).to_string())?;
            Ok(0)
        })
    }))?.purpose("Collect report options")
}
```

`--no-interaction`、`--quiet` 和 `--silent` 關閉問答及缺失必填參數提示；缺少必填參數仍失敗。HTTP 擷取呼叫不會讀取程序標準輸入。輸入來源耗盡時回傳預設值，沒有預設值則回傳具體錯誤，並關閉該輸入後續互動。

需要自訂驗證時，建立 `Question`，透過 `set_normalizer`、`set_validator` 設定 `QuestionCallback`，再呼叫 `get_output().ask_question(&question).await`。驗證器可回傳結構化 JSON；`ask_with_validator` 和 `ask_hidden_with` 保留該回傳值。非互動預設值只執行驗證器，不執行正規化器。`set_max_attempts`、`set_trimmable`、`set_multiline` 和 `set_timeout` 控制重試和讀取。

`Input::set_stream` 接收包裝非同步緩衝讀取器的 `ConsoleInputStream`。輸入複製共用串流位置，但獨立保存互動開關；向 Kernel 明確傳入 Input 時共用本次呼叫狀態。補全回呼在目前應用程式作用域執行，支援 await。`CompletionCallback::iter` 延遲快取單次迭代器，`pairs` 按順序提供字串鍵及值。Ctrl+C 回傳 130；取消、逾時和驗證失敗均還原終端模式。Windows 終端實測仍歸發布矩陣。

## 選擇題與多選 {#choice-questions}

`choice` 接收選項清單，不設定預設值。使用 `choice_with` 指定預設值、最大嘗試次數及多選開關。多選輸入用逗號分隔索引或選項值，結果為陣列。`choice_pairs_with` 保留宣告順序並回傳關聯鍵；規範的整數字串按數字鍵處理，其他字串保持原寫法。

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError},
    support::facades::Artisan,
};

fn register_choices() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:choices", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let channel = io.choice_with("Channel", ["email", "sms"], Some(serde_json::json!(0)), Some(2), false).await?;
            let formats = io.choice_with("Formats", ["csv", "json"], Some(serde_json::json!("0,1")), Some(2), true).await?;
            let priority = io.choice_pairs_with("Priority", [("normal", "Normal"), ("urgent", "Urgent")], Some(serde_json::json!("normal")), Some(2), false).await?;
            io.line(&serde_json::json!({"channel": channel, "formats": formats, "priority": priority}).to_string())?;
            Ok(0)
        })
    }))?.purpose("Collect report choices")
}
```

文字、數字和布林選項重用既有純量比較規則。重複且有歧義的標籤需要改用鍵選擇。無預設值的問題在非互動模式回傳 null；非法預設值仍驗證失敗。`OutputStyle::choice_with` 額外將預設選項值解析為鍵；命令 I/O 直接使用傳入的預設值。

自訂提示或錯誤文案時，建立 `ChoiceQuestion::new` 或 `from_pairs`，使用 `set_prompt`、`set_error_message`（只有一個回答參數，支援 `%s`、可重複的 `%1$s`、寬度／精度及字面值 `%%`）、`set_multiselect`，再將 `as_question()` 傳給 `ask_question`。`get_choices`、`get_prompt` 和 `is_multiselect` 讀取實際定義。修改去空白開關後，已取出的預設驗證器也跟隨更新。原生 Question 複本獨立保存設定；明確回呼及補全快取控制代碼仍共用。樣式標籤應圍繞完整字素；格式化可見文字按 NFC 和字素邊界處理，Raw 輸出、超連結目標及回傳鍵保持原資料。


回呼中的執行錯誤使用 `ConsoleError::from_runtime_source`：立即終止重試，同時保留原錯誤型別與來源鏈。一般驗證錯誤仍可重試；輸入缺失使用已驗證的預設值並關閉後續互動。終端在執行正規化器或驗證器之前還原。範本產生非 UTF-8 位元組時回傳含原始來源的 `ConsoleQuestionError::InvalidEncoding`，不將任意 PHP 二進位字串當作 Rust 文字回傳。

應用程式標籤型別實作 `Display` 後，可用 `ChoiceLabel(value)` 傳入選項。原生 JSON 回答模型保存標籤文字快照，不攜帶 PHP 物件身分；關聯選擇仍回傳原宣告鍵，應用程式物件保留在呼叫端集合中。

## 表格 {#tables}

使用 `input.io(output).table(headers, rows)` 呈現表格。表頭和資料列接受迭代器；JSON 純量儲存格沿用共用純量轉換，不額外加入 JSON 引號。模型資料先依需要的順序選取欄位，再將值傳入表格。

```rust
use openrust::{
    console::{ConsoleError, Input, Output},
    support::facades::Artisan,
};

fn register_tables() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:table", |input: &Input, output: &mut Output| {
        let users = serde_json::json!([
            {"name": "Ada", "email": "ada@example.test"},
            {"name": "Li", "email": "li@example.test"}
        ]);
        let rows = users.as_array().unwrap().iter().map(|user| {
            [user["name"].clone(), user["email"].clone()]
        });
        input.io(output).table(["Name", "Email"], rows)?;
        Ok(0)
    })?;
    Ok(())
}
```

`table_with` 選擇 `TableStyle`；`table_with_columns` 另外接受 `(欄位索引, 樣式)`。`TableStyle::from_name` 提供 `default`、`markdown`、`borderless`、`compact`、`symfony-style-guide`、`box` 和 `box-double`。可透過邊框、填充及格式設定方法自訂樣式。`TablePadding::{Left, Right, Both}` 表示填充所在側，預設 `Right`。

直接呈現時使用 `Table::new(io.get_output())`，設定表頭、資料列和欄位樣式後呼叫 `render()`。`TableCell::display(value)` 明確適配實作 `Display` 的業務型別；JSON 值和常用純量直接轉換。多行儲存格、Unicode 可見寬度、格式標籤、ANSI 輸出及 quiet／silent 過濾沿用原輸出管線。此矩形表格介面不代表已支援跨列／跨欄、列分隔符號、標題或整個繼承輔助庫。

## 進度條 {#progress-bars}

可計數的集合使用 `with_progress_bar_iter`。回呼接收元素、目前進度條及原整數／字串鍵；回呼成功後才前進一步，最後傳回原集合。`Vec`、陣列、有序映射、基礎集合、模型集合及其借用均實作 `ProgressIterable`，無需複製模型。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, ProgressBar, ProgressFormatter},
    support::facades::Artisan,
};
use std::sync::Arc;

fn register_progress() -> Result<(), ConsoleError> {
    ProgressBar::set_format_definition(
        "reports",
        "%task% %current%/%max% [%bar%] %percent:3s%%",
    )?;
    Artisan::command_with("reports:progress", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut processed = Vec::new();
        let items = io.with_progress_bar_iter(vec!["csv", "json", "html"], |item, _, _key| {
            processed.push(item.to_uppercase());
            Ok(())
        })?;
        io.new_line(1)?;

        let count = i64::try_from(items.len()).map_err(ConsoleError::from_source)?;
        let mut bar = io.get_output().create_progress_bar(count)?;
        let label: ProgressFormatter = Arc::new(|bar| {
            Ok(bar.get_message().unwrap_or("work").to_owned())
        });
        bar.set_format("reports")
            .set_message("build")
            .set_placeholder_formatter("task", label);
        bar.start()?;
        let mut total = 0;
        for item in &items {
            total += item.len();
            bar.advance()?;
        }
        bar.finish()?;
        drop(bar);
        io.new_line(1)?;
        io.info(&format!("{}:{total}", processed.join(",")))?;
        Ok(0)
    })?;
    Ok(())
}
```

數字總步數使用 `with_progress_bar(steps, callback)`，由回呼呼叫 `advance` 或 `advance_by`。手動控制時透過 `create_progress_bar(total)` 建立，再呼叫 `start`、`advance` 和 `finish`。這些回呼同步執行；錯誤向上傳遞，不會悄悄補上完成狀態。

工廠沿用目前輸出：擷取呼叫將進度保留在緩衝中；實際主控台將進度串流寫入 stderr，普通 stdout 保持獨立。quiet／silent 關閉顯示。預設重新整理間隔為40毫秒，完成步仍會檢查顯示。空集合、未知總數及重複 `finish` 保留各自生命週期。`progress_start`、`progress_advance` 和 `progress_finish` 提供樣式內保存的進度狀態；`progress_finish` 另外寫入兩個換行並釋放該狀態。

可設定格式、訊息、進度字元、寬度、重新整理頻率及佔位符。共用命名格式與佔位符定義屬於目前應用程式，在命令註冊或執行作用域內設定；進度條保留建立時所屬應用程式的定義。獨立進度條可直接使用字面格式與實例佔位符。佔位符回呼借用實際進度條，傳回 `Result<String, ConsoleError>`。記憶體佔位符顯示原生程序駐留記憶體，不是語言虛擬機堆積大小。

數字與集合輔助方法分開命名，使回呼及傳回型別明確。傳入 `&items` 可在失敗時保留呼叫端所有權。本節驗收同步集合與手動進度；惰性迭代器適配、非同步／可變任務回呼及其餘繼承輸出契約繼續分別核對。

## 惰性進度迭代 {#lazy-progress-iteration}

使用 `OutputStyle::progress_iterate(items)` 逐項處理任務。首次 `next()` 才啟動，取下一項時才推進，自然耗盡後才完成。每項傳回 `Result`，用 `?` 傳播輸出錯誤。提前 `break`、處理失敗或丟棄迭代器，不補寫完成影格及樣式末尾的兩個換行。

```rust
use openrust::{
    console::{ConsoleError, Input, Output},
    support::facades::Artisan,
};

fn register_progress_iteration() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:iterate", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut sum = 0;
        for number in io.get_output().progress_iterate([2, 4, 6]) {
            sum += number?;
        }
        io.info(&format!("sum:{sum}"))?;
        Ok(0)
    })?;
    Ok(())
}
```

手動進度條提供 `iterate(items)` 和 `iterate_with_max(items, max)`，可在項之間透過 `iterator.progress_bar()` 存取同一條進度。樣式入口另有 `progress_iterate_with_max`。`None` 與 `Some(0)` 從迭代器精確長度提示推導總數；無精確提示時保持未知，或保留手動進度條既有總數。明確正總數覆蓋推導。元素、元組鍵及借用模型身分保持不變，轉成底層迭代器的動作也延後至首次取值。精確空迭代器直接完成，不繪製啟動影格。非同步和可變回呼輔助方法見下節；這些入口不會阻塞執行非同步任務或將任務放到背景。

## 非同步與原地處理回呼 {#async-progress-callbacks}

數字總數使用 `with_progress_bar_async`，唯讀集合使用 `with_progress_bar_iter_async`，明確原地修改使用 `with_progress_bar_iter_mut_async`。它們複用同一進度條和命令執行管線，等待每個裝箱 Future 完成後才推進。

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError},
    support::facades::Artisan,
};

fn register_progress_async() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:async", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            io.with_progress_bar_async(1, |bar| Box::pin(async move {
                tokio::task::yield_now().await;
                bar.advance()?;
                Ok(())
            })).await?;
            io.new_line(1)?;

            let mut values = vec![1, 2, 3];
            io.with_progress_bar_iter_mut_async(&mut values, |value, _, _key| {
                Box::pin(async move {
                    tokio::task::yield_now().await;
                    *value *= 2;
                    Ok(())
                })
            }).await?;
            io.new_line(1)?;
            io.info(&format!("values:{}", values.iter().sum::<i32>()))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`ProgressIterableMut` 支援切片、陣列、向量、有序映射、基礎集合、模型集合及可變借用，保留鍵與模型身分。可變模型回呼可等待 `model.save()`，並用 `ConsoleError::from_source` 傳播具體錯誤。同步原地處理另有 `with_progress_bar_iter_mut`。原地修改屬於明確原生行為，預設回呼保持唯讀；輔助方法不隱式開啟交易或回復已完成的寫入。錯誤和取消均不補完成影格；傳入可變借用保留呼叫方所有權及已發生的修改。

## 占位符輸出存取 {#progress-placeholder-output}

在 `ProgressFormatter` 中透過 `bar.get_output()` 借用實際 `ProgressOutput` 輸出流。它提供 `write`、`writeln`、單次消費的迭代器變體、詳細程度查詢及 `is_decorated`；`OutputOptions` 控制格式和訊息層級。真實 CLI 寫入 stderr，擷取呼叫繼續使用原緩衝。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputFormat, ProgressFormatter},
    support::facades::Artisan,
};
use std::sync::Arc;

fn register_progress_output() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:progress-output", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut bar = io.get_output().create_progress_bar(2)?;
        let label: ProgressFormatter = Arc::new(|bar| {
            let current = bar.get_progress();
            bar.get_output().writeln(format!("step:{current}").as_str(), OutputFormat::Raw)?;
            Ok(format!("stage:{current}"))
        });
        bar.set_format("%stage%")
            .set_placeholder_formatter("stage", label);
        bar.start()?;
        bar.advance_by(2)?;
        bar.finish()?;
        Ok(0)
    })?;
    Ok(())
}
```

回呼接收進度條，輸出僅在使用期間借出，避免同時取得進度條及其內部輸出的重疊可變參照。先讀取所需進度狀態，再借用輸出，借用結束後可繼續操作進度條。寫入複用既有格式器與錯誤鏈，不更新外層樣式換行計數。占位符在排版時可能重複求值，業務副作用應放在回呼之外。本視圖不代表完整繼承輸出 SDK；格式器替換和輸出設定設置器仍為單獨入口。

## 輸出設定 {#output-configuration}

`Output` 與 `OutputStyle` 提供 `set_verbosity`、`set_decorated`；樣式另有 `get_verbosity`、`is_decorated` 和 `is_silent`，操作實際後端。後續改成 Quiet 或 Silent 不會刪除先前已接受的訊息。執行期設置器可以明確覆蓋 CLI 層級；範例先尊重呼叫方最初的 Quiet／Silent。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputOptions, OutputVerbosity},
    support::facades::Artisan,
};

fn register_output_configuration() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:output-config", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let style = io.get_output();
        let previous = style.get_verbosity();
        if previous <= OutputVerbosity::Quiet {
            return Ok(0);
        }
        style.set_verbosity(OutputVerbosity::Verbose);
        style.write("details;", false, OutputVerbosity::Verbose)?;
        style.set_verbosity(OutputVerbosity::Quiet);
        style.write("quiet;", false, OutputVerbosity::Quiet)?;
        style.set_verbosity(previous);
        let decorated = style.is_decorated();
        style.set_decorated(false);
        style.write("<info>plain</info>", false, OutputOptions::default())?;
        style.set_decorated(decorated);
        Ok(0)
    })?;
    Ok(())
}
```

進度輸出視圖也有 `set_verbosity` 和 `set_decorated`。真實主控台僅設定 stderr；主樣式的設置器同時更新兩條流。擷取呼叫的單一輸出共用設定。進度排版讀取所選流，巢狀命令的明確 CLI 旗標暫時作用於兩條流；既有命令作用域在結束或取消時還原父級設定。原生直接修改字串緩衝仍受命令過濾約束，按層級寫入應使用輸出方法。自訂格式器替換為單獨的 SDK 入口。
