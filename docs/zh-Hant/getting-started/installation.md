# 安裝

## 環境需求

安裝 Rust 1.94.0 或更新版本及 Cargo。OpenRust 使用 Rust 2024 edition，根應用程式和 `vendor/openrust/framework/` 中的框架 crate 一起建置。首次建置會下載 `Cargo.lock` 記錄的相依套件。

## 啟動應用程式

在應用程式根目錄執行下列命令。建立新的本機環境時複製環境範本。

```sh
cp .env.example .env
cargo +1.94.0 build --locked
./artisan inspire
cargo +1.94.0 run --locked
```

HTTP 預設監聽 `127.0.0.1:8000`。存取 `http://localhost:8000/` 查看歡迎頁，存取 `/up` 檢查服務狀態。使用 Ctrl+C 停止服務。

應用程式也能在沒有環境檔案時啟動。需要其他連接埠時，在 `.env` 中設定 `APP_PORT=9081`。範本的 `APP_URL="http://localhost:${APP_PORT}"` 會展開連接埠變數。處理程序環境優先於檔案中的同名值，例如 `APP_PORT=9081 cargo +1.94.0 run --locked` 會監聽 9081。URL 覆寫和環境檔案選擇見[設定](configuration.md)。

Windows 可用 `copy .env.example .env` 複製範本，再執行 `artisan.bat inspire`。Windows 實機執行仍待驗收。

## 建置獨立發佈產物

```sh
cargo +1.94.0 build --release --locked --features standalone
```

將 `target/release/main` 放在發佈根目錄，隨包提供 `resources/views/welcome.html`。網站檔案依原路徑放在 `public/`，公開磁碟檔案放在 `storage/app/public/`。可執行檔從所在目錄讀取 `.env`。應用程式根目錄位於其他位置時，透過處理程序環境設定絕對路徑 `APP_BASE_PATH`。在產物所在目錄執行 `./main artisan inspire`，也可以使用可執行檔的絕對路徑。原始碼和獨立產物的目錄解析規則見[請求生命週期](../architecture-concepts/lifecycle.md)。

下載相依套件後可加入 `--offline` 進行鎖定離線建置，建置仍需完整的應用程式及框架原始碼。
