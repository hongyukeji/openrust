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

OpenRust 的預設全域中介軟體堆疊包含 `openrust::http::middleware::SetLocale`，應用程式不必建立語言中介軟體或在 `bootstrap/app.rs` 手動註冊。

預設支援 `en`、`zh-Hans`、`zh-Hant`。中介軟體透過 `Request::getLanguages()` 共用請求標頭解析與偏好排序，依權重選擇、略過 `q=0`，同權重保留瀏覽器順序。沒有符合項目時使用支援的 `APP_LOCALE`，否則使用候選清單第一項（預設英文）。控制器、驗證與 Blade 模板共用目前請求語言。

`zh-Hans`、`zh-CN`、`zh-SG` 與單獨 `zh` 對應簡體；`zh-Hant`、`zh-TW`、`zh-HK`、`zh-MO` 對應繁體，明確的文字體系優先於地區，未知文字體系不依中文地區猜測。數字變體不視為文字體系。其他語言比對完整標識或基礎語言。此選擇規則不改變公開語言標籤的正規化語意。

需要其他語言時，在 `bootstrap/app.rs` 的 `withMiddleware` 中覆寫候選，不必新增設定鍵：

```rust
.withMiddleware(|middleware| {
    middleware.setLocale(&["en", "fr", "de"]).expect("語言標籤有效");
})
```

候選依 BCP 47 正規化；無效標籤回傳 `TranslationError` 並保留先前設定，重複呼叫使用最後一次成功設定。傳入空清單 `middleware.setLocale(&[])` 關閉辨識，此時不修改語言或回應標頭，也不觸發語言更新事件，應用程式繼續使用 `app.locale` 與手動 `App::setLocale`。僅用於特定路由時，先關閉全域辨識，再透過既有路由或路由群組中介軟體註冊 `SetLocale::new(&["en", "fr"])?`。

中介軟體在執行下游前呼叫 `App::setLocale`，同步設定、翻譯器與 `LocaleUpdated` 事件；監聽器失敗時交由既有例外處理，不繼續執行下游。下游處理完成（包括已渲染的例外回應）後讀取最終語言，在缺少時新增 `Content-Language`，將 `Accept-Language` 合併至 `Vary`，保留既有值或萬用字元。不寫入 Cookie，不改變其他請求、佇列工作或應用程式預設語言。預設執行位置在輸入轉換之後、應用程式追加的全域中介軟體與路由之前；先前已回傳回應的 CORS 預檢、維護模式或請求大小檢查不會經過語言選擇。

原先應用程式中的 `SetLocale` 檔案與 `append` 註冊應移除，避免重複執行；需要關閉辨識時改用上述空清單設定。

歡迎頁的 HTML `lang` 與 `Content-Language` 使用應用程式目前語言，顯示文案依設定進行翻譯備援。儲存庫網址來自框架套件中繼資料，範本使用應用程式目前語言組合文件路徑。預設的 `en`、`zh-Hans`、`zh-Hant` 均有對應文件目錄。關閉辨識並設定 `APP_LOCALE=fr` 時，文案可以備援至英文，但文件連結仍指向儲存庫未提供的 `docs/fr`。使用其他應用程式語言時，請提供對應文件或自訂歡迎頁範本。

### URL 語言選擇 {#locale-query}

預設中介軟體先讀取 `?lang=en`、`?lang=zh-Hans` 或 `?lang=zh-Hant`，再協商 `Accept-Language`。`localeUsing` 回呼傳回語言時優先於 URL；傳回 `None` 後依序嘗試 URL 與瀏覽器。URL 明確指定的值必須正規化後精確屬於候選：`ZH-hANT` 可選擇 `zh-Hant`，此處不將 `zh-CN` 當作別名。空值、非法標籤、陣列及不支援的語言傳回 HTTP 400。重複純量參數遵循查詢剖析器規則，後值覆蓋前值；請求本文欄位不參與選擇。

在 `config/app.rs` 的語言區域設定參數名：

```rust
"locale_query_parameter": "lang",
```

改為 `"language"` 後只讀取 `?language=...`；設為 JSON `null` 只關閉 URL 選擇，仍保留回呼與瀏覽器協商。缺鍵時預設使用 `lang`，相容舊設定快取。空名稱、空白、控制字元、點、方括號及非字串值屬於設定錯誤，請使用非空的頂層查詢鍵。設定快取保留原有值，修改後需重建快取；不需要額外環境變數或中介軟體註冊。

歡迎頁提供候選中實際可用的 `en`、`zh-Hans`、`zh-Hant` 及「跟隨瀏覽器」的一般 GET 連結。連結保留其他查詢參數，自動選項只刪除設定的語言鍵。目前語言使用底線標示，邊框標示明確選擇或自動模式。重新整理或分享帶參數 URL 可保留選擇；存取不帶該參數的首頁恢復瀏覽器協商，不寫入 Cookie、Session、資料庫或 localStorage。

