# 在地化

## 設定語言

透過 `config/app.rs` 的 `locale`、`fallback_locale` 設定預設語言及缺少群組詞條時的備援語言，對應根 `.env`：

```dotenv
APP_LOCALE=en
APP_FALLBACK_LOCALE=en
```

依照 Laravel 教學，使用 `App::setLocale` 切換一次 HTTP 請求的語言，使用 `App::currentLocale` 讀取語言，使用 `App::isLocale("en")?` 判斷語言。路由範例：

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode, abort},
    routing::RoutingError,
    support::facades::{App, Route},
};

pub fn register() -> Result<(), RoutingError> {
    Route::get("/greeting/{locale}", greeting)
}

fn greeting(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let locale = request.route("locale").unwrap_or("en").to_owned();
    Box::pin(async move {
        if !["en", "zh-Hans", "zh-Hant"].contains(&locale.as_str()) {
            return Err(abort(StatusCode::BAD_REQUEST, "", Default::default()));
        }
        App::setLocale(&locale).await.map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        let current = App::currentLocale().map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Ok(Response::from(current))
    })
}
```

`App::getLocale` 是 `currentLocale` 的同義入口；`getFallbackLocale`、`setFallbackLocale` 讀寫備援語言。`setLocale` 同步更新 `app.locale` 與譯器，再發送 `foundation::events::LocaleUpdated`；事件包含 `locale` 和 `previousLocale`。Rust 事件監聽器可以非同步執行，因此此方法需要 `.await`，失敗透過 `Result` 傳回。請求和佇列工作內的修改互相隔離；啟動階段設定則作為該應用程式的預設值。

服務錯誤在實際呼叫步驟傳播：先寫設定，再更新譯器，最後派送事件。常駐譯器首次解析時分別讀取應用程式預設值和目前作用域設定，避免把請求語言保存為應用程式預設值。監聽器失敗不會回復本次已更新的設定與譯器，錯誤保留原始來源；目前生命週期結束後仍依作用域還原。重複設定相同語言亦會派送事件，修改備援語言不派送該事件。

設定、API、資源、文件 URL、HTML 與 HTTP 統一使用 BCP 47。`setLocale("zh-CN")` 後仍為 `zh-CN`；`isLocale("ZH-cn")` 為真，`isLocale("zh-Hans")` 為假。正規化保留文字體系、地區、變體及支援的擴充，資源選擇另行處理。舊底線輸入會明確報錯。

`App` Facade 目前提供上述語言方法；其他 Laravel App 方法尚未透過此 Facade 提供。範本繼續使用下文的翻譯助手。`Lang::withLocale` 僅用於暫時的譯器語言作用域，不修改應用程式語言設定。

### 瀏覽器語言 {#welcome-page-language}

這是本專案為歡迎頁三語自動辨識啟用的應用程式設定，Laravel 預設骨架不預置瀏覽器語言自動辨識。依照教學僅使用預設語言或路由手動切換時，可移除 `bootstrap/app.rs` 中的 `SetLocale` 註冊；翻譯功能仍然可用。 中介軟體透過 `Request::getLanguages()` 共用請求標頭解析與偏好排序，三語資源對應仍由應用程式決定。

應用程式在 `bootstrap/app.rs` 註冊 `app/http/middleware/set_locale.rs`，根據 `Accept-Language` 為整個請求選擇語言。控制器、驗證和 Blade 模板共用目前語言，按權重選擇，略過 `q=0`，同權重保持瀏覽器順序。沒有符合項目時，使用支援的 `APP_LOCALE`，否則使用英文。

三語候選 `["en", "zh-Hans", "zh-Hant"]` 直接定義在應用程式中介軟體中，不增加 Laravel 應用程式設定鍵。`zh-Hans`、`zh-CN`、`zh-SG` 和僅 `zh` 對應簡體；`zh-Hant`、`zh-TW`、`zh-HK`、`zh-MO` 對應繁體，明確的文字體系優先於地區，未知文字體系不按中文地區猜測。其他語言符合完整識別碼或基礎語言。

中介軟體先呼叫 `App::setLocale` 設定目前請求語言，再執行後續處理，包括視圖渲染和例外回應。在缺少時加入 `Content-Language`，將 `Accept-Language` 合併至 `Vary` 並保留既有值。不寫入 Cookie，不改變其他請求、佇列工作或應用程式預設語言。移除該中介軟體註冊即可僅使用設定的預設語言，也可以在此應用程式中介軟體中調整自己的語言選擇策略。

歡迎頁的 HTML `lang` 與 `Content-Language` 使用應用程式目前語言。文案和連結直接在 Blade 中翻譯，缺少的詞條由譯器依設定備援。在 `lang/*/welcome.json` 中設定 `documentation_url`、`deployment_url`，即可為各語言指定文件連結，新增語言不需要修改控制器。例如 `APP_LOCALE=fr`、`APP_FALLBACK_LOCALE=en` 時，缺少的法文詞條使用英文文案和連結，應用程式及頁面語言仍為 `fr`。請在目前語言或設定的備援語言中提供詞條；兩者均缺少時遵循一般翻譯缺鍵行為。

### 在 Blade 中翻譯 {#blade-translations}

模板可以直接讀取翻譯，控制器只需提供頁面資料：

```blade
<h1>{{ __('welcome.heading') }}</h1>
<p>{{ trans('messages.greeting', ['name' => $name]) }}</p>
<p>{{ trans_choice('messages.items', $count) }}</p>
```

`__` 和 `trans` 接受字串鍵、可選替換陣列和可選語言；`trans_choice` 在鍵後增加數字或陣列計數。省略語言或傳入 `null`／空字串時使用目前請求語言，明確指定語言只影響該次呼叫。替換值支援字串、數字、布林值和 `null`。缺鍵和備援語言遵循 `Lang` 的規則。

<code v-pre>{{ ... }}</code> 跳脫 HTML。`@lang('messages.title')`、`@choice('messages.items', $count)` 和 `{!! __('messages.title') !!}` 輸出原始文字，僅用於可信 HTML 和替換值。子模板和元件繼承渲染語言，編譯快取中的模板在每次渲染時讀取翻譯。翻譯輔助函式需要有效的應用程式翻譯服務。翻譯輔助函式可以回傳文字、群組或清單。群組及清單可用於 `@foreach`，或使用陣列下標讀取詞條；直接輸出整個陣列會報錯。Rust 呼叫端可使用 `Lang::array`。

`@lang ... @endlang` 支援翻譯區塊：先渲染並去除首尾空白，再將內容作為翻譯鍵。`@lang(['name' => $name]) ... @endlang` 可傳入替換陣列，輸出原始文字。巢狀區塊共用替換參數，內層開始時會取代外層的替換陣列。`__()` 和 `__(null)` 傳回 `null`，直接輸出時為空。`@lang('messages.title', [], null, false)` 可關閉該次群組查找的備援；複數替換中的 `count => null` 使用實際計數，明確零值或空字串仍保留。`trans()`／`trans(null)` 應傳回譯器物件，目前範本值模型未暴露服務物件，請在 Rust 中使用 `Lang`；任意 PHP 函式、Facade 方法呼叫和具名參數尚未實作。參數數量與型別明確檢查，不依賴 PHP 的隱式型別轉換。

## 發佈語言檔案 {#publishing-the-language-files}

執行 `./artisan lang:publish` 發佈驗證身分、分頁、密碼重設和欄位驗證的英文 JSON 語言檔案。預設寫入 `lang/en`，應用程式已有 `resources/lang` 時寫入 `resources/lang/en`。預設保留既有檔案。用 `--force` 覆寫四份檔案，用 `--existing` 僅覆寫已發佈的檔案。兩個選項同時使用時，由 `--existing` 決定範圍。

## 定義翻譯字串

群組翻譯放在 `lang/en/messages.json` 等檔案中，巢狀項目使用點號存取。以完整句子為鍵的翻譯放在 `lang/en.json`。應用程式存在 `resources/lang` 時，會使用該目錄作為語言目錄。

```json
{
  "welcome": "Hello, :Name",
  "items": "{0} No items|{1} One item|[2,*] :count items"
}
```

### 登記目錄與執行期翻譯 {#registering-translation-sources}

在應用程式作用域首次查找之前，用 `Lang::addPath`、`Lang::addJsonPath` 或 `Lang::addNamespace` 登記目錄。較晚登記的目錄會覆蓋較早的目錄。分組資源遞迴合併，包括清單索引。JSON 字面鍵以整項覆蓋。應用程式目錄在 JSON 專用目錄之後載入。

用 `Lang::addLines` 登記帶點號的分組鍵。首次登記的分組以提供的翻譯為準。要擴充檔案中的分組，先呼叫 `Lang::load`。載入後的分組會留在應用程式快取，後續路徑登記適用於之後載入的分組。

```rust
use std::path::PathBuf;

use openrust::{
    support::facades::Lang,
    translation::{Line, TranslationResult},
};

pub fn register_translations(directory: PathBuf) -> TranslationResult<()> {
    Lang::addPath(directory)?;
    Lang::load(None, "messages", "en")?;
    Lang::addLines(
        [("messages.runtime".into(), Line::Text("Registered :name".into()))],
        "en",
        None,
    )
}
```

### 從自訂來源載入分組 {#loading-custom-translation-sources}

在應用程式的非同步入口向 `Lang::loadFrom` 傳入 `Loader`。每個資源指定語言、分組和選用的命名空間。框架一次替換這批分組。讀取失敗或其中一個分組在載入期間被執行期更新時，保留目前的目錄。後續翻譯查詢仍為同步呼叫。

```rust
use openrust::{
    contracts::translation::Loader,
    support::facades::Lang,
    translation::{TranslationResource, TranslationResult},
};

pub async fn load_notice(source: &dyn Loader) -> TranslationResult<()> {
    Lang::loadFrom(
        source,
        &[TranslationResource {
            locale: "en",
            group: "notices",
            namespace: None,
        }],
    )
    .await
}
```

### 資料庫翻譯來源（擴充） {#database-translation-loader}

在應用程式 Cargo 相依套件中透過 `path = "vendor/openrust/translation-database"` 加入 `openrust-translation-database`。`DatabaseLoader` 接收明確的框架 `Connection`，可從目前應用程式的資料庫管理器或 `DB::connection` 取得。支援 PostgreSQL 18、MySQL 8.4 和 SQLite 的資料庫詞條載入，其他資料庫目標須分別驗證行為。

先透過應用程式遷移建立資料表，再載入翻譯。可使用以下最小結構：

```rust
use openrust::database::{Connection, DatabaseResult};

pub async fn create_translations(connection: &Connection) -> DatabaseResult<()> {
    connection.getSchemaBuilder().create("language_lines", |table| {
        table.id();
        table.string("group", None).index();
        table.string("key", None);
        table.json("text");
        table.timestamps();
    }).await
}
```

資料表結構沿用 Spatie Laravel Translation Loader：每個群組與鍵儲存一列，`text` 是語言到字串的 JSON 物件，例如 `{"en":"Hello :name","fr":"Bonjour :name"}`。語言鍵使用標準識別碼（`en-US`、`zh-Hans`）。缺少或為 null 的語言保留檔案值；翻譯值必須為字串，空字串也是有效覆寫。群組 `*` 儲存字面鍵並保留句點，其他群組展開 `nested.title`、`items.0` 等點分鍵。套件命名空間與 Spatie 一樣僅使用檔案來源。重複鍵、父子鍵衝突、無效 JSON、鍵中空段及超過 128 層的鍵會使載入失敗。

在非同步應用程式作用域中呼叫以下函式，例如遷移就緒後由 Provider 的 `boot` 載入：

```rust
use std::{path::PathBuf, sync::Arc};
use openrust::{
    database::Connection,
    support::facades::Lang,
    translation::{FileLoader, TranslationResource, TranslationResult},
};
use openrust_translation_database::DatabaseLoader;

pub async fn load_database_translations(
    connection: Connection,
    language_directory: PathBuf,
) -> TranslationResult<()> {
    let source = DatabaseLoader::new(connection)
        .withTable("language_lines")?
        .withMaxRows(10_000)?
        .withFallback(Arc::new(FileLoader::new(language_directory)));
    Lang::loadFrom(
        &source,
        &[
            TranslationResource { locale: "fr", group: "messages", namespace: None },
            TranslationResource { locale: "en", group: "messages", namespace: None },
        ],
    ).await
}
```

資料表名稱套用所選連線的資料表前綴，只接受不帶 schema 的識別碼。預設資料表名稱為 `language_lines`，每個群組預設最多讀取 10,000 列；超過設定上限會回報錯誤，不截斷結果。

`withFallback` 從檔案來源載入相同語言、群組與命名空間，再用資料庫詞條覆寫。群組和清單索引遞迴合併，字面 JSON 值整體替換。資料庫空字串仍為有效覆寫；刪除一列後，明確重新載入該資源即可恢復檔案值。語言回退繼續使用翻譯器設定；需要資料庫回退詞條時，同時載入所選語言與回退語言資源。

`Lang::loadFrom` 一次發佈全部請求群組。資料庫、檔案或解析錯誤會回傳失敗，並保留先前發佈的目錄；沒有詞條的資源提供空資料庫覆寫。資料庫按群組讀取，發佈原子性不提供橫跨所有讀取的資料庫交易快照。重新載入由應用程式明確觸發。寫入、遷移與刷新時機由應用程式管理，套件不會自動選擇來源或啟動背景工作。

與 Spatie 的 PHP 模型不同，本來源讀取每列時不執行語言回退；OpenRust 翻譯器在載入精確語言後按自身設定回退，也因此保留所選語言的檔案詞條。缺表會回傳錯誤，避免明確重新載入失敗時靜默以檔案取代已發佈的資料庫目錄。

## 取得翻譯字串

使用 `__` 讀取文字。預留位置支援 `:name`、`:Name` 和 `:NAME`。群組和清單使用 `Lang::array` 讀取。缺少翻譯時回傳鍵本身。JSON 字面鍵在所選語言內查找，群組鍵可使用設定的備援語言。

### 語言回退與缺少的鍵 {#translation-lookup-hooks}

使用 `Lang::has(key, locale, true)` 檢查含回退語言的翻譯，使用 `Lang::hasForLocale(key, locale)` 僅檢查指定語言。兩種檢查都會略過缺鍵回呼。在應用程式作用域內登記掛鉤後，可調整分組翻譯的回退順序，JSON 字面鍵仍優先。缺鍵回呼傳回的文字會繼續處理預留位置，傳回 `None` 時使用原鍵。

```rust
use openrust::{support::facades::Lang, translation::TranslationResult};

pub fn register_lookup_hooks() -> TranslationResult<()> {
    Lang::determineLocalesUsing(|locales| {
        let mut locales = locales.to_vec();
        if locales.first().is_some_and(|locale| locale == "fr-CA") {
            locales.insert(1, "fr".into());
        }
        locales
    })?;

    Lang::handleMissingKeysUsing(|missing| {
        (missing.key == "messages.welcome").then(|| "Hello, :name".into())
    })
}
```

### 取代翻譯字串中的參數 {#replacing-parameters-in-translation-strings}

使用 `Lang::getWith` 依序傳入文字、標籤和物件。回呼接收同一行上每個相符的 `<name>…</name>` 的內容。標籤回呼依參數順序執行，接著取代文字預留位置。使用 `Lang::arrayWith` 取代群組或清單中的值。使用 `Lang::choiceWith` 時先選擇複數分支，再執行取代。

#### 格式化物件取代值 {#object-replacement-formatting}

在應用程式作用域內使用 `Lang::stringable` 為具體 Rust 型別註冊格式化器，通常放在 `AppServiceProvider::boot`。型別必須註冊格式化器時使用 `Replacement::object`。允許在未註冊時使用該型別的 `Display` 實作，則使用 `Replacement::display`。Rust 列舉可明確傳入變體名稱或值，也可為列舉註冊格式化器。

例如，在 `lang/en/payments.json` 中定義 `payments.notice`。

```json
{"notice": "<mark>Paid</mark> :amount"}
```

```rust
use openrust::{
    support::facades::Lang,
    translation::{Replacement, TranslationResult},
};

pub struct Amount(pub u32);

pub fn register_formatters() -> TranslationResult<()> {
    Lang::stringable(|amount: &Amount| format!("{} USD", amount.0))
}

pub fn payment_notice(amount: &Amount) -> TranslationResult<String> {
    Lang::getWith(
        "payments.notice",
        &[
            ("amount", Replacement::object(amount)),
            ("mark", Replacement::tag(|text| format!("[{text}]"))),
        ],
        None,
    )
}
```

## 複數

使用 `trans_choice` 選擇形式。先比對 `{0}`、`[2,*]` 等明確條件，其餘情況依找到翻譯的地區複數規則選擇。函式會自動提供 `:count`。

```rust
use std::collections::BTreeMap;

use openrust::{
    support::facades::Lang,
    translation::{TranslationResult, __, trans_choice},
};

pub async fn summary(locale: &str) -> TranslationResult<(String, String)> {
    Lang::withLocale(locale, async {
        let replacements = BTreeMap::from([("name".into(), "ada".into())]);
        Ok((
            __("messages.welcome", &replacements)?,
            trans_choice("messages.items", 2_i64, &replacements)?,
        ))
    })
    .await?
}
```

語言標籤使用連字號，例如 `en`、`en-GB`、`fr-CA`、`es-419`、`sr-Latn-RS`、`zh-Hans`、`zh-Hant`、`zh-Hant-HK`、`en-US-u-nu-latn` 與 `en-x-demo`。重用 ICU4X 2.4 解析器與 CLDR 別名資料處理大小寫及棄用別名（如 `iw-IL` → `he-IL`），不自動補入文字或地區。這驗證程式庫支援的 Unicode Locale 語法，並非逐項檢查 IANA 登記。grandfathered `i-klingon`、獨立 private-use `x-demo` 與 extlang 等不支援形式會明確報錯，支援的擴充不會靜默遺失。翻譯按完整正規標籤精確尋找資源，分組缺鍵再使用設定回退，句子鍵 JSON 保留既有查找規則；不隱式將地區合併至文字目錄。Number 消費 Unicode 擴充，Faker 僅於私有 provider 邊界轉換資料鍵，不改變公開語言。

## 覆寫套件語言檔案 {#overriding-package-language-files}

要覆寫 `shop::cart.summary.heading` 等套件翻譯，請在 `lang/vendor/shop/en/cart.json` 中定義同一個鍵。目錄與檔名沿用原翻譯的命名空間、語言及群組。應用程式只覆寫自己定義的項目，其餘項目仍由套件提供。

## Artisan 翻譯 {#artisan-translations}

Artisan 使用 `APP_LOCALE` 顯示命令清單標題、說明和引數錯誤。內建英文、簡體中文和繁體中文。應用程式可在 `lang/<locale>/console.json` 覆寫文字。翻譯先查目前語言，再查 `APP_FALLBACK_LOCALE`，最後使用內建文字。沒有翻譯的命令說明沿用註冊時的文字。

```sh
APP_LOCALE=en ./artisan help list
APP_LOCALE=zh-Hans ./artisan list make
APP_LOCALE=zh-Hant ./artisan help list
```

可在 `console` 群組中使用 `options`、`commands.list.description` 和 `commands.list.arguments.namespace` 等鍵調整說明。請參閱 [Artisan 輸出格式](./artisan.md#command-help-and-output-formats)。

正式環境確認提示和取消訊息使用同一翻譯群組。可在 `console.json` 中覆寫 `confirm.production` 或 `confirm.cancelled`。輸入 `y` 或 `yes` 確認，直接按 Enter 取消。

## 契約語言設定

Translator 契約、具體實作與 Lang Facade 都提供 `getLocale`、`setLocale`、`getFallback`、`setFallback`。啟動時設定應用程式預設值；請求／佇列作用域內設定僅影響目前執行。`withLocale` 支援巢狀並在離開後恢復，回退語言獨立設定。舊自訂消費者應將契約中的 snake_case 呼叫遷移為上述原名。

## 語言設定的所有權

語言與回退語言的讀取方法傳回擁有所有權的 `String` 快照。設定方法接收 `&self`，傳回 `TranslationResult<()>`，無效標籤傳回錯誤；`zh-CN` 與 `zh-Hans` 保持不同。啟動值作為預設值，請求或佇列工作期間的修改只保留在目前容器作用域。`withLocale` 僅暫時覆蓋目前工作，完成後恢復外層語言。

## 升級語言標識 {#bcp47-upgrade}

先暫停接收新工作並排空舊郵件／通知佇列，再停止舊 Web 與佇列程序；無法排空時保留工作供專用工具遷移。變更資源或持久化語言值前，備份設定、語言資源與受影響資料，避免新舊版本混跑。明確更新 `APP_LOCALE`、`APP_FALLBACK_LOCALE`、`APP_FAKER_LOCALE`，應用程式不會自動修改實際 `.env`。

| 舊專案值／路徑 | 新值／路徑 |
| --- | --- |
| `zh_CN`、`lang/zh_CN`、`docs/zh_CN` | `zh-Hans`、`lang/zh-Hans`、`docs/zh-Hans` |
| `zh_TW`、`lang/zh_TW`、`docs/zh_TW` | `zh-Hant`、`lang/zh-Hant`、`docs/zh-Hant` |
| 地區 `en_US`、`en_GB`、`zh_HK`、`zh_Hant_HK` | `en-US`、`en-GB`、`zh-HK`、`zh-Hant-HK` |

前兩項僅用於本專案預設資源與已確認歸屬的資料；有效的 `zh-CN`、`zh-TW` 仍為獨立地區標籤。遷移目錄時同步句子鍵 JSON 檔案與套件覆寫資源；先檢查目的檔案及 JSON 鍵衝突，衝突時保留兩份內容供人工處理。預設不安裝舊 URL 重新導向或底線輸入別名，需要過渡時由託管層明確設定。

替換舊執行檔前，使用舊版本執行 `./main artisan config:clear`、`./main artisan view:clear`，原始碼方式可用既有 `./artisan` 入口。新程式、資源及設定同時部署，再執行 `config:cache`、`view:cache` 重建快取並重啟所有常駐程序；程序內翻譯目錄快取隨重啟清除。舊郵件／通知佇列應先排空；無法排空時，由應用程式專用工具檢查序列化 `locale` 欄位，加密工作亦需要舊應用程式金鑰。不要在佇列、共用快取或設定 JSON 中做通用文字替換。

資料庫翻譯先唯讀盤點 `language_lines.text` 的 JSON 語言鍵，以及應用程式自有設定或語言欄位。`vendor/openrust/translation-database/database/upgrade-bcp47/`提供獨立唯讀盤點與 PostgreSQL、MySQL、SQLite 交易，映射預設兩種中文與 `en_US`／`en_GB`；審閱後按實際情況調整資料表名稱及其他舊語言映射。未列入映射的底線鍵、任意新舊鍵碰撞（包括相同值或 null）均使交易失敗，不覆寫內容。預設結尾為 **ROLLBACK**，核對結果與備份後，才明確改為 **COMMIT**。發生錯誤需回滾並關閉該專屬資料庫連線。成功後重複執行為空操作；已提交後的回退需要還原備份，不能僅憑新鍵名判斷哪些內容來自遷移。

Faker 原生 provider 鍵僅保留於私有邊界，`locale()`、`randomLocale()` 輸出 BCP 47；僅有文字體系的 `zh-Hans`／`zh-Hant` 使用既有對應中文 provider，公開標籤仍保留原地區及擴充。微信原生 `lang=zh_CN` 等第三方協定欄位保持不變。
