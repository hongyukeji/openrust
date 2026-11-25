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

直接呈現時使用 `Table::new(io.get_output())?`，設定表頭、資料列和欄位樣式後呼叫 `render()`。`TableCell::display(value)` 明確適配實作 `Display` 的業務型別；JSON 值和常用純量直接轉換。多行儲存格、Unicode 可見寬度、格式標籤、ANSI 輸出及 quiet／silent 過濾沿用原輸出管線。此矩形表格介面不代表已支援整個繼承輔助庫。

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

回呼接收進度條，輸出僅在使用期間借出，避免同時取得進度條及其內部輸出的重疊可變參照。先讀取所需進度狀態，再借用輸出，借用結束後可繼續操作進度條。寫入複用既有格式器與錯誤鏈，不更新外層樣式換行計數。占位符在排版時可能重複求值，業務副作用應放在回呼之外。本視圖不代表完整繼承輸出 SDK；格式器替換和輸出設定見下文對應章節。

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

進度輸出視圖也有 `set_verbosity` 和 `set_decorated`。真實主控台僅設定 stderr；主樣式的設置器同時更新兩條流。擷取呼叫的單一輸出共用設定。進度排版讀取所選流，巢狀命令的明確 CLI 旗標暫時作用於兩條流；既有命令作用域在結束或取消時還原父級設定。原生直接修改字串緩衝仍受命令過濾約束，按層級寫入應使用輸出方法。明確替換後的共用規則見「輸出格式器」。

## 樣式與樣式堆疊 {#formatter-styles}

低層 `OutputFormatterStyle` 提供前景色、背景色、選項及超連結設定，並實作公開的 `OutputFormatterStyleInterface`；`FormatterStyle` 是共用 Trait 物件控制代碼。`apply` 產生 ANSI 文字，不讀取輸出裝飾開關。只有明確要寫入預先繪製控制序列時才使用 Raw。

```rust
use openrust::{
    console::{ConsoleError, FormatterStyle, Input, Output, OutputFormat,
        OutputFormatterStyle, OutputFormatterStyleStack},
    contracts::service::ResetInterface,
    support::facades::Artisan,
};
use std::sync::Arc;

fn register_formatter_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:styles", |input: &Input, output: &mut Output| {
        let style = Arc::new(OutputFormatterStyle::new(Some("green"), None, &["bold"])?);
        let shared: FormatterStyle = style.clone();
        let mut stack = OutputFormatterStyleStack::default();
        stack.push(shared.clone());
        shared.set_foreground(Some("blue"))?;
        let rendered = stack.get_current().apply("ready")?;
        let mut io = input.io(output);
        io.get_output().write(rendered.as_str(), false, OutputFormat::Raw)?;
        let reset: &mut dyn ResetInterface = &mut stack;
        reset.reset();
        io.new_line(1)?;
        io.info(&format!("stack:{}", stack.get_current().apply("plain")?))?;
        Ok(0)
    })?;
    Ok(())
}
```

複製控制代碼保留同一個物件，複製具體樣式得到獨立設定。`OutputFormatterStyleStack` 保留樣式參照，支援一般彈出和按樣式比對彈出，複製堆疊仍共用原樣式。`reset` 清空項目並保留設定的空樣式，也可透過 `contracts::service::ResetInterface` 呼叫。比對比較實際空文字樣式輸出，包含超連結行為。

設置器傳回型別化錯誤；顯示可繼續使用先前已編譯樣式，後續修改會重新驗證最新欄位。用 `?` 傳播失敗，`ConsoleError` 保留具體來源。泛型迭代器及字串轉換在預設樣式加鎖前執行。這些型別複用既有格式化管線；具名樣式註冊和完整格式器替換見「輸出格式器」。

## 輸出格式器 {#output-formatters}

`OutputFormatter` 提供具名樣式註冊與查詢、跳脫、可選文字格式化及按字素換行。寬度為零關閉換行。未關閉的樣式狀態會保留至下次呼叫；`get_style_stack()` 傳回同一個實際堆疊的控制代碼。複製具體格式器深複製具名樣式，堆疊內樣式仍保留參照。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputFormat, OutputFormatter,
        OutputFormatterStyle},
    support::facades::Artisan,
};
use std::sync::Arc;

fn register_formatter() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:format", |input: &Input, output: &mut Output| {
        let formatter = Arc::new(OutputFormatter::new(false));
        formatter.set_style("report", Arc::new(OutputFormatterStyle::new(
            Some("green"), None, &["bold"],
        )?));
        let wrapped = formatter.format_and_wrap(Some("one two three"), 7)?;
        let mut io = input.io(output);
        let style = io.get_output();
        style.set_formatter(formatter);
        style.write("<report>ready</report>", true, OutputFormat::Normal)?;
        style.write(wrapped.as_str(), true, OutputFormat::Raw)?;
        style.get_formatter().set_decorated(true);
        style.write("<report>done</report>", true, OutputFormat::Normal)?;
        Ok(0)
    })?;
    Ok(())
}
```

`OutputFormatterInterface` 支援自訂實作及可選輸出；傳回 `None` 不寫文字。實作 `WrappableOutputFormatterInterface` 並由 `as_wrappable` 傳回，便可透過型別擦除控制代碼存取換行能力；這個原生能力方法避免不安全的 Trait 物件轉換。用 `?` 傳播錯誤，保留具體來源。

`Output`、`OutputStyle` 及 `ProgressOutput` 均提供 `get_formatter`、`set_formatter`。複製傳回的 Arc 共用同一物件。真實主控台明確替換主格式器時，stderr 亦參照該物件；隨後透過任一控制代碼修改裝飾會影響兩條流。未替換時，預設格式器彼此獨立。只替換進度輸出則僅影響 stderr；擷取命令共用單一串流。Raw 繞過格式器，進度游標控制序列亦使用 Raw。

巢狀 CLI 旗標在原輸出物件仍安裝時還原裝飾；明確替換則保留新格式器設定，不用舊物件旗標覆蓋。自訂樣式回呼可安全註冊樣式、重設同一堆疊或進行有限的巢狀格式化。格式器呼叫是同步的，排序鎖期間不執行非同步工作。

## 色彩與終端能力 {#colors-terminal}

`Color::new(foreground, background, options)` 編譯 ANSI 序列；`set`、`unset` 傳回起止序列，`apply` 與文字組合。`Color::default()` 為空樣式。這些低層方法不讀取輸出裝飾旗標；用 Raw 寫入預先繪製文字前應尊重實際輸出開關。無效色彩和選項傳回型別化錯誤。

```rust
use openrust::{
    console::{Color, ConsoleError, Input, Output, OutputFormat, Terminal},
    support::facades::Artisan,
};

