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

`SIGINT` 以及 Unix 上的 `SIGTERM` 會請求 HTTP 優雅關閉。使用程序管理器負責重新啟動。正式環境反向代理、TLS、資料庫驅動及平台支援需要針對部署環境驗收；本次最小交付涵蓋 macOS ARM64 啟動路徑。Windows `artisan.bat` 仍待實機驗收。

原始碼的 `artisan` 腳本執行 Cargo。獨立發布無需 Cargo，使用 `./main artisan …` 執行命令。參閱 [Artisan](../digging-deeper/artisan.md)和[應用設定](configuration.md)。
