# 設定

應用程式名稱和版本預設使用應用程式 `Cargo.toml` 中的 `name` 和 `version`，在編譯時嵌入。可在 `.env` 中透過 `APP_NAME` 和 `APP_VERSION` 分別覆寫。骨架的 `.env.example` 明確設定了 `APP_NAME=OpenRust`，此值優先於套件名稱。

在應用程式根目錄將 `.env.example` 複製為 `.env`，即可自訂本機設定。`APP_HOST` 和 `APP_PORT` 設定 HTTP 監聽位址，預設為 `127.0.0.1:8000`。範本沿用 本機開發預設值。應用程式將 `config/` 下的各區段載入同一個儲存庫，可以透過 `application.config()?.get("app.url")` 讀取最終生效的值。應用程式、驗證、資料庫與 Redis、快取、工作階段、日誌、郵件、佇列、檔案系統、廣播、CORS、並行、雜湊、影像、第三方服務、Reverb、gRPC 和檢視各有獨立設定。載入設定時不連線至外部服務。

`app.url` 優先使用明確設定的 `APP_URL`。未設定時，使用具體的 `APP_HOST` 和 `APP_PORT` 產生 URL，例如 `http://example.test:8080` 或 `http://[2001:db8::1]:8080`。未設定主機，或使用 `127.0.0.1`、`::1` 等回送位址或 `0.0.0.0`、`::` 等萬用監聽位址時，使用 `http://localhost:{port}`。環境範本設定了 `APP_URL="http://${APP_HOST}:${APP_PORT}"`，使用範本預設值時會展開為 `http://127.0.0.1:8000`。

HTTP 請求內產生 URL 時，包括使用 `Url::for_request(request)`，優先使用明確設定的 `APP_URL`，其次是目前請求的來源。請求沒有來源時，使用從監聽設定推導出的 `app.url`。Artisan 直接使用 `app.url`。

設定 `APP_VERSION` 可覆寫 `app.version` 中的應用程式版本。未設定時，預設採用編譯時由 `env!("CARGO_PKG_VERSION")` 讀取的 Cargo 套件版本。歡迎頁顯示 `app.version`。

## 框架版本與應用版本 {#framework-version}

`Application::VERSION` 和 `application.version()` 傳回框架套件版本，編譯時取自 `vendor/openrust/framework/Cargo.toml`。應用程式作用域內可用 `App::version()?` 解析目前的 Application 並取得相同版本；作用域外傳回容器綁定錯誤。常數和實例方法無需先引導設定。

```rust
use openrust::{foundation::Application, support::facades::App};

let framework_version = Application::VERSION;
let current_framework_version = App::version()?;
```

`./artisan --version` 使用既有的 `artisan <version>` 格式顯示框架版本。`Config::get("app.version")?` 則讀取應用程式版本，預設取應用程式套件版本，可透過 `APP_VERSION` 覆寫；歡迎頁繼續顯示該應用程式版本。修改應用程式套件版本、`app.version` 或 `APP_VERSION` 均不改變框架版本。更新框架套件後須重新編譯，才能嵌入新的框架版本。

## 目前設定儲存庫

設定在 boot 中、Provider register 前可用。與框架LoadConfiguration 一樣，boot 先綁定新儲存庫，再執行設定載入器並填入該儲存庫；`create()` 不提前放入空設定服務。`Application::config()` 每次解析目前 `config` 服務，傳回 `Result<Arc<dyn Repository>, BindingResolutionError>`；引導前存取或替換為不相容型別後存取會傳回錯誤。

在應用作用域內，匯入 `openrust::support::facades::Config`，透過 `Config::get("app.name")?` 讀取設定。門面每次呼叫都解析目前應用的儲存庫。鍵缺少時傳回 `None`，顯式空值傳回 `Some(Value::Null)`。在應用作用域外或儲存庫註冊前呼叫會傳回綁定錯誤。

`config` 是正規名稱，`dyn openrust::contracts::config::Repository` 是其型別別名。替換應用儲存庫時，使用 `openrust::config::Repository::new(values)` 建構，再呼叫 `instance_named::<dyn Repository>("config", repository)?`。已經持有 `Arc` 的消費者保留原快照；需要跟隨替換時重新解析，或使用重綁定/refresh 監聽器。按原始型別鍵重新綁定會替換該別名，而不是正規設定服務，遵循容器的原鍵規則。