fn register_colors() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:colors", |input: &Input, output: &mut Output| {
        let color = Color::new("green", "", &["bold"])?;
        let terminal = Terminal::new();
        let mut io = input.io(output);
        let style = io.get_output();
        let text = if style.is_decorated() {
            color.apply("ready")
        } else {
            "ready".to_owned()
        };
        style.write(text.as_str(), true, OutputFormat::Raw)?;
        style.write(
            format!("terminal:{}x{}", terminal.get_width(), terminal.get_height()).as_str(),
            true, OutputFormat::Raw,
        )?;
        Ok(0)
    })?;
    Ok(())
}
```

`AnsiColorMode::{Ansi4, Ansi8, Ansi24}` 提供 `convert_from_hex_to_ansi_color_code`。`Terminal::get_color_mode()` 優先辨識 COLORTERM，再辨識 TERM，並快取首次結果；預設 `Terminal::DEFAULT_COLOR_MODE` 為 Ansi4。`Terminal::set_color_mode(Some(mode))` 強制處理程序層級模式，傳 `None` 重設辨識。應在啟動時設定，不要按請求切換。已有 Color 物件保留已編譯序列，後續建立物件或重建樣式時使用目前模式。

`Terminal::get_width`、`get_height` 每次讀取 COLUMNS、LINES，包括零、負數與數值前綴。沒有覆蓋值時快取原生裝置尺寸，缺失則分別回退至 80 欄、50 列。Unix 直接讀取標準輸入終端裝置，即使 stdout、stderr 被重新導向亦可取得；Windows 優先使用設定的 ANSICON，否則讀取原生主控台。`has_stty_available()` 單獨快取 stty 查詢結果，一般尺寸讀取不啟動外部命令。Windows 實際執行仍歸平台發行驗收。

## 表格欄寬與標題 {#table-layout-controls}

用 `set_column_width` 設定單欄最小寬度，`set_column_widths` 設定連續欄列表；批次設定會替換先前的最小寬度。稀疏欄可用 `set_column_widths_with_keys` 傳入索引對。`set_column_max_width` 會立即檢查目前格式器的換行能力，不支援時傳回錯誤；內建格式器已支援。最大寬度優先於最小寬度。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, Table},
    support::facades::Artisan,
};

fn register_layout() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:layout", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut table = Table::new(io.get_output())?;
        table.set_headers(["Name", "Count"])
            .set_rows([["longer name", "42"]])
            .set_column_widths([12, 8])
            .set_header_title(Some("Report"))
            .set_footer_title(Some("done"));
        table.set_column_max_width(0, 8)?;
        table.get_style_mut().set_header_title_format("[%s]");
        table.render()?;
        Ok(0)
    })?;
    Ok(())
}
```

表頭／表尾標題使用 `set_header_title`、`set_footer_title`，傳 `None` 清除。表頭將空文字與 "0" 視為無標題，明確空表尾仍使用標題格式。`get_style_mut`、`get_column_style_mut` 提供可變存取，同時保留原來的唯讀 getter。未設定的欄回退至表格本身的實際樣式。

寬度保留帶符號值，非正數按繪製規則處理，不默默替換為預設值。最大寬度為零不換行；不相容的有效寬度可能在先前已有輸出後才於繪製時失敗，應傳播 `render` 錯誤。換行複用目前的 `WrappableOutputFormatterInterface`，亦支援自訂實作。標題按可見寬度與格式置中、截斷。具名樣式、複雜列和合併儲存格見後續對應章節。

表格僅在換行結果含 LF 時採用它來拆列；否則保留原儲存格，原始換行亦保持原樣，不展開成表格列。自訂換行介面不是一般儲存格內容轉換掛鉤。

## 表格列與版面方向 {#table-rows-orientation}

`add_row`、`add_rows` 傳回 `Result`，用 `?` 傳播數字索引耗盡錯誤，不觸發溢位 panic；先前成功附加的列保留。`set_row` 按整數或字串索引鍵替換列，保留插入順序；新索引鍵附加。標準數字字串共用整數索引鍵，負數索引鍵影響後續數字索引，`set_rows` 同時重設列與索引。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, Table},
    support::facades::Artisan,
};

fn register_rows() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:rows", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut table = Table::new(io.get_output())?;
        table.set_headers(["Name", "Value"])
            .set_rows([["alpha", "1"]]);
        table.add_rows([["beta", "2"]])?;
        table.set_row(0_i64, ["changed", "1"])
            .set_vertical(true);
        table.render()?;
        Ok(0)
    })?;
    Ok(())
}
```

`set_headers_rows` 接收多列表頭，原 `set_headers` 保留單列形式。`set_horizontal(true)` 將第一列表頭轉成各列首部，`set_vertical(true)` 用同一列表頭產生欄位標籤並在記錄間繪製分隔。兩個設置器傳 false 都還原預設版面，不還原先前方向。每次繪製從保存的列派生版面，並保留欄寬與標題設定。具名樣式註冊、明確分隔符物件、合併儲存格與主控台區段另行追蹤。

## 命名表格樣式 {#table-style-definitions}

在目前應用程式作用域內使用 `Table::set_style_definition` 和 `get_style_definition`。每個應用程式獨立保存內建及自訂定義，在同一應用程式內跨命令及請求保留。表格始終使用建立它的應用程式定義，不隨外層作用域切換；獨立表格具有自己的內建定義，應用程式外修改命名定義會回傳錯誤。

傳入 `&style` 共用同一個可變樣式物件，`style.clone()` 明確複製設定。修改共用定義也影響已使用它的表格；同名替換只影響後續查找，現有表格繼續持有原物件。`TableStyle::from_name` 仍回傳獨立的內建預設副本。

`Table::new` 回傳 `Result`，傳遞應用程式服務解析失敗。樣式文字及陣列 getter 回傳擁有所有權的快照，避免共用修改使借用失效。使用 `TableStyle::from(table.get_style())` 或欄位 getter 保留共用控制代碼；格式化回呼在樣式鎖外執行。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, Table, TableStyle},
    support::facades::Artisan,
};

fn register_table_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:table-styles", |input: &Input, output: &mut Output| {
        let mut style = TableStyle::from_name("box")?;
        Table::set_style_definition("report", &style)?;
        let mut io = input.io(output);
        let mut table = Table::new(io.get_output())?;
        table.set_headers(["A", "B"]).set_rows([["one", "two"]]);
        table.set_style_named("report")?;
        style.set_horizontal_border_chars("!", None);
        table.render()?;
        Table::set_style_definition("report", TableStyle::from_name("compact")?)?;
        table.render()?;
        table.set_style_named("report")?.render()?;
        Ok(0)
    })?;
    Ok(())
}
```

`set_style_named` 和 `set_column_style_named` 按名稱解析，未定義名稱回傳錯誤；原型別 setter 也可接收借用樣式。便利方法 `table` 使用應用程式目前預設定義，`table_named` 和 `table_with_named_columns` 支援命名表格與欄位樣式。分隔儲存格與跨度設定見[合併儲存格](#table-cell-spans)。

## 明確整列分隔線 {#table-row-separators}

同時包含資料列與整列分隔線的集合使用 `TableRow`。`TableSeparator::new()` 表示分隔線，空資料陣列仍是空列，兩者不混用。一般陣列和迭代器繼續自動轉換為資料列。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, TableRow, TableSeparator},
    support::facades::Artisan,
};

