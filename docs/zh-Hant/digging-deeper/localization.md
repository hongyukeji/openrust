# 在地化

## 設定語言

透過 `config/app.rs` 的 `locale`、`fallback_locale` 設定預設語言及缺少群組詞條時的備援語言，對應根 `.env`：

```dotenv
APP_LOCALE=en
APP_FALLBACK_LOCALE=en
```

依照框架教學，使用 `App::set_locale` 切換一次 HTTP 請求的語言，使用 `App::current_locale` 讀取語言，使用 `App::is_locale("en")?` 判斷語言。路由範例：

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
        App::set_locale(&locale).await.map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        let current = App::current_locale().map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Ok(Response::from(current))
    })
}
```

`App::get_locale` 是 `current_locale` 的同義入口；`get_fallback_locale`、`set_fallback_locale` 讀寫備援語言。`set_locale` 同步更新 `app.locale` 與譯器，再發送 `foundation::events::LocaleUpdated`；事件包含 `locale` 和 `previous_locale`。Rust 事件監聽器可以非同步執行，因此此方法需要 `.await`，失敗透過 `Result` 傳回。請求和佇列工作內的修改互相隔離；啟動階段設定則作為該應用程式的預設值。

服務錯誤在實際呼叫步驟傳播：先寫設定，再更新譯器，最後派送事件。常駐譯器首次解析時分別讀取應用程式預設值和目前作用域設定，避免把請求語言保存為應用程式預設值。監聽器失敗不會回復本次已更新的設定與譯器，錯誤保留原始來源；目前生命週期結束後仍依作用域還原。重複設定相同語言亦會派送事件，修改備援語言不派送該事件。

設定、API、資源、文件 URL、HTML 與 HTTP 統一使用 BCP 47。`set_locale("zh-CN")` 後仍為 `zh-CN`；`is_locale("ZH-cn")` 為真，`is_locale("zh-Hans")` 為假。正規化保留文字體系、地區、變體及支援的擴充，資源選擇另行處理。舊底線輸入會明確報錯。

`App` Facade 目前提供上述語言方法；其他框架App 方法尚未透過此 Facade 提供。範本繼續使用下文的翻譯助手。`Lang::with_locale` 僅用於暫時的譯器語言作用域，不修改應用程式語言設定。

### 瀏覽器語言 {#welcome-page-language}

應用程式預設自動裝配 `openrust::http::middleware::SetLocale`，應用程式不必建立語言中介軟體或在 `bootstrap/app.rs` 手動註冊。

預設候選在 HTTP 啟動時從框架及應用翻譯資源發現一次；框架內建全部18語。中介軟體為配置的協商標頭共用請求語言解析器，依權重選擇、略過 `q=0`，同權重保留瀏覽器順序。沒有符合項目時使用支援的 `APP_LOCALE`，否則使用按標籤穩定排序後的首個候選。控制器、驗證與 Blade 模板共用目前請求語言。

`zh-Hans`、`zh-CN`、`zh-SG` 與單獨 `zh` 對應簡體；`zh-Hant`、`zh-TW`、`zh-HK`、`zh-MO` 對應繁體，明確的文字體系優先於地區，未知文字體系不依中文地區猜測。數字變體不視為文字體系。其他語言比對完整標識或基礎語言。此選擇規則不改變公開語言標籤的正規化語意。

透過 [`app.supported_locales`](#available-locales) 設定即可限制語言、指定順序或啟用非檔案來源。需要在程式碼中覆寫設定時，在 `bootstrap/app.rs` 的 `with_middleware` 中明確指定候選：

```rust
.with_middleware(|middleware| {
    middleware.set_locale(&["en", "fr", "de"]).expect("語言標籤有效");
})
```

候選依 BCP 47 正規化；無效標籤回傳 `TranslationError` 並保留先前設定，重複呼叫使用最後一次成功設定。傳入空清單 `middleware.set_locale(&[])` 關閉辨識，此時不修改語言或回應標頭，也不觸發語言更新事件，應用程式繼續使用 `app.locale` 與手動 `App::set_locale`。僅用於特定路由時，先關閉全域辨識，再透過既有路由或路由群組中介軟體註冊 `SetLocale::new(&["en", "fr"])?`。

中介軟體在執行下游前呼叫 `App::set_locale`，同步設定、翻譯器與 `LocaleUpdated` 事件；監聽器失敗時交由既有例外處理，不繼續執行下游。下游處理完成（包括已渲染的例外回應）後讀取最終語言，在缺少時新增 `Content-Language`，將啟用的語言標頭（預設 `Accept-Language`）合併至 `Vary`，保留既有值或萬用字元。自動選擇不儲存偏好，不改變其他請求、佇列工作或應用程式預設語言。預設路由裝配位於輸入轉換與安全 Cookie 建立之後，詳見[自動與固定語言](#locale-preference)；先前已回傳回應的 CORS 預檢、維護模式或請求大小檢查不會經過語言選擇。

原先應用程式中的 `SetLocale` 檔案與 `append` 註冊應移除，避免重複執行；需要關閉辨識時改用上述空清單設定。

歡迎頁的 HTML `lang` 與 `Content-Language` 使用應用程式目前語言，文案依設定備援。文件連結為 `en`、`zh-Hans`、`zh-Hant` 選擇對應目錄，其他執行階段語言使用英文文件。此映射僅選擇文件位址，不改變應用語言或詞條備援。

### 框架預設譯文與應用覆寫 {#default-translations}

18語預設資源位於 `vendor/openrust/framework/src/translation/lang/`，隨二進位檔編譯交付；預設應用在 `lang/` 保留英語、簡體中文、繁體中文三份可編輯的 `welcome.json`；其他預設詞典無需複製到應用。只建立需要覆寫的鍵，例如 `lang/fr/validation.json`：

```json
{"required": "自訂提示：:attribute 必填", "min": {"string": "至少輸入 :min 個字元"}}
```

群組資源按鍵遞迴合併：上述鍵優先，其他法語驗證訊息繼續使用框架預設值。原句詞典 `lang/fr.json` 按頂層原句鍵覆寫。仍缺失的鍵再走已設定的回退語言；應用JSON損壞會報錯，不用預設值掩蓋錯誤。已載入的譯器會快取資源，修改覆寫檔後應重新啟動常駐程序。

框架預設資源不依賴執行期原始碼目錄，也不要求先發佈。需要整份自訂時可執行 `lang:publish`，按需發佈所有內建群組及原句詞典；未要求覆寫時保留已有檔案。套件命名空間仍按獨立的套件翻譯覆寫機制處理。

### 可用語言 {#available-locales}

一般應用可直接透過 `.env` 設定 `app.supported_locales`，無須編寫中介軟體回呼：

```dotenv
APP_SUPPORTED_LOCALES=en,zh-Hans,zh-Hant,ja
```

環境變數未設定時預設 `*`，自動探索框架及應用資源中的全部可用語言。`APP_SUPPORTED_LOCALES=` 產生空清單，關閉語言選擇和歡迎頁選單；逗號分隔的清單依指定順序啟用語言，也支援僅來自資料庫的語言。標籤正規化並依首次出現去重。明確設定 `null`、`(null)`（不區分大小寫）、非法標籤及清單空項在啟動時報錯。直接設定僅接受 `"*"` 或 JSON 陣列，拒絕 JSON `null`、其他型別及字串。星號不作為陣列成員，也不表示所有合法 BCP 47 標籤都有譯文。

候選優先序為明確 `middleware.set_locale(...)` → `app.supported_locales` → 資源自動發現，明確空清單同樣優先。設定清單和明確清單均略過掃描。預設中介軟體在 HTTP 啟動時儲存一次候選；修改後重新啟動應用，使用設定快取時同時重建快取。本設定不改變 `app.locale`、`app.fallback_locale` 或請求來源優先序。


Provider載入器包含框架內建語言；直接 `FileLoader::new(path)` 仍只載入指定的檔案來源。`FileLoader::available_locales()` 發現 `<語言>.json` 和包含分組 JSON 資源的語言目錄，也支援巢狀分組。掃描路徑與翻譯載入一致：存在 `resources/lang` 時使用該目錄，否則使用 `lang`，並包含 `add_path` 登記的應用程式路徑和 `add_json_path` 登記的頂層 JSON 檔案。套件命名空間、`vendor`、隱藏項目、符號連結和空語言目錄不會啟用語言。檔案及目錄名必須已是規範 BCP 47 標籤，例如使用 `zh-Hans`，不用 `zh_CN` 或 `ZH-hans`。結果去重並按標籤排序；發現過程不剖析詞條內容，也不判斷翻譯是否完整。

預設中介軟體在應用程式 Provider 啟動完成後、接收請求前保存這份候選。每個請求不再掃描；新增或移除語言資源後需重新啟動應用程式。候選發現獨立於設定快取。明確 `set_locale` 優先於設定；未明確設定時，`app.supported_locales` 陣列略過掃描。未發現語言時保留應用程式設定的語言並隱藏歡迎頁切換選單。僅來自資料庫的語言透過明確候選啟用；發現過程不查詢資料庫，也不自動啟用所有套件的語言。

例如新增 `lang/ja/welcome.json`，以及內容為 `{"name":"日本語"}` 的 `lang/ja/languages.json`，即可增加日語，無需修改控制器或選單。缺少文案繼續使用正常翻譯備援。範本可透過 `Lang::has_for_locale('languages.name', $choice)` 判斷該語言自身是否提供名稱。右至左頁面在該語言的 `languages.json` 中增加 `"direction":"rtl"`，歡迎頁據此設定 HTML `dir="rtl"`；選單支援長清單捲動。未提供方向時使用文件預設的左至右版面。新增介面翻譯不會建立文件目錄，歡迎頁文件連結仍須對應實際提供的文件。

### URL 語言選擇 {#locale-query}

`?locale=en`、`?locale=zh-Hans`、`?locale=zh-Hant` 僅覆蓋本次請求，不儲存或替換固定偏好。順序為回呼 → URL → 受保護的偏好 Cookie → 設定請求頭 → 預設語言或首個候選。回呼傳回 `None` 才進入下一來源。明確的 URL 值正規化後必須精確屬於候選；非法、空值、陣列或不支援的值傳回 HTTP 400。重複純量鍵沿用查詢剖析器的後值覆蓋規則。請求本文欄位不參與本次語言選擇。

在 `config/app.rs` 的語言區域設定：

```rust
"locale_query_parameter": get(env, "APP_LOCALE_QUERY_PARAMETER", "locale")?,
```

設定 `APP_LOCALE_QUERY_PARAMETER=language` 後只讀 `?language=...`。缺鍵預設 `locale`，JSON `null` 僅關閉 URL 選擇。參數名須為非空頂層鍵，不含空白、控制字元、點或方括號，錯誤設定型別明確報錯。環境值仍是字串。修改後重建設定快取；請求使用已載入的儲存庫，不重新讀取 `.env`。

查詢參數和 Cookie 預設名稱現統一為 `locale`（`?locale=zh-Hans`，Cookie 加密前為 `locale=zh-Hans`）。預設不再讀取舊 `lang` Cookie，請重新選擇語言並更新舊書籤。需保留舊名稱時，明確設定 `APP_LOCALE_COOKIE=lang` 和／或 `APP_LOCALE_QUERY_PARAMETER=lang`。升級後重建設定快取；快取中明確儲存的名稱仍會生效。儲存入口保持 `POST /locale`，表單欄位預設為 `locale`。

### 自動跟隨與固定語言 {#locale-preference}

從未手動設定時，Web 頁面每次請求重新協商語言。歡迎頁選單遍歷中介軟體的同一份有效候選，並提供「跟隨瀏覽器」。受現有 Web CSRF 中介軟體保護的 POST 表單儲存選擇，「跟隨瀏覽器」刪除偏好；自動識別和 GET 不儲存偏好。底線表示本次實際語言，勾選表示固定選擇或自動模式，URL 暫時覆蓋時也分別顯示。母語名稱來自各語言的 `languages.json`；該語言缺少名稱時顯示語言代碼，不備援為其他語言的名稱。POST 後以 303 返回表單指定的站內頁面，保留其他查詢參數並移除暫時語言參數；關閉 URL 識別不影響儲存表單。

語言偏好設定 `app.locale_cookie` 預設 `"locale"`，缺鍵相同，應用保留 `APP_LOCALE_COOKIE` 覆寫名稱。JSON `null` 關閉偏好讀取與寫入（環境字串 `null` 是名稱）。名稱須為非空頂層 Cookie 名稱，不能與 Session 或 `XSRF-TOKEN` 衝突，非法設定報錯。必須經過 `EncryptCookies`，不能豁免加密。預設實例自動裝配在 Web Cookie 解密和佇列之後、一般 Web Session 之前。無狀態 API 不讀取原始偏好 Cookie，也不自動啟動 Session。沒有安全 Cookie 佇列時，歡迎頁在 URL 識別啟用時保留單次 GET 連結。

偏好重用 `Cookie::forever`（400 天）、加密、HttpOnly，以及現有 CookieJar 的 `session.path`、`domain`、`secure`、`same_site` 預設值（通常為 `/`、目前主機、HTTPS 時 Secure、Lax）。刪除使用相同路徑與網域。僅提供 HTTPS 的部署應設 `session.secure=true`。過期、清除 Cookie 或更換瀏覽器都會遺失本地選擇，不承諾永久或跨裝置同步。損壞、非法或已不屬於候選的已儲存值會被忽略並恢復自動；明確的非法提交傳回 HTTP 400。過期由客戶端執行，重播的 Cookie 請求頭本身不攜帶過期資訊。

受保護的 Web 寫入入口可使用以下語言偏好輔助方法，只讀取指定頂層請求本文欄位，由控制器決定回應：

歡迎頁表單欄位在 `config/app.rs` 中獨立設定：

```rust
"locale_form_field": get(env, "APP_LOCALE_FORM_FIELD", "locale")?,
```

設定 `APP_LOCALE_FORM_FIELD=preferred_locale` 可同時修改歡迎頁表單與儲存入口。範本使用 `config('app.locale_form_field')`，儲存入口嚴格使用 `Config::string("app.locale_form_field")`，均不另設備援；預設值 `locale` 由應用程式設定定義。缺鍵、JSON `null` 或非字串會使儲存入口報錯且不寫入偏好。請使用非空頂層欄位名稱，避免與 `redirect`、`_token`、`_method` 衝突。修改後重建設定快取。此設定不修改 Cookie 或 URL 參數名稱；通用儲存方法仍接受呼叫方指定的欄位。

```rust
let redirect = match request.post(Some("redirect")).await? {
    None => "/".to_owned(),
    Some(serde_json::Value::String(path)) => path,
    _ => return Err(HttpError::InvalidRequest),
};
let target = SetLocale::url_without_locale(request, &redirect)?;
SetLocale::store_preference_from_request(request, "locale").await?;
Ok(Redirect::to(&target, RedirectOptions {
    status: StatusCode::SEE_OTHER,
    ..Default::default()
})?.into())
```

歡迎頁表單透過請求體欄位 `redirect` 提交目前頁面路徑。其他頁面可傳 `/account?tab=profile`，儲存後返回該頁面；缺少、null 或空值以 `/` 兜底，錯誤型別和非站內目標在寫入偏好前返回400。查詢參數不能補充此欄位。這是應用表單約定，不是新增中間件設定。

偏好欄位缺少，或其值為數字、布林、陣列、物件時傳回 HTTP 400；JSON `null` 或空字串清除偏好，非空字串重用既有 BCP 47 與候選驗證。查詢參數不會補齊請求本文欄位。欄位取自既有輸入中介軟體處理後的請求本文；預設裁剪和空字串轉換可能將純空白轉為清除操作。此方法不改變本次請求語言，也不繞過請求方法、Cookie 管線或 CSRF 約束。既有 `store_preference(request, Some("zh-Hans"))` 與 `store_preference(request, None)` 仍可處理經過驗證的值。

`url_without_locale(request, "/")` 傳回應用程式指定的目標，不會再次跳到儲存入口。它從目標和目前請求中移除實際語言查詢鍵，保留其他值與編碼，使用既有 URL 產生器處理可信代理子路徑；請求中的其他值覆寫目標中的同名值。只接受以單個 `/` 開頭的本機路徑，拒絕外部位址、通訊協定相對位址與反斜線。它只產生 URL，不儲存偏好或建立回應。頁面範例需匯入 `SetLocale`、`Redirect`、`RedirectOptions` 與 `StatusCode`。

使用相同受保護 Cookie 管線的 API 控制器也可以讀取自訂 `language` 欄位，直接傳回 204 或自行產生 JSON：

```rust
SetLocale::store_preference_from_request(request, "language").await?;
Ok(Response::empty(StatusCode::NO_CONTENT))
```

這不會自動為一般無狀態 API 啟用持久化；須明確接入既有具狀態 Cookie 管線與寫入保護，瀏覽器請求仍需 CSRF。輔助方法不會註冊路由、啟動 Session 或增加帳號儲存。兩個輔助入口均用於管理語言偏好。

每次寫請求選擇其中一種操作，再導向應用控制的站內位址。方法拒絕讀取請求及不可用或未加密的偏好儲存，只排隊 Cookie，不改變本次語言或再次發出事件，不能代替 CSRF 中介軟體。`LocaleSelection` 提供 `preference`、`can_store`、候選和 URL 資訊；實際語言使用 `App::current_locale()`。回呼已決定語言時，不顯示無法覆蓋它的控制項；空候選關閉整個中介軟體。

安全偏好頁面透過現有回應頭合併機制設定 `Cache-Control: private, no-store` 和 `Vary: Cookie`，未儲存偏好時也適用。請求頭識別啟用時另合併實際頭名，保留既有 `Vary` 和 `*`；路由快取指令不能將這些變體公開快取。帳號等私有來源的回呼仍須設定適當的應用快取策略，尤其是未接入 Cookie 管線時。

主題選單提供淺色、深色和跟隨系統。自動模式回應系統變化，手動主題存入 localStorage（`appearance`），跟隨系統會清除覆蓋。停用 JavaScript 後語言表單仍可使用，主題跟隨系統。

### API 客戶端偏好 {#api-locale-preference}

獨立前端和行動端自行儲存固定選擇。自動模式每次傳送目前系統／瀏覽器語言，固定模式透過設定請求頭（預設 `Accept-Language`）傳送儲存的標籤，恢復自動時清除客戶端固定選擇。傳送語言頭不會建立伺服器端永久偏好、Cookie、Session 或帳號記錄，只有接入翻譯資源的文案改變。儲存庫提供伺服器端能力與消費者行為，沒有實作行動端儲存。

同源有狀態瀏覽器 API 只有在管線於 SetLocale 之前完成 Cookie 解密和佇列裝配後，才可重用相同偏好。內建有狀態 API 包裝器為可信前端來源提供此管線，一般 API 仍無狀態；接入後同樣按回呼 → URL → Cookie → 請求頭排序，寫入入口保留 CSRF 保護。傳回 JSON 本身不會啟用偏好。

需要跨裝置同步時，在應用帳號設定中儲存語言，並在認證之後的實例 `locale_using` 回呼中傳回已驗證的可信候選；帳號未設定則傳回 `None`，繼續 Cookie 與瀏覽器備援。框架不要求或建立使用者模型欄位和帳號表。關閉預設實例、將同一中介軟體放到依賴之後的範例見[自訂語言選擇](#custom-locale-selection)。

### 語言偏好請求標頭 {#locale-header}

API 用戶端預設傳入標準請求標頭：

```http
Accept-Language: zh-Hans
```

在 `config/app.rs` 中，`"locale_header": get(env, "APP_LOCALE_HEADER", "Accept-Language")?` 指定應用程式語言協商標頭。設定 `APP_LOCALE_HEADER=X-Language` 後只讀取該標頭，名稱按 HTTP 規則不區分大小寫；缺少時不再嘗試 `Accept-Language`。值繼續使用相同的 BCP 47 驗證、權重、多標頭合併及候選比對規則。缺鍵預設 `Accept-Language`；JSON `null` 僅關閉標頭辨識。環境值以字串讀取：`APP_LOCALE_HEADER=null` 表示名為 `null` 的標頭，並不關閉辨識；關閉時請在應用程式配置中設定 JSON `null`。空名稱、非法名稱或非字串類型屬於配置錯誤（HTTP 500），即使回呼或 URL 已選擇語言也會回報。空候選仍關閉整個中介軟體。

選擇順序為 `locale_using` 傳回 `Some` → 配置的 URL 參數 → 受保護的偏好 Cookie → 配置的請求標頭 → 應用程式預設語言及候選備援；回呼傳回 `None` 才進入下一來源。配置使用既有儲存庫與配置快取，修改原始碼或環境不會覆蓋已載入的快取，修改 `APP_LOCALE_HEADER` 後須重建配置快取。標頭與 URL 語言辨識均可設定。

`Request::get_languages`／`get_preferred_language` 始終描述標準 `Accept-Language`，不受應用程式配置影響。只有接入翻譯資源的文案隨語言變化。啟用標頭辨識時，`Vary` 合併實際配置的標頭名稱，保留既有項目與 `*`；`null` 不新增語言標頭項目，仍保留 URL、回呼與 `Content-Language`。依賴 Cookie 或使用者資料的回呼仍需應用程式設定適當的私人快取策略及其他 `Vary` 項目。

業務、控制器與頁面讀取實際使用的語言時使用 `App::current_locale()?`；URL、持久化偏好回呼或下游 `App::set_locale` 都可能讓它不同於請求偏好。例如配置 `APP_LOCALE_HEADER=X-Language`，同時收到 `Accept-Language: en` 與 `X-Language: zh-Hant` 時，Request 仍傳回英文偏好，App 與翻譯使用繁體中文。停用 SetLocale 後，配置本身不會自動執行語言選擇。

### 自訂語言選擇 {#custom-locale-selection}

一般自訂協商標頭直接設定既有 `APP_LOCALE_HEADER=X-Locale`，未設定時協商 `Accept-Language`。該標頭位於 URL 和受保護 Cookie 偏好之後；請求缺少設定的自訂標頭時，使用預設語言或首個候選，不再嘗試 `Accept-Language`。這與優先於其他來源的回呼不同。一般候選清單使用上方設定，使用者或租戶語言等業務規則再使用回呼。

`locale_using` 可為目前應用程式的中介軟體實例設定同步回呼。例如優先讀取指定請求標頭，缺少時依序嘗試 URL、受保護的偏好 Cookie 與標頭協商：

```rust
.with_middleware(|middleware| {
    middleware.locale_using(|request| {
        Ok(request.header("X-Locale")?.map(str::to_owned))
    });
})
```

回呼回傳 `Result<Option<String>, HttpError>`：

- `Ok(Some(locale))` 先正規化 BCP 47 標籤，再與正規化後的候選清單精確比對，不執行瀏覽器別名、基礎語言比對或翻譯備援。預設候選下，`ZH-hANT` 選擇 `zh-Hant`；`zh-CN`、`en-US`、空字串及無效標籤均報錯。若需選擇地區或擴充標籤，請將有效標籤明確加入候選，選中後保留完整標識。
- `Ok(None)` 先嘗試設定的 URL 參數和受保護的偏好 Cookie，再使用既有瀏覽器協商、支援的應用程式預設語言或候選首項。
- `Err(error)` 將原錯誤交給既有例外處理並停止下游。無效或不支援的選擇視為內部錯誤；若值來自使用者輸入，可在回呼內驗證並回傳適當的 HTTP 錯誤。選擇失敗不切換語言，也不觸發 `LocaleUpdated`。

未設定回呼時行為不變。候選與回呼獨立設定，呼叫順序不限；候選更新失敗保留原候選及回呼，重複設定回呼使用最後一個。空候選關閉整個實例，不執行回呼、不觸發事件、不處理回應標頭，但保留回呼供重新啟用。路由實例也提供 `set_locale` 與 `locale_using`，候選更新與回呼只屬於該實例，不使用行程全域登錄表；回呼本身也應避免共用可變使用者狀態。

預設實例在路由管線執行，位於 Cookie 解密／佇列之後、一般 Web Session 與認證之前；有狀態 API 包裝器則先建立其 Cookie／Session 保護。未匹配錯誤頁與公開靜態回應仍執行無 Web 儲存的語言選擇。回呼可以讀取請求標頭及此時已準備的資訊，不能存取後續才提供的依賴。依賴 Session 時，先在 `with_middleware` 呼叫 `middleware.set_locale(&[]).expect("语言标签有效")` 關閉全域辨識，再於 `routes/web.rs` 註冊同一個中介軟體，放在預設 `web` 群組載入 Session 之後：

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
    locale.locale_using(|request| {
        Ok(request.session()?.get("locale")
            .and_then(|value| value.as_str().map(str::to_owned)))
    });
    Route::group(RouteGroup::new().middleware(Arc::new(locale)), |router| {
        router.get("/account", || Response::from("Account"))?;
        Ok(())
    })
}
```

