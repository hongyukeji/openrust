# 設定

在應用程式根目錄將 `.env.example` 複製為 `.env`，即可自訂本機設定。`APP_HOST` 和 `APP_PORT` 設定 HTTP 監聽位址，預設為 `127.0.0.1:8000`。範本沿用 Laravel 的本機開發預設值。應用程式將 `config/` 下的各區段載入同一個儲存庫，可以透過 `application.config()?.get("app.url")` 讀取最終生效的值。應用程式、驗證、資料庫與 Redis、快取、工作階段、日誌、郵件、佇列、檔案系統、廣播、CORS、並行、雜湊、影像、第三方服務、Reverb、gRPC 和檢視各有獨立設定。載入設定時不連線至外部服務。

`app.url` 優先使用明確設定的 `APP_URL`。未設定時，使用具體的 `APP_HOST` 和 `APP_PORT` 產生 URL，例如 `http://example.test:8080` 或 `http://[2001:db8::1]:8080`。未設定主機，或使用 `127.0.0.1`、`::1` 等回送位址或 `0.0.0.0`、`::` 等萬用監聽位址時，使用 `http://localhost:{port}`。環境範本設定了 `APP_URL="http://${APP_HOST}:${APP_PORT}"`，使用範本預設值時會展開為 `http://127.0.0.1:8000`。

HTTP 請求內產生 URL 時，包括使用 `URL::forRequest(request)`，優先使用明確設定的 `APP_URL`，其次是目前請求的來源。請求沒有來源時，使用從監聽設定推導出的 `app.url`。Artisan 直接使用 `app.url`。

設定 `APP_VERSION` 可覆寫 `app.version` 中的應用程式版本。未設定時，預設採用編譯時由 `env!("CARGO_PKG_VERSION")` 讀取的 Cargo 套件版本。歡迎頁顯示 `app.version`。

## 目前設定儲存庫

設定在 boot 中、Provider register 前可用。與 Laravel LoadConfiguration 一樣，boot 先綁定新儲存庫，再執行設定載入器並填入該儲存庫；`create()` 不提前放入空設定服務。`Application::config()` 每次解析目前 `config` 服務，傳回 `Result<Arc<dyn Repository>, BindingResolutionError>`；引導前存取或替換為不相容型別後存取會傳回錯誤。

在應用作用域內，匯入 `openrust::support::facades::Config`，透過 `Config::get("app.name")?` 讀取設定。門面每次呼叫都解析目前應用的儲存庫。鍵缺少時傳回 `None`，顯式空值傳回 `Some(Value::Null)`。在應用作用域外或儲存庫註冊前呼叫會傳回綁定錯誤。

`config` 是正規名稱，`dyn openrust::contracts::config::Repository` 是其型別別名。替換應用儲存庫時，使用 `openrust::config::Repository::new(values)` 建構，再呼叫 `instanceNamed::<dyn Repository>("config", repository)?`。已經持有 `Arc` 的消費者保留原快照；需要跟隨替換時重新解析，或使用重綁定/refresh 監聽器。按原始型別鍵重新綁定會替換該別名，而不是正規設定服務，遵循容器的原鍵規則。

