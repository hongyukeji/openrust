# 驗證

## 手動建立驗證器

用 `Validator::make` 建立驗證器，再呼叫 `validate().await`。回傳值包含通過驗證的欄位。用點路徑和 `*` 選擇巢狀元素。用 `nullable` 允許明確的 null，用 `sometimes` 在欄位存在時才執行規則。

```rust
use openrust::{
    contracts::validation::Validator as _,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Validator,
    validation::{Data, RuleSet},
};

fn store(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = Data::new(request.input(None).await?.unwrap_or_default())?;
        let rules = RuleSet::parse([
            ("name", "required|string|min:2"),
            ("users.*.email", "required|string"),
        ])?;
        let input = Validator::make(data, rules)?.validate().await?;
        Response::json(StatusCode::OK, input.into_value())
    })
}
```

`email` 與 `email:rfc` 接受僅有警告的 RFC 地址。`strict` 拒絕 RFC 解析警告；`filter`、`filter_unicode` 使用原生式過濾，後者允許地址本地部分中的 Unicode 字母與數字。`spoof` 檢查 Unicode 高度限制腳本組合。`dns` 檢查郵件域 DNS，包含 MX 拒收記錄與地址記錄回退，不檢查信箱是否存在。逗號組合表示所有檢查都須通過。單獨使用 DNS 或 spoof 不會額外加入 RFC 語法校驗。未知模式字串按 RFC 回退。

## 驗證傳入請求 {#validating-incoming-requests}

在處理器中直接呼叫 `request.validate(rules).await?`。它使用應用程式驗證器和目前請求，只傳回通過驗證的輸入。失敗走正常的 JSON 422 或 Web 重新導向／Session 錯誤管線。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::RuleSet,
};

fn store(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::parse([("title", "required|string|max:255")])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

用 `request.validate_with_bag("post", rules).await?` 選擇具名錯誤包。Request 門面轉發至同一實例方法。在驗證中介軟體中，Precognition 會依 `Precognition-Validate-Only` 篩選欄位，並在預驗證成功時提前傳回，後續業務邏輯不會執行。一般路由動作不參與預驗證執行；需要預驗證時使用 Form Request 或驗證中介軟體。

## 資料庫驗證規則 {#database-validation-rules}

用 `exists:table,column` 要求存在符合條件的資料列，用 `unique:table,column` 拒絕已有的值。省略欄位名稱時使用欄位名，在資料表名稱前加 `connection.` 可指定具名連線。兩條規則都使用寫入連線。陣列形式的 `exists` 用一次查詢檢查所有不同的值是否存在。

字串規則的參數值含逗號時，用雙引號包住該值，如 `exists:directory.references,reference,namespace,"team,west"`。用於排除資料列的文字 ID 如 `001` 會保留前導零。陣列存在性驗證按純量字串形式統計不同輸入。

```rust
use openrust::validation::{Rule, RuleSet, ValidationResult};

fn account_rules() -> ValidationResult<RuleSet> {
    RuleSet::parse([
        ("account_id", "required|integer|exists:directory.accounts"),
        ("ref", r#"exists:directory.references,reference,namespace,"team,west""#),
    ])?
        .field("email", [
            Rule::unique("directory.accounts", Some("email"))
                .without_trashed(None)
                .into(),
        ])
}
```

流暢介面支援附加查詢條件和軟刪除篩選。更新資料列時，用 `ignore` 傳入可信的識別碼，或將應用程式已解析並授權的模型傳給 `ignore_model`。並行寫入的唯一性由資料庫唯一約束保證。

`r#where` 接收欄位或 `DatabaseWhere::callback`，以及單一或陣列型別化綁定值。陣列轉為 `where_in`；`where_not` 的陣列轉為 `where_not_in`，包含空陣列。回呼或空值約束明確傳入 `Value::Null`：Rust 無法多載方法參數數量或省略一般參數。回呼共用原巢狀查詢條件，明確空值佔位不增加 SQL 條件。使用範例前須設定名為 `directory` 的資料庫連線。

```rust
use openrust::{database::Value, validation::{Rule, RuleSet, ValidationResult, rules::DatabaseWhere}};
fn directory_rules() -> ValidationResult<RuleSet> {
    RuleSet::new()
        .field("id", [Rule::exists("directory.accounts", Some("account_id"))
            .r#where(DatabaseWhere::callback(|query| query.r#where("tenant", "=", 7).or_where("tenant", "=", 8)), Value::Null)
            .where_null("deleted_at").into()])?
        .field("email", [Rule::unique("directory.accounts", Some("email"))
            .r#where("tenant", [7, 8]).where_null("deleted_at").into()])
}
```

## 表單請求驗證 {#form-request-validation}

### 建立表單請求 {#creating-form-requests}

```sh
./artisan make:request StoreArticleRequest
./artisan make:request Admin/StoreArticleRequest
```

命令建立 `app/http/requests/store_article_request.rs`，巢狀名稱寫入 `admin/` 子模組。產生型別實作 `FormRequest`、衍生 `Default`，並提供 `authorize` 和 `rules`。預設拒絕授權，使用前須明確撰寫授權邏輯；規則初始為空，由應用程式宣告欄位。

在 `app/http/mod.rs` 宣告 `pub(crate) mod requests;`，並在 `app/http/requests/mod.rs` 宣告產生檔案的模組，例如 `pub(crate) mod store_article_request;`。Rust 需要編譯期模組宣告；產生器保留既有模組檔案。之後以 `ValidatedFormRequest<StoreArticleRequest>` 作為動作的型別參數。

`-f`／`--force` 覆寫既有檔案；不加選項重複產生會報告衝突、保留內容並結束為 0，與通用產生器契約一致；無效名稱或寫入錯誤結束為 1。`./artisan stub:publish` 發布 `stubs/request.rs.stub`，修改後用於後續產生。巢狀名稱接受 `/` 或 `\`，亦支援 `App/Http/Requests/StoreArticleRequest`。

將 `ValidatedFormRequest<F>` 宣告為 action 的型別參數。路由在呼叫 action 前建立表單請求，依序準備輸入、授權、驗證並呼叫 `passed_validation`。授權拒絕回傳 HTTP 403。

```rust
use openrust::{
    foundation::http::{FormRequest, ValidatedFormRequest},
    http::{HttpError, Response, StatusCode},
    routing::RoutingError,
    support::facades::Route,
    validation::{RuleSet, ValidationResult},
};

#[derive(Default)]
struct StoreUser;

impl FormRequest for StoreUser {
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([("name", "required|string")])
    }
}

async fn store(form: ValidatedFormRequest<StoreUser>) -> Result<Response, HttpError> {
    Response::json(StatusCode::OK, form.validated().all())
}