範例要求已設定 Session 驅動及應用程式加密金鑰；只讀取既有的 `locale` Session 值，不儲存新偏好。沒有值時依序嘗試 URL、受保護的偏好 Cookie 與標頭協商。依賴認證的規則同樣應放在認證之後；影響本地化路由綁定的規則應在綁定替換之前執行，需要此位置時透過既有 `Middleware::group`／`MiddlewareEntry` 明確設定群組順序。新增回呼不會自動調整執行順序。非同步查詢應由前置中介軟體完成，再透過請求傳遞已準備的資料；此回呼本身同步執行。

這些來源統一由 `SetLocale` 呼叫 `App::set_locale`、派發事件並處理回應標頭。下游主動切換語言後，缺少的 `Content-Language` 使用最終語言，既有值保持不變。由於回呼可能退回標頭協商，啟用時仍將配置的語言標頭合併至 `Vary`。這**不能**完整隔離由 `X-Locale`、Cookie、Session 或使用者資料選擇的回應。請求標頭範例還需讓回應依 `X-Locale` 區分；私人內容應設定 `Cache-Control: private, no-store` 或應用程式所需的快取策略，並按需新增其他 `Vary` 欄位。中介軟體保留這些回應標頭，不推斷快取鍵、不持久化回呼結果，也不保證任意個人化回應可安全進入共用快取。安全 Cookie 偏好頁面還會取得 `private, no-store` 與 `Vary: Cookie`。

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