Provider 替換的儲存庫用於 HTTP 監聽綁定、路由/靜態磁碟裝配及檢視路徑設定。健康頁 HTML 在每次請求時讀取目前儲存庫，Artisan 使用相同應用存取器。執行期替換設定不會自動重建已經裝配的路由或檢視尋找器。容器錯誤沿相應的啟動、HTTP 或 Artisan 錯誤路徑傳播。參見[重綁定](../architecture-concepts/container.md#重綁定與重新整理消費者)。

數值和布林設定在啟動時驗證。布林值接受 `true`、`false`、`1` 和 `0`。`SESSION_DOMAIN` 等可空設定接受 `null`。執行驅動程式需要框架提供對應服務。

HTTP 繫結監聽器時驗證 `APP_PORT`。Artisan 無須有效的 HTTP 監聽位址。連接埠 `0` 允許作業系統選擇監聽連接埠，需要穩定的對外 URL 時請明確設定 `APP_URL`。

HTTP 檢視轉譯器依序查找 `view.paths` 中的目錄，預設從 `resources/views` 讀取 HTML。相對目錄以應用程式根目錄為基準。

程序環境變數優先於檔案中的值。在程序環境中設定 `APP_ENV` 可選擇 `.env.local` 等檔案。對應檔案不存在時，應用程式讀取 `.env`。部署路徑和 `APP_BASE_PATH` 的用法見[應用程式根目錄規則](../architecture-concepts/lifecycle.md)。

## 存取設定值 {#accessing-configuration-values}

```rust
use openrust::support::facades::Config;

pub fn application_name() -> String {
    Config::stringOr("app.name", "App")
}
```

以 `Config::set` 更新目前應用程式的設定。`string`、`integer`、`float`、`boolean`、`array` 和 `collection` 檢查值的型別。`array` 接受 JSON 清單和物件，`collection` 保留集合鍵。

用 `Config::stringOr("app.name", "App")` 直接取得字串。缺少鍵、值為 `null` 或型別不符、設定儲存庫無法使用時，均使用傳入的預設值。需要嚴格讀取並處理錯誤時，用 `Config::string("app.name")?`。原始值讀取 `Config::getOr("app.name", Value::from("App"))?` 僅在缺少鍵時使用預設值，並保留顯式空值。`Config::getMany([("app.name", Value::from("App")), ("app.debug", Value::Bool(false))])?` 按輸入順序讀取多個鍵，各自套用預設值。讀取整組設定時，為結構體衍生 `serde::Deserialize`，再用 `Config::deserialize::<MailOptions>("mail")?`。

## 設定快取 {#configuration-caching}

以 `config:cache` 儲存應用程式重新啟動後的設定，包含服務提供者設定的值。後續 HTTP 與 Artisan 啟動會讀取這份快照。更新設定後，用 `config:clear` 移除快照，再重新啟動應用程式。

```sh
./artisan config:cache
./artisan config:clear
```

設定 `APP_URL` 並產生設定快取後，一般連結和絕對簽章連結都會使用該位址。

更新時先移除所選的舊快照，再引導新應用程式。若引導失敗，快取仍為空。

預設路徑為 `storage/framework/config.json`。可用 `APP_CONFIG_CACHE` 指定絕對路徑或相對於應用程式根目錄的檔案路徑。將它設定在程序環境中，可在讀取 `.env` 前選定路徑。不同部署環境請使用各自的快取檔案。

若在載入 dotenv 前命中快取，應用程式會略過 dotenv 解析。快取包含生效設定，可能含有金鑰等秘密，請與私有部署資料一起保管。無效的 JSON 會中止啟動。快取損壞時，請先移除檔案再重建。

以 `./artisan config:show app.name` 查看單一鍵，或以 `./artisan config:show app` 查看設定區段。指令會顯示目前生效的值，其中可能包含憑證。

## 環境檔案加密 {#environment-file-encryption}

使用 `env:encrypt` 建立加密副本。執行 `env:decrypt` 前在程序環境中設定 `ENV_ENCRYPTION_KEY`，或傳入 `--key`。若由加密指令產生金鑰，請將輸出的 `base64:` 值儲存至金鑰管理系統，與加密檔案分開保管。

```sh
./artisan env:encrypt --env=staging --no-interaction
./artisan env:decrypt --env=staging --force --no-interaction
```

預設密碼演算法為 AES-256-CBC。選用 AES-128-CBC、AES-128-GCM 或 AES-256-GCM 時，加密與解密需使用相同的 `--cipher`。`--env=staging` 選擇 `.env.staging` 與 `.env.staging.encrypted`。省略 `--env` 時，加密來源由啟動時選擇的環境檔案決定，解密預設寫入 `.env`。

使用 `--readable` 逐項加密，變數名稱保持可見。之後用相同金鑰更新可讀密文，值未改變的項目會沿用原密文。`--force` 覆寫既有輸出，`--prune` 在加密成功後刪除明文。解密時可用 `--path` 和 `--filename` 指定目標目錄與檔名。

指令不會回顯手動傳入的金鑰。加密時使用 `--quiet` 或 `--silent` 需傳入金鑰，以便日後解密。

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
| `APP_PUBLIC_PATH` | `filesystems.disks.web.root` / `Application::publicPath()` | `public` |
| `APP_PUBLIC_URL_PREFIX` | `filesystems.disks.web.url` | `/` |
| `PUBLIC_STORAGE_ROOT` | 公開本機磁碟的 `filesystems.disks.public.root` | `storage/app/public` |
| `PUBLIC_STORAGE_URL` | 公開本機磁碟的 `filesystems.disks.public.url` | `app.url` 加 `/storage` |

未設定 `APP_PUBLIC_PATH` 時，`config/app.rs` 明確將 `app.public_path` 設為 `public`，`web` 磁碟沿用該值作為預設根目錄。網站檔案設定在 `filesystems.disks.web`，公開上傳檔案設定在 `filesystems.disks.public`。預設路由依序呼叫 `Route::staticDisk("web")` 和 `Route::staticDisk("public")`。引導前呼叫 `usePublicPath()` 會覆寫網站目錄並同步 `web.root`。相對目錄以應用程式根目錄為基準。掛載前綴取磁碟 URL 的完整路徑，例如 `/storage/tmp/` 掛載到 `/storage/tmp`。絕對 URL 必須與 `app.url` 同源。修改掛載設定後重新啟動應用程式。 明確設定為空字串或全空白的 `APP_PUBLIC_PATH` 會導致設定載入失敗。

## 維護模式 {#maintenance-mode}

部署時執行 Artisan 的 `down`，應用程式會回傳 HTTP 503。執行 `up` 恢復存取。執行中的服務會逐一請求讀取共用狀態，可以再次執行 `down` 更新選項。

```bash
./artisan down --retry=60 --with-secret
./artisan up
```

`--retry` 接受正數秒數或重試日期，`--refresh` 指定瀏覽器重新整理的間隔。未註明時區的日期時間使用應用程式時區，回應標頭統一使用 GMT。`--redirect` 接受站內路徑。`--render` 在執行 `down` 時將具名檢視渲染一次，並傳入 `$retryAfter`。`--status` 可覆寫預設的 503。預期 JSON 的請求沿用一般例外渲染，略過瀏覽器重新導向與預先渲染的頁面。

維護期間可開啟 `--with-secret` 印出的 URL 檢查網站。應用程式會設定有效期為 12 小時的簽名 Cookie，當前工作階段沒有原目標時重新導向至首頁。可以使用 `--secret` 指定自己的路徑。設定的健康路徑（預設 `/up`）在維護期間仍可存取。若要保留其他端點，在 `bootstrap/app.rs` 的 `withMiddleware` 設定。

```rust
.withMiddleware(|middleware| {
    middleware.preventRequestsDuringMaintenance(["status/*"]);
})
```

使用檔案驅動部署時，`./artisan down --render=errors::503` 可在 Provider 啟動期間提供預先渲染的頁面。可以在 `resources/views/errors/503.blade.html` 自訂頁面。監聽設定需成功載入。JSON 請求、排除路徑及有效繞過請求會等待應用程式核心。快取驅動由執行中的核心提供維護回應。佇列行為請見[維護期間的 Worker](../digging-deeper/queues.md#workers-during-maintenance)。

啟動期間請使用 Artisan 印出的完整繞過 URL。啟動前處理器會比對原始請求位址，查詢字串和尾斜線都會參與比對。HTTP 核心啟動後，維護中介層按路徑比對，忽略查詢字串及路徑兩端的斜線。

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

## 測試環境與啟動快照 {#testing-environment}

`Application::runningUnitTests()` 和應用契約傳回 `Result<bool, BindingResolutionError>`。僅當容器中的 `env` 精確為 `testing` 時結果為 true，與是否正在執行測試程式無關。環境未綁定時傳回 `Ok(false)`，不觸發服務解析；解析錯誤沿 `Result` 傳遞。後續修改 `config("app.env")` 不會替換已偵測的環境綁定。

啟動後，透過 `app.make::<openrust::config::Environment>().await?` 讀取目前應用不可變的環境快照。既有 `get`、`get_utf8` 傳回原始字串，具體旗標仍由對應套件解析。快照遵守處理程序變數優先、環境檔案選擇與設定快取規則，不重新載入檔案，也不修改處理程序全域變數。

## 應用儲存路徑 {#application-storage-path}

`app.storagePath("")` 傳回儲存根目錄，傳入 `"pail"` 等字串可取得子路徑。`app.useStoragePath(path)?` 覆蓋它並重新綁定 `path.storage`，應用契約也提供 getter。`openrust::support::storage_path(path)?` 讀取目前應用作用域。唯讀的 `foundation::Paths` 服務讓編譯型套件共用相同狀態，避免透過容器強持有 Application。

優先順序是非空且非 `0` 的明確覆蓋、應用環境快照中的 `APP_STORAGE_PATH`、應用根目錄下的 `storage`。該無品牌變數對應 Laravel 的 `LARAVEL_STORAGE_PATH`；環境中明確存在的空值仍會保留。Setter 傳入相對路徑時保持相對。串接追加本機分隔符並去掉子路徑開頭的本機分隔符，保留重複分隔符和 `..`。Rust 沒有預設參數，讀取根目錄時明確傳入 `""`。

建構時初始化 `path.storage`，後載入的環境檔案可以改變 getter，但不會自動重寫該綁定；`useStoragePath` 則明確重新綁定，沿用 Laravel 的順序。重新綁定錯誤透過 `Result` 傳回，此時路徑狀態已更新。

日誌、檔案 Session、檔案快取資料與鎖、編譯檢視以及預設 local/public 磁碟在載入設定時採用儲存根目錄。`PUBLIC_STORAGE_ROOT` 和 `VIEW_COMPILED_PATH` 仍可個別覆蓋，公開 URL 和掛載順序不變。`storage:link` 指向設定的公開磁碟。啟動前設定絕對路徑的 `APP_STORAGE_PATH`，或在載入設定前呼叫 `useStoragePath`。

編譯型設定載入器使用 `env.storage_path(base_path, subpath)` 作為 Rust 上下文適配。弱引用選取所屬應用的路徑，不依賴全域 Facade，也不延長應用生命週期。獨立環境快照依自身變數及傳入的預設根目錄解析。後續更改應用路徑不會回寫已經載入或快取的設定值。

檔案維護模式每次操作都解析目前路徑，服務已解析後仍有效。啟動前的預先轉譯檢查使用 boot 前可用的路徑：此時應提供程序變數或明確 setter，僅在 dotenv 中設定的值尚不可用。Pail 在首次解析 Files 時決定目錄，應在首次使用前設定。設定、路由、事件和套件快取產物的位置繼續遵守各自的快取路徑契約。