fn register() -> Result<(), RoutingError> {
    Route::post("/users", store)
}
```

表單請求實作 `Default` 以便建立。需要建構依賴時，可在容器中為表單型別註冊暫時繫結。一般控制器方法用 `ControllerDispatcher::with_form_request`，資源控制器用 `ResourceController::action_with_form_request`。兩種控制器皆可接收原始請求及其路由參數。需要明確驗證時呼叫 `validate_resolved`。

## 檔案驗證 {#validating-files}

手動驗證 multipart 時使用 `Data::from_request`。FormRequest 會自動包含上傳檔案。透過 `validated.file` 讀取已驗證的檔案，支援 `attachments.0` 等點路徑。JSON 檢視中的檔案位置表示為 null。

`mimes` 和 `mimetypes` 從檔案內容推斷類型，`extensions` 檢查用戶端檔名的副檔名。內容和副檔名都有要求時可同時使用。用 `image:allow_svg` 或 `File::image(true)` 接受 SVG。`size`、`min`、`max` 和 `between` 以 KiB 計量檔案大小，每 KiB 為 1,024 位元組。

內容偵測會將 JSON 物件和陣列辨識為 `application/json`，將 CSV 內容辨識為純文字，並將空上傳辨識為 `application/x-empty`。需要檢查用戶端檔名的 CSV 副檔名時，使用 `extensions:csv`。

用 `dimensions` 限制寬、高或長寬比。比例可寫成小數或 `16/9` 這樣的分數。圖片規則允許的 SVG 會略過點陣圖尺寸限制。用 `encoding:UTF-8` 或 `File::default().encoding("UTF-8")` 檢查檔案位元組。可使用 UTF-8、ASCII、UTF-16、UTF-32、SJIS、CP932 和 EUC-CN 等受支援的編碼名稱。Windows Shift-JIS 擴充使用 CP932 或 SJIS-win。

單位元組檔案可用 `encoding:CP850` 或 `File::default().encoding("ArmSCII-8")` 驗證。CP850 接受所有位元組。ArmSCII-8 以及存在未定義位置的 Windows 和 ISO 編碼依 PHP mbstring 的字表拒絕這些位元組。編碼驗證保留上傳檔案的原始位元組。

驗證東亞雙位元組檔案時，請選擇檔案實際使用的編碼。`encoding:BIG-5` 和 `encoding:EUC-KR` 檢查標準位元組範圍，對應的 Windows 擴充可用 `File::default().encoding("CP950")` 或 `File::default().encoding("UHC")`。各編碼依 PHP mbstring 的字表拒絕不完整位元組對與未分配位置。

`encoding:EUC-CN` 驗證該編碼已分配的雙位元組位置，`encoding:CP936` 驗證 Windows 的位元組範圍。`encoding:GB18030` 和 `File::default().encoding("GB18030-2022")` 都遵循 PHP mbstring，接受相同的雙位元組及四位元組序列。

日文 EUC 檔案可依實際字表使用 `encoding:EUC-JP` 或 `encoding:eucJP-win`。Fluent 檔案規則也支援 `File::default().encoding("CP51932")` 和 `File::default().encoding("EUC-JP-2004")`。各變體已分配的雙位元組及三位元組位置依 PHP mbstring 驗證。

Shift-JIS 檔案驗證也支援 `SJIS-mac`、`SJIS-2004`、`SJIS-Mobile#DOCOMO`、`SJIS-Mobile#KDDI` 和 `SJIS-Mobile#SOFTBANK`。依實際編碼選擇規則，例如 `encoding:SJIS-mac` 或 `File::default().encoding("SJIS-Mobile#SOFTBANK")`。SoftBank 變體也會驗證舊式表情跳脫序列。

`encoding:ISO-2022-JP` 和 `encoding:JIS` 驗證日文跳脫序列，並要求輸入結束前切回 ASCII。檔案規則也支援 `ISO-2022-JP-MS`、`ISO-2022-JP-2004`、`ISO-2022-JP-MOBILE#KDDI` 及 `CP50220` 至 `CP50222`，各變體依 PHP mbstring 驗證字表和結束狀態。

HZ、EUC-TW 和 ISO-2022-KR 檔案可用 `encoding:HZ`、`File::default().encoding("EUC-TW")` 或 `encoding:ISO-2022-KR` 驗證。HZ 和 ISO-2022-KR 的跳脫與移位依循 PHP mbstring，EUC-TW 檢查 CNS 11643 第 1、2、14 平面的已分配位置。

四種行動版 UTF-8 按標準 UTF-8 驗證位元組。可用 `encoding:UTF-8-Mobile#DOCOMO` 或 `File::default().encoding("UTF-8-Mobile#SOFTBANK")`，另外兩個名稱為 `UTF-8-Mobile#KDDI-A` 和 `UTF-8-Mobile#KDDI-B`。驗證保留原始位元組中的私用區字元。

TIFF 和 ICO 上傳可以使用 `file` 和 `dimensions:width=100,height=100` 一起驗證。TIFF 從第一個影像目錄取得尺寸，ICO 從位元深度最高的項目取得尺寸，位元深度相同時取最後一個。尺寸驗證只讀取中繼資料。`image` 規則排除 TIFF 和 ICO。

PSD 和 JPEG 2000 上傳也可以使用這組規則。JP2 從第一個根層級碼流讀取尺寸，裸 JPEG 2000 碼流從 SIZ 標記讀取尺寸。`image` 規則也排除這兩種格式。

相同的中繼資料驗證也支援 IFF 點陣圖、WBMP、XBM，以及未壓縮或使用 zlib 壓縮的 SWF。SWF 的尺寸來自舞台矩形。

```rust
use openrust::{
    contracts::validation::Validator as _,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Validator,
    validation::{Data, Rule, RuleSet, rules::File},
};

fn upload(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = Data::from_request(request).await?;
        let rules = RuleSet::new().field("image", [
            Rule::required(),
            File::image(false)
                .dimensions(Rule::dimensions().min_width(256).ratio("1/1"))
                .max("2mb")
                .into(),
        ])?;
        let validated = Validator::make(data, rules)?.validate().await?;
        let size = validated.file("image").map(|file| file.get_size());
        Response::json(StatusCode::OK, serde_json::json!({ "size": size }))
    })
}
```

### 傳輸編碼 {#transfer-encoding}

用 `base64` 驗證標準格式的 Base64 字串。上傳檔案的 `encoding:BASE64` 按 PHP mbstring 檢查字元表，允許 `=`、空格、定位字元及 CR/LF 出現在任意位置。`Quoted-Printable`、`HTML-ENTITIES` 和 `UUENCODE` 的 mbstring 相容檢查接受任意位元組。

```rust
use openrust::validation::{Rule, RuleSet, ValidationResult, rules::File};

fn transfer_rules() -> ValidationResult<RuleSet> {
    RuleSet::parse([("value", "required|base64")])?
        .field("upload", [Rule::required(), File::default().encoding("BASE64").into()])
}
```

### UTF-7 與 UTF7-IMAP {#utf7-encoding}

用 `UTF-7` 檢查標準 UTF-7，用 `UTF7-IMAP` 檢查 IMAP 的變體。兩者都會檢查移位段中的 UTF-16 碼元，拒絕錯誤的代理對和非零填充位元。UTF7-IMAP 的每個移位段都須以 `-` 結束。

```rust
use openrust::validation::{RuleSet, ValidationResult};

fn mailbox_rules() -> ValidationResult<RuleSet> {
    RuleSet::parse([
        ("subject", "required|string|encoding:UTF-7"),
        ("folder", "required|string|encoding:UTF7-IMAP"),
    ])
}
```

## 驗證錯誤回應格式

請求預期 JSON 時，驗證失敗回傳 HTTP 422，本文包含 `message` 和依欄位索引的 `errors`。展開後的欄位用點路徑表示，例如 `users.0.email`。可在目前語言的 `lang/<locale>/validation.json` 中覆寫規則訊息和欄位名稱。

## 顯示驗證錯誤

啟用工作階段的 Web 路由驗證失敗時，會重新導向表單，並將錯誤及送出的輸入快閃至下一次請求。Blade 檢視透過 `$errors` 取得目前請求的錯誤包。可用 `request.old(Some("profile.name"))` 回填欄位。

重新導向依序使用明確設定的目標、同源 Referer、工作階段記錄的上一頁。上一頁的查詢字串會保留。沒有可用目標時會重新導向至 `/`。

### 命名錯誤包

在驗證器上呼叫 `validate_with_bag("profile")`，或覆寫 `FormRequest::error_bag`，即可指定錯誤包。在 Blade 中，`$errors->profile->name` 選取該包內 name 欄位的訊息。

### 排除敏感輸入

密碼欄位會從舊輸入排除。可在 `with_exceptions` 中用 `dont_flash` 註冊額外路徑。

```rust
use openrust::foundation::configuration::Exceptions;

fn configure(exceptions: &mut Exceptions) {
    exceptions.dont_flash(["profile.secret"]);
}
```
## 標準規則與驗證鉤子

規則解析支援 `numeric`、`date`、`in:a,b`、`regex:/pattern/u`、`required_if:field,value`、`exclude_if:field,value`、`accepted`、`url`、`uuid`、`array:key1,key2`。`array:key1,key2` 拒絕清單外的鍵；`in` 與 `array` 組合時逐項檢查陣列。被排除的欄位及上傳檔案不會出現在已驗證資料中，規則按宣告順序執行。

預設 `integer` 遵循 PHP 整數驗證，包含部分整數值浮點數和 `true`；`integer:strict` 只接受 JSON 整數，`boolean:strict` 只接受 JSON 布林值。數值大小規則支援負數和小數閾值，使用十進位比較。科學記號指數超出 -1000 至 1000 時傳回執行錯誤。