執行 `./artisan lang:publish` 發布隨附十八語的身分驗證、分頁、密碼重設、欄位驗證與通知原句詞條。預設使用 `lang/`，已有 `resources/lang/` 時使用後者。預設保留既有檔案；`--force` 覆寫隨附檔案，`--existing` 僅覆寫已存在的隨附檔案。兩者同時使用時由 `--existing` 決定範圍。應用自訂群組與其他語言檔案維持不變。

## 定義翻譯字串

群組翻譯放在 `lang/en/messages.json` 等檔案中，巢狀項目使用點號存取。以完整句子為鍵的翻譯放在 `lang/en.json`。應用程式存在 `resources/lang` 時，會使用該目錄作為語言目錄。

```json
{
  "welcome": "Hello, :Name",
  "items": "{0} No items|{1} One item|[2,*] :count items"
}
```

### 登記目錄與執行期翻譯 {#registering-translation-sources}

在應用程式作用域首次查找之前，用 `Lang::add_path`、`Lang::add_json_path` 或 `Lang::add_namespace` 登記目錄。較晚登記的目錄會覆蓋較早的目錄。分組資源遞迴合併，包括清單索引。JSON 字面鍵以整項覆蓋。應用程式目錄在 JSON 專用目錄之後載入。

用 `Lang::add_lines` 登記帶點號的分組鍵。首次登記的分組以提供的翻譯為準。要擴充檔案中的分組，先呼叫 `Lang::load`。載入後的分組會留在應用程式快取，後續路徑登記適用於之後載入的分組。