Provider 替換的儲存庫用於 HTTP 監聽綁定、路由/靜態磁碟裝配及檢視路徑設定。健康頁 HTML 在每次請求時讀取目前儲存庫，Artisan 使用相同應用存取器。執行期替換設定不會自動重建已經裝配的路由或檢視尋找器。容器錯誤沿相應的啟動、HTTP 或 Artisan 錯誤路徑傳播。參見[重綁定](../architecture-concepts/container.md#重綁定與重新整理消費者)。

數值和布林設定在啟動時驗證。布林值接受 `true`、`false`、`1` 和 `0`。`SESSION_DOMAIN` 等可空設定接受 `null`。執行驅動程式需要框架提供對應服務。

HTTP 繫結監聽器時驗證 `APP_PORT`。Artisan 無須有效的 HTTP 監聽位址。連接埠 `0` 允許作業系統選擇監聽連接埠，需要穩定的對外 URL 時請明確設定 `APP_URL`。

HTTP 檢視轉譯器依序查找 `view.paths` 中的目錄，預設從 `resources/views` 讀取 HTML。相對目錄以應用程式根目錄為基準。

程序環境變數優先於檔案中的值。在程序環境中設定 `APP_ENV` 可選擇 `.env.local` 等檔案。對應檔案不存在時，應用程式讀取 `.env`。部署路徑和 `APP_BASE_PATH` 的用法見[應用程式根目錄規則](../architecture-concepts/lifecycle.md)。

## 存取設定值 {#accessing-configuration-values}

```rust
use openrust::support::facades::Config;

pub fn application_name() -> String {
    Config::string_or("app.name", "App")
}
```

以 `Config::set` 更新目前應用程式的設定。`string`、`integer`、`float`、`boolean`、`array` 和 `collection` 檢查值的型別。`array` 接受 JSON 清單和物件，`collection` 保留集合鍵。

語言切換使用 [App 語言方法](../digging-deeper/localization.md#配置語言)，同步設定、譯器和切換事件。在請求或佇列工作中，`app.locale`、`app.fallback_locale` 的修改僅影響目前作用域；整組替換 `app` 設定也僅影響目前作用域，並清除原有子鍵覆寫。直接修改 `Config` 不會自動更新已解析的譯器。

整體取代 `app` 後，子鍵依呼叫順序寫入：取代 `app.ui` 會移除其原有子鍵，之後寫入 `app.ui.title` 則加入新父物件。寫入陣列下標保留兄弟元素；`get`、`all`、`has`、`push`、`prepend` 讀取相同作用域值，稀疏陣列轉為帶數字鍵的 JSON 物件。同一生命週期中的巢狀呼叫共用變更，其他請求或工作以及應用程式預設值不受影響。`push` 可把目前為 `null` 的設定初始化為清單；兩種方法皆接受關聯陣列，追加保留原鍵，前插會重新排列整數鍵並保留具名鍵。對 `null` 使用 `prepend`，或對其他純量執行這兩種操作，皆傳回錯誤。

用 `Config::string_or("app.name", "App")` 直接取得字串。缺少鍵、值為 `null` 或型別不符、設定儲存庫無法使用時，均使用傳入的預設值。需要嚴格讀取並處理錯誤時，用 `Config::string("app.name")?`。原始值讀取 `Config::get_or("app.name", Value::from("App"))?` 僅在缺少鍵時使用預設值，並保留顯式空值。`Config::get_many([("app.name", Value::from("App")), ("app.debug", Value::Bool(false))])?` 按輸入順序讀取多個鍵，各自套用預設值。讀取整組設定時，為結構體衍生 `serde::Deserialize`，再用 `Config::deserialize::<MailOptions>("mail")?`。

嚴格讀取方法另有 `*_with_default`、`*_or_else`：僅在缺鍵時使用預設值，明確 null 或既有值型別不符仍回傳錯誤。惰性預設值在讀取後最多執行一次；整數與浮點數保持型別區分。倉儲不可用時，先回傳綁定錯誤，不呼叫預設值。

```rust
use openrust::config::ConfigKey;
let name = Config::string_with_default("app.name", "App")?;
let retries = Config::integer_with_default("services.retries", 3)?;
let ratio = Config::float_or_else("services.ratio", || 1.5)?;
let selected = Config::get_many(["app.name", "app.locale"])?;
let custom = Config::get_many([
    ConfigKey::from("app.name"),
    ConfigKey::lazy("services.label", || Value::from("fallback")),
])?;
Config::set_many([("services.enabled", Value::Bool(true))])?;
```

六種嚴格讀取均提供上述預設值入口；門面的字串預設值可直接傳 `&str`，陣列與集合預設值使用 JSON 清單或物件。`get_many` 也保留既有 `(鍵, 預設值)` 寫法，重複鍵保留首次位置並採用最後一次結果，既有 null 不執行惰性預設值。讀取按輸入順序進行，前一預設值回呼的修改可被後續鍵讀取。`set_many` 按順序寫入，不構成交易。完整字面鍵優先於點分路徑。

`openrust::config::Repository` 同樣提供這些讀取與批次操作；設定契約提供預設實作，替換倉儲後公開入口仍可用，門面也會遵循自訂倉儲對型別化讀取的覆寫。


下標存取使用具名入口 `offset_exists`、`offset_get`、`offset_set` 和 `offset_unset`，實際轉送目前倉儲及其自訂覆寫。`offset_unset("key")?` 是將值設為 null：鍵仍存在，`offset_exists` 傳回 true，`offset_get` 傳回 `Some(Value::Null)`；原來不存在的鍵也會建立為 null。字串鍵可以為空，也可以包含空的點分片段或字面 `*`；星號不是萬用字元。這些入口與 `set` 共用請求／任務的語言設定隔離。同步倉儲無法提供穩定的可變下標參照，因此 Rust 使用具名方法。

## 設定巨集 {#configuration-macros}

在應用啟動時註冊巨集，透過公開門面直接呼叫：

```rust
Config::r#macro("feature", |repository, key: String| {
    repository.boolean(&format!("features.{key}"))
})?;
let enabled = Config::macro_call::<bool, _>("feature", "search".to_owned())?;
```

回呼接收目前倉儲，包括替換後的倉儲及其讀取覆寫。`has_macro` 檢查註冊，`flush_macros` 清空註冊。`mixin([(name, ConfigMacro::new(callback))], true)` 替換同名巨集；傳 false 則保留既有巨集。`mixin_using` 依序執行工廠：傳 false 時既有名稱連工廠也不執行；工廠失敗保留先前註冊，後續工廠不再執行。回呼及被替換的捕獲值在註冊鎖外執行或釋放，可以再次註冊、呼叫及清空巨集。

註冊表歸所屬應用，替換預設倉儲後仍保留，HTTP、命令及 Worker 共用該應用的註冊；不同應用互不污染。應用上下文外的獨立 `Repository::new` 擁有自己的註冊表。自訂倉儲可以覆寫物件安全的註冊、檢查、呼叫或 `macro_registry` 契約。即使註冊同名巨集，`Config::get` 等普通方法仍按原行為執行；`macro_call` 明確呼叫指定巨集。

長期保存的回呼要求 `Send + Sync + 'static`，同步呼叫的參數及結果只要求 `Any`，可傳遞不可複製物件或 `Rc`。巨集未註冊、參數／結果型別不符及回呼失敗均明確報錯。具型別呼叫取代動態方法語法，無需取得底層物件。

## 編輯環境檔案 {#editing-environment-files}

`EnvironmentFile` 修改磁碟中的環境檔案；`Config::set` 只修改目前的設定儲存庫，`Environment` 則是不可變的啟動快照。寫入檔案不會自動更新後兩者，也不會重新啟動服務。

```rust
use openrust::config::EnvironmentFile;

// 應用程式引導後，使用實際選中的 .env 或 .env.<環境名稱>。
let file = EnvironmentFile::for_application(&app)?;
// 也可明確開啟既有檔案；create(path, content) 建立時不覆寫既有目標。
// let file = EnvironmentFile::open(project_root.join(".env"))?;
file.set("APP_NAME", "我的應用程式")?;
file.set_many([("MAIL_HOST", "smtp.example.test"), ("MAIL_PORT", "587")])?;
let name = file.get("APP_NAME")?;
let exists = file.contains("MAIL_HOST")?;
let raw = file.get_raw("APP_NAME")?; // 帶引號的原始值，不展開參照
let values = file.all()?;
file.remove("OBSOLETE_SETTING")?;
```

需要一次儲存多種變更時，建立獨立編輯工作階段：

```rust
let mut editor = file.edit()?;
editor.set("APP_NAME", "我的應用程式")?
    .set_expression("MAIL_FROM_NAME", "${APP_NAME}")?
    .remove("OBSOLETE_SETTING")?;
let changed = editor.save()?;
```

`set` 寫入字面字串，跳脫錢字號、引號和反斜線。需要 dotenv 變數展開時明確使用 `set_expression`，不會執行 shell 命令。`get`、`all` 使用應用程式的 dotenv 載入器解析，展開參照時也遵循其處理程序環境回退規則；結果均為字串，`null`、`false` 仍是文字。`get_raw` 傳回帶原有引號的值。目標變數重複定義時拒絕操作；`all` 對重複項沿用載入器的最後定義規則。未修改項的參照、註解、`export`、空白、UTF-8 BOM 和換行格式保留原樣。

直接寫入先取得檔案鎖再讀取；編輯工作階段儲存時發現原檔案變更傳回 `Conflict`，鎖被佔用傳回 `Busy`，待對方完成後重試。批次操作失敗不改檔案，沒有變更時不更新修改時間。同目錄暫存檔完成寫入後原子替換，保留權限及 Unix 擁有者並在 Unix 同步至磁碟；Windows 使用 `ReplaceFileW` 保留目標 ACL。Unix 新檔案僅擁有者可讀寫，Windows 遵循目錄 ACL。穩定的 `<檔案名稱>.lock` 鎖定檔案在可能存在寫入者時必須保留，日常清理不要刪除。

僅支援不超過 8 MiB 的一般 UTF-8 檔案，拒絕目標符號連結及 Windows 重新解析點。新變數名稱以 ASCII 字母或底線開頭，後續允許字母、數字、底線和點。唯讀檔案或語法錯誤不會被替換。協作寫入者透過同一鎖循序處理；繞過該鎖的外部編輯器仍可能與儲存競爭。`DurabilityUncertain` 表示替換已完成但目錄同步失敗，重試前應先核查磁碟內容。Windows 傳回 `ReplacementUncertain` 時會保留復原檔案並報告路徑，重試前應核對目標及復原檔案。 錯誤及編輯器偵錯輸出不含變數值，`content`、`all` 和讀取結果含明文，不應直接記錄到日誌。

正式環境修改後應明確執行 `artisan config:cache` 並重新啟動常駐 HTTP、佇列和排程處理程序；未快取時，新啟動的應用程式讀取新檔案。既有快照不變，處理程序環境覆寫的優先順序保持原樣。`key:generate` 和 `init` 共用檔案寫入後端，繼續保留各自的金鑰驗證、正式環境確認和初始化規則。

### 插入位置與空行 {#environment-file-layout}

```rust
use openrust::config::{EnvironmentDocument, EnvironmentWriteOptions};

let options = EnvironmentWriteOptions::new()
    .after("DB_*")
    .blank_lines(1);
file.set_with_options("SMS_DRIVER", "log", &options)?;
```

使用 `before(key)`、`after(key)`、`at_start()` 或 `at_end()` 指定位置。`blank_lines(1)` 在前後各留一行空行；`blank_lines_before(n)`、`blank_lines_after(n)` 分別控制兩側。零表示沒有額外空行，宣告之間必要的換行仍然存在。未指定的間距保持原樣；明確數量只整理緊鄰的空白行，不刪除註解。檔案首尾也可明確保留指定數量的空行。繼續保留 LF／CRLF 和 UTF-8 BOM。

既有變數預設原位更新，明確空行設定仍然生效；需要重新定位時加上 `.move_existing(true)`。新增和明確移動才解析位置錨點：找不到錨點或錨點重複時拒絕修改，相對於自身定位也會報錯。移動會保留宣告的 `export` 和行尾註解，獨立註解留在原位置。重複執行不會累積空行。移動宣告可能影響變數參照的展開順序。運算式使用 `set_expression_with_options`，多次編輯可放在同一檔案編輯工作階段中，最後統一儲存。

錨點還支援 `*` 萬用字元，匹配零個或多個字元，按完整鍵名匹配並區分大小寫。例如 `after("DB_*")` 放在檔案中最後一個匹配宣告後，`before("DB_*")` 放在第一個匹配宣告前。支援前綴、後綴及多個星號；句點按字面匹配，`DB.*` 可匹配 `DB.host`。它不是正規表示式，不支援 `?` 或字元群組 `[]`。註解、變數值及多行值中的文字都不會成為錨點。

明確移動時，萬用字元匹配會排除被移動變數自身，重複操作不會改變位置或累積空行。沒有其他匹配項時傳回 `MissingAnchor`，不修改文件；匹配到同一個鍵的重複定義時傳回 `DuplicateKey`，多個不同鍵則正常作為一組。既有變數仍預設原位更新，只有 `move_existing(true)` 才移動。

Windows 已知替換窗口內的暫時缺失會等待寫入者，最長 100 ms；鎖仍被佔用則傳回 `Busy`，真正缺失的檔案仍報錯。

同一編輯引擎也可以脫離應用程式、容器和磁碟檔案使用：

```rust
let mut document = EnvironmentDocument::parse("APP_NAME=Example\n")?;
document.set_with_options(
    "SMS_DRIVER", "log",
    &EnvironmentWriteOptions::new().after("APP_NAME").blank_lines(1),
)?;
let output = document.content();
```

`EnvironmentDocument` 支援同樣的讀取、字面值／運算式寫入、批次修改和刪除，也可用 `new()` 建立空文件。它只修改自身文字；二次開發可將 `content()` 交給自己的儲存適配器，並自行處理持久化與並行策略。需要內建檔案鎖和原子儲存時使用 `EnvironmentFile::open(path)`。`for_application` 只是可選的快捷入口，不要求固定檔案名稱 `.env`，也不要求應用程式容器。

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
        .with_config(|_, _| {
            Ok([("app".into(), serde_json::json!({"env": "staging"}))].into())
        })
        .create()?;
    app.boot().await?;

    assert_eq!(app.environment(None)?, EnvironmentResult::from("staging"));
    assert!(bool::from(app.environment(Some("stag*".into()))?));
    assert!(bool::from(app.container().current_environment_is(&["local", "staging"])?));
    assert!(!bool::from(app.container().current_environment_is(&[])?));
    Ok(())
}
```

引導階段將偵測到的環境名稱以 `Arc<str>` 儲存到 `env` 服務。環境查詢讀取該服務。啟動後修改 `app.env` 只更新設定。

<a id="public-files"></a>

## 公開檔案

| 環境變數 | 用途 | 預設值 |
|---|---|---|
| `APP_PUBLIC_PATH` | `filesystems.disks.web.root` / `Application::public_path()` | `public` |
| `FILESYSTEM_WEB_ROOT` | 網站磁碟根目錄，優先於 `APP_PUBLIC_PATH` | `app.public_path` |
| `FILESYSTEM_WEB_URL` | `filesystems.disks.web.url` | `app.url` |
| `FILESYSTEM_LOCAL_ROOT` | 私有本機磁碟的 `filesystems.disks.local.root` | `storage/app/private` |
| `FILESYSTEM_PUBLIC_ROOT` | 公開本機磁碟的 `filesystems.disks.public.root` | `storage/app/public` |
| `FILESYSTEM_PUBLIC_URL` | 公開本機磁碟的 `filesystems.disks.public.url` | `app.url` 加 `/storage` |

`FILESYSTEM_WEB_ROOT` 優先指定網站磁碟根目錄；未設定時使用 `APP_PUBLIC_PATH`（預設 `public`）。相對路徑基於應用根目錄，絕對路徑直接使用。空字串或全空白值會導致設定載入失敗。

未設定 `APP_PUBLIC_PATH` 時，`config/app.rs` 明確將 `app.public_path` 設為 `public`，`web` 磁碟沿用該值作為預設根目錄。網站檔案設定在 `filesystems.disks.web`，公開上傳檔案設定在 `filesystems.disks.public`。預設路由依序呼叫 `Route::static_disk("web")` 和 `Route::static_disk("public")`。引導前呼叫 `use_public_path()` 會覆寫網站目錄並同步 `web.root`。相對目錄以應用程式根目錄為基準。掛載前綴取磁碟 URL 的完整路徑，例如 `/storage/tmp/` 掛載到 `/storage/tmp`。絕對 URL 必須與 `app.url` 同源。修改掛載設定後重新啟動應用程式。 明確設定為空字串或全空白的 `APP_PUBLIC_PATH` 會導致設定載入失敗。

## 維護模式 {#maintenance-mode}

部署時執行 Artisan 的 `down`，應用程式會回傳 HTTP 503。執行 `up` 恢復存取。執行中的服務會逐一請求讀取共用狀態，可以再次執行 `down` 更新選項。

```bash
./artisan down --retry=60 --with-secret
./artisan up
```

`--retry` 接受正數秒數或重試日期，`--refresh` 指定瀏覽器重新整理的間隔。未註明時區的日期時間使用應用程式時區，回應標頭統一使用 GMT。`--redirect` 接受站內路徑。`--render` 在執行 `down` 時將具名檢視渲染一次，並傳入 `$retryAfter`。`--status` 可覆寫預設的 503。預期 JSON 的請求沿用一般例外渲染，略過瀏覽器重新導向與預先渲染的頁面。

維護期間可開啟 `--with-secret` 印出的 URL 檢查網站。應用程式會設定有效期為 12 小時的簽名 Cookie，當前工作階段沒有原目標時重新導向至首頁。可以使用 `--secret` 指定自己的路徑。設定的健康路徑（預設 `/up`）在維護期間仍可存取。若要保留其他端點，在 `bootstrap/app.rs` 的 `with_middleware` 設定。

```rust
.with_middleware(|middleware| {
    middleware.prevent_requests_during_maintenance(["status/*"]);
})
```

使用檔案驅動部署時，`./artisan down --render=errors::503` 可在 Provider 啟動期間提供預先渲染的頁面。可以在 `resources/views/errors/503.blade.html` 自訂頁面。監聽設定需成功載入。JSON 請求、排除路徑及有效繞過請求會等待應用程式核心。快取驅動由執行中的核心提供維護回應。佇列行為請見[維護期間的 Worker](../digging-deeper/queues.md#workers-during-maintenance)。

啟動期間請使用 Artisan 印出的完整繞過 URL。啟動前處理器會比對原始請求位址，查詢字串和尾斜線都會參與比對。HTTP 核心啟動後，維護中介層按路徑比對，忽略查詢字串及路徑兩端的斜線。

### 共用維護狀態 {#shared-maintenance-state}

應用程式透過 `maintenance_mode()` 提供維護狀態。預設 `file` 驅動程式把資料寫入 `storage/framework/down`。選擇 `cache` 和共享快取儲存，可讓多台主機上的 Worker 讀取相同狀態。`array` 驅動程式將狀態儲存在目前應用程式實例內，可用於測試。

```rust
use openrust::foundation::Application;
use serde_json::{Map, json};

async fn activate_maintenance(
    app: &Application,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let payload = Map::from_iter([("status".into(), json!(503))]);
    app.maintenance_mode()?.activate(&payload).await?;
    Ok(())
}
```

用 `is_down_for_maintenance().await` 讀取狀態，用 `app.maintenance_mode()?.deactivate().await?` 清除。佇列 Worker 在保留工作前讀取此狀態。

### 維護模式門面與驅動 {#maintenance-facade}

`MaintenanceMode` 使用 HTTP、Artisan 和排程器共用的應用管理器。`get_default_driver` 讀取 `app.maintenance.driver`，缺少時預設 `file`。`driver(None)` 選擇預設驅動；`driver(Some("file"))`、`driver(Some("array"))` 和 `driver(Some("cache"))` 明確選擇內建驅動。已解析驅動在應用內共用，不同應用相互隔離。

```rust
use openrust::{foundation::maintenance::MaintenanceError, support::facades::MaintenanceMode};
use serde_json::{Map, json};

pub async fn update_maintenance() -> Result<(), MaintenanceError> {
    MaintenanceMode::activate(&Map::from_iter([("status".into(), json!(503))])).await?;
    assert!(MaintenanceMode::active().await?);
    let _payload = MaintenanceMode::data().await?;
    MaintenanceMode::deactivate().await
}
```

`extend(name, creator)` 註冊建立器，接收 `&Container` 並傳回 `Arc<dyn contracts::foundation::MaintenanceMode>`。既有驅動繼續使用，直到 `forget_drivers` 清空實例。建立器不持註冊表鎖，可解析其他驅動；並行解析同一驅動只執行一次建立。循環依賴傳回 `MaintenanceError::DriverResolutionCycle`。`get_drivers` 傳回已解析控制代碼，清空後舊控制代碼仍有效。後續 HTTP 請求、命令和排程器解析目前綁定，也遵循明確替換的 MaintenanceMode Contract。

`get_container` 傳回所屬容器。`set_container(&Arc<Container>)` 影響後續建立，保留管理器建構時取得的設定、已解析驅動和建立器。容器使用弱持有，所屬應用釋放後存取會報錯。這些入口用於嵌入應用的生命週期控制，不宜作為逐請求設定。

快取驅動記錄 `app.maintenance.store`，未指定時回退 `cache.default`，每次操作重新解析該具名儲存庫，因此能跟隨儲存庫替換。寫入使用 `put_with_default`：初始有效期為 3600 秒，儲存庫預設時長設為 `None` 時永久儲存。檔案維護狀態保留至明確清除。沒有狀態時 `data()` 傳回 `None`，後端、解碼及拒絕寫入的錯誤向上傳遞。`array` 僅限應用程序內，不能協調獨立的伺服器或 Worker 程序。

## 測試環境與啟動快照 {#testing-environment}

`Application::running_unit_tests()` 和應用契約傳回 `Result<bool, BindingResolutionError>`。僅當容器中的 `env` 精確為 `testing` 時結果為 true，與是否正在執行測試程式無關。環境未綁定時傳回 `Ok(false)`，不觸發服務解析；解析錯誤沿 `Result` 傳遞。後續修改 `config("app.env")` 不會替換已偵測的環境綁定。

啟動後，透過 `app.make::<openrust::config::Environment>().await?` 讀取目前應用不可變的環境快照。既有 `get`、`get_utf8` 傳回原始字串，具體旗標仍由對應套件解析。快照遵守處理程序變數優先、環境檔案選擇與設定快取規則，不重新載入檔案，也不修改處理程序全域變數。

## 應用儲存路徑 {#application-storage-path}

`app.storage_path("")` 傳回儲存根目錄，傳入 `"pail"` 等字串可取得子路徑。`app.use_storage_path(path)?` 覆蓋它並重新綁定 `path.storage`，應用契約也提供 getter。`openrust::support::storage_path(path)?` 讀取目前應用作用域。唯讀的 `foundation::Paths` 服務讓編譯型套件共用相同狀態，避免透過容器強持有 Application。

優先順序是非空且非 `0` 的明確覆蓋、應用環境快照中的 `APP_STORAGE_PATH`、應用根目錄下的 `storage`。環境中明確存在的空值仍會保留。Setter 傳入相對路徑時保持相對。串接追加本機分隔符並去掉子路徑開頭的本機分隔符，保留重複分隔符和 `..`。Rust 沒有預設參數，讀取根目錄時明確傳入 `""`。

建構時初始化 `path.storage`，後載入的環境檔案可以改變 getter，但不會自動重寫該綁定；`use_storage_path` 則明確重新綁定，沿用 順序。重新綁定錯誤透過 `Result` 傳回，此時路徑狀態已更新。

日誌、檔案 Session、檔案快取資料與鎖、編譯檢視以及預設 local/public 磁碟在載入設定時採用儲存根目錄。`FILESYSTEM_LOCAL_ROOT`、`FILESYSTEM_PUBLIC_ROOT` 和 `VIEW_COMPILED_PATH` 仍可個別覆蓋，公開 URL 和掛載順序不變。`storage:link` 指向設定的公開磁碟。啟動前設定絕對路徑的 `APP_STORAGE_PATH`，或在載入設定前呼叫 `use_storage_path`。

編譯型設定載入器使用 `env.storage_path(base_path, subpath)` 作為 Rust 上下文適配。弱引用選取所屬應用的路徑，不依賴全域 Facade，也不延長應用生命週期。獨立環境快照依自身變數及傳入的預設根目錄解析。後續更改應用路徑不會回寫已經載入或快取的設定值。

檔案維護模式每次操作都解析目前路徑，服務已解析後仍有效。啟動前的預先轉譯檢查使用 boot 前可用的路徑：此時應提供程序變數或明確 setter，僅在 dotenv 中設定的值尚不可用。Pail 在首次解析 Files 時決定目錄，應在首次使用前設定。設定、路由、事件和套件快取產物的位置繼續遵守各自的快取路徑契約。

升級既有專案時，請依[語言標識升級說明](../digging-deeper/localization.md#bcp47-upgrade)同步遷移語言資源、持久化值與快取。
