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

預設執行 `./main` 會啟動 HTTP 及其已設定的監聽器。背景服務可獨立執行，使用 `./main artisan queue:work`、`./main artisan schedule:work` 或 `./main artisan schedule:run`。

若要以一條命令啟動 HTTP、Worker 和排程器，在應用程式環境中設定以下值，然後執行 `./main`。

```dotenv
RUNTIME_ENABLED=true
RUNTIME_HTTP=true
RUNTIME_QUEUE=true
RUNTIME_SCHEDULE=true
```

啟用統一啟動後，各服務開關預設都是 `true`，設為 `false` 可停用對應服務。Worker 使用預設佇列連線及其預設佇列，跳過使用 `sync` 驅動程式的連線。應用程式註冊了排程工作才會啟動排程器。gRPC 等監聽器仍依各自的設定運作。沒有可執行的服務時，啟動會回傳錯誤。

Ctrl+C 或 Unix 的 SIGTERM 會停止接收新的 HTTP 請求、佇列工作和排程工作，並等待執行中的工作完成。任何受管理的服務結束也會觸發其餘服務關閉。Worker 設定為工作逾時後結束時，應用程式程序會保留它的逾時結束碼。統一啟動的 Worker 與其他服務共用程序，因此停用 Worker 的程序記憶體檢查。需要獨立的記憶體上限、重新啟動策略或命令參數時，請使用獨立的 Worker。 在共用執行環境中執行阻塞程式碼會拖慢 HTTP 和排程。這類工作可交由獨立 Worker 消費，也可使用佇列的強制逾時隔離執行。