```rust
use std::path::PathBuf;

use openrust::{
    support::facades::Lang,
    translation::{Line, TranslationResult},
};

pub fn register_translations(directory: PathBuf) -> TranslationResult<()> {
    Lang::add_path(directory)?;
    Lang::load(None, "messages", "en")?;
    Lang::add_lines(
        [("messages.runtime".into(), Line::Text("Registered :name".into()))],
        "en",
        None,
    )
}
```

### 從自訂來源載入分組 {#loading-custom-translation-sources}

在應用程式的非同步入口向 `Lang::load_from` 傳入 `Loader`。每個資源指定語言、分組和選用的命名空間。框架一次替換這批分組。讀取失敗或其中一個分組在載入期間被執行期更新時，保留目前的目錄。後續翻譯查詢仍為同步呼叫。

```rust
use openrust::{
    contracts::translation::Loader,
    support::facades::Lang,
    translation::{TranslationResource, TranslationResult},
};

pub async fn load_notice(source: &dyn Loader) -> TranslationResult<()> {
    Lang::load_from(
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

在應用程式 Cargo 相依套件中透過 `path = "vendor/openrust/translation-database"` 加入 `openrust-translation-database`。`DatabaseLoader` 接收明確的框架 `Connection`，可從目前應用程式的資料庫管理器或 `Db::connection` 取得。支援 PostgreSQL 18、MySQL 8.4 和 SQLite 的資料庫詞條載入，其他資料庫目標須分別驗證行為。

先透過應用程式遷移建立資料表，再載入翻譯。可使用以下最小結構：

```rust
use openrust::database::{Connection, DatabaseResult};

