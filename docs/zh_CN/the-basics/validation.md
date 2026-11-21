# 验证

## 手动创建验证器

用 `Validator::make` 创建验证器，再调用 `validate().await`。返回值包含通过验证的字段。用点路径和 `*` 选择嵌套元素。用 `nullable` 允许显式 null，用 `sometimes` 在字段存在时才运行规则。

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

## 数据库验证规则 {#database-validation-rules}

用 `exists:table,column` 要求存在匹配行，用 `unique:table,column` 拒绝已有值。省略列名时使用字段名，在表名前加 `connection.` 可指定具名连接。两条规则都使用写连接。数组形式的 `exists` 用一次查询检查所有不同的值是否存在。

字符串规则的参数值含逗号时，用双引号包住该值，如 `exists:directory.references,reference,namespace,"team,west"`。用于排除记录的文本 ID 如 `001` 会保留前导零。数组存在性验证按 Laravel 的标量字符串形式统计不同输入。

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

流式规则支持附加查询条件和软删除筛选。更新记录时，用 `ignore` 传入可信的记录标识，或把应用已解析并授权的模型传给 `ignoreModel`。并发写入的唯一性由数据库唯一约束保证。

## 表单请求验证 {#form-request-validation}

把 `ValidatedFormRequest<F>` 声明为 action 的类型参数。路由在调用 action 前创建表单请求，依次准备输入、授权、验证并调用 `passedValidation`。授权拒绝返回 HTTP 403。

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

表单请求实现 `Default` 用于构造。需要构造依赖时，可在容器中为表单类型注册瞬态绑定。普通控制器方法用 `ControllerDispatcher::withFormRequest`，资源控制器用 `ResourceController::actionWithFormRequest`。两种控制器都可接收原始请求及其路由参数。需要显式验证时调用 `validateResolved`。

## 文件验证 {#validating-files}

手动验证 multipart 时使用 `Data::fromRequest`。FormRequest 会自动包含上传文件。通过 `validated.file` 读取已验证的文件，支持 `attachments.0` 等点路径。JSON 视图中的文件位置表示为 null。

`mimes` 和 `mimetypes` 从文件内容推断类型，`extensions` 检查客户端文件名的扩展名。内容和扩展名都有要求时可同时使用。用 `image:allow_svg` 或 `File::image(true)` 接受 SVG。`size`、`min`、`max` 和 `between` 以 KiB 计量文件大小，每 KiB 为 1,024 字节。

内容检测将 JSON 对象和数组识别为 `application/json`，将 CSV 内容识别为纯文本，将空上传识别为 `application/x-empty`。需要检查客户端文件名的 CSV 扩展名时，使用 `extensions:csv`。

用 `dimensions` 限制宽、高或宽高比。比例可写成小数或 `16/9` 这样的分数。图片规则允许的 SVG 会跳过位图尺寸约束。用 `encoding:UTF-8` 或 `File::default().encoding("UTF-8")` 检查文件字节。可使用 UTF-8、ASCII、UTF-16、UTF-32、SJIS、CP932 和 EUC-CN 等受支持的 Laravel 编码名称。Windows Shift-JIS 扩展使用 CP932 或 SJIS-win。

单字节文件可用 `encoding:CP850` 或 `File::default().encoding("ArmSCII-8")` 校验。CP850 接受所有字节。ArmSCII-8 以及存在未定义位置的 Windows 和 ISO 编码按 PHP mbstring 的字表拒绝这些字节。编码校验保留上传文件原始字节。

验证东亚双字节文件时，请选择文件实际使用的编码。`encoding:BIG-5` 和 `encoding:EUC-KR` 检查标准字节范围，对应的 Windows 扩展可用 `File::default().encoding("CP950")` 或 `File::default().encoding("UHC")`。各编码按 PHP mbstring 的字表拒绝不完整字节对和未分配位置。

`encoding:EUC-CN` 校验该编码已分配的双字节位置，`encoding:CP936` 校验 Windows 的字节范围。`encoding:GB18030` 和 `File::default().encoding("GB18030-2022")` 都遵循 PHP mbstring，接受相同的双字节及四字节序列。

日文 EUC 文件可按实际字表使用 `encoding:EUC-JP` 或 `encoding:eucJP-win`。Fluent 文件规则也支持 `File::default().encoding("CP51932")` 和 `File::default().encoding("EUC-JP-2004")`。各变体已分配的双字节和三字节位置按 PHP mbstring 校验。

Shift-JIS 文件校验还支持 `SJIS-mac`、`SJIS-2004`、`SJIS-Mobile#DOCOMO`、`SJIS-Mobile#KDDI` 和 `SJIS-Mobile#SOFTBANK`。按实际编码选择规则，例如 `encoding:SJIS-mac` 或 `File::default().encoding("SJIS-Mobile#SOFTBANK")`。SoftBank 变体也会验证旧式表情转义序列。

`encoding:ISO-2022-JP` 和 `encoding:JIS` 校验日文转义序列，并要求输入结束前切回 ASCII。文件规则也支持 `ISO-2022-JP-MS`、`ISO-2022-JP-2004`、`ISO-2022-JP-MOBILE#KDDI` 及 `CP50220` 至 `CP50222`，各变体按 PHP mbstring 校验字表和结束状态。

HZ、EUC-TW 和 ISO-2022-KR 文件可用 `encoding:HZ`、`File::default().encoding("EUC-TW")` 或 `encoding:ISO-2022-KR` 校验。HZ 和 ISO-2022-KR 的转义与移位遵循 PHP mbstring，EUC-TW 检查 CNS 11643 第 1、2、14 平面的已分配位置。