關閉 URL 選擇、停用中介軟體、沒有歡迎頁語言候選，或回呼已選定 URL 無法覆蓋的語言時，不顯示切換控制項。自訂候選不會產生未翻譯的選單項目。中介軟體將 `LocaleSelection` 放入請求原生擴充，提供實際參數名、候選及 `from_query` 標示；控制器僅用它產生控制項，不再選擇語言。URL 選擇保留 `Content-Language`、`Vary: Accept-Language` 及下游回應標頭行為；回呼涉及私人內容時，仍需應用程式設定快取策略。

### 自訂語言選擇 {#custom-locale-selection}

`localeUsing` 是 OpenRust 擴充介面，可為目前應用程式的中介軟體實例設定同步回呼。例如優先讀取指定請求標頭，缺少時依序嘗試 URL 選擇與瀏覽器協商：

```rust
.withMiddleware(|middleware| {
    middleware.setLocale(&["en", "zh-Hans", "zh-Hant"]).expect("语言标签有效");
    middleware.localeUsing(|request| {
        Ok(request.header("X-Locale")?.map(str::to_owned))
    });
})
```

回呼回傳 `Result<Option<String>, HttpError>`：

- `Ok(Some(locale))` 先正規化 BCP 47 標籤，再與正規化後的候選清單精確比對，不執行瀏覽器別名、基礎語言比對或翻譯備援。預設候選下，`ZH-hANT` 選擇 `zh-Hant`；`zh-CN`、`en-US`、空字串及無效標籤均報錯。若需選擇地區或擴充標籤，請將有效標籤明確加入候選，選中後保留完整標識。
- `Ok(None)` 先嘗試設定的 URL 參數，再使用既有瀏覽器協商、支援的應用程式預設語言或候選首項。
- `Err(error)` 將原錯誤交給既有例外處理並停止下游。無效或不支援的選擇視為內部錯誤；若值來自使用者輸入，可在回呼內驗證並回傳適當的 HTTP 錯誤。選擇失敗不切換語言，也不觸發 `LocaleUpdated`。

未設定回呼時行為不變。候選與回呼獨立設定，呼叫順序不限；候選更新失敗保留原候選及回呼，重複設定回呼使用最後一個。空候選關閉整個實例，不執行回呼、不觸發事件、不處理回應標頭，但保留回呼供重新啟用。路由實例也提供 `setLocale` 與 `localeUsing`，候選更新與回呼只屬於該實例，不使用行程全域登錄表；回呼本身也應避免共用可變使用者狀態。

預設全域位置早於路由、Session 與認證。回呼可以讀取請求標頭及此時已準備的資訊，不能存取後續才提供的依賴。依賴 Session 時，先在 `withMiddleware` 呼叫 `middleware.setLocale(&[]).expect("语言标签有效")` 關閉全域辨識，再於 `routes/web.rs` 註冊同一個中介軟體，放在預設 `web` 群組載入 Session 之後：

```rust
use std::sync::Arc;
use openrust::{
    contracts::session::Session,
    http::{Response, middleware::SetLocale},
    routing::{RouteGroup, RoutingError},
    support::facades::Route,
};

pub fn register() -> Result<(), RoutingError> {
    let mut locale = SetLocale::new(&["en", "zh-Hans", "zh-Hant"])
        .expect("语言标签有效");
    locale.localeUsing(|request| {
        Ok(request.session()?.get("locale")
            .and_then(|value| value.as_str().map(str::to_owned)))
    });
    Route::group(RouteGroup::new().middleware(Arc::new(locale)), |router| {
        router.get("/account", || Response::from("Account"))?;
        Ok(())
    })
}
```

範例要求已設定 Session 驅動及應用程式加密金鑰；只讀取既有的 `locale` Session 值，不儲存新偏好。沒有值時依序嘗試 URL 選擇與瀏覽器協商。依賴認證的規則同樣應放在認證之後；影響本地化路由綁定的規則應在綁定替換之前執行，需要此位置時透過既有 `Middleware::group`／`MiddlewareEntry` 明確設定群組順序。新增回呼不會自動調整執行順序。非同步查詢應由前置中介軟體完成，再透過請求傳遞已準備的資料；此回呼本身同步執行。

這些來源統一由 `SetLocale` 呼叫 `App::setLocale`、派發事件並處理回應標頭。下游主動切換語言後，缺少的 `Content-Language` 使用最終語言，既有值保持不變。由於回呼可能退回瀏覽器協商，仍會將 `Accept-Language` 合併到 `Vary`。這**不能**完整隔離由 `X-Locale`、Cookie、Session 或使用者資料選擇的回應。請求標頭範例還需讓回應依 `X-Locale` 區分；私人內容應設定 `Cache-Control: private, no-store` 或應用程式所需的快取策略，並按需新增其他 `Vary` 欄位。中介軟體保留這些回應標頭，不推斷快取鍵、不持久化 Cookie，也不保證個人化回應可安全進入共用快取。

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