pub async fn create_translations(connection: &Connection) -> DatabaseResult<()> {
    connection.get_schema_builder().create("language_lines", |table| {
        table.id();
        table.string("group", None).index();
        table.string("key", None);
        table.json("text");
        table.timestamps();
    }).await
}
```

資料表結構沿用 Spatie框架Translation Loader：每個群組與鍵儲存一列，`text` 是語言到字串的 JSON 物件，例如 `{"en":"Hello :name","fr":"Bonjour :name"}`。語言鍵使用標準識別碼（`en-US`、`zh-Hans`）。缺少或為 null 的語言保留檔案值；翻譯值必須為字串，空字串也是有效覆寫。群組 `*` 儲存字面鍵並保留句點，其他群組展開 `nested.title`、`items.0` 等點分鍵。套件命名空間與 Spatie 一樣僅使用檔案來源。重複鍵、父子鍵衝突、無效 JSON、鍵中空段及超過 128 層的鍵會使載入失敗。

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
        .with_table("language_lines")?
        .with_max_rows(10_000)?
        .with_fallback(Arc::new(FileLoader::new(language_directory)));
    Lang::load_from(
        &source,
        &[
            TranslationResource { locale: "fr", group: "messages", namespace: None },
            TranslationResource { locale: "en", group: "messages", namespace: None },
        ],
    ).await
}
```

