# 安裝

## 環境需求

安裝 Rust 1.95.0 或更新版本及 Cargo。OpenRust 使用 Rust 2024 edition，根應用程式和 `vendor/openrust-framework/` 中的框架 crate 一起建置。首次建置會下載 `Cargo.lock` 記錄的相依套件。

## 完整初始化 {#complete-initialization}

先準備 MySQL 資料庫和 Redis 服務，再透過程序環境或應用程式環境檔提供連線。MySQL 使用 `DB_URL`，或提供 `DB_DATABASE` 和 `DB_USERNAME`。Redis 使用 `REDIS_URL` 或 `REDIS_HOST`。依實際服務補充主機、連接埠、密碼和 TLS 設定。

```sh
./artisan init:full --dry-run
./artisan init:full
cargo run --locked
```

`init:full` 全程非互動，寫入檔案前檢查設定。它啟用完整預設及網頁和 API 使用者套件，安裝套件資源，建置應用程式，執行遷移與 HTTP、Artisan 和 Redis Worker 自我檢查。快取、工作階段和佇列使用 Redis。MySQL、Redis 和 Docker Compose 由命令外的環境管理。透過程序環境注入的憑證仍由程序環境提供，啟動應用程式時也需要提供。既有驅動設定發生衝突時須先明確調整。`init:full --force` 用於明確授權生產遷移，並保留既有原始碼客製內容。需要互動選配時請使用 `./artisan init`。

`init:full` 透過套件探索啟用[機器翻譯套件](../packages/translate.md)。應用程式需要機器翻譯時，再設定具名服務並提供對應憑證。

單機和多副本使用同一套應用程式設定。同一部署內各執行角色共用 MySQL、Redis 和應用程式密鑰。執行角色、遷移協調和檔案共享由部署工具管理。

## 啟動應用程式

在應用程式根目錄執行下列命令。建立新的本機環境時複製環境範本。

```sh
cp .env.example .env
touch database/database.sqlite
cargo +1.95.0 build --locked
./artisan key:generate
./artisan migrate
./artisan inspire
cargo +1.95.0 run --locked
```

環境範本使用 SQLite 儲存應用程式資料、工作階段、快取及佇列。使用這些服務前先建立資料庫檔案並執行遷移。

HTTP 預設監聽 `127.0.0.1:8000`。存取 `http://localhost:8000/` 查看歡迎頁，存取 `/up` 檢查服務狀態。使用 Ctrl+C 停止服務。

應用程式也能在沒有環境檔案時啟動。需要其他連接埠時，在 `.env` 中設定 `APP_PORT=9081`。使用範本的預設監聽主機時，`APP_URL` 會展開為 `http://127.0.0.1:9081`。處理程序環境優先於檔案中的同名值，例如 `APP_PORT=9081 cargo +1.95.0 run --locked` 會監聽 9081。URL 覆寫和環境檔案選擇見[設定](configuration.md)。

Windows 可用 `copy .env.example .env` 複製範本，再執行 `artisan.bat inspire`。使用目標架構的 MSVC Rust 工具鏈。獨立發佈可依 Windows 部署步驟執行。 [Windows 獨立部署](deployment.md#windows-standalone)

## 建置獨立發佈產物

```sh
cargo +1.95.0 build --release --locked --features standalone
```

將 `target/release/main` 放在發佈根目錄，隨包提供 `resources/views/welcome.blade.html`。網站檔案依原路徑放在 `public/`，公開磁碟檔案放在 `storage/app/public/`。可執行檔從所在目錄讀取 `.env`。應用程式根目錄位於其他位置時，透過處理程序環境設定絕對路徑 `APP_BASE_PATH`。在產物所在目錄執行 `./main artisan inspire`，也可以使用可執行檔的絕對路徑。原始碼和獨立產物的目錄解析規則見[請求生命週期](../architecture-concepts/lifecycle.md)。

下載相依套件後可加入 `--offline` 進行鎖定離線建置，建置仍需完整的應用程式及框架原始碼。
