# 設定

在應用程式根目錄將 `.env.example` 複製為 `.env`，即可自訂本機設定。`APP_HOST` 和 `APP_PORT` 設定 HTTP 監聽位址，預設為 `127.0.0.1:8000`。未設定 `APP_URL` 時，預設 URL 使用 `http://localhost` 和已設定的 `APP_PORT`。範本沿用 Laravel 的本機開發預設值。應用程式將 `config/` 下的各區段載入同一個儲存庫，可以透過 `application.config().get("app.url")` 讀取最終生效的值。應用程式、驗證、資料庫與 Redis、快取、工作階段、日誌、郵件、佇列、檔案系統、廣播、CORS、並行、雜湊、影像、第三方服務、Reverb、gRPC 和檢視各有獨立設定。載入設定時不連線至外部服務。

數值和布林設定在啟動時驗證。布林值接受 `true`、`false`、`1` 和 `0`。`SESSION_DOMAIN` 等可空設定接受 `null`。執行驅動程式需要框架提供對應服務。

HTTP 繫結監聽器時驗證 `APP_PORT`。Artisan 無須有效的 HTTP 監聽位址。連接埠 `0` 允許作業系統選擇監聽連接埠，需要穩定的對外 URL 時請明確設定 `APP_URL`。

HTTP 檢視轉譯器依序查找 `view.paths` 中的目錄，預設從 `resources/views` 讀取 HTML。相對目錄以應用程式根目錄為基準。

程序環境變數優先於檔案中的值。在程序環境中設定 `APP_ENV` 可選擇 `.env.local` 等檔案。對應檔案不存在時，應用程式讀取 `.env`。部署路徑和 `APP_BASE_PATH` 的用法見[應用程式根目錄規則](../architecture-concepts/lifecycle.md)。

<a id="public-files"></a>

## 公開檔案

| 環境變數 | 用途 | 預設值 |
|---|---|---|
| `PUBLIC_PATH` | `Application::publicPath()` 傳回的應用程式公開目錄 | `public` |
| `PUBLIC_URL_PREFIX` | 應用程式公開目錄的 URL 前綴 | `/` |
| `PUBLIC_STORAGE_ROOT` | 公開本機磁碟的 `filesystems.disks.public.root` | `storage/app/public` |
| `PUBLIC_STORAGE_URL` | 公開本機磁碟的 `filesystems.disks.public.url` | `APP_URL` 加 `/storage` |

相對目錄以應用程式根目錄為基準。應用程式可在啟動前呼叫 `usePublicPath()` 覆寫 `PUBLIC_PATH`。預設 Web 路由透過 `Route::staticDisk("public")` 套用公開磁碟設定。掛載前綴取磁碟 URL 的完整路徑，預設為 `/storage`。以 `/storage/tmp/` 結尾的同源 URL 或絕對 URL 路徑 `/storage/tmp/` 都會掛載到 `/storage/tmp`。跨來源 URL 會使路由註冊失敗。修改掛載設定後重新啟動應用程式。
