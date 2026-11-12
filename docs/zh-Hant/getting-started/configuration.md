# 設定

在應用程式根目錄將 `.env.example` 複製為 `.env`，即可自訂本機設定。`APP_HOST` 和 `APP_PORT` 設定 HTTP 監聽位址，預設為 `127.0.0.1:8000`。未設定 `APP_URL` 時，預設 URL 使用 `http://localhost` 和已設定的 `APP_PORT`。範本沿用 Laravel 的本機開發預設值。應用程式將 `config/` 下的各區段載入同一個儲存庫，可以透過 `application.config()?.get("app.url")` 讀取最終生效的值。應用程式、驗證、資料庫與 Redis、快取、工作階段、日誌、郵件、佇列、檔案系統、廣播、CORS、並行、雜湊、影像、第三方服務、Reverb、gRPC 和檢視各有獨立設定。載入設定時不連線至外部服務。

## 目前設定儲存庫

設定在 boot 中、Provider register 前可用。與 Laravel LoadConfiguration 一樣，boot 先綁定新儲存庫，再執行設定載入器並填入該儲存庫；`create()` 不提前放入空設定服務。`Application::config()` 每次解析目前 `config` 服務，傳回 `Result<Arc<dyn Repository>, BindingResolutionError>`；引導前存取或替換為不相容型別後存取會傳回錯誤。

`config` 是正規名稱，`dyn openrust::contracts::config::Repository` 是其型別別名。替換應用儲存庫時，使用 `openrust::config::Repository::new(values)` 建構，再呼叫 `instanceNamed::<dyn Repository>("config", repository)?`。已經持有 `Arc` 的消費者保留原快照；需要跟隨替換時重新解析，或使用重綁定/refresh 監聽器。按原始型別鍵重新綁定會替換該別名，而不是正規設定服務，遵循容器的原鍵規則。

