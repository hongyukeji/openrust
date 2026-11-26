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

產生到 `app/console/commands` 的命令在建置時註冊。需要明確註冊時，使用 `with_commands` 和 `app/console/commands/mod.rs`。在 `bootstrap/app.rs` 的 `with_routing` 中宣告 `routes/console.rs` 的控制台路由。應用程式命令可以取代同名預留項目。

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

### 結束代碼 {#exit-codes}

只接收輸出的閉包正常回傳時，結束代碼為 0。型別化命令和可失敗閉包用 `Ok(0)` 表示成功，也可用 `Ok(code)` 指定結束代碼。`Artisan::call` 回傳該值，命令列啟動器也使用該值。

透過 `?` 傳遞 `Input::fail` 或 `CommandIo::fail` 的結果，也可以直接回傳。Artisan 停止命令，透過錯誤元件輸出訊息並回傳結束代碼 1。`fail_default` 使用目前應用程式語言。其他 `ConsoleError` 仍進入例外處理。

```rust
use openrust::console::{ConsoleError, Input, Output};
use openrust::support::facades::Artisan;

fn check_readiness(input: &Input) -> Result<(), ConsoleError> {
    if input.argument("state").and_then(|state| state.as_str().map(str::to_owned)).as_deref()
        != Some("ready")
    {
        input.fail("The job is not ready.")?;
    }
    Ok(())
}

fn register_exit_codes() -> Result<(), ConsoleError> {
    Artisan::command_with("jobs:check {state}", |input: &Input, output: &mut Output| {
        check_readiness(input)?;
        input.io(output).line("Ready")?;
        Ok(0)
    })?;
    Ok(())
}
```

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

## 撰寫命令 {#writing-commands}

用 `make:command` 產生命令檔案。先定義參數與選項，再實作業務邏輯。

## 產生命令 {#generating-commands}

破壞性命令可呼叫 `openrust::console::confirm_to_proceed(production, input.matches())?`，僅在回傳 `true` 時繼續。`production` 由應用程式環境或命令確認條件決定。共用的多語提示預設拒絕，非互動輸入拒絕；命令宣告的 `--force` 可略過確認。

在原始碼目錄產生命令。產生的型別實作 Artisan 命令契約。

```sh
./artisan make:command SendReport
./artisan make:command Reports/ArchiveReport --command=reports:archive
```

第一條命令寫入 `app/console/commands/send_report.rs`，命令簽章為 `app:send-report`。第二條在 `reports` 模組下產生命令並指定簽章。使用 `-f` 或 `--force` 覆寫既有檔案。

在產生的 `handle` 方法填寫命令邏輯，然後重新建置。預設範本宣告 `COMMAND` 註冊入口，建置時自動探索 `app/console/commands` 下的檔案並註冊至 Artisan。編譯完成後可執行 `./artisan app:send-report` 或 `./artisan help app:send-report`。

在應用程式根目錄放置 `stubs/console.rs.stub` 可自訂產生內容。產生器替換 `{{class}}` 和 `{{command}}`。

自訂範本保留頂層的 `COMMAND` 註冊入口即可參與自動探索。缺少該入口的舊檔案可沿用明確的模組與註冊。可在 `build.rs` 的 `COMMANDS` 中新增應用程式命令目錄。

### 產生配套測試 {#generating-an-accompanying-test}

加入 `--test` 可在產生命令時同時產生一個功能測試。

```sh
./artisan make:command SendReport --test
```