資料表名稱套用所選連線的資料表前綴，只接受不帶 schema 的識別碼。預設資料表名稱為 `language_lines`，每個群組預設最多讀取 10,000 列；超過設定上限會回報錯誤，不截斷結果。

`with_fallback` 從檔案來源載入相同語言、群組與命名空間，再用資料庫詞條覆寫。群組和清單索引遞迴合併，字面 JSON 值整體替換。資料庫空字串仍為有效覆寫；刪除一列後，明確重新載入該資源即可恢復檔案值。語言回退繼續使用翻譯器設定；需要資料庫回退詞條時，同時載入所選語言與回退語言資源。

`Lang::load_from` 一次發佈全部請求群組。資料庫、檔案或解析錯誤會回傳失敗，並保留先前發佈的目錄；沒有詞條的資源提供空資料庫覆寫。資料庫按群組讀取，發佈原子性不提供橫跨所有讀取的資料庫交易快照。重新載入由應用程式明確觸發。寫入、遷移與刷新時機由應用程式管理，套件不會自動選擇來源或啟動背景工作。

與 Spatie 的 PHP 模型不同，本來源讀取每列時不執行語言回退；翻譯器在載入精確語言後按自身設定回退，也因此保留所選語言的檔案詞條。缺表會回傳錯誤，避免明確重新載入失敗時靜默以檔案取代已發佈的資料庫目錄。