尺寸規則只在宣告 `numeric`、`integer` 或 `decimal` 時比較數值；其餘字串按字元數、陣列按項數、上傳檔案按 KB 計算。錯誤訊息類型由宣告的規則決定，優先順序為數值、array/list、檔案、字串。布林值和 null 使用純量文字長度；浮點數在比較前沿用 14 位有效數字的純量轉換。驗證結果保留原輸入類型。

浮點數先依 PHP 預設的 14 位有效數字轉為文字，例如浮點值 `1e13` 通過、`1e14` 不通過，而整數值 `100000000000000` 通過。`regex` 只接受字串或數值；布林值與未宣告 `nullable` 的 `null` 不通過。

正則包含 `|` 時，使用 `RuleSet::field` 的規則陣列並透過 `Rule::parse` 解析，避免被當成規則分隔符號。支援 PCRE2 修飾符 `i m s x u A U J n D S X r`；`D` 使 `$` 僅比對輸入末尾，多行模式下遵循 PCRE2 原生語義。`S` 接受但不額外 study，`X` 沿用 PCRE2 既有的嚴格跳脫檢查，`r` 限制 ASCII 與非 ASCII 字元間的大小寫折疊。忽略模式開頭的 ASCII 空白和修飾符間的空格／換行；未知修飾符（含修飾符間的定位字元）傳回 `InvalidRule`。

`Validator::after` 在規則檢查後執行，可追加錯誤。`sometimes` 按條件追加規則，萬用字元回呼收到所在父項。`stop_on_first_failure(true)` 在不同欄位間停止檢查，`bail` 只停止目前欄位。`safe()` 必須在驗證成功後呼叫，只公開已驗證資料。

FormRequest 順序為 `prepare_for_validation`、`authorize`、`with_validator`、驗證和 `after`、`passed_validation`。授權或驗證失敗不執行業務方法。攜帶 `Precognition-Validate-Only` 的預驗證成功後不呼叫 `passed_validation`；未帶該標頭時可能先呼叫，再短路業務方法。驗證器按請求隔離。

## 使用驗證後的資料 {#working-with-validated-input}

`Validator::safe()` 在驗證成功後傳回 `support::ValidatedInput`；`ValidatedFormRequest::safe()` 借用已驗證的結果。`only`、`except` 按具體巢狀路徑篩選，`merge` 傳回新結果，`collect` 傳回既有的 `Collection<RequestValue>`。原資料保持不變，借用和擁有式迭代均提供頂層鍵值對。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::RuleSet,
};
use serde_json::json;

fn store_safe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let input = request.validate(RuleSet::parse([
            ("name", "required|string"),
            ("email", "required|email"),
        ])?).await?;
        let selected = input.only(&["name", "email"])?;
        let without_email = input.except(&["email"])?;
        let merged = input.merge(json!({"role": "reader"}))?;
        let collection = input.collect();
        Response::json(StatusCode::CREATED, json!({
            "selected": selected,
            "without_email": without_email,
            "merged": merged,
            "collection": collection,
            "name": input["name"],
        }))
    })
}
```

子集保留為型別化的 `ValidatedInput`，而非無型別陣列，以保留篩選和合併後的上傳檔案控制代碼。透過 `file()` 或型別化集合讀取檔案；JSON 投影將檔案表示為 `null`，不公開本機路徑。`to_array()` 和索引讀取此 JSON 投影。`contracts::support::{ValidatedData, Arrayable, ArrayAccess}` 提供對應泛型契約。手動建構 `ValidatedInput::new` 不執行驗證；不可信輸入應透過請求或驗證器處理。

## 列舉與值清單 {#rule-enum}

`Rule::r#enum::<E>()` 和 `rules::Enum<E>` 透過既有 `BackedEnum` 映射驗證應用列舉。`only`、`except` 接收列舉成員；非空 `only` 清單優先。`when`、`unless`、`when_else` 按條件設定同一個規則；Rust 沒有可選函式參數，`when_else` 明確傳入兩個分支。JSON 保持後備純量，不轉換成 Rust 列舉物件。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::BackedEnum,
    validation::{Rule, RuleSet},
};

#[derive(PartialEq)]
enum ServerStatus { Pending, Active }

impl BackedEnum for ServerStatus {
    type Backing = String;
    fn try_from_backing(value: String) -> Option<Self> {
        match value.as_str() {
            "pending" => Some(Self::Pending),
            "active" => Some(Self::Active),
            _ => None,
        }
    }
}

fn store_status(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rule = Rule::r#enum::<ServerStatus>()
            .when(true, |rule| rule.only([ServerStatus::Active]));
        let data = request.validate(RuleSet::new()
            .field("status", [Rule::required(), rule.into()])?).await?;
        Response::json(StatusCode::CREATED, data)
    })
}

fn store_zone(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = request.validate(RuleSet::new()
            .field("zone", [Rule::required(), Rule::r#in(["first-zone", "second-zone"]).into()])?
            .field("topping", [Rule::not_in(["sprinkles", "cherries"]).into()])?).await?;
        Response::json(StatusCode::CREATED, data)
    })
}
```

`Rule::r#in`、`rules::In`、`Rule::not_in`、`rules::NotIn` 保留每個清單成員邊界，包括逗號和引號。成員接收 `Into<serde_json::Value>`，重用既有純量轉換；清單中的應用列舉明確映射為後備值。`r#` 是 Rust 關鍵字所需語法，不改變方法名稱。型別化規則與一般規則放入同一個陣列時使用 `.into()`。

## 鏈式規則建構器 {#fluent-rule-builders}

`rules::{StringRule, ArrayRule, Numeric, Date}` 編譯到既有規則管線，共用執行順序、錯誤訊息、萬用字元依賴和驗證投影。匯入 `RuleBuilder` 後可鏈式設定字串和陣列，同時保留既有 `Rule::min`、`Rule::max` 建構器；Rust 不允許靜態建構器與固有實例方法同名。型別化建構器也可直接透過 `.into()` 傳入。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{Rule, RuleBuilder, RuleSet},
};