fn register_separators() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:separators", |input: &Input, output: &mut Output| {
        let rows = [
            TableRow::from(["one", "two"]),
            TableSeparator::new().into(),
            TableRow::from(["three"]),
        ];
        input.io(output).table_named(["A", "B"], rows, "box-double")?;
        Ok(0)
    })?;
    Ok(())
}
```

`set_rows`、`add_rows`、`add_row` 和表格便利方法接收這類異質列。`add_row`／`add_rows` 繼續用 `?` 傳遞索引耗盡；`set_row` 可將同鍵分隔線替換為資料陣列。開頭、末尾和連續分隔線保留宣告順序。橫向與縱向配置略過明確整列分隔線，縱向配置仍自行產生記錄間分隔。儲存格分隔選項與跨度設定見[合併儲存格](#table-cell-spans)。

## 儲存格樣式與內部分隔 {#table-cell-styles}

使用選項鍵值對建立 `TableCellStyle`，或用 `Default` 取得 `fg=default`、`bg=default`、無文字選項、靠左對齊和無自訂格式。支援鍵為 `fg`、`bg`、`options`、`align` 和 `cellFormat`，最後一項保留設定拼寫。`align` 接收 `left`、`center` 或 `right`；未知鍵和無效的非空對齊值在建構時失敗。

`get_options`、`get_tag_options` 回傳獨立的對映，空值標籤選項被略去。`get_pad_by_align`、`get_cell_format` 回傳 `Result`，保留明確空對齊與非法格式型別延遲到讀取時失敗的行為；格式的一般純量沿用既有字串轉換。`DEFAULT_ALIGN` 為 `left`。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, TableCell, TableCellStyle, TableSeparator},
    support::facades::Artisan,
};

fn register_cell_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:cell-styles", |input: &Input, output: &mut Output| {
        let style = TableCellStyle::new([
            ("fg", serde_json::json!("green")),
            ("align", serde_json::json!("right")),
        ])?;
        let rows = vec![
            vec![TableCell::new("Ada").with_style(style), "42".into()],
            vec![TableSeparator::new().into(), TableSeparator::new().into()],
            vec!["Li".into(), "7".into()],
        ];
        input.io(output).table_named(["Name", "Score"], rows, "box-double")?;
        Ok(0)
    })?;
    Ok(())
}
```

用 `TableCell::with_style` 附加不可變樣式，`get_style` 讀取該樣式。已有完整格式標籤時保留其文字樣式，儲存格對齊仍生效；其他情況用 `cellFormat` 覆蓋自動產生的顏色標籤。按寬度換行與多行展開會產生一般文字列，可能捨棄原儲存格樣式；橫向配置保留搬移的儲存格樣式，縱向配置將值轉換為文字。

將 `TableSeparator` 轉換為儲存格後，按該欄樣式繪製內部邊框，與整列 `TableRow::Separator` 不同。跨度與分隔建構選項見[合併儲存格](#table-cell-spans)。

每條實際輸出列在開始時讀取列文字格式。格式化回呼的修改會影響後續邊框與下一列，不會回改正在呈現的本列儲存格格式。

## 合併儲存格與建構選項 {#table-cell-spans}

使用 `with_colspan`、`with_rowspan` 合併儲存格。`get_colspan`、`get_rowspan` 重用既有純量轉換並回傳有符號整數，預設均為 `1`。建構選項保留傳入值，包含零、負值；配置無法使用這些值計算時傳遞錯誤。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, Table, TableCell},
    support::facades::Artisan,
};

fn register_spans() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:spans", |input: &Input, output: &mut Output| {
        let headers = vec![
            vec![TableCell::new("Summary").with_colspan(2)],
            vec!["Name".into(), "Score".into()],
        ];
        let rows = vec![
            vec![TableCell::new("Ada").with_rowspan(2), "42".into()],
            vec!["7".into()],
            vec![TableCell::new("Total").with_colspan(2)],
        ];
        let mut io = input.io(output);
        Table::new(io.get_output())?
            .set_headers_rows(headers)
            .set_rows(rows)
            .set_style_named("box-double")?
            .render()?;
        Ok(0)
    })?;
    Ok(())
}
```

`TableCellOptions` 彙集 `colspan`、`rowspan`、`style`，可傳給儲存格或分隔符號的 `with_options`。跨度欄位保存 JSON 值，`style` 為 `Option<TableCellStyle>`；未知欄位與冒充樣式物件的值由型別檢查拒絕。`TableCell::default()` 建立空儲存格，`TableCell::new` 包裝既有儲存格時重新使用預設選項。

`TableSeparator` 保留選項與繼承 getter，並支援 `Clone`；同一分隔符號放入多個擁有所有權的列時可複製使用。跨度參與多表頭、跨列填充、方向與欄寬換行。配置可能更新欄最小寬度，`set_column_widths([])` 清空這些最小值並保留最大寬度限制。分區重繪見下節。


## 主控台分區與表格更新 {#console-sections}

`Output::section()` 在目前輸出串流建立分區。`write`／`writeln`、`clear`、`overwrite`、`set_max_height`、`get_content` 和 `get_visible_content` 操作該分區。重寫前面的分區時，後續分區依序清除並恢復；最大高度用於捲動可見內容。不啟用裝飾時按普通輸出寫入，清除不執行重繪。

`ConsoleSectionOutput` 暫時借用原輸出。儲存 `handle()`，寫完其他分區後透過 `section_output(&handle)` 再次借用；控制代碼只屬於一個輸出，其他輸出會拒絕它。`style(input)` 把既有樣式／表格管線綁定到分區。顯式借用取代共用可變串流參照，不建立額外輸出串流。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputOptions, Table},
    support::facades::Artisan,
};

fn register_sections() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:sections", |_input: &Input, output: &mut Output| {
        let first = {
            let mut section = output.section();
            section.writeln("prefix", OutputOptions::default())?;
            section.handle()
        };
        {
            let mut section = output.section();
            section.writeln("other section", OutputOptions::default())?;
        }
        let mut section = output.section_output(&first)?;
        let mut table = Table::new(&mut section)?;
        table.set_headers(["A", "B"]).set_rows([["x", "y"]]);
        table.append_row(["one", "two"])?;
        table.append_row(["longer", "last"])?;
        Ok(0)
    })?;
    Ok(())
}
```

`append_row` 要求分區輸出；首次繪製後按計算列數清除表格區域，附加列並再次繪製，保留後續分區。普通輸出會在新增列之前傳回錯誤；只建立表格而不重繪請用 `add_row`。分區詳細程度獨立，初始格式器共用，亦可為該分區替換。所選進度與問答輸出見下節。


## 所選輸出、進度列與問答 {#section-consumers}

`OutputStyle::get_output()` 傳回攜帶目前輸出和分區身分的 `OutputReference`。`OutputStyle::new`、`ProgressBar::new` 和 `new_with_redraw_interval` 同時接受它及原來的 `&mut Output`，普通呼叫語法不變。顯式標註 `&mut Output` 參數的助手若要支援分區，應接受 `&mut impl AsOutput`。該借用允許讀取原擷取緩衝，設定和寫入轉送到所選輸出，不能透過可變解參照繞過分區。

進度列清除／重繪使用分區列數，並恢復後續分區。`bar.get_output()` 為回呼寫入和設定提供同一所選串流；`as_section()` 提供分區操作，普通輸出則傳回錯誤。這種順序借用取代在同一閉包中同時擷取可變串流與進度列。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputOptions, OutputStyle, ProgressBar},
    support::facades::Artisan,
};