Provider 替換的儲存庫用於 HTTP 監聽綁定、路由/靜態磁碟裝配及檢視路徑設定。健康頁 HTML 在每次請求時讀取目前儲存庫，Artisan 使用相同應用存取器。執行期替換設定不會自動重建已經裝配的路由或檢視尋找器。容器錯誤沿相應的啟動、HTTP 或 Artisan 錯誤路徑傳播。參見[重綁定](../architecture-concepts/container.md#重綁定與重新整理消費者)。

數值和布林設定在啟動時驗證。布林值接受 `true`、`false`、`1` 和 `0`。`SESSION_DOMAIN` 等可空設定接受 `null`。執行驅動程式需要框架提供對應服務。

HTTP 繫結監聽器時驗證 `APP_PORT`。Artisan 無須有效的 HTTP 監聽位址。連接埠 `0` 允許作業系統選擇監聽連接埠，需要穩定的對外 URL 時請明確設定 `APP_URL`。

HTTP 檢視轉譯器依序查找 `view.paths` 中的目錄，預設從 `resources/views` 讀取 HTML。相對目錄以應用程式根目錄為基準。

程序環境變數優先於檔案中的值。在程序環境中設定 `APP_ENV` 可選擇 `.env.local` 等檔案。對應檔案不存在時，應用程式讀取 `.env`。部署路徑和 `APP_BASE_PATH` 的用法見[應用程式根目錄規則](../architecture-concepts/lifecycle.md)。

<a id="determining-the-current-environment"></a>

## 確定目前環境

應用在載入設定之後、Provider 登記之前偵測目前環境。偵測讀取 `app.env` 字串，預設值為 `production`。預設設定從 `APP_ENV` 讀取該值，HTTP 依此設定啟動。

Artisan 支援在命令前後傳入 `--env=staging` 或 `--env staging`。同一次呼叫傳入多次時，取第一個值。載入器依序嘗試 `--env` 對應檔案、外部 `APP_ENV` 對應檔案及 `.env`。行程環境變數優先於檔案中的值。即使檔案回退，本次啟動偵測到的環境也取 `--env` 的值，設定中的 `app.env` 保留載入後的值。

```sh
./artisan env --env=staging
```

向 `Application::environment` 傳入 `None` 可讀取偵測到的環境名稱。將單一模式或模式切片放入 `Some` 可取得布林結果。比對區分大小寫，`*` 可比對任意字元，空切片回傳 false。應用啟動時也將同一比對規則安裝為容器的環境解析器。

```rust
use openrust::{
    container::EnvironmentResult,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = Application::configure(std::env::current_dir()?)
        .withConfig(|_, _| {
            Ok([("app".into(), serde_json::json!({"env": "staging"}))].into())
        })
        .create()?;
    app.boot().await?;

    assert_eq!(app.environment(None)?, EnvironmentResult::from("staging"));
    assert!(bool::from(app.environment(Some("stag*".into()))?));
    assert!(bool::from(app.container().currentEnvironmentIs(&["local", "staging"])?));
    assert!(!bool::from(app.container().currentEnvironmentIs(&[])?));
    Ok(())
}
```

引導階段將偵測到的環境名稱以 `Arc<str>` 儲存到 `env` 服務。環境查詢讀取該服務。啟動後修改 `app.env` 只更新設定。

<a id="public-files"></a>

## 公開檔案

| 環境變數 | 用途 | 預設值 |
|---|---|---|
| `PUBLIC_PATH` | `Application::publicPath()` 傳回的應用程式公開目錄 | `public` |
| `PUBLIC_URL_PREFIX` | 應用程式公開目錄的 URL 前綴 | `/` |
| `PUBLIC_STORAGE_ROOT` | 公開本機磁碟的 `filesystems.disks.public.root` | `storage/app/public` |
| `PUBLIC_STORAGE_URL` | 公開本機磁碟的 `filesystems.disks.public.url` | `APP_URL` 加 `/storage` |

相對目錄以應用程式根目錄為基準。應用程式可在啟動前呼叫 `usePublicPath()` 覆寫 `PUBLIC_PATH`。預設 Web 路由透過 `Route::staticDisk("public")` 套用公開磁碟設定。掛載前綴取磁碟 URL 的完整路徑，預設為 `/storage`。以 `/storage/tmp/` 結尾的同源 URL 或絕對 URL 路徑 `/storage/tmp/` 都會掛載到 `/storage/tmp`。跨來源 URL 會使路由註冊失敗。修改掛載設定後重新啟動應用程式。

## 維護模式 {#maintenance-mode}

部署時執行 Artisan 的 `down`，應用程式會回傳 HTTP 503。執行 `up` 恢復存取。執行中的服務會逐一請求讀取共用狀態，可以再次執行 `down` 更新選項。

```bash
./artisan down --retry=60 --with-secret
./artisan up
```

`--retry` 接受正數秒數或重試日期，`--refresh` 指定瀏覽器重新整理的間隔。未註明時區的日期時間使用應用程式時區，回應標頭統一使用 GMT。`--redirect` 接受站內路徑。`--render` 在執行 `down` 時將具名檢視渲染一次，並傳入 `$retryAfter`。`--status` 可覆寫預設的 503。預期 JSON 的請求沿用一般例外渲染，略過瀏覽器重新導向與預先渲染的頁面。

維護期間可開啟 `--with-secret` 印出的 URL 檢查網站。應用程式會設定有效期為 12 小時的簽名 Cookie，當前工作階段沒有原目標時重新導向至首頁。可以使用 `--secret` 指定自己的路徑。若要保留端點，在 `bootstrap/app.rs` 的 `withMiddleware` 設定。

```rust
.withMiddleware(|middleware| {
    middleware.preventRequestsDuringMaintenance(["up", "status/*"]);
})
```

預先渲染的頁面由執行中的 HTTP 程序提供。服務重新啟動時，應用程式成功啟動後開始監聽。佇列行為請見[維護期間的 Worker](../digging-deeper/queues.md#workers-during-maintenance)。

### 共用維護狀態 {#shared-maintenance-state}

應用程式透過 `maintenanceMode()` 提供維護狀態。預設 `file` 驅動程式把資料寫入 `storage/framework/down`。選擇 `cache` 和共享快取儲存，可讓多台主機上的 Worker 讀取相同狀態。`array` 驅動程式將狀態儲存在目前應用程式實例內，可用於測試。

```rust
use openrust::foundation::Application;
use serde_json::{Map, json};

async fn activate_maintenance(
    app: &Application,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let payload = Map::from_iter([("status".into(), json!(503))]);
    app.maintenanceMode()?.activate(&payload).await?;
    Ok(())
}
```

用 `isDownForMaintenance().await` 讀取狀態，用 `app.maintenanceMode()?.deactivate().await?` 清除。佇列 Worker 在保留工作前讀取此狀態。
