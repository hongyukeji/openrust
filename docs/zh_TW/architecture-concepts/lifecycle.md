# 請求生命週期

Tokio 入口將 `bootstrap::createApplication` 交給 `openrust::foundation::runApplication`。框架先選擇執行入口，再建立應用程式。沒有參數時呼叫 `Application::start()` 執行 HTTP，第一個參數為 `artisan` 時呼叫 [`Application::handleCommand`](../digging-deeper/artisan.md)。

應用程式入口將選定的根目錄命名為 `basePath`，傳給 `Application::configure(basePath)`，對應 Laravel 的 `configure(basePath: ...)`。接著 `create()` 驗證根目錄並建立服務容器，由選定的 Kernel 引導應用程式。引導時載入環境和設定，執行所有一般 Provider 的 `register`，再執行各 Provider 的 `boot`，接著註冊共用路由。程序的環境變數優先於環境檔案。

程序環境中的絕對路徑 `APP_BASE_PATH` 優先。未設定時，原始碼建置使用建置時記錄的應用程式 crate 目錄，`cargo run --release` 也採用這項規則。啟用 `standalone` 特性的建置使用可執行檔所在目錄。HTTP 和 Artisan 都從選定的應用程式根目錄讀取 `.env`，啟動時的工作目錄不影響選擇。

使用 `cargo build --release --locked --features standalone` 建置，將 `target/release/main`（Windows 上為 `target/release/main.exe`）與 `resources/views/welcome.html` 一併放入發佈目錄，保留檢視檔案的相對路徑，即可整體搬移。二進位檔與應用程式分開部署時，將 `APP_BASE_PATH` 設為應用程式根目錄的絕對路徑。部署靜態資源時，依原相對路徑保留 `public/`。使用預設公開磁碟掛載時，將要公開的儲存檔案放在 `storage/app/public/` 或設定的目錄中。

HTTP Kernel 建立時套用中介軟體設定，引導後解析路由中介軟體堆疊，完成後才繫結監聽連接埠。Console Kernel 引導同一個應用程式，接著註冊命令。

HTTP 核心執行[中介軟體](../the-basics/middleware.md)，並透過框架[路由器](../the-basics/routing.md)分派請求。應用程式骨架在 `/` 呼叫 `view("welcome")`。框架將檢視名稱解析為應用程式根目錄下的 `resources/views/welcome.html`，並在請求時讀取。修改 HTML 後，下次請求即可看到結果。`/up` 提供健康檢查。

預設應用程式透過 `bootstrap/app.rs` 的 `withRouting` 宣告 Web 路由、主控台路由及健康路徑。Web 路由使用[預設 Web 中介軟體群組](../the-basics/middleware.md)，主控台路由在 Console Kernel 引導後登記。API、廣播和 gRPC 使用各自的選用註冊入口。應用程式引導完成後，`/up` 處理器傳回 `200 OK`。

路由或中介軟體傳回 HTTP 錯誤時，核心套用 `bootstrap/app.rs` 中宣告的[例外轉譯策略](../the-basics/errors.md#將例外轉譯為-json)。

延遲 Provider 在首次解析服務時載入。Console Kernel 在註冊命令前載入所有剩餘延遲 Provider；HTTP Kernel 保留未使用項目。執行順序及非同步解析見[延遲服務提供者](providers.md#延遲服務提供者)。