fn register_section_progress() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:section-progress", |input: &Input, output: &mut Output| {
        let first = {
            let mut section = output.section();
            section.writeln("prefix", OutputOptions::default())?;
            section.handle()
        };
        {
            let mut section = output.section();
            section.writeln("later", OutputOptions::default())?;
        }
        let mut section = output.section_output(&first)?;
        let mut style = OutputStyle::new(input, &mut section);
        let mut bar = ProgressBar::new(style.get_output(), 3)?;
        bar.set_format("<info>%current%/%max%</info>")
            .min_seconds_between_redraws(0.0)
            .max_seconds_between_redraws(0.0)
            .set_redraw_frequency(Some(1));
        bar.start()?;
        bar.advance()?;
        bar.display()?;
        bar.get_output().writeln("notice", OutputOptions::default())?;
        bar.advance()?;
        bar.clear()?;
        bar.display()?;
        bar.finish()?;
        Ok(0)
    })?;
    Ok(())
}
```

問答使用所選格式器，在隨後空列之前計入提交輸入的換列；驗證重試只在問題開始前準備一次空列，不在每次重試重複執行。普通終端編輯直接回顯輸入，不把回顯再次儲存到分區內容。已驗證串流輸入問答、確認、隱藏輸入、重試、取消和普通互動 PTY；補全和多行行為見下節。


## 分區補全與多行輸入 {#section-editors}

補全開始時沒有選中候選。初次按上／下方向鍵選擇最後／第一個候選，輸入字元會篩選候選，Tab 接受目前候選，Enter 接受已選候選或輸入值／預設值。候選可見時，第一次退格移除候選，下一次才刪除已輸入字元。單選和多選使用同一編輯器，支援逗號分隔候選。

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError, OutputOptions, Question},
    support::facades::Artisan,
};

fn register_editor() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:editor", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut section = output.section();
            let mut style = section.style(input);
            let mut question = Question::new("Name", Some("Guest".into()))?;
            question.set_autocompleter_values(Some(vec![
                "Ada".into(), "Adam".into(), "Beta".into(),
            ]))?;
            let answer = style.ask_question(&question).await?;
            style.writeln(
                format!("answer={}", answer.as_str().unwrap_or_default()).as_str(),
                OutputOptions::default(),
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`Question::set_multiline(true)` 讀取到原生 EOF 鍵，保留中間換行；開始時直接按空 Enter 使用預設值。同時啟用補全和多行時，補全優先，Enter 完成本次回答。普通多行回顯使用原生非同步編輯器，而非作業系統規範模式回顯；終端重繪位元組存在差異，答案與分區記帳一致。

Ctrl+C 維持結束碼 `130`，在命令處理完成前恢復終端模式。取消和逾時沿用既有問答錯誤／預設值規則。Windows 終端實機驗收仍屬於發佈檢查。

## 巢狀輸出樣式 {#nested-output-styles}

`OutputStyle::new(input, &mut parent)` 包裝現有樣式，寫入與設定仍轉送至同一輸出，同時保留父層的換行與問答緩衝狀態。助手透過 `console::contracts::NewLineAware` 讀取實際換行契約。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputOptions, OutputStyle,
    contracts::NewLineAware,
};
use openrust::support::facades::Artisan;

fn register_nested() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:nested", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut first = OutputStyle::new(input, output);
            first.new_line(1)?;
            {
                let mut second = OutputStyle::new(input, &mut first);
                second.write("A", false, OutputOptions::default())?;
                second.new_line(2)?;
                let inner = second.get_output().as_new_line_aware().unwrap();
                assert_eq!(NewLineAware::new_lines_written(&inner), 2);
                second.writeln("B", OutputOptions::default())?;
            }
            let count = NewLineAware::new_lines_written(&first);
            first.writeln(format!("count={count}").as_str(), OutputOptions::default())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`new_lines_written()` 委派至最底層被包裝的樣式；`new_line_written()` 也保留任一包裝層的真值。`new_line(n)` 向父層寫入換行文字，不呼叫父層的 `new_line(n)`，兩種操作的計數行為因此不同。

`get_output()` 借用目前直接選取的輸出。`OutputReference::as_new_line_aware()` 僅在該目標是被包裝樣式時回傳 `Some`，此時還能透過它的 `get_output()` 繼續借用底層輸出；一般串流沒有這兩種能力。Rust 獨占借用確保子層使用父層期間，不能同時從父層寫入。

被包裝樣式不等於直接的主控台分區：表格追加重繪會拒絕它，進度條使用一般游標輸出，也不會自動改走 stderr。需要分區專屬行為時直接使用分區。一般自訂輸出串流及其餘元件助手仍是獨立能力。

## 標題、文字與清單 {#style-text-layout}

`OutputStyle` 提供 `title`、`section`、`text` 與 `listing`。主標題使用 `=` 底線，分節標題使用 `-`，寬度依所選格式器與 Unicode 顯示寬度計算。文字縮排一個空格，清單使用 ` * ` 前綴；區塊之間的空行取決於先前的實際輸出。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle};
use openrust::support::facades::Artisan;

fn register_text_layout() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:text-layout", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            style.title("Hello")?;
            style.section("Part")?;
            style.text(&["one", "two"])?;
            style.listing(["A", "B"])?;
            style.title("")?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`text` 接受字串或借用的字串陣列，`listing` 接受字串迭代器。空陣列保留各助手自身的間距規則。行內格式標籤繼續生效，標題底線忽略裝飾，尾端反斜線正常顯示。無效格式回傳既有的型別化主控台錯誤，保留失敗前已經寫出的內容。

`OutputStyle::section("Part")` 輸出分節標題；`Output::section()` 建立可重繪輸出分區，兩者是獨立介面。這四個助手支援所選分區與借用的巢狀樣式，沿用同一格式器、詳細程度與換行狀態。

## 狀態文字區塊 {#styled-message-blocks}

使用 `comment`、`success`、`error`、`warning`、`note`、`info` 與 `caution` 輸出標準狀態區塊。`block` 透過明確的 `BlockOptions` 開放它們共用的渲染器。

```rust
use openrust::console::{AsyncCommandCallback, BlockOptions, ConsoleError, OutputStyle};
use openrust::support::facades::Artisan;

fn register_blocks() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:blocks", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            style.comment("Preparing report")?;
            style.success("Report ready")?;
            style.warning("Review the queue")?;
            style.block(&["Build finished", "Ready to deploy"], BlockOptions {
                kind: Some("BUILD"),
                style: Some("fg=blue"),
                padding: true,
                ..BlockOptions::default()
            })?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`BlockOptions::default()` 的 `kind`、`style` 為 `None`，`prefix` 為一個空格，`padding: false`、`escape: true`。`kind` 是可選的方括號標籤；借用選項表達可選參數，不複製應用字串。所有助手接受與 `text` 相同的借用字串或字串陣列。

標準狀態區塊轉義訊息標籤，`comment` 保留行內格式；只有需要依格式器標記解讀訊息時，才設定 `escape: false`。`padding` 僅在裝飾輸出中加入首尾空行。區塊寬度在樣式建構時確定，最多 120 欄；設定為零時使用 120，Windows 還會預留一欄。之後修改 `COLUMNS` 不改變該樣式的區塊寬度。

`error` 輸出錯誤區塊，命令結束碼仍由回傳值控制。無效格式或零寬度分區可能在部分輸出後回傳型別化主控台錯誤。問答驗證重試重用同一錯誤區塊渲染器，包括格式器、轉義與建構時寬度。

