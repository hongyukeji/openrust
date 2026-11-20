# 在地化

## 設定語言

透過 `APP_LOCALE` 設定應用程式的預設語言，透過 `APP_FALLBACK_LOCALE` 設定群組翻譯的備援語言。使用 `Lang::withLocale` 為一次請求或工作操作指定語言，操作結束後恢復先前的語言。

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

資料表結構沿用 Spatie Laravel Translation Loader：每個群組與鍵儲存一列，`text` 是語言到字串的 JSON 物件，例如 `{"en":"Hello :name","fr":"Bonjour :name"}`。語言鍵使用標準識別碼（`en_US`、`zh_CN`）。缺少或為 null 的語言保留檔案值；翻譯值必須為字串，空字串也是有效覆寫。群組 `*` 儲存字面鍵並保留句點，其他群組展開 `nested.title`、`items.0` 等點分鍵。套件命名空間與 Spatie 一樣僅使用檔案來源。重複鍵、父子鍵衝突、無效 JSON、鍵中空段及超過 128 層的鍵會使載入失敗。

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
        if locales.first().is_some_and(|locale| locale == "fr_CA") {
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

應用設定、語言檔案與文件路徑統一使用底線標識。語言小寫，地區大寫，文字代碼首字母大寫。例如 `en`、`en_GB`、`fr_CA`、`es_419`、`sr_Latn_RS`、`zh_CN`、`zh_TW`。輸入接受連字號、底線與不同大小寫。`zh-Hans` 解析為 `zh_CN`，`zh-Hant` 解析為 `zh_TW`。明確指定的 `zh_HK`、`zh_Hant_HK` 等繼續讀取各自的語言檔案。HTML 使用 `zh-CN`、`zh-TW` 等語言標籤。既有語言檔案需改為正規化後的名稱。

## 覆寫套件語言檔案 {#overriding-package-language-files}

要覆寫 `shop::cart.summary.heading` 等套件翻譯，請在 `lang/vendor/shop/en/cart.json` 中定義同一個鍵。目錄與檔名沿用原翻譯的命名空間、語言及群組。應用程式只覆寫自己定義的項目，其餘項目仍由套件提供。

## Artisan 翻譯 {#artisan-translations}

Artisan 使用 `APP_LOCALE` 顯示命令清單標題、說明和引數錯誤。內建英文、簡體中文和繁體中文。應用程式可在 `lang/<locale>/console.json` 覆寫文字。翻譯先查目前語言，再查 `APP_FALLBACK_LOCALE`，最後使用內建文字。沒有翻譯的命令說明沿用註冊時的文字。

```sh
APP_LOCALE=en ./artisan help list
APP_LOCALE=zh_CN ./artisan list make
APP_LOCALE=zh_Hant ./artisan help list
```

可在 `console` 群組中使用 `options`、`commands.list.description` 和 `commands.list.arguments.namespace` 等鍵調整說明。請參閱 [Artisan 輸出格式](./artisan.md#command-help-and-output-formats)。

正式環境確認提示和取消訊息使用同一翻譯群組。可在 `console.json` 中覆寫 `confirm.production` 或 `confirm.cancelled`。輸入 `y` 或 `yes` 確認，直接按 Enter 取消。
