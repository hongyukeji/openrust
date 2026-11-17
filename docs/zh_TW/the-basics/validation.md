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

字串規則的參數值含逗號時，用雙引號包住該值，如 `exists:directory.references,reference,namespace,"team,west"`。用於排除資料列的文字 ID 如 `001` 會保留前導零。陣列存在性驗證按 Laravel 的純量字串形式統計不同輸入。

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
    http::{HttpError, Method, Response, StatusCode},
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
    Route::r#match(&[Method::POST], "/users", store)
}
```

表單請求實作 `Default` 以便建立。需要建構依賴時，可在容器中為表單型別註冊暫時繫結。一般控制器方法用 `ControllerDispatcher::withFormRequest`，資源控制器用 `ResourceController::actionWithFormRequest`。兩種控制器皆可接收原始請求及其路由參數。需要明確驗證時呼叫 `validateResolved`。

## 檔案驗證 {#validating-files}

手動驗證 multipart 時使用 `Data::fromRequest`。FormRequest 會自動包含上傳檔案。透過 `validated.file` 讀取已驗證的檔案，支援 `attachments.0` 等點路徑。JSON 檢視中的檔案位置表示為 null。

`mimes` 和 `mimetypes` 從檔案內容推斷類型，`extensions` 檢查用戶端檔名的副檔名。內容和副檔名都有要求時可同時使用。用 `image:allow_svg` 或 `File::image(true)` 接受 SVG。`size`、`min`、`max` 和 `between` 以 KiB 計量檔案大小，每 KiB 為 1,024 位元組。

內容偵測會將 JSON 物件和陣列辨識為 `application/json`，將 CSV 內容辨識為純文字，並將空上傳辨識為 `application/x-empty`。需要檢查用戶端檔名的 CSV 副檔名時，使用 `extensions:csv`。

用 `dimensions` 限制寬、高或長寬比。比例可寫成小數或 `16/9` 這樣的分數。圖片規則允許的 SVG 會略過點陣圖尺寸限制。用 `encoding:UTF-8` 或 `File::default().encoding("UTF-8")` 檢查檔案位元組。可使用 UTF-8、ASCII、UTF-16、UTF-32、SJIS、CP932 和 EUC-CN 等受支援的 Laravel 編碼名稱。Windows Shift-JIS 擴充使用 CP932 或 SJIS-win。

單位元組檔案可用 `encoding:CP850` 或 `File::default().encoding("ArmSCII-8")` 驗證。CP850 接受所有位元組。ArmSCII-8 以及存在未定義位置的 Windows 和 ISO 編碼依 PHP mbstring 的字表拒絕這些位元組。編碼驗證保留上傳檔案的原始位元組。

驗證東亞雙位元組檔案時，請選擇檔案實際使用的編碼。`encoding:BIG-5` 和 `encoding:EUC-KR` 檢查標準位元組範圍，對應的 Windows 擴充可用 `File::default().encoding("CP950")` 或 `File::default().encoding("UHC")`。各編碼依 PHP mbstring 的字表拒絕不完整位元組對與未分配位置。

`encoding:EUC-CN` 驗證該編碼已分配的雙位元組位置，`encoding:CP936` 驗證 Windows 的位元組範圍。`encoding:GB18030` 和 `File::default().encoding("GB18030-2022")` 都遵循 PHP mbstring，接受相同的雙位元組及四位元組序列。

日文 EUC 檔案可依實際字表使用 `encoding:EUC-JP` 或 `encoding:eucJP-win`。Fluent 檔案規則也支援 `File::default().encoding("CP51932")` 和 `File::default().encoding("EUC-JP-2004")`。各變體已分配的雙位元組及三位元組位置依 PHP mbstring 驗證。

TIFF 和 ICO 上傳可以使用 `file` 和 `dimensions:width=100,height=100` 一起驗證。TIFF 從第一個影像目錄取得尺寸，ICO 從位元深度最高的項目取得尺寸，位元深度相同時取最後一個。尺寸驗證只讀取中繼資料。Laravel 的 `image` 規則排除 TIFF 和 ICO。

PSD 和 JPEG 2000 上傳也可以使用這組規則。JP2 從第一個根層級碼流讀取尺寸，裸 JPEG 2000 碼流從 SIZ 標記讀取尺寸。Laravel 的 `image` 規則也排除這兩種格式。

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