## 樣式層問答助手 {#style-question-helpers}

樣式物件也直接提供問答助手。簡寫方法採用預設參數，`_with` 變體傳入可選參數；它們與 `ask_question` 共用 `Question`、`ChoiceQuestion`、`ConfirmationQuestion`、輸入串流與取消機制。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputFormat, OutputStyle, QuestionCallback,
};
use openrust::support::facades::Artisan;
use serde_json::json;

fn register_style_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:style-questions", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let profile = style.ask_with_validator("Name", Some("Ada"), Some(
                QuestionCallback::sync(|value| Ok(json!({"name": value}))),
            )).await?;
            let _token = style.ask_hidden("Token").await?;
            let confirmed = style.confirm("Continue").await?;
            let color = style.choice_with(
                "Color", ["red", "green"], Some(json!("green")), false,
            ).await?;
            style.writeln(
                json!({"profile": profile, "confirmed": confirmed, "color": color})
                    .to_string().as_str(),
                OutputFormat::Raw,
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`ask`、`ask_with` 回傳 `Option<String>`；`ask_with_validator` 保留驗證器完整的 JSON 回傳值。`ask_hidden` 回傳隱藏輸入的文字，`ask_hidden_with` 也保留結構化驗證結果。範例使用權杖但不列印其內容。

`OutputStyle::confirm` 預設 `true`，命令層 `CommandIo::confirm` 預設 `false`，兩者都可用 `confirm_with` 明確指定。樣式的 `choice_with` 將匹配的預設值映射至選項鍵，`choice_pairs_with` 保留具名鍵，最後的布林參數啟用多選。

非互動模式下，非 null 預設值會執行驗證器；null 預設值直接回傳 null，不呼叫驗證器。驗證失敗遵循既有重試規則，`ConsoleError::from_runtime_source` 明確標記需要立即終止的錯誤。需要設定次數、補全或逾時時，建立 `Question` 並呼叫 `ask_question`。

## 表格輸出目標與樣式工廠 {#table-output-factory}

`Table::new` 接受既有 `AsOutput` 介接器：`Output`、`ConsoleSectionOutput`、`OutputReference`、`OutputStyle` 與盒裝介接器。一般輸出不需輸入物件即可建構表格。表格現在只有一個輸出借用生命週期，明確的 `Table<'a, 'o>` 註記改為 `Table<'a>`，一般建構呼叫語法不變。

`OutputStyle::create_table` 選取底層輸出，複製具名的樣式指南定義，並採用 `info` 表頭格式。實際主控台使用獨立分區，擷取輸出仍使用擷取串流；該方法不加入尾端空行。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle};
use openrust::support::facades::Artisan;

