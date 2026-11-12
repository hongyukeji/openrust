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

名言庫與固定版 Laravel 一致，共 41 則。支援色彩的終端機會將正文顯示為粗體、署名顯示為灰色。重新導向時預設輸出純文字。可以用 `--ansi` 強制開啟樣式，或用 `--no-ansi` 關閉。

Windows 上使用 `artisan.bat`。編譯後的應用程式透過 `main artisan` 接收相同的命令。

清單和說明成功時傳回結束代碼 0。未知命令和無效引數傳回 1。標示為 `Planned` 的項目可以檢視說明，執行時傳回 1。

透過 `withCommands` 接入 `app/console/commands/mod.rs` 中的命令型別。在 `bootstrap/app.rs` 的 `withRouting` 中宣告 `routes/console.rs` 的控制台路由。應用程式命令可以取代同名預留項目。

`routes/console.rs` 匯入 `openrust::support::facades::Artisan`，透過 `Artisan::command("inspire", …)` 向 `register()` 執行期間繫結的主控台註冊閉包，透過 `purpose()` 設定說明和命令清單中的描述。閉包接收命令輸出物件，正常傳回時結束代碼為 0。

`Application::handleCommand` 呼叫 Console Kernel 的 `handle` 和 `terminate`。Kernel 完成引導後載入待實作命令清單，再註冊應用程式命令並分派。


## 原始碼與發布入口

原始碼 `artisan` 腳本先進入應用目錄，再將參數原樣交給 Cargo，保留空格、空參數和非 ASCII 文字。腳本回傳命令的結束碼。

獨立建置使用 `./main artisan list`、`./main artisan help inspire` 或 `./main artisan inspire`，無需 Cargo。設定相對於可執行檔解析，從其他工作目錄啟動也可執行。參閱[部署](../getting-started/deployment.md)。

編譯入口不帶模式參數時啟動 HTTP，使用 `artisan` 時執行主控台命令。其他首參數會在建立應用前顯示外層用法並回傳結束碼 2。Artisan 分派錯誤回傳 1。外層分流是 Rust 單二進位檔的語言適配；Laravel 使用獨立的 HTTP 和 `artisan` 入口檔案。Console Kernel 的 handle/terminate 順序仍由 `Application::handleCommand` 負責。

命令清單同時包含待實作條目。名稱和說明用於保留預定契約，不代表對應模組已經完成。`artisan.bat` 尚未通過 Windows 實機驗收。

## 命令說明和輸出格式 {#command-help-and-output-formats}

`./artisan help migrate` 與 `./artisan migrate --help` 從同一命令定義讀取引數、選項和說明。檢視說明時不會執行目標命令。精確的命令名稱和別名優先，可依完整名稱或冒號分隔的各段使用唯一縮寫。

```sh
./artisan list cache --format=json --short
./artisan help migrate --format=json
./artisan help migrate --format=xml
./artisan help migrate --format=md
```

`list` 和 `help` 支援 `txt`、`json`、`xml` 和 `md`。`--raw` 輸出平鋪清單或精簡文字說明。`list --short` 保留名稱、別名和實作狀態，省略命令詳細資料。各語言的結構化輸出使用相同的命令和選項識別碼。說明和清單成功時結束碼為 0。引數錯誤、歧義和未知命令的結束碼為 1。

`--quiet` 隱藏一般輸出。`--silent` 同時隱藏主控台錯誤。命令定義也可使用全域 `--no-interaction`、`--ansi`、`--no-ansi`、`--version` 和詳細程度選項。

語言選擇與覆寫見 [Artisan 翻譯](./localization.md#artisan-translations)。

## 按需安裝路由 {#optional-route-installers}

在原始碼目錄執行下列命令，產生路由檔案並更新模組宣告與啟動設定，然後重新建置。既有路由檔案會保留。

```sh
./artisan install:api
./artisan install:broadcasting
./artisan install:grpc
```

API 預設掛載到 `/api` 並使用 `api` 中介軟體群組，端點與驗證由應用程式補充。廣播接入後會註冊 `/broadcasting/auth`，授權需要廣播驅動程式和頻道規則。gRPC 須註冊 Tonic 服務，並設定 `GRPC_HOST` 和 `GRPC_PORT` 才會開啟監聽。
