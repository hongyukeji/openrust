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
                .withoutTrashed(None)
                .into(),
        ])
}
```

流暢介面支援附加查詢條件和軟刪除篩選。更新資料列時，用 `ignore` 傳入可信的識別碼，或將應用程式已解析並授權的模型傳給 `ignoreModel`。並行寫入的唯一性由資料庫唯一約束保證。

## 表單請求驗證 {#form-request-validation}

將 `ValidatedFormRequest<F>` 宣告為 action 的型別參數。路由在呼叫 action 前建立表單請求，依序準備輸入、授權、驗證並呼叫 `passedValidation`。授權拒絕回傳 HTTP 403。

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

表單請求實作 `Default` 以便建立。需要建構依賴時，可在容器中為表單型別註冊暫時繫結。一般控制器方法用 `ControllerDispatcher::withFormRequest`，資源控制器用 `ResourceController::actionWithFormRequest`。兩種控制器皆可接收原始請求及其路由參數。需要明確驗證時呼叫 `validateResolved`。

## 檔案驗證 {#validating-files}

手動驗證 multipart 時使用 `Data::fromRequest`。FormRequest 會自動包含上傳檔案。透過 `validated.file` 讀取已驗證的檔案，支援 `attachments.0` 等點路徑。JSON 檢視中的檔案位置表示為 null。

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
        let data = Data::fromRequest(request).await?;
        let rules = RuleSet::new().field("image", [
            Rule::required(),
            File::image(false)
                .dimensions(Rule::dimensions().minWidth(256).ratio("1/1"))
                .max("2mb")
                .into(),
        ])?;
        let validated = Validator::make(data, rules)?.validate().await?;
        let size = validated.file("image").map(|file| file.getSize());
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

在驗證器上呼叫 `validateWithBag("profile")`，或覆寫 `FormRequest::errorBag`，即可指定錯誤包。在 Blade 中，`$errors->profile->name` 選取該包內 name 欄位的訊息。

### 排除敏感輸入

密碼欄位會從舊輸入排除。可在 `withExceptions` 中用 `dontFlash` 註冊額外路徑。

```rust
use openrust::foundation::configuration::Exceptions;

fn configure(exceptions: &mut Exceptions) {
    exceptions.dontFlash(["profile.secret"]);
}
```
## 標準規則與驗證鉤子

規則解析支援 `numeric`、`date`、`in:a,b`、`regex:/pattern/u`、`required_if:field,value`、`exclude_if:field,value`、`accepted`、`url`、`uuid`、`array:key1,key2`。`array:key1,key2` 拒絕清單外的鍵；`in` 與 `array` 組合時逐項檢查陣列。被排除的欄位及上傳檔案不會出現在已驗證資料中，規則按宣告順序執行。

預設 `integer` 遵循 PHP 整數驗證，包含部分整數值浮點數和 `true`；`integer:strict` 只接受 JSON 整數，`boolean:strict` 只接受 JSON 布林值。數值大小規則支援負數和小數閾值，使用十進位比較。科學記號指數超出 -1000 至 1000 時傳回執行錯誤。

浮點數先依 PHP 預設的 14 位有效數字轉為文字，例如浮點值 `1e13` 通過、`1e14` 不通過，而整數值 `100000000000000` 通過。`regex` 只接受字串或數值；布林值與未宣告 `nullable` 的 `null` 不通過。

正則包含 `|` 時，使用 `RuleSet::field` 的規則陣列並透過 `Rule::parse` 解析，避免被當成規則分隔符號。支援 PCRE2 修飾符 `i m s x u A U J n D`；`D` 使 `$` 僅比對輸入末尾，多行模式下遵循 PCRE2 原生語義。其他尚不支援的修飾符傳回 `InvalidRule`，不表示已支援 PHP 的全部 PCRE 選項。

`Validator::after` 在規則檢查後執行，可追加錯誤。`sometimes` 按條件追加規則，萬用字元回呼收到所在父項。`stopOnFirstFailure(true)` 在不同欄位間停止檢查，`bail` 只停止目前欄位。`safe()` 必須在驗證成功後呼叫，只公開已驗證資料。

FormRequest 順序為 `prepareForValidation`、`authorize`、`withValidator`、驗證和 `after`、`passedValidation`。授權或驗證失敗不執行業務方法。攜帶 `Precognition-Validate-Only` 的預驗證成功後不呼叫 `passedValidation`；未帶該標頭時可能先呼叫，再短路業務方法。驗證器按請求隔離。

## 訊息包 {#message-bags}

使用 `MessageBag` 收集有序欄位錯誤，`ViewErrorBag` 管理具名包。`add` 去重，`merge(&other)` 保留重複。`first(key, format)` 在沒有訊息時傳回空字串。`get(key, format)` 傳回 `MessageLookup::Exact` 或保留欄位名的萬用字元映射；僅在確實需要扁平訊息時呼叫 `intoMessages()`。

```rust
use openrust::support::{MessageBag, ViewErrorBag};