fn register_factory_table() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:factory-table", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
        let mut style = OutputStyle::new(input, output);
        let mut table = style.create_table()?;
        table.set_headers(["H", "V"]).set_rows([["x", "y"]]);
        table.render()?;
        Ok(0)
        })
    }))?;
    Ok(())
}
```

追加支援由實際建構目標決定。直接選取分區時，使用 `Table::new(&mut section)` 或 `Table::new(style.get_output())`。傳入 `&mut style` 表示包裝該樣式，即使最終寫往分區，`append_row` 也會拒絕；渲染與設定仍遵循同一格式器與詳細程度規則。

修改工廠回傳表格的樣式，不會修改應用的具名樣式指南定義。上述建構介接器不代表其餘一般自訂輸出介面已經完成。

## 樣式表格與定義列表 {#style-table-helpers}

`OutputStyle::table` 與 `horizontal_table` 使用既有表格工廠，僅在渲染成功後加入一個空行，不額外插入前置區塊間距。資料列接受 `TableRow`，包含明確的 `TableSeparator`；一般表頭陣列直接傳入，多層表頭透過 `TableHeaders::rows` 與儲存格值區分。

`definition_list` 接受 `DefinitionListItem` 迭代器：標題跨兩欄，`entry` 提供單個鍵值，`entries` 保留有序陣列輸入並只渲染首項，`TableSeparator::new().into()` 加入分隔線。空 `Entries` 仍保留一個空項。列舉表達原始字串／陣列／分隔線聯合型別，不支援的型別在編譯時拒絕。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, DefinitionListItem, OutputStyle, TableCell, TableHeaders};
use openrust::support::facades::Artisan;

fn register_style_tables() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:style-tables", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let rows = vec![
                vec![TableCell::from("Ada"), TableCell::from(42)],
                vec![TableCell::from("Lin"), serde_json::Value::Null.into()],
            ];
            let mut style = OutputStyle::new(input, output);
            style.table(["Name", "Value"], rows)?;
            style.horizontal_table(TableHeaders::rows([["A", "B"], ["C", "D"]]), [["x", "y"]])?;
            style.definition_list([
                DefinitionListItem::from("Profile"),
                DefinitionListItem::entry("Name", "Ada"),
                DefinitionListItem::entry("Count", 42),
            ])?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

可空純量儲存格使用 `serde_json::Value::Null.into()`。一般儲存格裡的 null 與空字串顯示相同，水平表格仍須區分：跨欄標題下的 null 被省略，空字串保留為儲存格；明確的樣式／跨欄儲存格仍作為儲存格物件處理。定義列表標題複用這項公共表格行為，不另造渲染器。格式錯誤保留既有 `ConsoleError` 來源鏈與部分輸出，失敗後不補成功空行。

## 樣式介面 {#style-interface}

`openrust::console::style::StyleInterface` 是正式公開 Trait，`openrust::console::StyleInterface` 匯出同一型別。它包含標題、分節、列表、文字、成功／錯誤／警告／備註／注意訊息、表格、四類問答、換行及進度開始／推進／完成。`OutputStyle` 實作全部 18 個入口，服務可以接收泛型樣式或 `&mut dyn StyleInterface`。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle};
use openrust::console::style::StyleInterface;
use openrust::support::facades::Artisan;

fn render_report<S: StyleInterface + ?Sized>(style: &mut S) -> Result<(), ConsoleError> {
    style.title("Report")?;
    style.table(["Name", "Value"].into(), vec![["Ada", "42"].into()])?;
    style.success("Ready".into())
}

fn register_style_interface() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:interface", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            render_report(&mut style)?;
            let dynamic: &mut dyn StyleInterface = &mut style;
            render_report(dynamic)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

Trait 呼叫明確傳入原可選參數：`ask` 接受可選預設值與驗證器，`ask_hidden` 接受可選驗證器，`confirm` 接受布林預設值，`choice` 接受有序 `Vec<(ChoiceKey, Value)>` 與可選預設值；整數鍵表達順序選擇，字串鍵保留關聯結果。問答保留完整 `serde_json::Value` 回傳值。`table` 接受 `TableHeaders` 與 `Vec<TableRow>`，訊息複用 `OutputMessages`，`new_line`／`progress_advance` 明確傳入次數。具體型別的既有便利方法繼續可用。

四類非同步 Trait 方法回傳既有 `ConsoleFuture`，為支援動態派發，在 Trait 邊界增加一次 Future 裝箱；直接呼叫具體 `OutputStyle` 的非同步方法不增加這一層。同步 Trait 方法不裝箱 Future。Trait 為既有非同步命令執行要求 `Send`，不要求實作 `NewLineAware`。連續問答共用同一輸入與輸出歷史，包含隱藏輸入與失敗復原。

## 錯誤樣式與基礎樣式型別 {#error-style}

`OutputStyle::get_error_style` 回傳基礎型別 `ConsoleStyle`，也可從 `openrust::console::ConsoleStyle` 匯入。基礎型別提供同一套既有渲染助手並實作 `StyleInterface`，但不實作 `NewLineAware`，也不暴露衍生型別的換行計數。也可以使用 `ConsoleStyle::new(input, output)` 直接建構。

直接包裝實際主控台時選取 stderr；擷取串流、分區與包裝樣式繼續使用已選輸出，不會一律切換到獨立錯誤緩衝。格式器、裝飾與詳細程度跟隨該目標；修改直接主控台錯誤樣式的設定，不會改變 stdout 的設定。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle};
use openrust::console::style::ConsoleStyle;
use openrust::support::facades::Artisan;

fn register_error_style() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:error-style", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut original = OutputStyle::new(input, output);
            {
                let mut errors: ConsoleStyle<'_> = original.get_error_style();
                errors.warning("Review")?;
                errors.table(["Name", "Value"], [["Ada", "42"]])?;
            }
            original.info("Done")?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

回傳的樣式獨占借用輸出，結束其作用域後再透過原樣式寫入；它共用實際目標，不複製輸出緩衝。換行計數查詢在基礎樣式邊界停止，實際寫入仍到達底層目標。巢狀呼叫 `get_error_style` 仍回傳基礎型別。

`ConsoleStyle::write_iter` 與 `writeln_iter` 在請求下一項之前逐條寫出；衍生 `OutputStyle` 保留原有的換行統計準備。兩種型別複用同一格式器與渲染引擎，不代表一般第三方 `OutputInterface` 已實作。樹助手與其餘元件 API 另行追蹤。

## 樹節點與惰性子節點 {#tree-nodes}

`TreeNode` 儲存唯讀標籤和共享的節點身分。`new(value)` 建立空節點，`with_children(value, children)` 加入初始子節點，`default()` 使用空標籤。`add_child` 接受節點、字串或 `TreeChild::lazy`；複製節點會共享身分，不複製整棵樹。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputOptions, OutputStyle, TreeChild, TreeNode, TreeValue};
use openrust::support::facades::Artisan;

fn register_tree_nodes() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:tree-nodes", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let root = TreeNode::from_values(
                [("languages", TreeValue::branch([(0_i64, "Rust"), (1_i64, "Go")]))],
                Some(TreeNode::new("Project")),
            )?;
            root.add_child("Ready");
            root.add_child(TreeChild::lazy(|| [Ok(TreeNode::new("Generated"))]));
            OutputStyle::new(input, output).writeln(
                format!("{}:{}", root.get_value(), root.count()?).as_str(),
                OutputOptions::default(),
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`from_values` 接受有序鍵值對。`TreeValue::branch` 表達巢狀值；巢狀鍵成為標籤，純量鍵被忽略。`TreeValue::Node` 本身可迭代，因此在所給鍵下展開其子節點，不保留該節點標籤；要保留節點本身，使用 `add_child(node)`。`Some(root)` 向同一根節點追加，`None` 建立空根。

`get_children`、`get_iterator` 和 `&TreeNode` 的原生迭代均回傳 `Result<TreeNode, ConsoleError>` 項目。首次推進時取得子清單快照：此前的變動會包含在內，之後的追加需新遍歷才能看到。工廠在節點鎖外執行，每次遍歷及 `count()` 都會重新呼叫；工廠回傳擁有所有權的 `Send` 迭代器，可傳播錯誤。`lazy_keyed` 保留 `ArrayKey` 鍵，透過迭代器的 `key()` 讀取；`lazy` 對每個工廠從零編號，直接子節點使用獨立的順序編號。

節點使用標準 `Arc`，不提供循環垃圾收集。回呼反向引用請用 `downgrade`／`WeakTreeNode::upgrade`；若主動建立強引用循環，結束使用後透過 `clear_children` 解除，`ptr_eq` 比較節點身分。透過 `from_values` 展開循環節點會回傳錯誤，避免無限遞迴。普通根節點及其子節點在擁有者和迭代器釋放後回收。

可失敗的輸入產生者使用 `try_from_values`，逐項回傳 `Result<(key, value), ConsoleError>`；失敗前已追加的兄弟節點會保留。

巢狀的一次性產生者使用 `TreeValue::iterable` 或 `try_iterable`，擁有 `Send + 'static` 迭代器，後者接受可失敗的項目。輸入按順序消費，不提前收集；巢狀分支只有完整成功後才附加。`TreeValue` 不複製或重播一次性產生者。

## 樹輸出與樹游標 {#trees}

`OutputStyle` 和基礎 `ConsoleStyle` 均提供 `tree(values, root)` 與 `create_tree(values, root)`，同直接建構入口共用 `TreeHelper`。預設空根傳入空字串；即使空樹也寫出根行，渲染結束後不額外追加空白行。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle, TreeHelper, TreeNode, TreeStyle, TreeValue, TreeValues};
use openrust::support::facades::Artisan;

fn register_tree_output() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:trees", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            style.tree([(0_i64, "A"), (1_i64, "B")], "Project")?;
            {
                let mut tree = style.create_tree(
                    [("Group", TreeValue::branch([(0_i64, "Leaf")]))], "Root",
                )?;
                tree.render()?;
            }
            let root = TreeNode::with_children("Styled", ["Leaf"]);
            TreeHelper::create_tree(
                &mut style, root, TreeValues::new(), Some(TreeStyle::rounded()),
            )?.render()?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`TreeHelper::create_tree(output, root, values, style)` 接受根標籤、既有 `TreeNode` 或 `None`；空 `TreeValues` 保留既有子節點，樣式傳 `None` 使用預設值。值會追加到同一個根節點。樣式工廠只有直接包裝真實控制台時才新建分區；捕獲流、既有分區和巢狀樣式保留所選輸出，錯誤樣式也保留 stdout／stderr 選擇。

助手提供 `current`、`key`、`advance`、`rewind`、`valid`、`has_children` 和 `get_children`。建構時預讀首個子節點；`advance()` 僅推進，標準迭代器仍以 `next()` 回傳元素，`current()` 回傳共享同一身分的節點，`key()` 回傳整數。無效游標的 current／key 存取回傳 `ConsoleTreeError`；數字字串鍵沿用既有弱純量規則，非數或溢位鍵失敗。`for item in &mut helper` 遍歷直接子節點；`recursive_iterator()` 深度優先回傳 `TreeEntry { node, line }`，不包含根，也不寫輸出。`TreeStyle::apply_prefixes` 設定該迭代器，重複節點偵測由 `render()` 執行。

游標為單次消費：越過首次 yield 前可以 rewind，空游標也可反覆 rewind。已推進的非空游標重繞失敗，包括第二次 `render()`；渲染會先寫根行再報告該錯誤。再次遍歷請建立新助手。`get_children()` 借用所選輸出，需要結束子助手的作用域後再使用父助手。即使 `valid()` 為 false，最後的節點仍由游標持有，直到游標釋放。