四种移动版 UTF-8 按标准 UTF-8 校验字节。可用 `encoding:UTF-8-Mobile#DOCOMO` 或 `File::default().encoding("UTF-8-Mobile#SOFTBANK")`，另外两个名称为 `UTF-8-Mobile#KDDI-A` 和 `UTF-8-Mobile#KDDI-B`。校验保留原始字节中的私用区字符。

TIFF 和 ICO 上传可以使用 `file` 和 `dimensions:width=100,height=100` 一起验证。TIFF 从第一个图像目录取尺寸，ICO 从位深最高的条目取尺寸，位深相同时取最后一个。尺寸校验只读取元数据。Laravel 的 `image` 规则排除 TIFF 和 ICO。

PSD 和 JPEG 2000 上传也可以使用这组规则。JP2 从第一个根级码流读取尺寸，裸 JPEG 2000 码流从 SIZ 标记读取尺寸。Laravel 的 `image` 规则也排除这两种格式。

相同的元数据校验也支持 IFF 位图、WBMP、XBM，以及未压缩或使用 zlib 压缩的 SWF。SWF 的尺寸来自舞台矩形。

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

### 传输编码 {#transfer-encoding}

用 `base64` 验证规范的 Base64 字符串。上传文件的 `encoding:BASE64` 按 PHP mbstring 检查字符表，允许 `=`、空格、制表符及 CR/LF 出现在任意位置。`Quoted-Printable`、`HTML-ENTITIES` 和 `UUENCODE` 的 mbstring 兼容检查接受任意字节。

```rust
use openrust::validation::{Rule, RuleSet, ValidationResult, rules::File};

fn transfer_rules() -> ValidationResult<RuleSet> {
    RuleSet::parse([("value", "required|base64")])?
        .field("upload", [Rule::required(), File::default().encoding("BASE64").into()])
}
```

### UTF-7 与 UTF7-IMAP {#utf7-encoding}

用 `UTF-7` 检查标准 UTF-7，用 `UTF7-IMAP` 检查 IMAP 的变体。两者都会检查移位段中的 UTF-16 码元，拒绝错误的代理对和非零填充位。UTF7-IMAP 的每个移位段都须以 `-` 结束。

```rust
use openrust::validation::{RuleSet, ValidationResult};

fn mailbox_rules() -> ValidationResult<RuleSet> {
    RuleSet::parse([
        ("subject", "required|string|encoding:UTF-7"),
        ("folder", "required|string|encoding:UTF7-IMAP"),
    ])
}
```

## 验证错误响应格式

请求期望 JSON 时，验证失败返回 HTTP 422，正文包含 `message` 和按字段索引的 `errors`。展开后的字段用点路径表示，例如 `users.0.email`。可在当前语言的 `lang/<locale>/validation.json` 中覆盖规则消息和字段名称。

## 显示验证错误

启用会话的 Web 路由验证失败时，会重定向回表单，并把错误和提交的输入闪存到下一次请求。Blade 视图通过 `$errors` 取得当前请求的错误包。可用 `request.old(Some("profile.name"))` 回填字段。

重定向依次使用显式配置的目标、同源 Referer、会话保存的上一页。上一页的查询字符串会保留。没有可用目标时重定向到 `/`。

### 命名错误包

在验证器上调用 `validateWithBag("profile")`，或覆盖 `FormRequest::errorBag`，即可指定错误包。在 Blade 中，`$errors->profile->name` 选择该包内 name 字段的消息。

### 排除敏感输入

密码字段会从旧输入中排除。可在 `withExceptions` 中用 `dontFlash` 注册额外路径。

```rust
use openrust::foundation::configuration::Exceptions;

fn configure(exceptions: &mut Exceptions) {
    exceptions.dontFlash(["profile.secret"]);
}
```
## 标准规则与验证钩子

规则解析支持 `numeric`、`date`、`in:a,b`、`regex:/pattern/u`、`required_if:field,value`、`exclude_if:field,value`、`accepted`、`url`、`uuid`、`array:key1,key2`。`array:key1,key2` 拒绝清单外的键；`in` 与 `array` 组合时逐项检查数组。被排除的字段及上传文件不会出现在已验证数据中，规则按声明顺序执行。

默认 `integer` 遵循 PHP 整数验证，包含部分整数值浮点数和 `true`；`integer:strict` 只接受 JSON 整数，`boolean:strict` 只接受 JSON 布尔值。数值大小规则支持负数和小数阈值，使用十进制比较，避免浮点舍入。科学计数法指数超出 -1000 至 1000 时返回执行错误。

浮点数先按 PHP 默认的 14 位有效数字转为文本，例如浮点值 `1e13` 通过、`1e14` 不通过，而整数值 `100000000000000` 通过。`regex` 只接受字符串或数值；布尔值与未声明 `nullable` 的 `null` 不通过。

正则包含 `|` 时，使用 `RuleSet::field` 的规则数组并通过 `Rule::parse` 解析，避免被当成规则分隔符。支持 PCRE2 修饰符 `i m s x u A U J n D`；`D` 使 `$` 仅匹配输入末尾，多行模式下遵循 PCRE2 原生语义。其他尚不支持的修饰符返回 `InvalidRule`，不表示已经支持 PHP 的全部 PCRE 选项。

`Validator::after` 在规则检查后运行，可追加错误。`sometimes` 按条件追加规则，通配符回调收到所在父项。`stopOnFirstFailure(true)` 在不同字段间停止检查，`bail` 只停止当前字段。`safe()` 必须在验证成功后调用，只暴露已验证的数据。

FormRequest 顺序为 `prepareForValidation`、`authorize`、`withValidator`、验证和 `after`、`passedValidation`。授权或验证失败不会执行业务方法。携带 `Precognition-Validate-Only` 的预验证成功后不调用 `passedValidation`；未带该头时可能先调用，再短路业务方法。验证器按请求隔离。