測試寫入 `tests/feature/console/commands/send_report_test.rs`，巢狀命令保留原有子目錄。依照[測試接線](../testing/getting-started.md#creating-tests)宣告模組後，以 Cargo 執行。樣板沿用 `make:test` 的健康檢查，可在產生的測試中加入命令斷言。

`--force` 允許取代兩個產生的檔案。命令產生後若測試產生失敗，Artisan 以狀態碼 1 結束並保留命令檔案。解決錯誤後，執行 `./artisan make:test Console/Commands/SendReportTest` 單獨產生測試。

## 命令結構 {#command-structure}

命令型別實作 `Command`。用 `CommandDefinition::from_signature` 和 `with_description` 宣告。Artisan 使用同一份定義解析輸入與顯示說明。`handle` 方法可透過應用程式容器解析型別化相依項目。

```rust
use std::sync::Arc;
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{container::Container, foundation::Application as _},
    foundation::{Application, configuration::ApplicationBuilder},
    support::{ProviderResult, ServiceProvider, facades::Artisan},
};

struct Reporter;

impl Reporter {
    fn report(&self, user: &str) -> String {
        format!("Report ready for {user}")
    }
}

struct SendReport(CommandDefinition);

impl Command for SendReport {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self, app: &'a Application, input: &'a Input, output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let reporter = app.make::<Reporter>().await.map_err(ConsoleError::from_source)?;
            let Some(user) = input.argument("user") else {
                return input.fail("A user is required.");
            };
            input.io(output).line(&reporter.report(user.as_str().unwrap_or_default()))?;
            Ok(0)
        })
    }
}

struct ReportProvider;

impl ServiceProvider for ReportProvider {
    fn register<'a>(
        &'a mut self, app: &'a Application,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async move {
            app.container().instance(Arc::new(Reporter))?;
            let definition = CommandDefinition::from_signature("reports:send {user}")?
                .with_description("Send a report");
            app.container().bind::<SendReport, _>(move |_| {
                Ok(Arc::new(SendReport(definition.clone())))
            })?;
            Ok(())
        })
    }
}

fn configure_commands(builder: ApplicationBuilder) -> ApplicationBuilder {
    builder.with_providers(vec![Box::new(ReportProvider)]).with_command::<SendReport>()
}

async fn report_from_handler(user: &str, output: &mut Output) -> Result<i32, ConsoleError> {
    Artisan::call_type::<SendReport>(&[user.into(), "--no-ansi".into()], output).await
}
```

## 註冊命令 {#registering-commands}

建置時探索預設目錄及 `COMMANDS` 指定的額外目錄。註冊其他模組中的型別時，在 Provider 中綁定該型別，再於應用程式建構器呼叫 `with_command::<Type>()`。可多次呼叫以登記多個型別。`with_commands` 也接受明確的註冊函式。

Artisan 在主控台組裝時從容器解析型別，之後同一應用程式重用該命令實例。可依簽章名稱呼叫，也可用 `Artisan::call_type::<Type>`。依型別呼叫會使用已註冊實例所對應的名稱。

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

## 命令輸入與輸出 {#command-io}

用 `input.io(output)` 同時存取命令解析後的值與原始輸出。請參閱[型別化輸入](#typed-command-input)、[互動問答](#interactive-questions)、[輸出訊息](#writing-output)、[表格](#tables)及[進度條](#progress-bars)。

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

<a id="writing-output"></a>

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
            io.output_components().info("Saved")?;
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


### 元件工廠 {#component-factories}

`CommandIo::components()` 和 `output_components()` 傳回借用的 `Factory`，不再傳回一般輸出串流。型別化方法重用對應元件：`info`、`success`、`warn`、`error`、`line`、`alert`、`bullet_list`、`two_column_detail`、`task`、`ask`、`ask_with_completion`、`choice`、`confirm` 和 `secret`。顯示方法傳回 `Result<(), ConsoleError>`；問答方法非同步執行。需要直接操作串流時使用 `get_output()` 或 `AsOutput`。

```rust
use openrust::{console::{AsyncCommandCallback, ConsoleError}, support::facades::Artisan};
fn register_component_factories() -> Result<(), ConsoleError> {
    Artisan::command_with("components:save", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let name = io.components().ask_with("Name", Some("Guest".into()), false).await?;
            io.output_components().info(&format!("Saved {name}"))?;
            io.components().task_callback("Index", |_| Ok(true))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

工廠保留命令最初的輸出樣式和問答輸入。`set_output` 只替換一般輸出，多次替換不會遷移元件輸出或問答輸入；`set_input` 更換命令輸入存取，不替換原樣式。獨立的 `Factory::new(&mut output)` 可在 `Output`、輸出參照和區段上顯示元件。問答需要 `OutputStyle`、`ConsoleStyle` 或自訂 `ComponentOutput::question_style` 實作；一般串流會在列印問題前傳回執行階段錯誤。任務回呼接收同一所選輸出參照，保留任務失敗顯示與錯誤傳遞行為。


### 動態元件呼叫 {#dynamic-component-calls}

元件名稱在執行階段確定時使用 `Factory::call(name, parameters).await`。名稱保留動態字串契約：`askWithCompletion`、`bulletList` 和 `twoColumnDetail`，不區分大小寫，但不把底線名稱作為別名。未知名稱傳回 `ConsoleError`。一般 Rust 方法繼續使用蛇形命名。

```rust
use openrust::{
    console::{AsyncCommandCallback, CompletionCallback, ConsoleError, view::components::ComponentArgument},
    support::facades::Artisan,
};
use serde_json::json;
fn register_dynamic_components() -> Result<(), ConsoleError> {
    Artisan::command_with("components:dynamic", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let color = io.components().call("askWithCompletion", vec![
                "Color".into(), CompletionCallback::sync(|_| Ok(vec!["red".into(), "green".into()])).into(),
                json!("red").into(),
            ]).await?;
            io.components().call("info", vec!["Saved".into()]).await?;
            io.components().call("task", vec!["Index".into(), ComponentArgument::task(|_| Ok(true))]).await?;
            io.line(&format!("Choice:{color}"))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`ComponentArgument::Value` 承載 JSON 資料。顯示呼叫傳回 JSON null，問答傳回既有值，確認傳回 JSON 布林值。`Completion` 接收現有 `CompletionCallback`；`ComponentArgument::task` 持有編譯後的 `FnOnce` 回呼，可在呼叫期間借用 `Send` 狀態。回呼只執行一次，錯誤傳遞前仍完成任務顯示收尾。字串是資料，不作為可執行函式名稱。`Options` 接收 `OutputOptions`。整數選項接受文件常數中的單一詳細程度與單一格式旗標；衝突位元遮罩和非法參數傳回錯誤。既有所有權、問答取消及暫時協助器還原規則仍適用。

文字參數誤傳陣列時在渲染前拒絕，不會因錯誤把未完成編譯的 HTML 範本留在處理程序輸出緩衝中。


### 自訂控制台元件 {#custom-console-components}

在應用程式 Provider 中註冊編譯後的建構器。每次動態呼叫都新建 `RenderableComponent`，借出原輸出上下文，等待 `render` 後釋放實例。建構器由所屬應用程式的 `ComponentRegistry` 共用，可變元件實例不共用。將 `BadgeProvider` 加入一般 Provider 清單，將 `register_custom_components` 接入控制台路由。

```rust
use openrust::{
    console::{AsOutput as _, AsyncCommandCallback, ConsoleError, ConsoleFuture, OutputFormat,
        view::components::{ComponentArgument, ComponentConstructor, ComponentContext, ComponentRegistry, RenderableComponent}},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    support::{ProviderResult, ServiceProvider, facades::Artisan},
};
use serde_json::{Value, json};
use std::sync::Arc;
struct Badge<'a>(ComponentContext<'a>);
struct BadgeConstructor;
impl ComponentConstructor for BadgeConstructor {
    fn make<'a>(&'a self, output: ComponentContext<'a>) -> Result<Box<dyn RenderableComponent + 'a>, ConsoleError> {
        Ok(Box::new(Badge(output)))
    }
}
impl RenderableComponent for Badge<'_> {
    fn render<'a>(&'a mut self, parameters: Vec<ComponentArgument<'a>>) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(async move {
            let label = match parameters.first() {
                Some(ComponentArgument::Value(Value::String(label))) => label,
                _ => return Err(ConsoleError::new("需要文本标签")),
            };
            self.0.as_output().writeln(&format!("Badge:{label}"), OutputFormat::Raw)?;
            Ok(json!({"label": label}))
        })
    }
}
struct BadgeProvider;
impl ServiceProvider for BadgeProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async move {
            app.container().make::<ComponentRegistry>()?.register("Badge", Arc::new(BadgeConstructor))?;
            Ok(())
        })
    }
}
fn register_custom_components() -> Result<(), ConsoleError> {
    Artisan::command_with("components:badge", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let result = io.components().call("Badge", vec!["Ready".into()]).await?;
            io.line(&result.to_string())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

名稱可以包含命名空間分隔符號，例如 `Widgets\Badge`。註冊不區分大小寫；重複、非法及已有內建元件名稱會被拒絕。建構器和其解構器在註冊表鎖外執行。建構及渲染錯誤繼續傳遞；取消呼叫後釋放元件及輸出借用。

`Factory::new` 記錄目前應用程式容器，用於查找自訂元件；進入另一個應用程式作用域不會更換這項註冊歸屬。獨立消費者可用 `Factory::with_registry(&mut output, registry)` 明確傳入 `Arc<ComponentRegistry>`。資料與回呼繼續透過 `ComponentArgument` 傳遞。

`ComponentContext` 實作 `AsOutput`。`question_style()` 傳回原問答樣式，或在列印提示前傳回執行階段錯誤。自訂輸出適配器實作 `ComponentOutput::component_context`：傳回 `ComponentContext::Style` 保留問答輸入，一般串流傳回 `ComponentContext::Output`。預設 `ComponentOutput::question_style` 跟隨該上下文；已有明確問答樣式覆寫仍適用於內建問答元件。


### 元件資料與問答作用域 {#component-data-scopes}

`Component` 透過組合重用元件能力。可借用 `ComponentOutput` 建構，也可在已註冊元件的建構器中使用 `from_context`。`get_output` 和 `AsOutput` 借用原輸出；`question_style` 提供既有問答能力，不替換輸入。

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError, OutputFormat, OutputStyle, Question,
        view::components::{Component, DefaultMutatorConstructor, mutators::EnsureNoPunctuation}},
    support::facades::Artisan,
};
use serde_json::json;
fn register_component_data() -> Result<(), ConsoleError> {
    Artisan::command_with("components:data", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let mut component = Component::new(&mut style);
            let mutator = DefaultMutatorConstructor::<EnsureNoPunctuation>::new();
            let data = component.mutate(json!({"message": "Created [report].", "active": true}), &[&mutator])?;
            let mut calls = 0;
            let name = component.using_question_helper(async |mut style| {
                calls += 1;
                let question = Question::new("Name", Some(json!("Guest")))?;
                style.ask_question(&question).await
            }).await?;
            component.get_output().writeln(
                format!("Data:{data}; Name:{name}; Calls:{calls}").as_str(), OutputFormat::Raw,
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`mutate` 按順序執行建構器，每輪為每項變換器建立一次物件，再套用於各個頂層值。陣列保留位置，物件保留鍵。建構器清單為空時傳回原資料。`DefaultMutatorConstructor<M>` 使用 `M::default`；自訂 `MutatorConstructor` 可建立有狀態物件或傳回建構錯誤。發生錯誤後不再執行後續管線。

`Mutator::apply` 繼續作為字串介面；`apply_value` 處理 JSON 值，自訂變換器可覆寫。去標點時，不需要變化的值保留原型別，不會把數字、布林值和陣列默默轉為字串。高亮變換會將值轉為文字。未繫結應用程式根路徑時，相對路徑變換保留原值；繫結後，陣列目標保留鍵並轉換其直接成員，遵循字串取代行為。

`using_question_helper` 接受可借用區域狀態的非同步閉包，暫時安裝元件協助器，在成功、錯誤、panic 展開及取消後還原同一個原協助器。原協助器尚未初始化時還原一般預設協助器。閉包內可以更新借用的區域狀態；一般輸出會在呼叫閉包前失敗。可恢復的失敗優先使用 `Result`。

閉包接收 `QuestionStyleReference`，即原樣式的短期借用，支援 `QuestionStyle` 和 `AsOutput`。使用 `async |mut style|` 繫結。具名參照讓要求 `Send` 的命令處理器能明確識別借用生命週期，不另配置樣式物件，也不複製輸入。


### 編譯元件標記 {#component-markup-compilation}

`Component::compile(view, data)` 傳回元件 HTML 標記，不寫入終端輸出。預設編譯器重用 Blade，內嵌四個範本；不可變編譯結果共用，每次呼叫的資料獨立。執行階段不執行來源檔案，也不寫入編譯視圖目錄；修改內嵌範本需要重新建構二進位檔。

| 視圖 | 必要資料鍵 |
| --- | --- |
| `alert` | `content` |
| `line` | `marginTop`, `bgColor`, `fgColor`, `title`, `content` |
| `bullet-list` | `elements` |
| `two-column-detail` | `first`, `second` |

這些名稱是範本資料鍵，保持原拼寫。內容執行 HTML 跳脫；行樣式值與標題使用範本的原始插值。`elements` 接受純量清單或具名條目。缺少變數、型別非法及未知內建視圖傳回 `ConsoleError`。

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError, OutputFormat,
        view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}},
    support::facades::Artisan,
};
use serde_json::{Value, json};
struct HeadingCompiler;
impl ComponentViewCompiler for HeadingCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        if view == "heading" {
            let text = data["text"].as_str().ok_or_else(|| ConsoleError::new("需要标题文本"))?;
            return Ok(format!("<strong>{}</strong>", text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")));
        }
        BuiltinComponentViewCompiler::new().compile(view, data, output)
    }
}
fn register_component_compilation() -> Result<(), ConsoleError> {
    Artisan::command_with("components:compile", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            let html = component.compile("alert", json!({"content": "Compiled <report>"}))?;
            component.get_output().writeln(format!("Builtin:{html}").as_str(), OutputFormat::Raw)?;
            component.set_view_compiler(Some(Box::new(HeadingCompiler)));
            let html = component.compile("heading", json!({"text": "Ready"}))?;
            component.get_output().writeln(format!("Custom:{html}").as_str(), OutputFormat::Raw)?;
            component.set_view_compiler(None);
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`ComponentViewCompiler` 由元件持有，接收原輸出上下文，可以保留實例狀態、傳回錯誤或委派 `BuiltinComponentViewCompiler`。`set_view_compiler(None)` 還原預設編譯器。編譯為同步操作；非同步資料應先準備好再編譯標記。

視圖選擇參數與範本資料分開，資料鍵 `view` 不會重新導向編譯，也不能覆寫編譯器區域變數。自訂視圖來源透過自訂編譯器接入，不作為路徑傳給內建範本目錄。

### 渲染元件標記 {#component-markup-rendering}

`render_view` 先使用元件目前編譯器，再把實際 HTML 佈局寫入原輸出。`render_view_with` 將詳細級別和格式選項傳給同一個輸出。此層資料已經準備完成，不會再次執行具體元件的標點或醒目提示變換。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputFormat, view::components::Component};
use openrust::support::facades::Artisan;
use serde_json::json;
fn register_component_rendering() -> Result<(), ConsoleError> {
    Artisan::command_with("components:render", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.render_view("line", json!({
                "marginTop": 1, "bgColor": "blue", "fgColor": "white",
                "title": "info", "content": "Rendered <report>"
            }))?;
            component.render_view_with("bullet-list", json!({"elements": ["First", "Second"]}), OutputFormat::Plain)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

預設渲染器複用 HTML 解析器和現有輸出格式器，支援四個內建範本、行內／區塊／彈性佈局、邊距、內距、固定／完整／分數寬度、最小／最大寬度、對齊與大小寫、固定預設顏色及色階、粗體／斜體／底線／刪除線、連結及彈性重複填充。它還不是完整的終端 HTML/CSS 引擎：表格／程式碼／預格式／清單渲染、自訂樣式倉庫仍未支援。未支援的樣式回傳錯誤，不會靜默丟棄佈局。寬度和填充有安全上限，巢狀上限為 256 層。

`ComponentViewRenderer` 收到實際編譯結果、`OutputOptions` 和原 `ComponentContext`。透過 `set_view_renderer(Some(Box::new(renderer)))` 安裝元件擁有的渲染器，可保留實例狀態、委託 `BuiltinComponentViewRenderer` 或回傳錯誤；`set_view_renderer(None)` 恢復預設。自訂編譯結果也沿同一路徑，不按內建視圖名稱替換 HTML，不切換全域終端輸出。編譯失敗不會呼叫渲染器；渲染失敗保留已經發生的寫入並向呼叫者傳播。

後端按 HTML5 解析。任意畸形標記的恢復行為及 HTML4 特有相容性尚未因內建範本測試而閉合。明確的 `strong`、`italic`、`underline`、`line-through` 和 `invisible` 樣式直接產生終端跳脫序列，在 Raw 輸出或關閉裝飾時也會保留；`font-bold` 則使用輸出格式器。

### 元件顏色與斷點 {#component-color-breakpoints}

終端渲染器支援固定預設調色盤，包括 `text-amber-200`、`bg-slate-900`、基本顏色及 `text-brightgreen` 等亮色名稱。名稱與色階按固定集合提供，不接受任意 CSS 顏色運算式。無效顏色在佈局寫入前回傳錯誤。下面的 Raw 模式用於檢查產生的格式器標籤。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct PaletteCompiler;
impl ComponentViewCompiler for PaletteCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        if view == "palette" {
            return Ok("<span class='bg-slate-900 text-amber-200 px-1 sm:px-2'>Palette</span>".into());
        }
        BuiltinComponentViewCompiler::new().compile(view, data, output)
    }
}
fn register_component_colors() -> Result<(), ConsoleError> {
    Artisan::command_with("components:colors", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(PaletteCompiler)));
            component.render_view("palette", json!({}))?;
            component.render_view_with("palette", json!({}), OutputFormat::Raw)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

響應式前綴按終端列寬啟用：`sm:64`、`md:76`、`lg:102`、`xl:128`、`2xl:153`。先套用無前綴類別，再按斷點升序套用已啟用的前綴；未啟用的前綴不解析對應樣式，因此其中的未知樣式不會觸發錯誤。這裡使用終端列，不是瀏覽器像素。

類名拆分保留方括號內的空白，例如 `content-repeat-[. ]`；不換行空格不是類名分隔符號。原始類名會保留：帶前綴的 `sm:flex-1` 不會產生用於判定彈性增長的無前綴 `flex-1` 標記。自訂樣式倉庫及其他待補的佈局功能仍不屬於目前支援子集。

### 元件尺寸與分佈對齊 {#component-sizes-and-alignment}

使用 `w-1/2`、`w-2/3` 或 `w-full` 設定分數寬度，使用數值 `min-w-10`／`max-w-30` 設定限制。分數按繼承寬度計算，並處理元素原始水平邊距和內距。分母為零會回傳錯誤，不會靜默採用完整終端寬度。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct ColumnsCompiler;
impl ComponentViewCompiler for ColumnsCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        match view {
            "columns" => Ok("<div class='w-20 justify-between'><span>A</span><span>B</span></div>".into()),
            "half" => Ok("<span class='w-1/2'>Half</span>".into()),
            _ => BuiltinComponentViewCompiler::new().compile(view, data, output),
        }
    }
}
fn register_component_sizes() -> Result<(), ConsoleError> {
    Artisan::command_with("components:sizes", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(ColumnsCompiler)));
            component.render_view("columns", json!({}))?;
            component.render_view("half", json!({}))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`justify-between`、`justify-evenly`、`justify-around` 和 `justify-center` 在子節點間分配剩餘空間。第一個範例固定容器為 20 列，在 A 與 B 之間放置 18 個空格；非 flex 容器也可使用這些規則。剩餘空間不足時保留子內容，不額外新增分佈填充。取整保留渲染器既定的浮點 floor／ceil 規則。

寬度參數保留數值或文字表示。分數與最小寬度混用時，先比較這些表示，再解析列數：`w-full min-w-2` 會選擇兩列。建議使用數值限制並驗收組合佈局，不把所有組合當作 CSS 計算後寬度比較。

彈性項目先渲染並快取巢狀內容，再調整自身寬度、重新格式化該快取。因此巢狀的完整寬度子節點，可能不同於先分配所有 flex 寬度再渲染後代的結果。每次呼叫使用獨立的樹和繼承屬性快照。無效佈局沿原渲染器傳播錯誤，不影響後續請求。完整樣式方法分派及其他專用 HTML 渲染器仍未完成。

### 元件文字變換 {#component-text-transforms}

`snakecase` 在駝峰邊界插入底線，保留原有空白。`prepend-[X]` 和 `append-[Y]` 保留方括號：範例輸出 `[X]mixed_words[Y]`。重複同一種變換只更新參數，不改變首次登記的執行位置。`truncate-4` 的四欄限制包含預設 `…` 後綴，輸出 `Mix…`；沒有正數限制時使用元素寬度減去內距。分數寬度在截斷前解析。

樣式方法名按 ASCII 大小寫不敏感分派：`W-1/2` 和 `FONT-BOLD` 可用。佈局標記仍保留原始類名語義，例如修改 `flex-1` 大小寫或加回應式前綴，不會建立該精確標記。數字類參數先轉整數，再檢查目標參數型別，所以 `prepend-1` 回傳錯誤，不會靜默轉成文字。`font-normal` 明確關閉繼承的粗體，`<b>` 標籤則在類樣式之後設定自身粗體。

預設文字管線不代表已提供渲染依賴的獨立樣式物件或自訂樣式／顏色倉庫。表格和程式碼等專用元素仍不屬於已實作的 HTML 子集。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct TextCompiler;
impl ComponentViewCompiler for TextCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        match view {
            "text" => Ok("<span class='snakecase prepend-[X] append-[Y]'>MixedWords</span>".into()),
            "short" => Ok("<span class='truncate-4'>MixedWords</span>".into()),
            _ => BuiltinComponentViewCompiler::new().compile(view, data, output),
        }
    }
}
fn register_component_text() -> Result<(), ConsoleError> {
    Artisan::command_with("components:text", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(TextCompiler)));
            component.render_view("text", json!({}))?;
            component.render_view("short", json!({}))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 列表、定義與分隔線 {#component-list-elements}

`ul`／`ol` 的直接子元素使用 `li`；文字節點保留，但不佔編號。無序列表預設圓點，可用 `list-square`／`list-none`。有序列表只給實際編號項遞增，所以範例輸出 `1. First` 和 `2. Second`，跳過項不帶編號。標記檢查使用原始類名。父列表在建構時附加樣式，會重新套用子項預設的 block；子項已有文字變換仍保留首次登記的執行順序。

`dl` 接受 `dt` 和 `dd`，預設術語粗體、定義左邊距四欄。無效的直接子元素回傳錯誤。`hr` 在寬度處理後產生邊框：`w-12` 輸出十二個線字元；內容寬度為零時，後續邊框階段仍可能填滿繼承寬度。`border-t` 的數字參數不設定粗細。表格與程式碼渲染器、自訂樣式／顏色倉庫仍是獨立未完成能力。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct ListCompiler;
impl ComponentViewCompiler for ListCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        match view {
            "list" => Ok("<div><ul><li>Alpha</li><li>Beta</li></ul><ol><li>First</li><li class='list-none'>Skipped</li><li>Second</li></ol><dl><dt>Term</dt><dd>Definition</dd></dl><hr class='w-12'></div>".into()),
            _ => BuiltinComponentViewCompiler::new().compile(view, data, output),
        }
    }
}
fn register_component_lists() -> Result<(), ConsoleError> {
    Artisan::command_with("components:lists", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(ListCompiler)));
            component.render_view("list", json!({}))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 預格式輸出 {#component-preformatted-output}

`pre` 保留行內容，不折疊空白。最多移除首尾各一個空行，將其餘行按最大 UTF-8 位元組長度加三個空格補齊，再透過一般 div 樣式逐行渲染。因此初始填充對多位元組文字使用位元組長度，而不是終端欄寬。範例輸出首個換行以及 `One   `／`Two   `。

內部標籤作為原樣標記保留，不遞迴註冊成元件；最終格式器如何處理它們仍取決於輸出選項。預格式結果不繼承父樣式，佈局可測量其已渲染內容。空結果和隱藏結果仍是有效元素。未被 HTML 恢復移到列表外的直接預格式子元素，會被列表子型別檢查拒絕。CRLF、註解、顯式空屬性和省略值的布林屬性保留既定序列化區別。通用異常 HTML 恢復仍是獨立未完成能力。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct PreCompiler;
impl ComponentViewCompiler for PreCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        match view {
            "pre" => Ok("<pre>One\nTwo</pre>".into()),
            _ => BuiltinComponentViewCompiler::new().compile(view, data, output),
        }
    }
}
fn register_component_pre() -> Result<(), ConsoleError> {
    Artisan::command_with("components:pre", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(PreCompiler)));
            component.render_view("pre", json!({}))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### HTML 表格輸出 {#component-html-tables}

元件標記重用內建 Table 管線。`style` 選擇命名表格樣式；`thead`、`tbody` 和 `tfoot` 提供表頭、正文、標題與表尾分隔行。儲存格支援 `align`、`rowspan`、`colspan` 及既有文字／顏色類。表頭儲存格設定 strong，儲存格內 `<br>` 轉成換行。無效樣式或對齊沿原機制回傳錯誤。

表格先經過獨立的彩色緩衝輸出，再進入元件佈局，所以外層關閉裝飾時，內部 ANSI 輸出仍可能保留。HTML 標題樣式使用所選命名樣式的本次副本，避免臨時顏色修改長駐應用的後續請求；直接 Table API 的共享樣式行為不變。自訂樣式／顏色倉庫及通用異常 HTML 恢復仍未完成。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct TableCompiler;
impl ComponentViewCompiler for TableCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        match view {
            "table" => Ok("<table style='box'><thead title='Report'><tr><th>Name</th><th>Value</th></tr></thead><tbody><tr><td>Alpha</td><td align='right'>12</td></tr></tbody></table>".into()),
            _ => BuiltinComponentViewCompiler::new().compile(view, data, output),
        }
    }
}
fn register_component_table() -> Result<(), ConsoleError> {
    Artisan::command_with("components:table", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(TableCompiler)));
            component.render_view("table", json!({}))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 程式碼片段 {#component-code-snippets}

`code` 輸出行號，並在 `line` 指定的絕對行放置標記；`start-line` 預設 1，最小為 1。非零標記即使超出片段範圍，也會保留標記佔位。首尾空行會修剪，CRLF 正規化；若內容有只包含空白的行，以該行決定縮排前綴，並從匹配的各行移除。程式碼節點的 `class` 不設定權杖主題。

固定片段契約使用 PHP 詞法分類，包括短標籤、字串與註解。其他文字按內嵌文字顯示；這不是 Rust 專用或通用多語言高亮器。詞法處理完全執行於 Rust，無須 PHP 程式或執行環境。權杖組、行號與標記重用元件樣式和原輸出格式器；部分 strong／italic ANSI 修飾在無裝飾輸出中仍會保留。極大行偏移保留整數／浮點文字規則，無法表示的行陣列鍵回傳錯誤。自訂樣式／顏色倉庫和其他獨立終端擴充介面仍未完成。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct CodeCompiler;
impl ComponentViewCompiler for CodeCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        match view {
            "code" => Ok("<code start-line='8' line='9'>first line\nsecond line</code>".into()),
            _ => BuiltinComponentViewCompiler::new().compile(view, data, output),
        }
    }
}
fn register_component_code() -> Result<(), ConsoleError> {
    Artisan::command_with("components:code", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(CodeCompiler)));
            component.render_view("code", json!({}))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 時間與延遲輔助方法 {#component-time-helpers}

`InteractsWithTime` 提供 `seconds_until`、`available_at`、`parse_date_interval`、`current_time` 和 `run_time_for_humans`。Task 同時直接提供繼承的四個延遲／日期方法；既有耗時覆寫透過 Trait 呼叫仍然生效。實作者選擇可接收 `DateError` 的關聯錯誤型別，保留命令既有的錯誤處理。

`TimeDelay` 接收 `i64`、`Date` 或 `DateInterval`。整數延遲保留正負；絕對日期的剩餘秒數最低為零，可執行時間則保留原時間戳。日期區間相對於日期工廠目前值解析，保留月份及夏令時間前後的本地日曆天，不簡單折算固定秒數；年／月／日分量共同正規化，再疊加固定時長分量。解析後回傳日期或秒數，不改變原值。紀元前時間按向下取整的整秒處理。

預設可執行偏移明確傳 `0`，因為 Rust 沒有省略位置參數。`available_at` 的相對秒數使用日期工廠，不複用覆寫的 `current_time`；只有絕對日期的 `seconds_until` 才減去該鉤子。凍結時鐘沿用應用／請求／任務作用域，耗時顯示仍使用真實牆鐘。日曆區間超出既有原生日期範圍回傳受檢錯誤。整數可執行時間戳保留有符號 64 位回繞規則；最小整數沿用日期修改運算式的後備路徑。這些助手不代替佇列排程，也不表示整個日期庫已經完成。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, view::components::Task};
use openrust::support::{DateError, InteractsWithTime, facades::Artisan};
struct RetryClock;
impl InteractsWithTime for RetryClock { type Error = DateError; }
fn register_time_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("components:time", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let clock = RetryClock;
            assert_eq!(clock.seconds_until(-5)?, -5);
            let mut task = Task::new(output);
            let current = task.current_time()?;
            let available = task.available_at(60)?;
            assert!(available >= current + 60);
            let runtime = InteractsWithTime::run_time_for_humans(&mut task, 0.0, Some(0.1))?;
            drop(task);
            output.comment(&format!("delay=-5; runtime={runtime}"));
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 控制台輔助工具 {#console-helper-tools}

使用 `Helper` 計算顯示寬度、字素數量和子字串。`width` 採用固定控制台寬度表，連續控制字元只計一次。`length` 和 `substr` 使用 Unicode 17 字素邊界。負起點從末尾計算，負長度從末尾裁掉對應數量，子字串採用 NFC 正規化。位元組入口同時接受非 UTF-8 資料，此時按位元組計算寬度、長度和偏移。

`format_time` 預設精度為 1，`format_time_with` 接收明確精度。`format_memory` 格式化有號位元組數。`remove_decoration` 使用傳入的格式器，移除 SGR 和終端超連結，成功後恢復原裝飾設定。格式化失敗時回傳錯誤並維持關閉裝飾的狀態，遵循固定版助手的執行順序。QuestionHelper 型別也直接提供這些繼承工具。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, Helper, OutputFormatter, QuestionHelper};
use openrust::support::facades::Artisan;

fn register_helper_tools() -> Result<(), ConsoleError> {
    Artisan::command_with("helpers:measure", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let text = Some("界e\u{301}");
            let width = Helper::width(text);
            let length = QuestionHelper::length(text);
            let last = Helper::substr(text, -1, None);
            let elapsed = Helper::format_time_with(61.001, 2)?;
            let formatter = OutputFormatter::new(true);
            let label = Helper::remove_decoration(&formatter, Some("<info>Ready</info>"))?;
            output.comment(&format!("{label} {width}/{length} {last} {elapsed}"));
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 控制台助手集合 {#console-helper-sets}

`HelperSet` 按標準名稱註冊每個 `HelperInterface` 實例，也可指定區分大小寫的別名。替換同名項目時保留該名稱的位置，既有別名仍可指向舊實例。`get_iterator` 取得呼叫時的註冊順序和實例參照。名稱不存在時回傳 `HelperNotFound`。批次註冊有別名或無別名的項目可用 `from_helpers`。

自訂助手可用 `Helper` 儲存關聯。集合擁有註冊實例，助手只保留最後一次關聯集合的弱參照。手動改變實例關聯時保留現有註冊項目。預設問答助手和元件問答助手皆提供此關聯入口。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, DefaultQuestionHelper, HelperSet,
};
use openrust::support::facades::Artisan;
use std::sync::Arc;

fn register_helper_set() -> Result<(), ConsoleError> {
    Artisan::command_with("helpers:lookup", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let helpers = HelperSet::new();
            let question = Arc::new(DefaultQuestionHelper::new());
            helpers.set(question.clone(), Some("ask"));
            let resolved = helpers.get("ask").map_err(ConsoleError::from_source)?;
            assert!(question.get_helper_set().unwrap().same_set(&helpers));
            output.comment(&format!("{} has {} entries", resolved.get_name(), helpers.get_iterator().count()));
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 格式化與除錯助手 {#console-formatter-helpers}

`FormatterHelper` 為節標題和文字區塊產生帶樣式標記的文字。節標題預設使用 `info`。區塊會跳脫輸入，並把各行補齊至相同寬度。`format_block_with` 可選擇在上下增加空行的寬鬆樣式。`truncate_with` 先比較顯示寬度，再按字素切片並附加後綴。

`DebugFormatterHelper` 按工作階段 ID 產生起始行、輸出與錯誤前綴及結果行。重新啟動同一 ID 會重設輸出流狀態並輪換邊框顏色。切換輸出流後的第一行加入對應前綴。存取未知工作階段時回傳錯誤。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, DebugFormatterHelper, FormatterHelper};
use openrust::support::facades::Artisan;
use openrust::console::{AsOutput, OutputFormat};

fn register_formatter_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("helpers:format-example", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let formatter = FormatterHelper::new();
            output.as_output().writeln(&formatter.format_section("check", "Ready"), OutputFormat::Normal)?;
            let debug = DebugFormatterHelper::new();
            output.as_output().writeln(&debug.start("report", "Running"), OutputFormat::Normal)?;
            output.as_output().writeln(&debug.progress("report", "Processing", false)?, OutputFormat::Normal)?;
            output.as_output().writeln(&debug.stop("report", "Done", true)?, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 行程助手 {#console-process-helper}

將 `ProcessHelper` 和 `DebugFormatterHelper` 加入同一個助手集合。以下範例需要能在可執行檔搜尋路徑中找到 Git。可以傳入字面參數或已設定的 `PendingProcess`。

`run` 在行程以非零狀態結束時也會回傳結果。`must_run` 在底層行程錯誤中保留該結果。預設的 `VeryVerbose` 層級顯示命令及結果，`Debug` 還會顯示即時輸出。終端機診斷寫入 stderr，主動選擇的區段或擷取緩衝區保留原輸出目標。使用 `run_with` 和 `must_run_with` 設定層級、失敗提示和原始輸出位元組回呼。獨立包裝回呼時，先在已註冊的除錯助手上啟動工作階段，再將 ID 交給 `wrap_callback`。關閉擷取的行程沿用其輸出設定。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, DebugFormatterHelper, HelperSet, ProcessHelper,
};
use openrust::support::facades::Artisan;
use std::sync::Arc;

fn register_process_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:git-version", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let helpers = HelperSet::new();
            let process = Arc::new(ProcessHelper::new());
            helpers.set(Arc::new(DebugFormatterHelper::new()), None);
            helpers.set(process.clone(), None);
            let result = process.must_run(output, ["git", "--version"]).await?;
            output.comment(String::from_utf8_lossy(result.output()).trim());
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 應用程式與命令的助手集合 {#command-helper-sets}

每個控制台應用程式都會建立自己的助手集合，包含 `formatter`、`debug_formatter`、`process` 和 `question`。執行中的命令透過 `Input::get_helper` 或 `CommandIo::get_helper` 依名稱查詢助手。以下範例需要能在可執行檔搜尋路徑中找到 Git。

註冊時，`Artisan::get_helper_set` 回傳目前應用程式的集合。`Artisan::set_helper_set` 更換往後命令繼承的集合，現有命令保留其集合。使用 `Artisan::set_command_helper_set` 依名稱或別名更換單個命令的集合。修改共享集合的內容會影響持有該集合的命令。查詢未知助手時回傳錯誤。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, ProcessHelper};
use openrust::support::facades::Artisan;
use std::{any::Any, sync::Arc};

fn register_default_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:helpers", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let helper: Arc<dyn Any + Send + Sync> = input.get_helper("process")?;
            let process = helper.downcast::<ProcessHelper>()
                .map_err(|_| ConsoleError::new("进程助手类型不匹配"))?;
            let result = process.must_run(output, ["git", "--version"]).await?;
            output.comment(String::from_utf8_lossy(result.output()).trim());
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 自訂終端機樣式與色彩 {#terminal-style-repository}

建立 `StyleRepository` 並交給內建渲染器。複製的控制代碼共用已註冊樣式，獨立儲存庫可為同名樣式設定不同行為。使用 `create` 或 `create_with` 註冊自訂類別。回呼接收目前樣式內容，其後依序執行透過註冊控制代碼的 `apply` 附加的樣式類別。

色彩使用 `#a855f7` 或 `#a5f` 等十六進位值。透過 `text-brand-500` 或 `bg-brand-500` 使用色彩時，註冊完整鍵名 `brand-500`。自訂名稱區分大小寫，精確註冊名稱優先於內建處理，包括響應式前綴。重複註冊會取代該名稱的項目，原控制代碼仍持有原定義。 自訂樣式的巢狀層級達到呈現深度上限時回傳錯誤，循環參照也能安全結束。

```rust
use openrust::console::{
    AsOutput, AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{
        BuiltinComponentViewRenderer, ComponentContext, ComponentViewRenderer, StyleRepository,
    },
};
use openrust::support::facades::Artisan;

fn register_style_repository() -> Result<(), ConsoleError> {
    let styles = StyleRepository::new();
    styles.create("brand-500").color("#a855f7")?;
    styles
        .create_with("status", |element, _| {
            element.uppercase();
            Ok(())
        })
        .apply("font-bold text-brand-500");

    Artisan::command_with("tools:status-style", AsyncCommandCallback::new(move |_, _, output| {
        let styles = styles.clone();
        Box::pin(async move {
            BuiltinComponentViewRenderer::with_styles(styles).render(
                "<span class='status'>Ready for work</span>",
                OutputFormat::Normal.into(),
                &mut ComponentContext::Output(output.as_output()),
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 型別化終端機樣式方法 {#typed-terminal-style-methods}

註冊回呼內的 `TerminalStyles` 提供色彩、間距、尺寸、文字轉換與版面的型別化方法。整數寬度使用 `StyleArgument::Integer`，分數寬度使用 `StyleArgument::Text`。這些設定會套用到目前元素。

`set_style` 記錄類別名稱，供 `has_style` 查詢。使用 `add_style` 解析並套用一組樣式類別。直接呼叫 `font_bold` 等方法只修改對應格式，不加入類別標記。

```rust
use openrust::console::{
    AsOutput, AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{
        BuiltinComponentViewRenderer, ComponentContext, ComponentViewRenderer, StyleArgument,
        StyleRepository,
    },
};
use openrust::support::facades::Artisan;

fn register_terminal_styles() -> Result<(), ConsoleError> {
    let styles = StyleRepository::new();
    styles.create_with("report-label", |style, _| {
        style.uppercase().font_bold();
        style.w(StyleArgument::Integer(24))?.px(1)?.text("cyan")?;
        Ok(())
    });

    Artisan::command_with("tools:report-label", AsyncCommandCallback::new(move |_, _, output| {
        let styles = styles.clone();
        Box::pin(async move {
            BuiltinComponentViewRenderer::with_styles(styles).render(
                "<span class='report-label'>Report ready</span>",
                OutputFormat::Normal.into(),
                &mut ComponentContext::Output(output.as_output()),
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 終端機元素 {#typed-terminal-elements}

使用 `TerminalElement::new` 建立一般元素，或用 `from_styles` 解析預設與傳入的樣式類別。元素可包含文字或依序排列的子項目。透過 `from_styles_with` 傳入自訂樣式儲存庫。使用 `configure` 可呼叫目前樣式的型別化方法。

`set_content` 建立新元素，沿用同一份樣式設定並使用自己的內容。使用 `to_markup` 取得帶有樣式標記的文字，或使用 `render` 將其寫入所選的輸出。兩個方法都會向呼叫端回傳呈現錯誤。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{TerminalChild, TerminalElement, TerminalElementKind},
};
use openrust::support::facades::Artisan;

fn register_terminal_elements() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:terminal-element", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let prefix = TerminalElement::from_styles(TerminalElementKind::Span, "Status", "font-bold")?;
            let mut line = TerminalElement::from_styles(
                TerminalElementKind::Div,
                vec![TerminalChild::from(prefix), TerminalChild::from(" ready")],
                "text-cyan",
            )?;
            line.render(output, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 獨立終端機樣式 {#standalone-terminal-styles}

建立獨立的 `TerminalStyles` 可直接格式化文字。`with_default_styles` 儲存預設類別名稱，`add_style` 同時套用預設類別與傳入的類別。`format` 回傳帶有樣式標記的文字。

`get_length` 忽略格式標記並測量顯示欄數。獨立樣式沒有內容時，`get_inner_width` 從零扣除左右邊距。`get_parent_width` 依從外到內的順序接收祖先元素的寬度限制。

```rust
use openrust::console::{
    AsOutput, AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::TerminalStyles,
};
use openrust::support::facades::Artisan;

fn register_standalone_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:standalone-style", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut styles = TerminalStyles::new().with_default_styles(["font-bold", "text-cyan"]);
            styles.add_style("ml-2")?;

            let message = "Available";
            let width = styles.get_length(Some(message));
            let formatted = styles.format(&format!("{message}: {width} columns"))?;

            output.as_output().writeln(&formatted, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 獨立樣式屬性 {#terminal-style-properties}

`get_properties` 讀取獨立樣式的目前屬性。`with` 遞迴取代指定的鍵與陣列位置。傳入空陣列會保留既有項目。`set_properties` 取代整個屬性映射，同時保留文字修飾器。

屬性保留格式化所用的鍵，包括 `isFirstChild`、`colors`、`options` 和 `styles`。預設樣式已包含 `isFirstChild`。取代整個屬性映射後若要格式化文字，請同時提供這個鍵。

```rust
use openrust::console::{
    AsOutput, AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::TerminalStyles,
};
use openrust::support::facades::Artisan;
use serde_json::{Map, json};

fn register_style_properties() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:style-properties", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut styles = TerminalStyles::new();
            styles.with(Map::from_iter([
                ("styles".into(), json!({ "ml": 2 })),
                ("colors".into(), json!({ "fg": "blue" })),
            ]))?;
            styles.with(Map::from_iter([
                ("colors".into(), json!({ "bg": "white" })),
            ]))?;

            output.as_output().writeln(&styles.format("Properties ready")?, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 元素樣式關聯 {#terminal-style-attachment}

使用 `TerminalElement::with_styles` 將獨立樣式及其屬性和預設類別關聯到元素。`get_properties`、`set_properties` 和 `with` 會操作元素實際使用的樣式。透過 `set_content` 建立的元素共用該樣式。

`get_length(Some(text))` 測量指定文字。`get_length(None)` 和 `get_inner_width` 使用最近關聯的存活元素。呈現仍使用接收者自己的內容。最近關聯的元素釋放後，測量會使用接收者自己的內容。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{TerminalElement, TerminalElementKind, TerminalStyles},
};
use openrust::support::facades::Artisan;

fn register_attached_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:attached-style", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let style = TerminalStyles::new().with_default_styles(["text-cyan", "font-bold"]);
            let mut label = TerminalElement::with_styles(TerminalElementKind::Span, "Ready", style);
            label.configure(|styles| styles.add_style("ml-1").map(|_| ()))?;

            let mut next = label.set_content("Attached ready");
            let width = label.get_length(None)?;
            next.render(output, OutputFormat::Normal)?;
            output.comment(&format!("{width} columns"));
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 明確樣式繼承 {#terminal-style-inheritance}

`inherit_from_styles` 將祖先屬性套用到獨立樣式或終端機元素。子項保留已設定的色彩和粗體選項，缺少的值由父項補齊。

計算分數寬度時依祖先順序使用其寬度、外距和內距。父層間距作用於第一項之後的元素，第一項保留自己的外距。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{StyleArgument, TerminalElement, TerminalElementKind, TerminalStyles},
};
use openrust::support::facades::Artisan;

fn register_inherited_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:inherited-style", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut parent = TerminalStyles::new();
            parent.text("cyan")?.w(StyleArgument::Integer(24))?.px(1)?;
            parent.font_bold();

            let mut element = TerminalElement::new(TerminalElementKind::Span, "Inherited ready");
            element.inherit_from_styles(&parent)?;
            element.render(output, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 自訂文字與樣式修飾器 {#terminal-style-modifiers}

`TerminalStyleModifiers` 儲存兩組依序執行的回呼。用 `set_text` 註冊文字回呼，回呼在計算寬度前收到目前文字、樣式屬性與祖先屬性。用 `set_style` 註冊的回呼在封裝輸出標記前收到調整後的文字與樣式屬性。

透過 `TerminalStyles::with_modifiers` 關聯回呼。同組回呼名稱重複時會取代原回呼並保留位置。每個階段先執行自訂回呼，再執行內建轉換。回呼回傳錯誤時，格式化會停止並將錯誤回傳呼叫端。

```rust
use openrust::console::{
    AsOutput, AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{TerminalStyleModifiers, TerminalStyles},
};
use openrust::support::facades::Artisan;

fn register_style_modifiers() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:style-modifiers", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut modifiers = TerminalStyleModifiers::new();
            modifiers.set_text("prefix", |value, _, _| Ok(format!("Result: {value}")));
            modifiers.set_style("punctuate", |value, _| Ok(format!("{value}!")));

            let mut style = TerminalStyles::new().with_modifiers(modifiers);
            style.text("green")?;
            let text = style.format("ready")?;
            output.as_output().writeln(&text, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 按名稱呼叫元素方法 {#terminal-element-dispatch}

用 `TerminalElement::call` 傳入方法名稱和 JSON 參數。`get_length` 等查詢回傳 `TerminalElementResult::Value`。修改方法和未知方法回傳同一元素的借用，後續呼叫可以讀到更新後的樣式。

參數無效或呼叫私有方法時會回傳 `ConsoleError`。傳入 Rust 樣式物件時，使用型別化的 `TerminalElement::with_styles` 建構入口。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{TerminalElementKind, TerminalElementResult, TerminalStyles, TerminalElement},
};
use openrust::support::facades::Artisan;
use serde_json::json;

fn register_element_dispatch() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:element-dispatch", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut element = TerminalElement::with_styles(
                TerminalElementKind::Span, "ready", TerminalStyles::new(),
            );
            element.call("prepend", &[json!("Result: ")])?;
            if let TerminalElementResult::Value(length) = element.call("get_length", &[json!("ready")])? {
                assert_eq!(length, json!(5));
            }
            element.render(output, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 終端機 HTML 片段復原 {#terminal-html-recovery}

內建終端機視圖渲染器按 HTML4 規則復原片段。自閉合的樣式 span 保持空白，後續文字保有自己的樣式。框架也處理錯配標籤與標準 HTML4 實體引用。

含標記的片段需要有可渲染的內文首個子節點。片段準備失敗時，渲染器向命令回傳 `ConsoleError`。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError,
    view::components::{Component, ComponentContext, ComponentViewCompiler},
};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};

struct Markup;
impl ComponentViewCompiler for Markup {
    fn compile(
        &mut self, html: &str, _: Value, _: &mut ComponentContext<'_>,
    ) -> Result<String, ConsoleError> {
        Ok(html.to_owned())
    }
}

fn register_html_recovery() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:html-recovery", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(Markup)));
            component.render_view(
                r#"<div><span class="text-red"/>Result: ready</div>"#, json!({}),
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 綁定命令元件工廠 {#command-component-factory}

在應用程式容器中綁定 `dyn CommandComponentFactory`，通常放在服務提供者中。Artisan 每次執行時解析建構器，並傳入已解析的輸入與輸出串流借用。命令內取得的各個元件控制代碼共用回傳的 `ComponentDispatch`。

用 `handles_call` 選擇接管的動態呼叫，用 `display_line` 自訂同步的 `info`、`success`、`warn` 和 `error` 方法。命令之後更換一般輸出時，元件回呼仍使用原本的輸出情境。建構失敗會在命令本文執行前回傳給呼叫端。

```rust
use std::sync::Arc;
use openrust::console::{
    AsOutput, AsyncCommandCallback, ConsoleError, ConsoleFuture, Input, OutputOptions,
    OutputReference,
    view::components::{CommandComponentFactory, ComponentArgument, ComponentContext, ComponentDispatch},
};
use openrust::contracts::{container::Container, foundation::Application as _};
use openrust::foundation::Application;
use openrust::support::facades::Artisan;
use serde_json::{Value, json};

struct Labels;

impl CommandComponentFactory for Labels {
    fn make(
        &self, _: &Input, _: OutputReference<'_>,
    ) -> Result<Arc<dyn ComponentDispatch>, ConsoleError> {
        Ok(Arc::new(Self))
    }
}

impl ComponentDispatch for Labels {
    fn handles_call(&self, name: &str) -> bool {
        name == "label"
    }
    fn call<'a>(
        &'a self, _: &'a str, _: Vec<ComponentArgument<'a>>, _: ComponentContext<'a>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(async { Ok(json!("ready")) })
    }
    fn display_line(
        &self, name: &str, text: &str, options: OutputOptions, mut output: ComponentContext<'_>,
    ) -> Option<Result<(), ConsoleError>> {
        (name == "info").then(|| output.as_output().writeln(&format!("Result: {text}"), options))
    }
}

fn bind_component_factory(app: &Application) -> Result<(), ConsoleError> {
    app.container().instance::<dyn CommandComponentFactory>(Arc::new(Labels))
        .map_err(ConsoleError::from_source)?;
    Ok(())
}

fn register_component_factory() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:component-factory", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let label = io.components().call("label", vec![]).await?;
            io.output_components().info(label.as_str().unwrap_or_default())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```
