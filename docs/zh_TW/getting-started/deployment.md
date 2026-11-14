# 部署

## 建置應用

使用 Rust 1.94 或更新版本及儲存庫鎖定檔。在目標作業系統與架構建置，或使用另行驗收的交叉編譯工具鏈。

```sh
cargo +1.94.0 build --locked --release --features standalone
mkdir -p release/resources/views release/public release/storage/app/public
cp target/release/main release/main
cp resources/views/welcome.html release/resources/views/welcome.html
```

將網站資源複製到 `release/public`，公共儲存檔案複製到 `release/storage/app/public`。歡迎頁在執行時讀取，必須隨二進位檔部署。Rust 設定和路由宣告編譯進 `main`，修改 Rust 原始碼需要重新建置；執行時環境值和視圖檔案不需要。

`standalone` 特性以可執行檔所在目錄作為應用根目錄，不依賴工作目錄。絕對 `APP_BASE_PATH` 可覆蓋該位置。使用程序環境變數，或將 `.env` 放在應用根目錄；程序環境值優先。不要公開正式環境金鑰，也不要未經修改就把 `.env.example` 用作正式環境設定。

## 準備執行環境

啟動前設定正式環境值。設定 `APP_ENV=production`、`APP_DEBUG=false` 和正式環境的 `APP_KEY`。授予服務帳戶 `storage/` 及本機資料庫目錄的寫入權限。將持久化儲存和資料庫放在可替換的發行目錄之外，依需要在部署設定中使用絕對路徑。

將流量切換至新版本前，使用新產物執行遷移：

```sh
./main artisan migrate --force
```

首次部署 SQLite 時先建立設定指定的資料庫檔案。現有應用程式繼續使用其應用程式金鑰和持久化資料。見[遷移](../database/migrations.md)。

## 啟動 HTTP 與 Artisan

```sh
cd release
APP_HOST=127.0.0.1 APP_PORT=8088 ./main
```

在另一個終端進入發布目錄：

```sh
./main artisan list
./main artisan help inspire
./main artisan inspire
curl -i http://127.0.0.1:8088/up
```

將 `APP_URL` 設定為應用的公開 URL。未設定時，預設本機 URL 的連接埠取自 `APP_PORT`。公共磁碟使用絕對 URL 時必須與 `APP_URL` 同源。HTTP 監聽器在綁定前驗證連接埠；Artisan 無需合法的 HTTP 連接埠或 HTTP 監聽器。

`SIGINT` 以及 Unix 上的 `SIGTERM` 會請求 HTTP 優雅關閉。使用程序管理器負責重新啟動。已驗證的平台與驅動見[發行說明](../prologue/releases.md)。

原始碼的 `artisan` 腳本執行 Cargo。獨立發布無需 Cargo，使用 `./main artisan …` 執行命令。參閱 [Artisan](../digging-deeper/artisan.md)和[應用設定](configuration.md)。

## 一起執行服務

直接執行 `./main` 會啟動 HTTP、預設非同步佇列的 Worker，以及已註冊工作的排程器。`sync` 連線會略過 Worker。gRPC 等監聽器遵循各自的設定。啟動前先準備佇列後端。使用資料庫佇列時，請先執行上面的遷移指令。

設定 `APP_DISABLED_RUNNERS=queue,schedule` 可停用佇列和排程。以逗號分隔的清單寫入 `app.disabled_runners`，空清單允許所有符合條件的 Runner。內建簽名為 `http`、`queue`、`schedule`、`grpc` 和 `reverb`。可用 `q*` 等萬用字元模式比對簽名。模式支援 `*`、`?`、`[a-z]` 和 `[^a-z]` 等字元類別，以及反斜線跳脫。`*` 和 `?` 不跨越 `/`。應用程式會警告並略過無效模式。所有 Runner 均被篩除或不符合條件時，應用程式會正常結束。

Provider 透過 `ServiceProvider::runners` 提供 Runner，應用程式可用 `withRunners(|| vec![...])` 加入自訂 Runner，多次呼叫時以最後傳入的回呼為準。相同簽名以首次註冊為準，Provider 優先於自訂清單。應用程式先篩除停用的簽名，再呼叫 `shouldRun`。符合條件的 Runner 在應用程式程序內並行執行。

Ctrl+C、Unix SIGTERM 或 Runner 執行失敗時，應用程式依 `shutdownPriority` 由小到大關閉，同組並行，預設優先級為零。`run` 可透過 `RunnerContext` 取消權杖完成處理中的工作，自訂 `shutdown` 可釋放其他資源。`run` 成功回傳後仍會執行關閉鉤子，以支援自行啟動背景工作的 Runner。

框架新增的常駐服務在完成裝配與設定並符合啟動條件時，預設納入主應用程式啟動。各服務仍可停用或獨立執行。一次性 Artisan 指令由明確呼叫執行。

```sh
./main artisan queue:work
./main artisan schedule:work
./main artisan schedule:run
```
HTTP、受管理的 Worker 和排程器共用應用程式程序。受管理的 Worker 沿用獨立 `queue:work` 的逾時、重試和結束碼。程序 RSS 涵蓋三個服務的記憶體用量，因此受管理的 Worker 不套用獨立記憶體門檻。獨立 `queue:work` 保留現有記憶體上限。啟用強制逾時的工作仍由隔離執行器執行。Ctrl+C 或 Unix SIGTERM 會停止接收新工作，並等待處理中的 HTTP 請求、佇列工作和排程工作完成。應用程式程序會傳遞 Worker 的非零結束碼。