工廠可能在建構、子節點探測和建立子游標時執行，不應假設每個節點只呼叫一次。底層遍歷將子樹探測錯誤視為沒有子節點；推進迭代器時的錯誤會傳播，且可能已經寫出部分輸出。渲染中再次遇到同一節點身分會回傳 `ConsoleTreeError::Cycle`，DAG 中重用節點也屬於重複；不同節點可以具有相同標籤。渲染期間保留已訪問節點，避免臨時節點分配位址重用造成誤判。

`TreeStyle` 提供 `default`、`r#box`、`box_double`、`compact`、`light`、`minimal` 和 `rounded`。`box` 是 Rust 保留字，因此使用原始識別字 `r#box`。`TreeStyle::new` 的六個唯讀前綴依序為：有下一項的末端、最後末端、左側、有下一項的中段、最後中段、右側。預設使用靜態字串，自訂前綴在子助手間共享。

## 元件文字變換器 {#component-mutators}

四個公開變換器共用物件安全的 `Mutator::apply(&str) -> Result<String, ConsoleError>` 介面。明確的 `apply` 對應可呼叫物件；直接實作 Rust 可呼叫 trait 需要不穩定語言特性。輸入固定為字串，不重現 PHP 對任意物件或 null 的弱轉換。

```rust
use openrust::console::{ConsoleError, view::components::mutators::{EnsureDynamicContentIsHighlighted, EnsureNoPunctuation, EnsurePunctuation, EnsureRelativePaths, Mutator}};

fn transform_component_text(value: &str) -> Result<String, ConsoleError> {
    let value = EnsureDynamicContentIsHighlighted.apply(value)?;
    let value = EnsurePunctuation.apply(&value)?;
    let value = EnsureRelativePaths.apply(&value)?;
    EnsureNoPunctuation.apply(&value)
}
```

動態內容變換將非空 `[內容]` 包入粗體標記。標點判斷只識別末尾 ASCII `.`, `?`, `!`, `:`，不裁剪空白、不正規化全形標點。`EnsureNoPunctuation` 只移除一個末尾字元。相對路徑變換在目前應用作用域中移除所有精確符合的 `base_path + "/"`，不正規化路徑；沒有應用作用域或缺少 `path.base` 綁定時保持原文。這是顯示格式處理，不是路徑安全校驗。

## 行元件 {#line-components}

`Info`、`Success`、`Warn` 和 `Error` 加入標籤、以粗體顯示方括號內的動態內容，並補齊句末標點。各型別透過 `new(output)` 借用所選輸出，`render(text)` 使用一般輸出層級，`render_with(text, verbosity)` 指定層級門檻。`render_with` 也接受 `OutputOptions`，保留輸出層級門檻以及 raw／plain 輸出模式。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle, OutputVerbosity,
    view::components::{Error as ErrorComponent, Info, Line, Success, Warn}};
use openrust::support::facades::Artisan;

fn register_line_components() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:components", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            Info::new(&mut style).render("Creating [reports]")?;
            Success::new(&mut style).render("Ready")?;
            Warn::new(&mut style).render("Check [configuration]")?;
            ErrorComponent::new(&mut style).render("Missing [field]")?;
            Line::new(&mut style).render("info", "Custom [line]")?;
            Line::new(&mut style).render_with("warn", "Verbose detail", OutputVerbosity::Verbose)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`Line` 接受 `info`、`success`、`warn`、`error` 樣式鍵；未知鍵回傳 `ConsoleError`，不寫入新行。元件共用所選輸出、格式器和換行狀態。`Error` 在該輸出上呈現 ERROR 標籤；底層主控台支援時，透過 `get_error_style()` 選擇 stderr。

元件文字會合併 ASCII 空格、定位字元與換行；NUL 轉為空格，其他 C0 控制字元被移除，包括原始 ANSI 跳脫字元，不換行空格保留。需要樣式時使用格式器標籤。長文字不自動折行。支援換行統計的樣式決定前置間距，底層樣式與原始擷取使用一行前置間距。

`OutputFormat::Normal` 保持原有預設值 0。`ExplicitNormal` 對應明確指定的 normal 位 1，保留與輸出層級組合時的換行計數。窮盡比對 `OutputFormat` 的程式碼需要加入該分支。

## 提示框、清單與雙欄詳細資料 {#display-components}

這些元件借用所選輸出，保留其格式器及 stdout／stderr 選擇。`render` 使用一般輸出層級，`render_with` 接受 `OutputOptions` 或輸出層級門檻。終端寬度優先讀取 `COLUMNS`。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle, OutputVerbosity,
    view::components::{Alert, BulletList, TwoColumnDetail}};
use openrust::support::facades::Artisan;

fn register_display_components() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:display", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            Alert::new(&mut style).render("Review [reports]")?;
            BulletList::new(&mut style).render(["CSV.", "JSON.", "HTML."])?;
            TwoColumnDetail::new(&mut style).render("Report [daily].", Some("Ready"))?;
            TwoColumnDetail::new(&mut style).render("Optional value.", None)?;
            Alert::new(&mut style).render_with("Verbose alert", OutputVerbosity::Verbose)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`Alert` 使用整行背景，將內容轉為大寫並置中；內容超過可用寬度時按帶樣式文字規則截斷，無法處理的寬度回傳 `ConsoleError`。`BulletList` 對每項以粗體顯示方括號內容、移除一個末尾 ASCII 標點，並解析相對路徑。輸入只收集一次，隨後逐階段處理所有項目，再一次寫入；空清單仍寫入一個換行。

`TwoColumnDetail` 移除第一欄的末尾標點，保留第二欄標點。`None` 和 `Some("")` 省略第二欄，其他輸入在文字正規化後仍保留欄位置。點狀填充按至多 150 欄的版面寬度計算，過長的欄內容仍完整輸出。元件建構後，透過原共用控制代碼變更格式器樣式仍立即生效。

## 任務結果識別值 {#task-results}

`console::view::TaskResult` 定義 `Success = 1`、`Failure = 2` 和 `Skipped = 3`。`value()` 回傳整數背值，`cases()` 保留宣告順序。`TryFrom<i64>` 拒絕其他值，不將無效識別值當成成功。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputOptions, OutputStyle,
    view::TaskResult};
use openrust::support::facades::Artisan;