let mut messages = MessageBag::new([("email", ["Invalid email"])]);
messages.addIf(true, "name", "Required");
assert_eq!(messages.first(Some("email"), None), "Invalid email");
assert!(messages.hasAny(["email", "phone"]));
messages.setFormat(":key: :message");
assert_eq!(messages.first(Some("name"), None), "name: Required");
let mut errors = ViewErrorBag::new();
errors.put("profile", messages);
assert_eq!(errors.getBag("profile").count(), 2);
assert_eq!(errors.count(), 0);
```

具名包計數及轉發的唯讀方法只針對 `default` 包。讀取缺失包不會登記它；用 `getBagMut` 修改已有包，或用 `put` 新增包。JSON 儲存原始訊息，不受顯示格式影響；反序列化恢復預設格式。訊息列表使用連續的 Rust 索引，JSON 選項遵循 Serde，不提供 PHP 陣列索引、JSON 位元旗標或執行期動態方法分派。


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

`active_url` 要求主機存在 DNS A 或 AAAA 紀錄，使用非同步 DNS 查詢，不因作業系統能解析數字 IP 就判為成功。`validator.setDnsResolver(resolver)` 接受 Hickory 0.25 解析器，可指定名稱伺服器、逾時與重試策略；DNS 查詢失敗即驗證失敗。測試時用 `validator.fakeDnsLookups(true)` 對目前驗證器啟用僅檢查主機名稱的行為，傳入 `false` 恢復實際 DNS 查詢。開關按實例隔離，並行驗證器互不影響。

## 修改驗證器

用 `getData`、`getValue` 和 `getRules` 讀取輸入及展開後的規則。`getRules` 按宣告順序傳回 `(欄位路徑, 規則)`，路徑使用原生跳脫。`setValue` 更新一個具體路徑，拒絕萬用字元。`setData` 替換輸入，並恢復最近一次 `setRules` 或工廠建立時的規則。`addRules` 合併暫時規則；`appendRules` 同時將合併後的宣告作為下次重設的基準。

`setCustomMessages` 合併訊息。`setAttributeNames`、`setValueNames` 替換對應映射；`addCustomAttributes`、`addCustomValues` 按欄位合併。`getTranslator`、`setTranslator` 讀取或替換錯誤訊息使用的譯器。修改後會捨棄快取的成功結果和錯誤，需要重新執行 `validate().await`，再讀取 `validated()` 或 `safe()`。

`getException`、`setException` 使用型別化函式 `fn(MessageBag, Option<&Translator>) -> ValidationException`，可自訂實際拒絕回應的狀態、錯誤包及重新導向。固定回傳型別替代執行期例外類別名稱及繼承，每個驗證器獨立保存工廠。

## 註冊驗證擴充

在服務提供者中用 `Validator::extend` 註冊一般規則，`extendImplicit` 註冊缺失或空值也必須執行的規則，`extendDependent` 註冊參數引用其他欄位的規則。名稱可用 `ProjectCode` 或 `project_code`，宣告統一使用 `project_code`。工廠建立驗證器時複製目前註冊項，後續註冊不會改變已建立的執行個體。

```rust
use openrust::support::facades::Validator;

Validator::extend("upper_code", |_, value, _, _| {
    Ok(value.and_then(serde_json::Value::as_str)
        .is_some_and(|value| value.chars().all(|ch| ch.is_ascii_uppercase())))
}, Some(":attribute 必須由大寫字母組成。"))?;
```

回呼接收欄位名稱、`Option<&Value>`、參數及唯讀驗證器，傳回 `ValidationResult<bool>`。一般規則略過缺失和空字串；`nullable` 略過 null，`sometimes` 仍限制缺失欄位。隱式規則可檢查缺失、空字串及 null。依賴規則將 `items.*.other` 替換為目前項目的具體路徑。執行錯誤透過 `Err` 傳播，不視為一般驗證失敗。非同步規則繼續使用 `Rule::custom` 和 `ValidationRule`。

用 `Validator::replacer` 註冊訊息替換回呼，參數為訊息、欄位、規則名稱、參數列表及唯讀驗證器，傳回最終訊息。先選擇自訂或翻譯訊息並替換欄位名稱，再呼叫替換器；註冊替換器後，由它處理 `:min` 等規則參數。`Factory::resolver` 接收並傳回擁有的驗證器，用於建立時的定制。Rust 使用閉包及明確解析的服務，不支援執行期 `Class@method` 字串或動態驗證器子類別。

單一驗證器可使用 `addExtension`、`addImplicitExtension`、`addDependentExtension` 和 `addReplacer`；對應複數方法接受 `(String, Arc<回呼>)`。這些修改會清除既有驗證結果，需要再次驗證。

工廠預設 `excludeUnvalidatedArrayKeys()`：父欄位宣告無參數 `array` 或 `list` 且有子欄位規則時，只傳回經過驗證的子鍵。用 `includeUnvalidatedArrayKeys()` 將之後建立的驗證器切換為包含父欄位全部鍵；呼叫 `excludeUnvalidatedArrayKeys()` 恢復預設。`array:name,email` 已明確限制允許鍵，不採用此裁剪策略。單一驗證器的 `excludeUnvalidatedArrayKeys` 布林欄位可獨立調整，`validated()` 和 `safe()` 隨即採用新策略。