## 取得翻譯字串

使用 `__` 讀取文字。預留位置支援 `:name`、`:Name` 和 `:NAME`。群組和清單使用 `Lang::array` 讀取。缺少翻譯時回傳鍵本身。JSON 字面鍵在所選語言內查找，群組鍵可使用設定的備援語言。

### 語言回退與缺少的鍵 {#translation-lookup-hooks}

使用 `Lang::has(key, locale, true)` 檢查含回退語言的翻譯，使用 `Lang::has_for_locale(key, locale)` 僅檢查指定語言。兩種檢查都會略過缺鍵回呼。在應用程式作用域內登記掛鉤後，可調整分組翻譯的回退順序，JSON 字面鍵仍優先。缺鍵回呼傳回的文字會繼續處理預留位置，傳回 `None` 時使用原鍵。

```rust
use openrust::{support::facades::Lang, translation::TranslationResult};

pub fn register_lookup_hooks() -> TranslationResult<()> {
    Lang::determine_locales_using(|locales| {
        let mut locales = locales.to_vec();
        if locales.first().is_some_and(|locale| locale == "fr-CA") {
            locales.insert(1, "fr".into());
        }
        locales
    })?;

    Lang::handle_missing_keys_using(|missing| {
        (missing.key == "messages.welcome").then(|| "Hello, :name".into())
    })
}
```

### 取代翻譯字串中的參數 {#replacing-parameters-in-translation-strings}