fn register_task_results() -> Result<(), ConsoleError> {
    Artisan::command_with("tasks:states", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            for state in TaskResult::cases() {
                style.writeln(format!("{state:?}:{}", state.value()).as_str(), OutputOptions::default())?;
            }
            let failure = TaskResult::try_from(2)?;
            assert_eq!(failure, TaskResult::Failure);
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`TaskResult` 序列化為整數背值，而非 Rust 變體名稱。JSON 反序列化要求整數且屬於已知變體。請求的 `enum::<TaskResult>` 透過 `BackedEnum` 讀取同一型別，沿用既有請求純量轉換與預設值機制。例如查詢值 `2.0` 解析為 `Failure`，未知整數使用請求預設值。JSON 反序列化與請求輸入轉換是兩個不同入口。

## 任務顯示 {#task-components}

`Task` 借用所選輸出。`render` 不執行回呼、不顯示耗時，直接顯示任務完成。`render_task` 執行一次同步回呼，並安全借出同一輸出供回呼使用。對應的 `_with` 方法接受輸出選項；隱藏顯示不會略過回呼。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle,
    view::{TaskResult, components::Task}};
use openrust::support::facades::Artisan;

fn register_task_components() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:tasks", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let mut task = Task::new(&mut style);
            task.render("Ready")?;
            task.render_task("Build [report].", |selected| {
                selected.write("CSV ", false, openrust::console::OutputOptions::default())?;
                Ok(TaskResult::Success)
            })?;
            task.render_task("Optional [CSV].", |_| Ok(TaskResult::Skipped))?;
            task.render_task("Boolean [false].", |_| Ok(false))?;
            task.render_task("Failed status.", |_| Ok(TaskResult::Failure))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

任務回呼中的 `TaskResult` 轉換為整數背值。只有整數 `2` 顯示 FAIL、整數 `3` 顯示 SKIPPED；`false`、null（`()`）、浮點數及字串皆顯示 DONE。回傳值明確轉換為 `serde_json::Value`，不會隱含序列化任意物件。顯示 FAIL 本身不會改變命令結束代碼。回呼回傳 `Err(ConsoleError)` 時先顯示 FAIL，再傳播錯誤；Rust 展開堆疊時同樣先收尾顯示，再繼續 panic。

耗時使用牆上時鐘。不超過一秒時顯示含千位分組、兩位小數及 half-up 捨入的毫秒；超過一秒後使用參考預設短單位（`yr`、`mo`、`w`、`d`、`h`、`m`、`s`），捨棄不足一秒的部分。此顯示中一個月按四週、一年按十二個這樣的月進位。`run_time_for_humans(start, Some(end))` 接受明確的 Unix 秒數用於格式化。`set_run_time_formatter(Some(Box::new(...)))` 透過組合替代耗時方法覆寫，傳入 `None` 恢復預設。格式器接受開始時間及可選結束時間，回傳 `Result<String, ConsoleError>`，可依應用需求自訂耗時措辭。此顯示入口不提供通用日期區間或語言工廠。

## 問答輔助器與臨時作用域 {#question-helpers}

`QuestionHelper` 使用元件提示：兩個前導空格、粗體文字、結尾標點及 `❯` 游標。`DefaultQuestionHelper` 使用一般樣式提示。兩者皆提供 `ask(input, output, question)` 和 `get_name()`。直接呼叫輔助器時沿用傳入的輸入及所選輸出；原始主控台輸出選取 stderr，既有樣式則保留自己的串流。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputOptions, OutputStyle,
    QuestionHelper};
use openrust::support::facades::Artisan;
use std::sync::Arc;

fn register_question_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:question-helpers", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let first = {
                let mut scoped = style.using_question_helper(Arc::new(QuestionHelper::new()));
                scoped.ask_with("Name", Some("Guest")).await?
            };
            let second = style.ask_with("Next", Some("Guest")).await?;
            style.writeln(format!("Users:{}/{}", first.unwrap_or_default(), second.unwrap_or_default()).as_str(), OutputOptions::default())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`using_question_helper` 借用同一樣式，作用域結束時還原原輔助器實例，包括錯誤、展開堆疊及取消。先前未初始化輔助器時，還原一般預設輔助器。`OutputStyle` 和 `ConsoleStyle` 皆支援此作用域。`QuestionStyle` 透過 trait 提供相同操作，`QuestionHelperScope::new` 可接收動態樣式。

實作 `QuestionHelperBehavior` 可自訂標點、提示或重試錯誤，再用 `set_question_helper(Some(Arc::new(...)))` 安裝或使用臨時作用域。`write_prompt` 寫入提示，並回傳已格式化的結尾游標文字供原生行編輯重繪。`write_error` 接收原輸出是否為樣式的標記。`Question::get_kind()` 提供借用的 `QuestionKind` 檢視，可區分文字、確認及選擇題，並讀取原選擇鍵值。這些輔助器尚未公開通用輔助器集合登錄表。

既有原生輸入引擎保留完整 UTF-8 長行，使用原生終端 API，不執行 shell 命令。`disable_stty()` 維持不呼叫外部 stty 的保證，不關閉原生補全或隱藏輸入；這與依賴 shell 的終端實作不同。多行提示繼續使用應用譯器。

## 問答元件 {#question-components}

`Ask`、`AskWithCompletion`、`Choice`、`Confirm` 和 `Secret` 借用目前的 `QuestionStyle`，支援 `OutputStyle` 和 `ConsoleStyle`。每次 render 臨時選取元件輔助器，結束後還原原輔助器；建構、驗證失敗及取消皆走相同還原路徑。

```rust
use openrust::console::{AsyncCommandCallback, CompletionCallback, ConsoleError, OutputOptions,
    OutputStyle, view::components::{Ask, AskWithCompletion, Choice, Confirm, Secret}};
use openrust::support::facades::Artisan;
use serde_json::json;

fn register_question_components() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:question-components", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let name = Ask::new(&mut style).render_with("Name", Some(json!("Guest")), false).await?;
            let color = AskWithCompletion::new(&mut style).render_with_callback(
                "Color", CompletionCallback::sync(|typed| {
                    Ok(["red", "green"].into_iter().filter(|value| value.starts_with(&typed)).map(str::to_owned).collect())
                }), Some(json!("red")),
            ).await?;
            let confirmed = Confirm::new(&mut style).render("Continue").await?;
            let format = Choice::new(&mut style).render_with(
                "Format", ["CSV", "JSON"], Some(json!(0)), None, false,
            ).await?;
            let secret = Secret::new(&mut style).render("Password").await?;
            style.writeln(format!("User:{name}; Color:{color}; Confirmed:{confirmed}; Format:{format}; Secret supplied:{}", !secret.is_null()).as_str(), OutputOptions::default())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`Ask::render` 無預設值，讀取一行；`render_with` 接受純量 JSON 預設值及多行開關。`AskWithCompletion::render` 接收候選字串，`render_with` 增加預設值，`render_with_callback` 使用既有支援非同步的補全回呼。候選輸入只走訪一次。這些元件回傳 `serde_json::Value`；`Confirm` 回傳 `bool`，預設 false。`Secret::render_with(question, false)` 在原生隱藏不可用時禁止明文退回，請勿印出回傳的秘密。

`Choice::render` 使用列表，無預設值及嘗試次數上限。`render_with` 增加預設值、最多嘗試次數及多選開關，次數為零時回報錯誤。`render_pairs_with` 保留關聯鍵及稀疏整數鍵。元件將非 list 鍵排列視為關聯選擇：稀疏數字鍵回傳 `"1"` 這樣的字串鍵；標準列表回傳選中值。一般 `ChoiceQuestion::from_pairs` 仍沿用原本的字串鍵分類機制。

`Choice::get_choice_question` 和 `get_choice_question_pairs` 提供已準備的問題定義，供組合使用；`is_assoc` 檢查 `ChoiceQuestion::get_choices` 回傳的規範鍵值順序。`ChoiceQuestion::from_pairs_with_associative` 接收 `Send + Sync` 鍵分類函式，參數是規範鍵值；分類結果在建構或重建驗證器時保存，不在每次回答時重新計算。補全候選保留建構時的快照。複製保留分類，同時沿用既有獨立 trim 設定。