fn store_policy(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("name", [Rule::required(), Rule::string().alpha_dash(true).min(2).max(20)])?
            .field("amount", [Rule::required(), Rule::numeric(false).min(1).max(100).decimal(2, Some(2)).into()])?
            .field("starts_on", [Rule::required(), Rule::date().format("Y-m-d").after("2023-12-31").into()])?
            .field("website", [Rule::required(), Rule::url().protocols(["https"]).into()])?
            .field("id", [Rule::required(), Rule::uuid().version(4).into()])?
            .field("address", [Rule::required(), Rule::ip().version(4)?.into()])?
            .field("zone", [Rule::required(), Rule::timezone(["per_country", "US"]).into()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}

fn store_typed(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use openrust::validation::rules::{ArrayRule, Numeric, StringRule};
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("name", [StringRule::new().min(2).max(20).into()])?
            .field("profile", [ArrayRule::new(["name", "email"]).min(1).into()])?
            .field("count", [Numeric::new(true).integer(true).min(1).into()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}
fn store_condition(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use openrust::validation::RuleCondition;
    Box::pin(async move {
        let required = RuleCondition::callback(|data| Ok(data.as_value()["requires_title"] == true));
        let rules = RuleSet::new()
            .field("title", [Rule::required_if(required).into()])?
            .field("metadata", [Rule::when(true, vec![Rule::array()], Vec::new()).into()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}

```

`Numeric` 支援小數位、位數、比較、倍數和嚴格整數檢查；小數邊界明確使用 `BigDecimal`。`Date` 支援格式、前後／相等、開閉區間及 today／now 快捷方法，重用目前應用時鐘和時區。日期參數接收字串或 `support::Date`，日期物件在新增約束時按當時格式轉換。建構器支援 `when`、`unless`、`when_else`。

`rules::{Url, Uuid, IpAddress, Distinct, Timezone}` 實作 `contracts::validation::ParameterizedRule`，自訂實作透過 `Rule::parameterized` 接入。`Rule::distinct().strict().ignore_case()` 設定元素比較。IP 版本接收 4 或 6，其他值傳回錯誤。URL 協定、UUID 版本（含 `nil`／`max`）及 `numeric:strict` 也支援字串規則。`integer_strict()`、`boolean_strict()` 在保留原零參數建構器的同時提供嚴格版本。

`Rule::when`、`Rule::unless` 在萬用字元展開前編譯選擇完整規則清單；`RequiredIf`、`RequiredUnless`、`ExcludeIf`、`ExcludeUnless`、`ProhibitedIf`、`ProhibitedUnless` 等布林／回呼物件按展開後的屬性編譯。`RuleCondition::lazy` 接收零參數回呼，`callback` 接收既有型別化 `Data`。條件在 `set_data`、`set_rules` 後重新執行，不在每次驗證時重複執行。

### 具名規則工廠 {#named-rule-factories}

具名工廠與字串規則使用同一個驗證器。`rules::{AcceptedIf, DeclinedIf, MissingIf, MissingUnless, PresentIf, PresentUnless}` 接收欄位及單一或清單比較值。`RequiredWith`、`RequiredWithout`、`PresentWith` 等欄位清單規則明確接收陣列或迭代器；Rust 方法沒有可變參數。參數化規則保留逗號和引號邊界。`ArrayKeys`、`Contains`、`DoesntContain`、`Regex`、`NotRegex` 也能作為實際型別透過 `.into()` 使用。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{Rule, RuleBuilder, RuleSet},
};
fn store_factories(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("username", [Rule::required(), Rule::regex("/^[a-z]+$/").into(), Rule::confirmed().custom_field("confirmation")])?
            .field("terms", [Rule::accepted_if("account_type", ["personal", "business"]).into()])?
            .field("nickname", [Rule::exclude_with("username").into()])?
            .field("email", [Rule::required_without(["phone", "mobile"]).into()])?
            .field("settings", [Rule::present_if("account_type", "business").into()])?
            .field("profile", [Rule::array_keys(["name", "email"]).into()])?
            .field("roles", [Rule::contains(["reader"]).into(), Rule::doesnt_contain(["admin"]).into()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}
```

`Confirmed::new().custom_field("confirmation")` 與 `Rule::confirmed().custom_field("confirmation")` 選擇自訂確認欄位。`CurrentPassword::new().guard("web")` 或 `Rule::current_password(None).guard("web")` 選擇具名認證 guard，取代預設選擇；需要實際認證 guard 及已設定的雜湊器。`Rule::file()` 傳回 `File`；`Rule::image_file(false)` 傳回 `ImageFile`。ImageFile 共用檔案大小、副檔名、編碼與尺寸限制；允許 SVG 時明確傳入 `true`。

## 接收資料與驗證器的規則 {#accessing-additional-data}

實作 `DataAwareRule::set_data` 接收目前型別化 `Data`，包括上傳中繼資料；實作 `ValidatorAwareRule::set_validator` 接收同一個驗證器的控制代碼。分別透過 `Rule::data_aware`、`Rule::validator_aware` 接入，兩種 trait 都實作時使用 `Rule::aware`。穩定 Rust 無法動態發現 `dyn ValidationRule` 上的可選 trait，因此由適配器明確選擇 setter；`Rule::custom` 保持一般無狀態入口。

```rust
use openrust::{
    contracts::validation::{DataAwareRule, ValidationRule, ValidatorAwareRule},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{
        Data, Rule, RuleContext, RuleFailure, RuleSet, ValidationError,
        ValidationFuture, ValidationResult, ValidatorHandle,
    },
};
use serde_json::{Value, json};

#[derive(Default)]
struct MatchesConfirmation { expected: Option<Value> }
impl DataAwareRule for MatchesConfirmation {
    fn set_data(&mut self, data: &Data) -> ValidationResult<()> {
        // 只保留当前规则需要的字段，不复制整份输入。
        self.expected = data.as_value().get("confirmation").cloned();
        Ok(())
    }
}
impl ValidationRule for MatchesConfirmation {
    fn validate<'a>(
        &'a self, _: &'a RuleContext<'_>, _: &'a str, value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            Ok(if value == self.expected.as_ref() {
                Vec::new()
            } else {
                vec![RuleFailure::new("validation.confirmed")]
            })
        })
    }
}

#[derive(Default)]
struct NeedsReview { validator: Option<ValidatorHandle> }
impl ValidatorAwareRule for NeedsReview {
    fn set_validator(&mut self, validator: ValidatorHandle) -> ValidationResult<()> {
        self.validator = Some(validator);
        Ok(())
    }
}
impl ValidationRule for NeedsReview {
    fn validate<'a>(
        &'a self, _: &'a RuleContext<'_>, _: &'a str, _: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            let validator = self.validator.as_ref().ok_or(ValidationError::NotValidated)?;
            validator.with(|validator| {
                if validator.get_value("review")? == Some(&json!(true)) {
                    validator.errors_mut().add("review", "需要人工复核。");
                }
                Ok(())
            })?;
            Ok(Vec::new())
        })
    }
}

fn store_aware(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("value", [Rule::required(), Rule::data_aware(MatchesConfirmation::default())])?
            .field("status", [Rule::required(), Rule::validator_aware(NeedsReview::default())])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}
```

同時實作兩個 trait 時，先執行 `set_data`，再執行 `set_validator`，最後執行 `validate`。一般缺失／空字串／nullable 跳過也跳過 setter，隱式規則沿用既有排程。setter 使用 `&mut self` 和 `ValidationResult<()>`，失敗進入原異常管線。只有同一狀態感知規則物件的 setter 與驗證期間循序借用，一般規則不加該鎖。

`ValidatorHandle::with` 安全讀寫原驗證器的值、訊息和錯誤包，`with_async` 支援非同步借用；巢狀衝突借用傳回錯誤，不無限等待。控制代碼僅在目前規則任務有效，完成或取消後失效，不繼承到新建子任務；同一驗證器執行期間禁止遞迴驗證。`RuleContext` 和已提供的 `Data` 是不可變快照，修改後應透過控制代碼讀取即時狀態。後續規則讀取更新後的值，取消時將原驗證器還給呼叫端。

`RuleContext::file()` 提供傳給規則的原始值所對應的上傳檔案，即使 setter 已取代該欄位仍可讀取。`context.data` 是 setter 執行後的快照，透過控制代碼讀取驗證器即時狀態；規範跳脫路徑保留在內部，規則參數接收可顯示的屬性名稱。

## 訊息包 {#message-bags}

使用 `MessageBag` 收集有序欄位錯誤，`ViewErrorBag` 管理具名包。`add` 去重，`merge(&other)` 保留重複。`first(key, format)` 在沒有訊息時傳回空字串。`get(key, format)` 傳回 `MessageLookup::Exact` 或保留欄位名的萬用字元映射；僅在確實需要扁平訊息時呼叫 `into_messages()`。

```rust
use openrust::support::{MessageBag, ViewErrorBag};

let mut messages = MessageBag::new([("email", ["Invalid email"])]);
messages.add_if(true, "name", "Required");
assert_eq!(messages.first(Some("email"), None), "Invalid email");
assert!(messages.has_any(["email", "phone"]));
messages.set_format(":key: :message");
assert_eq!(messages.first(Some("name"), None), "name: Required");
let mut errors = ViewErrorBag::new();
errors.put("profile", messages);
assert_eq!(errors.get_bag("profile").count(), 2);
assert_eq!(errors.count(), 0);
```

具名包計數及轉發的唯讀方法只針對 `default` 包。讀取缺失包不會登記它；用 `get_bag_mut` 修改已有包，或用 `put` 新增包。JSON 儲存原始訊息，不受顯示格式影響；反序列化恢復預設格式。訊息列表使用連續的 Rust 索引，JSON 選項遵循 Serde，不提供 PHP 陣列索引、JSON 位元旗標或執行期動態方法分派。


## 純量與數字位數規則

使用 `ascii`、`alpha`、`alpha_dash` 和 `alpha_num` 限制字元。三個 alpha 規則接受 `ascii` 參數，預設支援 Unicode 字母和組合標記，後兩者還接受數字。`list` 要求從零開始的連續整數鍵。位址格式使用 `ip`、`ipv4`、`ipv6` 和 `mac_address`；`hex_color` 接受以 `#` 開頭的 3、4、6 或 8 位十六進位顏色。

`json` 驗證純量中的 JSON 文字，剖析器接受少於 128 層的容器巢狀結構。`digits:n`、`digits_between:min,max`、`min_digits:n` 和 `max_digits:n` 按純量轉換後的 ASCII 數字計數，正負號及小數點不能通過數字位數規則。`decimal:n` 或 `decimal:min,max` 檢查小數位數，不接受科學記號。缺少必要參數或邊界不是數字時傳回 `InvalidRule`。這些規則沿用缺失欄位、空白字串和 `nullable` 行為；必須提供值時同時使用 `required`。

使用 `lowercase`、`uppercase` 檢查 Unicode 大小寫，不修改輸入。`starts_with:a,b`、`ends_with:a,b`、`doesnt_start_with:a,b`、`doesnt_end_with:a,b` 接受字串和數字，以任一非空參數比對。`not_regex:/pattern/flags` 拒絕符合的值，模式語法與 `regex` 相同；非法模式傳回 `InvalidRule`。模式含管道字元時，透過 `RuleSet` 傳入單獨的 `Rule::parse` 結果。

## 欄位存在與條件規則

`missing` 要求欄位缺失，明確傳入 `null` 也算存在。`filled` 允許缺失，但已提供的欄位不能是空值。`declined` 接受 `no`、`off`、`0`、整數零、`false` 字串或布林假值。`prohibited` 允許缺失和空值，拒絕已填寫的值。`same:other` 使用嚴格相等，`different:other,third` 對每個已提供的關聯欄位檢查嚴格不等。

條件存在規則包括 `accepted_if`、`declined_if`、`missing_if`、`missing_unless`、`missing_with`、`missing_with_all`、`present_if`、`present_unless`、`present_with`、`present_with_all`。條件必填使用 `required_unless`、`required_with`、`required_with_all`、`required_without`、`required_without_all`、`required_if_accepted`、`required_if_declined`。存在檢查把 null 視為存在；必填檢查還考慮空字串和空陣列。條件參數中的萬用索引按目前欄位展開。

`prohibited_if`、`prohibited_unless`、`prohibited_if_accepted`、`prohibited_if_declined` 和 `prohibits` 按條件拒絕已填寫的欄位。`exclude`、`exclude_unless`、`exclude_with`、`exclude_without` 從驗證結果移除符合的欄位，並移除子欄位錯誤和上傳檔案。排除按宣告順序執行，前面的必填失敗可能提早停止欄位驗證。`nullable` 不跳過隱式的存在規則。

## 陣列成員與格式規則

`array_keys:a,b` 限制允許的鍵；`required_array_keys:a,b` 要求全部鍵存在，`in_array_keys:a,b` 要求至少一個鍵存在。`contains:a,b` 對每個指定值使用寬鬆比較；`doesnt_contain:a,b` 以嚴格比較拒絕列出的字串。`not_in:a,b` 反轉 `in`；同時使用 `array` 時，反轉的是整個陣列的檢查結果。

`in_array:pool.*` 對展開的候選欄位使用嚴格相等檢查。`distinct` 在宣告的萬用欄位中拒絕重複值；`strict` 保留純量型別區別，`ignore_case` 對字串使用 Unicode 正則大小寫比對。`date_format:Y-m-d,Y/m/d` 接受任一給定格式，並檢查格式化結果與原輸入是否相等，錯誤訊息顯示第一個格式。`ulid` 檢查 26 個字元及首字元範圍。

`multiple_of:0.1` 檢查精確十進位餘數，除數為零時始終失敗。輸入或除數的科學記號指數超出 -1000 至 1000 時傳回執行錯誤。可透過 `:unexpected` 自訂額外陣列鍵的錯誤說明；`:Unexpected` 只將整段列表的首字元大寫，`:UNEXPECTED` 將全部字母大寫。

## 日期、大小與 DNS

`before:other`、`before_or_equal:other`、`after:other`、`after_or_equal:other` 和 `date_equals:other` 將日期與字面量或另一欄位比較，使用應用程式時區及測試時鐘。未宣告 `date_format` 時按整秒比較，宣告格式時納入微秒。要求輸入本身是有效日期時，同時使用 `date` 或 `date_format`。

`gt:other`、`lt:other`、`gte:other` 和 `lte:other` 比較數值、字串字元數、陣列項數或檔案 KB 數。數值輸入也可與字面量門檻比較；欄位值應同型別，雙方均為數值時可跨數值型別。每條比較規則獨立決定數值分類，不改變後續大小規則。

`timezone` 接受內建 IANA 2025b 資料中的精確識別碼。`timezone:all_with_bc` 包含相容別名，`timezone:asia` 等參數限制地區，`timezone:per_country,CN` 按國家篩選。識別碼區分大小寫，群組與國家參數不區分大小寫。無效群組或國家參數長度傳回 `InvalidRule`。

`active_url` 要求主機存在 DNS A 或 AAAA 紀錄，使用非同步 DNS 查詢，不因作業系統能解析數字 IP 就判為成功。`validator.set_dns_resolver(resolver)` 接受 Hickory 0.25 解析器，可指定名稱伺服器、逾時與重試策略；DNS 查詢失敗即驗證失敗。測試時用 `validator.fake_dns_lookups(true)` 對目前驗證器啟用僅檢查主機名稱的行為，傳入 `false` 恢復實際 DNS 查詢。開關按實例隔離，並行驗證器互不影響。

## 修改驗證器

用 `get_data`、`get_value` 和 `get_rules` 讀取輸入及展開後的規則。`get_rules` 按宣告順序傳回 `(欄位路徑, 規則)`，路徑使用原生跳脫。`set_value` 更新一個具體路徑，拒絕萬用字元。`set_data` 替換輸入，並恢復最近一次 `set_rules` 或工廠建立時的規則。`add_rules` 合併暫時規則；`append_rules` 同時將合併後的宣告作為下次重設的基準。

`set_custom_messages` 合併訊息。`set_attribute_names`、`set_value_names` 替換對應映射；`add_custom_attributes`、`add_custom_values` 按欄位合併。`get_translator`、`set_translator` 讀取或替換錯誤訊息使用的譯器。在驗證執行之外修改後，會捨棄快取的成功結果和錯誤，需要重新執行 `validate().await`，再讀取 `validated()` 或 `safe()`。

`get_exception`、`set_exception` 使用型別化函式 `fn(MessageBag, Option<&Translator>) -> ValidationException`，可自訂實際拒絕回應的狀態、錯誤包及重新導向。固定回傳型別替代執行期例外類別名稱及繼承，每個驗證器獨立保存工廠。

## 註冊驗證擴充

在服務提供者中用 `Validator::extend` 註冊一般規則，`extend_implicit` 註冊缺失或空值也必須執行的規則，`extend_dependent` 註冊參數引用其他欄位的規則。名稱可用 `ProjectCode` 或 `project_code`，宣告統一使用 `project_code`。工廠建立驗證器時複製目前註冊項，後續註冊不會改變已建立的執行個體。

```rust
use openrust::support::facades::Validator;

Validator::extend("upper_code", |_, value, _, _| {
    Ok(value.and_then(serde_json::Value::as_str)
        .is_some_and(|value| value.chars().all(|ch| ch.is_ascii_uppercase())))
}, Some(":attribute 必須由大寫字母組成。"))?;
```

回呼接收欄位名稱、`Option<&Value>`、參數及唯讀驗證器，傳回 `ValidationResult<bool>`。一般規則略過缺失和空字串；`nullable` 略過 null，`sometimes` 仍限制缺失欄位。隱式規則可檢查缺失、空字串及 null。依賴規則將 `items.*.other` 替換為目前項目的具體路徑。執行錯誤透過 `Err` 傳播，不視為一般驗證失敗。非同步規則繼續使用 `Rule::custom` 和 `ValidationRule`。

用 `Validator::replacer` 註冊訊息替換回呼，參數為訊息、欄位、規則名稱、參數列表及唯讀驗證器，傳回最終訊息。先選擇自訂或翻譯訊息並替換欄位名稱，再呼叫替換器；註冊替換器後，由它處理 `:min` 等規則參數。`Factory::resolver` 接收並傳回擁有的驗證器，用於建立時的定制。Rust 使用閉包及明確解析的服務，不支援執行期 `Class@method` 字串或動態驗證器子類別。

單一驗證器可使用 `add_extension`、`add_implicit_extension`、`add_dependent_extension` 和 `add_replacer`；對應複數方法接受 `(String, Arc<回呼>)`。這些修改會清除既有驗證結果，需要再次驗證。

工廠預設 `exclude_unvalidated_array_keys()`：父欄位宣告無參數 `array` 或 `list` 且有子欄位規則時，只傳回經過驗證的子鍵。用 `include_unvalidated_array_keys()` 將之後建立的驗證器切換為包含父欄位全部鍵；呼叫 `exclude_unvalidated_array_keys()` 恢復預設。`array:name,email` 已明確限制允許鍵，不採用此裁剪策略。單一驗證器的 `exclude_unvalidated_array_keys` 布林欄位可獨立調整，`validated()` 和 `safe()` 隨即採用新策略。

## 自訂訊息與欄位顯示名稱 {#custom-validation-messages}

需要可選訊息和欄位顯示名稱時使用 `validate_with(rules, messages, attributes)`；具名錯誤包使用 `validate_with_bag_and_options(bag, rules, messages, attributes)`。Request 門面提供相同入口；原有 `validate` 和 `validate_with_bag` 預設行為不變。Rust 用額外方法表達 PHP 的可選參數。

```rust
use std::collections::BTreeMap;
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::RuleSet,
};

fn store_with_messages(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::parse([("title", "required|string|min:3")])?;
        let messages = BTreeMap::from([("title.required".into(), "Please provide :attribute.".into())]);
        let attributes = BTreeMap::from([("title".into(), "article title".into())]);
        let data = request.validate_with(rules, messages, attributes).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

手動建立驗證器時呼叫 `Validator::make_with(data, rules, messages, attributes)`。Form Request 可覆寫 `messages()` 和 `attributes()`，兩者傳回 `BTreeMap<String, String>`。這些入口復用應用程式驗證工廠，保留 JSON 422、Web 重新導向／錯誤包和 Precognition；訊息只影響本次驗證器，不修改共用翻譯。

## 拒絕未知欄位 {#failing-on-unknown-fields}

在 `FormRequest::fail_on_unknown_fields` 傳回 `Some(true)`，拒絕正文中未宣告規則的欄位。`Some(false)` 明確關閉此表單的策略，預設 `None` 繼承應用程式預設值；應用程式預設關閉。

```rust
use openrust::{
    foundation::http::FormRequest,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{RuleSet, ValidationResult},
};

struct StoreArticle;

impl FormRequest for StoreArticle {
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([("title", "required|string")])
    }

    fn fail_on_unknown_fields(&self) -> Option<bool> {
        Some(true)
    }
}

fn store_strict(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = StoreArticle.validate_resolved(request).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

在 Provider 的 boot 階段，透過 `app.container().make::<FormRequestDefaults>()?` 取得 `foundation::http::FormRequestDefaults`，呼叫 `fail_on_unknown_fields(true)`。這裡以應用程式狀態替代上游行程靜態開關，避免一個應用程式改變另一個應用程式。策略核對正文葉子鍵，支援巢狀、萬用字元宣告和確認欄位；查詢參數、上傳檔案、`_token` 和 `_method` 不在此拒絕範圍。只有 `array` 規則並不代表任意子鍵都獲授權。失敗復用已翻譯的 `validation.prohibited`、JSON 422 或 Web 錯誤包；Precognition 亦在成功回應前執行此檢查。

具名錯誤包亦可使用 `@error` 及其 `@else` 回退分支：

```blade
@error('title', 'article')
    <span>{{ $message }}</span>
@else
    <span>—</span>
@enderror
```

## 產生規則物件 {#creating-custom-rules}

使用不同名稱，分別產生一般規則和隱式規則：

```sh
./artisan make:rule Uppercase
./artisan make:rule RequiredTitle --implicit
```

檔案分別位於 `app/rules/uppercase.rs` 和 `app/rules/required_title.rs`。在 `app/rules/mod.rs` 宣告模組，並在 `app/mod.rs` 匯出 `rules` 模組；匯入產生的型別後交給 `Rule::custom`。支援 `Admin/Uppercase` 等巢狀名稱。`-i` 等同於 `--implicit`；`-f` / `--force` 覆寫既有檔案。不帶 force 的重名操作回報衝突並保留原檔案。

執行 `./artisan stub:publish` 後，可透過 `stubs/rule.rs.stub` 與 `stubs/rule.implicit.rs.stub` 自訂產生範本。兩種範本最初均回傳空失敗清單；隱式標誌本身不代表欄位必填。請在 `validate` 中實作拒絕邏輯，例如下文所示。範本使用模組限定匯入，型別命名為 `RuleContext`、`Box`、`Option` 或 `Result` 也不會與內部名稱衝突。

## 欄位層級閉包 {#using-closures}

直接向 `Rule::custom` 傳入同步閉包，無需應用程式層級註冊。參數為目前規則上下文、已還原跳脫的欄位名稱和 `Option<&Value>`，傳回 `RuleFailure` 清單。執行錯誤進入既有 HTTP 例外管線；規則物件保留既有非同步 `ValidationRule::validate` 契約。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{Rule, RuleContext, RuleFailure, RuleSet, ValidationResult},
};
use serde_json::Value;

fn store_callback(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rule = Rule::custom(|_: &RuleContext<'_>, attribute: &str, value: Option<&Value>| -> ValidationResult<_> {
            Ok(if value.and_then(Value::as_str) == Some("foo") {
                vec![RuleFailure::new(format!("The {attribute} is invalid."))]
            } else {
                Vec::new()
            })
        });
        let rules = RuleSet::new().field("title", [Rule::required(), Rule::string(), rule])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

## 隱式規則物件 {#implicit-rules}

覆寫 `ValidationRule::is_implicit()`，在欄位缺失、為空或 null 時亦執行，包括 nullable 欄位；`sometimes` 仍跳過缺失欄位。旗標本身不會拒絕輸入，是否傳回失敗由規則決定。普通閉包跳過缺失欄位、原始空字串和 nullable null；正常 HTTP 的空字串轉 null 發生在請求驗證前。`RuleContext::data` 借用本驗證器輸入，不向共用規則物件複製狀態。失敗訊息的翻譯鍵使用驗證器目前的翻譯器；本介面不提供ValidatorAware setter。

```rust
use openrust::{
    contracts::validation::ValidationRule,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{Rule, RuleContext, RuleFailure, RuleSet, ValidationFuture, ValidationResult},
};
use serde_json::Value;

struct RequiredTitle;

impl ValidationRule for RequiredTitle {
    fn is_implicit(&self) -> bool { true }

    fn validate<'a>(
        &'a self,
        _context: &'a RuleContext<'_>,
        _attribute: &'a str,
        value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            Ok(if value.is_none_or(|v| v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty())) {
                vec![RuleFailure::new("validation.required")]
            } else {
                Vec::new()
            })
        })
    }
}

fn store_implicit(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new().field("title", [Rule::bail(), Rule::custom(RequiredTitle), Rule::string()])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

## 翻譯規則失敗訊息 {#translating-validation-messages}

呼叫 `RuleFailure::translate`，傳入目前 `RuleContext`、替換參數與可選語言；`None` 使用目前驗證器語言。可使用 `lang/fr/rules.json` 中的 `rules.invalid` 等應用詞條。`translate_choice(context, count, replacements, locale)` 額外透過既有翻譯器選擇複數分支。每條失敗單獨保存選擇，不改變其他失敗或請求語言。缺少詞條保留鍵名，字面訊息仍替換參數；明確翻譯的訊息不被通用規則訊息覆寫。請使用 `RuleFailure::new` 建構失敗，翻譯標誌不進入 HTTP 序列化回應。明確的上下文借用原翻譯器並立即解析訊息，避免隱藏全域狀態和延遲語言選擇。

```rust
use openrust::validation::{RuleContext, RuleFailure};
use std::collections::BTreeMap;

fn localized_failure(context: &RuleContext<'_>) -> RuleFailure {
    RuleFailure::new("rules.invalid").translate(
        context,
        BTreeMap::from([("token".into(), "X".into())]),
        Some("fr"),
    )
}
```

## 任一完整規則組 {#rule-any-of}

`Rule::any_of` 接收多組備選規則。一組中的全部規則通過即可，成功後不再執行後續組；`rules::AnyOf::new` 提供相同建構器。缺失欄位也必須拒絕時，請在外層另加 `required`。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{Rule, RuleSet},
};

fn store_choice(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let choice = Rule::any_of([
            vec![Rule::email()],
            vec![Rule::string(), Rule::min(6)],
        ]);
        let rules = RuleSet::new().field("contact", [Rule::required(), choice.into()])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}

fn store_contact(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let choice = Rule::any_of([
            RuleSet::parse([("email", "required|email")])?,
            RuleSet::parse([("name", "required|string|min:6")])?,
        ]);
        let rules = RuleSet::new().field("contact", [Rule::required(), choice.into()])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

關聯輸入可傳入由 `RuleSet` 構成的備選組，每組驗證各自的欄位映射。子驗證器使用目前應用工廠，保留擴充、資料庫存在性驗證、自訂訊息和欄位顯示名稱；巢狀組內的上傳檔案保留型別化控制代碼。檢查規則組不會用子驗證器篩選後的結果替換外層值；需要拒絕額外欄位時，應宣告允許的陣列鍵。

此規則與其他驗證門面一樣，需要作用中的應用門面範圍。空備選清單拒絕已提供的值；單個空規則組則通過。所有組失敗時，在外層欄位回報 `validation.any_of`，並支援該規則的自訂訊息。

## 編譯每項規則 {#accessing-nested-array-data}

`Rule::for_each` 傳回 `NestedRules`。回呼在建立驗證器時執行，先於欄位驗證，接收目前值和展開後的屬性名稱。傳回 `Vec<Rule>` 表示目前值的規則，或傳回欄位相對於目前值的 `RuleSet`。替換整份資料或規則會重新編譯，重複驗證不會重複執行回呼。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{Rule, RuleSet},
};
use serde_json::Value;

fn store_each(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("companies", [Rule::array()])?
            .field("companies.*.id", [Rule::for_each(|_value, _attribute| {
                Ok(vec![Rule::required(), Rule::integer(), Rule::min(1)])
            }).into()])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}

fn store_bulk(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("items", [Rule::array()])?
            .field("items.*.bulk", [Rule::sometimes(), Rule::parse("boolean:strict")?])?
            .field("items.*.quantity", [Rule::for_each_with(|_value, _attribute, _flat, item| {
                let bulk = item.and_then(|item| item.get("bulk")).and_then(Value::as_bool) == Some(true);
                Ok(vec![Rule::required(), Rule::integer(), Rule::min(if bulk { 10 } else { 1 })])
            }).into()])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}

fn store_nested(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("items", [Rule::array()])?
            .field("items.*", [Rule::for_each(|_value, _attribute| {
                RuleSet::parse([("id", "required|integer"), ("tag", "sometimes|string")])
            }).into()])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

需要完整四個參數時，使用 `Rule::for_each_with` 或 `NestedRules::new`：目前值、屬性名稱、收集後的扁平資料和父項上下文。扁平映射使用規範轉義路徑，字面點與巢狀路徑不會互相覆蓋。自訂編譯器可實作 `contracts::validation::CompilableRules`，並呼叫 `Rule::compile`。傳回清單中的資料庫規則繼續使用既有工廠和存在性驗證器。 需要按檔案資訊編譯時，使用 `Rule::for_each_context` 或 `NestedRules::with_context`：`NestedRuleContext::file()` 讀取原型別化檔案，`data` 保留原始輸入，`flattened` 保留目前收集的映射；其中 `attribute` 是規範轉義路徑。自訂編譯器可透過 `CompilableRules::compile_context` 使用相同上下文。

編譯後的欄位保留萬用字元訊息比對、錯誤位置、Precognition 選擇和 validated 欄位篩選。既有不傳回 Result 的 `set_data` / `set_rules` 會保留編譯錯誤，下次 `get_rules` 或驗證傳回原錯誤種類；修正資料或規則後恢復。空的型別化映射仍序列化為 JSON 物件，不把 PHP 陣列／物件歧義引入既有回應契約。

## 陣列錯誤索引與位置 {#error-message-indexes-and-positions}

`:index` 從零開始，`:position` 從一開始，`:ordinal-position` 輸出 `2nd` 等序數。更深層數字路徑使用 `:second-index`、`:second-position`、`:second-ordinal-position`，也支援第三至第十層對應單字；`other` 對應第十一層數字段。沒有對應層級時保留佔位符。`:input` 使用純量或空值的顯示值，包括已設定的值標籤；陣列不替換。序數共用數字格式化的預設英語語言，與驗證訊息語言分別處理。

```rust
use std::collections::BTreeMap;
use openrust::{http::{HttpError, HttpFuture, Request, Response, StatusCode}, validation::RuleSet};
fn store_photos(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::parse([("photos.*.description", "required|string")])?;
        let messages = BTreeMap::from([("photos.*.description.required".into(), "請描述第 :position 張照片（:ordinal-position），輸入=:input。".into())]);
        let data = request.validate_with(rules, messages, BTreeMap::new()).await?;
        Response::json(StatusCode::CREATED, data)
    })
}
```

## 信箱驗證模式 {#email-validation-modes}

`RuleBuilder` 保留原 `Rule::email()` 回傳型別並接通鏈式方法。`Email::new()` 提供型別化建構器，包含 `strict`、`when`、`unless`、`when_else` 與附加 `rules`。附加檢查透過原工廠建立巢狀驗證器，執行原 resolver 並讀取目前擴充註冊，使用原型別化資料、訊息及屬性；自訂 DataAware/ValidatorAware 規則接收這個巢狀驗證器。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Validator,
    validation::{Rule, RuleBuilder, RuleSet, ValidationResult, rules::Email},
};
fn store_email(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new().field("email", [Rule::required(), Rule::email()
            .rfc_compliant(false).validate_mx_record().prevent_spoofing()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}
fn store_native_email(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new().field("email", [Email::new().with_native_validation(true).into()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}
fn store_custom_email(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let email = Email::new().rfc_compliant(false)
            .rules([Rule::not_in(["blocked@example.com"]).into()]);
        let rules = RuleSet::new().field("email", [email.into()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}
fn configure_email_tests() -> ValidationResult<()> {
    Validator::fake_dns_lookups(true)
}
```

測試時，在應用作用域內呼叫 `Validator::fake_dns_lookups(true)`，或設定目前應用的 `Factory::fake_dns_lookups(true)`；傳入 `false` 恢復真實查詢。已建立的驗證器也會觀察其應用工廠的切換，其他應用不受影響；實例方法仍可明確覆寫單一驗證器。Rust 需要明確傳入布林值。DNS 模擬保留其他已選檢查與保留域處理；成功模擬用 `example.com` 等網域，不用 `.test`。

工廠的複製共用註冊表與 DNS 測試策略。需要獨立註冊狀態時，用 `Factory::default` 或 `Factory::new` 建立新工廠。

## 語言檔案訊息與欄位名稱 {#language-file-messages}

在 `lang/zh-Hant/validation.json` 建立或覆寫詞條。語言檔案 `custom` 的萬用字元可以跨巢狀路徑；行內訊息與欄位名稱的萬用字元只比對一層。尺寸專用訊息按宣告規則選擇 `numeric`、`array`、`file` 或 `string`，精確詞條優先。欄位名稱先查目前語言的完整 attributes 群組，再執行群組層級後備語言回退。未指定名稱的萬用字元欄位保留原路徑；一般欄位的底線或駝峰名稱轉換為單字。

```json
{
  "custom": {
    "photos.*.description": {
      "min": {
        "string": "第 :position 張：:attribute至少需要 :min 個字元。"
      }
    }
  },
  "attributes": {
    "photos.*.description": "描述"
  }
}
```

## 密碼策略 {#password-policies}

在 `bootstrap/providers.rs` 註冊此 Provider。`Password::defaults()` 讀取應用策略，`set_defaults` 註冊回呼；Rust 將無參數讀取與回呼設定分為兩個方法。沒有應用上下文時預設至少八個字元。`required()` 和 `sometimes()` 使用已設定的策略，`min(n)` 則建立新的明確策略。修改預設值不會改變已建立的策略物件。

```rust
use openrust::{
    foundation::Application,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::{ProviderResult, ServiceProvider},
    validation::{Rule, RuleSet, rules::Password},
};

struct PasswordServiceProvider;
impl ServiceProvider for PasswordServiceProvider {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            Password::set_defaults(|| Password::min(12).mixed_case().numbers())?;
            Ok(())
        })
    }
}

fn store_password(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rule = Password::defaults()?.symbols().rules([Rule::max(64)]);
        let data = request.validate(RuleSet::new().field("password", [rule.into()])?).await?;
        Response::json(StatusCode::CREATED, data)
    })
}
```

使用 `letters`、`mixed_case`、`numbers`、`symbols`、`max`、`when`、`unless` 和 `when_else` 組合策略。`rules` 取代前一批附加規則。基本與附加規則使用原驗證工廠，包含自訂訊息和 aware 規則；複雜度檢查仍讀取原密碼。迭代策略取得基本與附加規則，複雜度和外洩檢查屬於策略本身。`applied_rules()` 傳回型別化狀態，`to_password_rules_string()` 產生 HTML `passwordrules` 屬性。

附加 `.uncompromised(0)` 拒絕任何已知外洩，傳入較高閾值則允許出現不超過該次數。預設驗證器只向外洩範圍 API 傳送 SHA-1 大寫雜湊的前五位並啟用填充，基本規則失敗時不查詢。依固定機制，連線例外會報告；服務無法連線或回應不成功時允許密碼通過，因此此檢查是本地策略的補充。預設逾時 30 秒。可替換 `UncompromisedVerifier` 契約接入其他來源；獨立工廠可呼叫 `set_uncompromised_verifier`。Trait 明確接收值與有號閾值，取代動態資料陣列。預設策略與繫結按應用隔離，不使用行程全域可變狀態。

## 直接驗證與手動錯誤重新導向 {#direct-validator-entries}

`Validator::validate` 建立並執行目前應用設定的驗證器；自訂訊息與欄位名稱使用 `validate_with`。`Factory::validate` 明確接收四個參數。具體驗證器的 `passes().await` 重新執行驗證，傳回與 `fails()` 相反的結果。

```rust
use openrust::{
    contracts::validation::Validator as _,
    foundation::http::FormRequest,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Redirect, Validator},
    validation::{AfterValidation, AfterValidationRule, Data, RuleSet, ValidationResult},
};

fn validate_article(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = Validator::validate(
            Data::from_request(request).await?,
            RuleSet::parse([("title", "required|string|min:3")])?,
        ).await?;
        Response::json(StatusCode::CREATED, data)
    })
}

fn save_form(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut validator = Validator::make(
            Data::from_request(request).await?,
            RuleSet::parse([("title", "required|string|min:3")])?,
        )?;
        if !validator.passes().await? {
            return Ok(Redirect::to("/edit", Default::default())?
                .with_errors(request, &validator, "article")?
                .with_input(request, None).await?.into());
        }
        Response::json(StatusCode::CREATED, validator.validated()?)
    })
}

struct CheckTitle;
impl AfterValidationRule for CheckTitle {
    fn after(&self, validator: &mut openrust::validation::Validator) -> ValidationResult<()> {
        if validator.get_value("title")?.and_then(|value| value.as_str()) == Some("blocked") {
            validator.errors_mut().add("title", "Title is blocked.");
        }
        Ok(())
    }
}

struct StoreArticle;
impl FormRequest for StoreArticle {
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([("title", "required|string")])
    }
    fn after(&self) -> Vec<AfterValidation> {
        vec![CheckTitle.into_callback()]
    }
}
fn validate_form(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = StoreArticle.validate_resolved(request).await?;
        Response::json(StatusCode::CREATED, data)
    })
}
```

`with_errors` 接收驗證器、`MessageProvider`、訊息包或訊息陣列，例如 `[("title", "標題無效。")]`。既有訊息包保持借用直到工作階段儲存，適配器不會先複製一次。請求與錯誤包名稱需明確提供。`after_many` 附加盒裝回呼清單；`after_rule` 和 `after_rules` 註冊實作 `AfterValidationRule` 的服務。在 Form Request 中，`into_callback()` 將服務轉換為原回呼型別並保留其清單順序。這些入口適配動態可呼叫物件，不改變執行順序或驗證管線。

## 對多個欄位附加條件規則 {#conditional-multiple-fields}

`sometimes` 接收單一欄位、陣列、向量或借用切片，保留欄位順序、重複欄位及每個萬用字元項目的回呼上下文。條件在驗證前讀取原始資料，因此範例復用既有純量比較橋，保留數值字串和布林值的比較行為。Rust 回呼接收 `&Data` 與 `Option<&Value>`，取代動態 Fluent 物件。單欄位借用不配置新字串。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::{facades::Validator, php_integer_less_than_value, php_loose_string_value_equal},
    validation::{Data, Rule, RuleSet},
};

pub fn store_collector(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut validator = Validator::make(
            Data::from_request(request).await?,
            RuleSet::parse([("games", "required|integer|min:0")])?,
        )?;
        validator.sometimes(["reason", "cost"], vec![Rule::required()], |input, _| {
            let games = &input.as_value()["games"];
            php_integer_less_than_value(100, games) || php_loose_string_value_equal("100", games)
        })?;
        Response::json(StatusCode::CREATED, validator.validate_request(request).await?)
    })
}
```

行內訊息依序查找欄位／規則、規則、欄位，每層保留原生 `BTreeMap` 的遍歷順序。同層中較早的萬用字元詞條可以優先於後面的精確詞條；若不希望順序決定訊息，應避免重疊詞條。行內萬用字元只比對一層，也支援萬用字元規則名稱；語言檔案 custom 訊息仍走獨立的跨路徑查找。`sometimes` 條件為 false 時也記錄已解析萬用字元欄位的主名稱，供後續錯誤標籤使用。

## 顯示錯誤並還原表單輸入 {#displaying-errors-and-old-input}

`web` 中介層共享目前請求的 `$errors` 物件；沒有錯誤時也存在。可呼叫預設包的 `any`、`all`、`first`、`get`、`has`，或選擇具名包。`@error` 讀取第一條比對訊息並支援萬用字元欄位；`$message` 只在此指令內繫結。

```blade
@if ($errors->any())
    <ul>
        @foreach ($errors->all() as $error)
            <li>{{ $error }}</li>
        @endforeach
    </ul>
@endif

<input name="title" value="{{ old('title', 'Untitled') }}">
@error('title')
    <p>{{ $message }}</p>
@enderror

@error('title', 'login')
    <p>{{ $message }}</p>
@enderror
<p>{{ $errors->login->first('title') }}</p>
```

`old(key, default)` 與 `request()->old(key, default)` 透過 HTTP 渲染橋讀取既有工作階段快閃資料。缺失鍵使用預設值，已儲存的 `null` 仍為 null。傳回視圖或使用 `Response::view` 渲染，確保真實請求可用。轉義輸出會保護錯誤文字與舊輸入。後續請求按原機制回收快閃資料，範本快取仍讀取目前請求的錯誤和舊值。


## 表單請求的重新導向目標 {#form-request-redirect-targets}

透過 `redirect_to()` 指定 URL，或透過 `redirect_route()` 指定命名路由。明確 URL 優先，並經目前請求的 URL 產生器構造；命名路由僅在驗證失敗後解析。這裡用原生 trait 方法表達 URL／命名路由屬性。接受請求前須註冊 `dashboard`；未知路由傳播 URL 產生錯誤，不靜默後備。目標可解析時 JSON 失敗仍傳回 422；驗證成功不會解析失敗目標。

```rust
use openrust::{
    foundation::http::{FormRequest, ValidatedFormRequest},
    http::{HttpError, Response, StatusCode},
    validation::{RuleSet, ValidationResult},
};

#[derive(Default)]
pub struct StorePostRequest;

impl FormRequest for StorePostRequest {
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([("title", "required|string")])
    }

    fn redirect_route(&self) -> Option<&str> {
        Some("dashboard")
    }

    fn error_bag(&self) -> &str {
        "post"
    }
}

pub async fn store_post(_: ValidatedFormRequest<StorePostRequest>) -> Result<Response, HttpError> {
    Ok(Response::empty(StatusCode::CREATED))
}
```