使用 `Lang::get_with` 依序傳入文字、標籤和物件。回呼接收同一行上每個相符的 `<name>…</name>` 的內容。標籤回呼依參數順序執行，接著取代文字預留位置。使用 `Lang::array_with` 取代群組或清單中的值。使用 `Lang::choice_with` 時先選擇複數分支，再執行取代。

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
    Lang::get_with(
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
    Lang::with_locale(locale, async {
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

Artisan 使用 `APP_LOCALE` 顯示命令清單標題、說明和引數錯誤。應用隨附下列十八語 console 資源，可在 `lang/<locale>/console.json` 覆寫。翻譯先查目前語言，再查 `APP_FALLBACK_LOCALE`，最後使用內建文字。沒有翻譯的命令說明沿用註冊文字。

```sh
APP_LOCALE=en ./artisan help list
APP_LOCALE=zh-Hans ./artisan list make
APP_LOCALE=zh-Hant ./artisan help list
```

可在 `console` 群組中使用 `options`、`commands.list.description` 和 `commands.list.arguments.namespace` 等鍵調整說明。請參閱 [Artisan 輸出格式](./artisan.md#command-help-and-output-formats)。

正式環境確認提示和取消訊息使用同一翻譯群組。可在 `console.json` 中覆寫 `confirm.production` 或 `confirm.cancelled`。輸入 `y` 或 `yes` 確認，直接按 Enter 取消。

## 契約語言設定

Translator 契約、具體實作與 Lang Facade 都提供 `get_locale`、`set_locale`、`get_fallback`、`set_fallback`。啟動時設定應用程式預設值；請求／佇列作用域內設定僅影響目前執行。`with_locale` 支援巢狀並在離開後恢復，回退語言獨立設定。Rust 方法採用原生 snake_case 命名；舊 camelCase Rust 消費者改用上述方法。範本運算式保留各自既有名稱。

## 語言設定的所有權

語言與回退語言的讀取方法傳回擁有所有權的 `String` 快照。設定方法接收 `&self`，傳回 `TranslationResult<()>`，無效標籤傳回錯誤；`zh-CN` 與 `zh-Hans` 保持不同。啟動值作為預設值，請求或佇列工作期間的修改只保留在目前容器作用域。`with_locale` 僅暫時覆蓋目前工作，完成後恢復外層語言。

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

Faker 原生 provider 鍵僅保留於私有邊界，`locale()`、`random_locale()` 輸出 BCP 47；僅有文字體系的 `zh-Hans`／`zh-Hant` 使用既有對應中文 provider，公開標籤仍保留原地區及擴充。微信原生 `lang=zh_CN` 等第三方協定欄位保持不變。


## 替換複數選擇器

`Translator::get_selector()` 傳回目前的 `Arc<dyn Selector>`。實作 `openrust::translation::Selector` 後傳入 `set_selector`，即可自訂複數分支選擇。其 `choose` 接收原始文案、`PluralCount` 和解析後的語言，傳回 `TranslationResult<String>`，之後才替換 `:count` 等預留位置。透過 `set_selector(MessageSelector)` 恢復預設選擇器。`get_loader()` 傳回實際共用的 `Arc<FileLoader>`；先前取出的選擇器和載入器控制代碼仍然有效。

## 隨附語言資源 {#bundled-locales}

`en`, `zh-Hans`, `zh-Hant`, `ja`, `ko`, `fr`, `de`, `es`, `pt`, `it`, `ru`, `ar`, `hi`, `id`, `vi`, `th`, `tr`, `nl`.

一起部署二進位檔、歡迎頁範本及應用自訂語言覆寫；預設譯文無需獨立複製。重建設定與視圖快取，並重啟常駐 Worker。明確候選清單仍由應用控制。語言資源、數字格式、訊息複數與地區 Faker 資料屬於不同能力，請另行設定 Faker。


### 語言能力邊界

| 能力 | 範圍 |
| --- | --- |
| 執行期資源 | 18語均包含歡迎頁、語言名、命令、欄位說明、驗證身分、密碼、分頁、資料驗證群組及通知原句詞典。 |
| 訊息複數 | 沿用既有訊息選擇器；阿語六分支、俄語三分支，不變形語言取首分支。小數取模按選擇器契約處理，與ICU數字格式化分開。 |
| 數字 | ICU小數格式化接受18個語言標識，不表示所有語言都有數字拼寫能力。 |
| Faker | 獨立設定 `app.faker_locale`。介面通用語言 `fr` 不會自動選擇 `fr-FR`；各格式器仍按其地區資料與既有回退運行。`zh-Hans`／`zh-Hant` 保留既有中文提供者對應。 |
| 開發者文件 | 英語、簡體中文、繁體中文；歡迎頁其餘語言連結至英語文件。 |

歡迎頁視圖透過目前語言資源中的 `welcome.documentation_locale` 取得文件目錄。隨附語言尚無對應文件時使用 `en`；自訂語言缺少此鍵時也使用 `en`。新增某種語言的文件後，在該語言的應用程式 `welcome.json` 中覆寫此鍵即可，例如 `"documentation_locale": "ja"`。這只改變文件連結，不改變頁面語言。

歡迎頁介紹使用 `:name` 預留位置，視圖傳入 `config('app.name')`。修改 `APP_NAME` 後，各隨附語言的介紹都會使用新的應用程式名稱，輸出維持 HTML 跳脫。

標準譯文隨附完整MIT聲明。框架補充譯文包含機器輔助初稿，已做技術校驗，母語編輯審校須單獨進行。本機捕獲及佇列投遞測試不代表外部郵件供應商投遞驗收。

## 語言目錄

應用程式建立時，若 `resources/lang/` 已存在，優先使用它，否則選擇根目錄的 `lang/`。讀取路徑不會建立目錄。應用程式 Contract 提供 `lang_path("")`，也可傳入 `lang_path("en/validation.json")` 等後綴。

需要自訂目錄時，在應用程式組裝階段、解析翻譯載入器之前設定：

```rust
use openrust::contracts::foundation::Application as _;

app.use_lang_path(app.base_path().join("translations"))?;
```

在 HTTP、Artisan 或佇列作用域內，`App::lang_path("")` 與 `openrust::support::lang_path("")` 讀取相同的應用程式路徑，也可使用 `App::use_lang_path(...)`。目錄屬於整個應用程式，不應逐請求切換。已解析的載入器保留原目錄，因此應在載入服務之前設定，不依賴熱切換。

`lang:publish` 發布至此目錄，預設保留既有檔案；`--force` 覆寫可發布範本，`--existing` 只覆寫已發布的範本。套件資源可將 `app.lang_path("vendor/package/...")` 傳給 `publishes_paths` 作為目標；`vendor:publish` 支援明確設定的絕對路徑，包括專案外目錄，仍拒絕相對路徑穿越及符號連結。套件翻譯的應用程式覆寫也從此語言目錄載入。
