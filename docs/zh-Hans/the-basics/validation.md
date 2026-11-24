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

`email` 和 `email:rfc` 验证默认 RFC 地址语法，允许仅产生警告的地址。尚未实现 DNS、spoof、strict 和 filter 模式；解析规则时，不支持的邮箱参数返回 `ValidationError::InvalidRule`，不会作为应用自定义规则处理。

## 验证传入请求 {#validating-incoming-requests}

在处理器中直接调用 `request.validate(rules).await?`。它使用应用验证器和当前请求，只返回通过验证的输入。失败走正常的 JSON 422 或 Web 重定向／Session 错误管线。

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

用 `request.validate_with_bag("post", rules).await?` 选择具名错误包。Request 门面转发到同一实例方法。在验证中间件中，Precognition 会按 `Precognition-Validate-Only` 筛选字段，并在预验证成功时提前返回，后续业务逻辑不会执行。普通路由动作不参与预验证执行；需要预验证时使用 Form Request 或验证中间件。

## 数据库验证规则 {#database-validation-rules}

用 `exists:table,column` 要求存在匹配行，用 `unique:table,column` 拒绝已有值。省略列名时使用字段名，在表名前加 `connection.` 可指定具名连接。两条规则都使用写连接。数组形式的 `exists` 用一次查询检查所有不同的值是否存在。

字符串规则的参数值含逗号时，用双引号包住该值，如 `exists:directory.references,reference,namespace,"team,west"`。用于排除记录的文本 ID 如 `001` 会保留前导零。数组存在性验证按标量字符串形式统计不同输入。

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

流式规则支持附加查询条件和软删除筛选。更新记录时，用 `ignore` 传入可信的记录标识，或把应用已解析并授权的模型传给 `ignore_model`。并发写入的唯一性由数据库唯一约束保证。

## 表单请求验证 {#form-request-validation}

### 创建表单请求 {#creating-form-requests}

```sh
./artisan make:request StoreArticleRequest
./artisan make:request Admin/StoreArticleRequest
```

命令创建 `app/http/requests/store_article_request.rs`，嵌套名称写入 `admin/` 子模块。生成类型实现 `FormRequest`、派生 `Default`，并提供 `authorize` 和 `rules`。默认拒绝授权，使用前须明确编写授权逻辑；规则初始为空，由应用声明字段。

在 `app/http/mod.rs` 声明 `pub(crate) mod requests;`，并在 `app/http/requests/mod.rs` 声明生成文件模块，例如 `pub(crate) mod store_article_request;`。Rust 需要编译期模块声明；生成器保留已有模块文件。之后以 `ValidatedFormRequest<StoreArticleRequest>` 作为动作的类型化参数。

`-f`／`--force` 覆盖已有文件；不加选项重复生成会报告冲突、保留内容并退出 0，与通用生成器契约一致；无效名称或写入错误退出 1。`./artisan stub:publish` 发布 `stubs/request.rs.stub`，修改后用于后续生成。嵌套名称接受 `/` 或 `\`，也支持 `App/Http/Requests/StoreArticleRequest`。

把 `ValidatedFormRequest<F>` 声明为 action 的类型参数。路由在调用 action 前创建表单请求，依次准备输入、授权、验证并调用 `passed_validation`。授权拒绝返回 HTTP 403。

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

表单请求实现 `Default` 用于构造。需要构造依赖时，可在容器中为表单类型注册瞬态绑定。普通控制器方法用 `ControllerDispatcher::with_form_request`，资源控制器用 `ResourceController::action_with_form_request`。两种控制器都可接收原始请求及其路由参数。需要显式验证时调用 `validate_resolved`。

## 文件验证 {#validating-files}

手动验证 multipart 时使用 `Data::from_request`。FormRequest 会自动包含上传文件。通过 `validated.file` 读取已验证的文件，支持 `attachments.0` 等点路径。JSON 视图中的文件位置表示为 null。

`mimes` 和 `mimetypes` 从文件内容推断类型，`extensions` 检查客户端文件名的扩展名。内容和扩展名都有要求时可同时使用。用 `image:allow_svg` 或 `File::image(true)` 接受 SVG。`size`、`min`、`max` 和 `between` 以 KiB 计量文件大小，每 KiB 为 1,024 字节。

内容检测将 JSON 对象和数组识别为 `application/json`，将 CSV 内容识别为纯文本，将空上传识别为 `application/x-empty`。需要检查客户端文件名的 CSV 扩展名时，使用 `extensions:csv`。

用 `dimensions` 限制宽、高或宽高比。比例可写成小数或 `16/9` 这样的分数。图片规则允许的 SVG 会跳过位图尺寸约束。用 `encoding:UTF-8` 或 `File::default().encoding("UTF-8")` 检查文件字节。可使用 UTF-8、ASCII、UTF-16、UTF-32、SJIS、CP932 和 EUC-CN 等受支持的编码名称。Windows Shift-JIS 扩展使用 CP932 或 SJIS-win。

单字节文件可用 `encoding:CP850` 或 `File::default().encoding("ArmSCII-8")` 校验。CP850 接受所有字节。ArmSCII-8 以及存在未定义位置的 Windows 和 ISO 编码按 PHP mbstring 的字表拒绝这些字节。编码校验保留上传文件原始字节。

验证东亚双字节文件时，请选择文件实际使用的编码。`encoding:BIG-5` 和 `encoding:EUC-KR` 检查标准字节范围，对应的 Windows 扩展可用 `File::default().encoding("CP950")` 或 `File::default().encoding("UHC")`。各编码按 PHP mbstring 的字表拒绝不完整字节对和未分配位置。

`encoding:EUC-CN` 校验该编码已分配的双字节位置，`encoding:CP936` 校验 Windows 的字节范围。`encoding:GB18030` 和 `File::default().encoding("GB18030-2022")` 都遵循 PHP mbstring，接受相同的双字节及四字节序列。

日文 EUC 文件可按实际字表使用 `encoding:EUC-JP` 或 `encoding:eucJP-win`。Fluent 文件规则也支持 `File::default().encoding("CP51932")` 和 `File::default().encoding("EUC-JP-2004")`。各变体已分配的双字节和三字节位置按 PHP mbstring 校验。

Shift-JIS 文件校验还支持 `SJIS-mac`、`SJIS-2004`、`SJIS-Mobile#DOCOMO`、`SJIS-Mobile#KDDI` 和 `SJIS-Mobile#SOFTBANK`。按实际编码选择规则，例如 `encoding:SJIS-mac` 或 `File::default().encoding("SJIS-Mobile#SOFTBANK")`。SoftBank 变体也会验证旧式表情转义序列。

`encoding:ISO-2022-JP` 和 `encoding:JIS` 校验日文转义序列，并要求输入结束前切回 ASCII。文件规则也支持 `ISO-2022-JP-MS`、`ISO-2022-JP-2004`、`ISO-2022-JP-MOBILE#KDDI` 及 `CP50220` 至 `CP50222`，各变体按 PHP mbstring 校验字表和结束状态。

HZ、EUC-TW 和 ISO-2022-KR 文件可用 `encoding:HZ`、`File::default().encoding("EUC-TW")` 或 `encoding:ISO-2022-KR` 校验。HZ 和 ISO-2022-KR 的转义与移位遵循 PHP mbstring，EUC-TW 检查 CNS 11643 第 1、2、14 平面的已分配位置。

四种移动版 UTF-8 按标准 UTF-8 校验字节。可用 `encoding:UTF-8-Mobile#DOCOMO` 或 `File::default().encoding("UTF-8-Mobile#SOFTBANK")`，另外两个名称为 `UTF-8-Mobile#KDDI-A` 和 `UTF-8-Mobile#KDDI-B`。校验保留原始字节中的私用区字符。

TIFF 和 ICO 上传可以使用 `file` 和 `dimensions:width=100,height=100` 一起验证。TIFF 从第一个图像目录取尺寸，ICO 从位深最高的条目取尺寸，位深相同时取最后一个。尺寸校验只读取元数据。`image` 规则排除 TIFF 和 ICO。

PSD 和 JPEG 2000 上传也可以使用这组规则。JP2 从第一个根级码流读取尺寸，裸 JPEG 2000 码流从 SIZ 标记读取尺寸。`image` 规则也排除这两种格式。

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

在验证器上调用 `validate_with_bag("profile")`，或覆盖 `FormRequest::error_bag`，即可指定错误包。在 Blade 中，`$errors->profile->name` 选择该包内 name 字段的消息。

### 排除敏感输入

密码字段会从旧输入中排除。可在 `with_exceptions` 中用 `dont_flash` 注册额外路径。

```rust
use openrust::foundation::configuration::Exceptions;

fn configure(exceptions: &mut Exceptions) {
    exceptions.dont_flash(["profile.secret"]);
}
```
## 标准规则与验证钩子

规则解析支持 `numeric`、`date`、`in:a,b`、`regex:/pattern/u`、`required_if:field,value`、`exclude_if:field,value`、`accepted`、`url`、`uuid`、`array:key1,key2`。`array:key1,key2` 拒绝清单外的键；`in` 与 `array` 组合时逐项检查数组。被排除的字段及上传文件不会出现在已验证数据中，规则按声明顺序执行。

默认 `integer` 遵循 PHP 整数验证，包含部分整数值浮点数和 `true`；`integer:strict` 只接受 JSON 整数，`boolean:strict` 只接受 JSON 布尔值。数值大小规则支持负数和小数阈值，使用十进制比较，避免浮点舍入。科学计数法指数超出 -1000 至 1000 时返回执行错误。

浮点数先按 PHP 默认的 14 位有效数字转为文本，例如浮点值 `1e13` 通过、`1e14` 不通过，而整数值 `100000000000000` 通过。`regex` 只接受字符串或数值；布尔值与未声明 `nullable` 的 `null` 不通过。

正则包含 `|` 时，使用 `RuleSet::field` 的规则数组并通过 `Rule::parse` 解析，避免被当成规则分隔符。支持 PCRE2 修饰符 `i m s x u A U J n D`；`D` 使 `$` 仅匹配输入末尾，多行模式下遵循 PCRE2 原生语义。其他尚不支持的修饰符返回 `InvalidRule`，不表示已经支持 PHP 的全部 PCRE 选项。

`Validator::after` 在规则检查后运行，可追加错误。`sometimes` 按条件追加规则，通配符回调收到所在父项。`stop_on_first_failure(true)` 在不同字段间停止检查，`bail` 只停止当前字段。`safe()` 必须在验证成功后调用，只暴露已验证的数据。

FormRequest 顺序为 `prepare_for_validation`、`authorize`、`with_validator`、验证和 `after`、`passed_validation`。授权或验证失败不会执行业务方法。携带 `Precognition-Validate-Only` 的预验证成功后不调用 `passed_validation`；未带该头时可能先调用，再短路业务方法。验证器按请求隔离。

## 使用验证后的数据 {#working-with-validated-input}

`Validator::safe()` 在验证成功后返回 `support::ValidatedInput`；`ValidatedFormRequest::safe()` 借用已验证的结果。`only`、`except` 按具体嵌套路径筛选，`merge` 返回新结果，`collect` 返回已有的 `Collection<RequestValue>`。原数据保持不变，借用和拥有式迭代均提供顶层键值对。

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

子集保留为类型化的 `ValidatedInput`，而不是无类型数组，以保留筛选和合并后的上传文件句柄。通过 `file()` 或类型化集合读取文件；JSON 投影将文件表示为 `null`，不公开本地路径。`to_array()` 和索引读取此 JSON 投影。`contracts::support::{ValidatedData, Arrayable, ArrayAccess}` 提供对应泛型契约。手动构造 `ValidatedInput::new` 不执行验证；不可信输入应通过请求或验证器处理。

## 枚举与值列表 {#rule-enum}

`Rule::r#enum::<E>()` 和 `rules::Enum<E>` 通过已有 `BackedEnum` 映射验证应用枚举。`only`、`except` 接收枚举成员；非空 `only` 列表优先。`when`、`unless`、`when_else` 按条件配置同一个规则；Rust 没有可选函数参数，`when_else` 显式传入两个分支。JSON 保持后备标量，不转换成 Rust 枚举对象。

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

`Rule::r#in`、`rules::In`、`Rule::not_in`、`rules::NotIn` 保留每个列表成员边界，包括逗号和引号。成员接收 `Into<serde_json::Value>`，复用已有标量转换；列表中的应用枚举显式映射为后备值。`r#` 是 Rust 关键字所需语法，不改变方法名。类型化规则与普通规则放入同一个数组时使用 `.into()`。

## 消息包 {#message-bags}

使用 `MessageBag` 收集有序字段错误，`ViewErrorBag` 管理命名包。`add` 去重，`merge(&other)` 保留重复。`first(key, format)` 在没有消息时返回空字符串。`get(key, format)` 返回 `MessageLookup::Exact` 或保留字段名的通配映射；仅在确实需要扁平消息时调用 `into_messages()`。

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

命名包计数及转发的只读方法只针对 `default` 包。读取缺失包不会登记它；用 `get_bag_mut` 修改已有包，或用 `put` 添加包。JSON 保存原始消息，不受显示格式影响；反序列化恢复默认格式。消息列表使用连续的 Rust 索引，JSON 选项遵循 Serde，不提供 PHP 数组索引、JSON 位标志或运行期动态方法分派。


## 标量与数字位数规则

使用 `ascii`、`alpha`、`alpha_dash` 和 `alpha_num` 限制字符。三个 alpha 规则接受 `ascii` 参数，默认支持 Unicode 字母和组合标记，后两者还接受数字。`list` 要求从零开始的连续整数键。地址格式使用 `ip`、`ipv4`、`ipv6` 和 `mac_address`；`hex_color` 接受以 `#` 开头的 3、4、6 或 8 位十六进制颜色。

`json` 验证标量中的 JSON 文本，解析器接受少于 128 层的容器嵌套。`digits:n`、`digits_between:min,max`、`min_digits:n` 和 `max_digits:n` 按标量转换后的 ASCII 数字计数，正负号及小数点不能通过数字位数规则。`decimal:n` 或 `decimal:min,max` 检查小数位数，不接受科学计数法。缺少必需参数或边界不是数字时返回 `InvalidRule`。这些规则沿用缺失字段、空白字符串和 `nullable` 行为；必须提供值时同时使用 `required`。

使用 `lowercase`、`uppercase` 检查 Unicode 大小写，不修改输入。`starts_with:a,b`、`ends_with:a,b`、`doesnt_start_with:a,b`、`doesnt_end_with:a,b` 接受字符串和数字，以任一非空参数匹配。`not_regex:/pattern/flags` 拒绝匹配的值，模式语法与 `regex` 相同；非法模式返回 `InvalidRule`。模式含管道字符时，通过 `RuleSet` 传入单独的 `Rule::parse` 结果。

## 字段存在与条件规则

`missing` 要求字段缺失，显式传入 `null` 也算存在。`filled` 允许缺失，但已提供的字段不能是空值。`declined` 接受 `no`、`off`、`0`、整数零、`false` 字符串或布尔假值。`prohibited` 允许缺失和空值，拒绝已填写的值。`same:other` 使用严格相等，`different:other,third` 对每个已提供的关联字段检查严格不等。

条件存在规则包括 `accepted_if`、`declined_if`、`missing_if`、`missing_unless`、`missing_with`、`missing_with_all`、`present_if`、`present_unless`、`present_with`、`present_with_all`。条件必填使用 `required_unless`、`required_with`、`required_with_all`、`required_without`、`required_without_all`、`required_if_accepted`、`required_if_declined`。存在检查把 null 视为存在；必填检查还考虑空字符串和空数组。条件参数中的通配索引按当前字段展开。

`prohibited_if`、`prohibited_unless`、`prohibited_if_accepted`、`prohibited_if_declined` 和 `prohibits` 按条件拒绝已填写的字段。`exclude`、`exclude_unless`、`exclude_with`、`exclude_without` 从验证结果移除匹配字段，并移除子字段错误和上传文件。排除按声明顺序执行，前面的必填失败可能提前停止字段校验。`nullable` 不跳过隐式的存在规则。

## 数组成员与格式规则

`array_keys:a,b` 限制允许的键；`required_array_keys:a,b` 要求全部键存在，`in_array_keys:a,b` 要求至少一个键存在。`contains:a,b` 对每个指定值使用宽松比较；`doesnt_contain:a,b` 以严格比较拒绝列出的字符串。`not_in:a,b` 反转 `in`；同时使用 `array` 时，反转的是整个数组的检查结果。

`in_array:pool.*` 对展开的候选字段使用严格相等检查。`distinct` 在声明的通配字段中拒绝重复值；`strict` 保留标量类型区别，`ignore_case` 对字符串使用 Unicode 正则大小写匹配。`date_format:Y-m-d,Y/m/d` 接受任一给定格式，并检查格式化结果与原输入是否相等，错误消息显示第一个格式。`ulid` 检查 26 个字符及首字符范围。

`multiple_of:0.1` 检查精确十进制余数，除数为零时始终失败。输入或除数的科学计数指数超出 -1000 至 1000 时返回执行错误。可通过 `:unexpected` 定制额外数组键的错误说明；`:Unexpected` 只将整段列表的首字符大写，`:UNEXPECTED` 将全部字母大写。

## 日期、大小与 DNS

`before:other`、`before_or_equal:other`、`after:other`、`after_or_equal:other` 和 `date_equals:other` 将日期与字面量或另一字段比较，使用应用时区和测试时钟。未声明 `date_format` 时按整秒比较，声明格式时纳入微秒。要求输入本身是有效日期时，同时使用 `date` 或 `date_format`。

`gt:other`、`lt:other`、`gte:other` 和 `lte:other` 比较数值、字符串字符数、数组项数或文件千字节数。数值输入也可与字面量阈值比较；字段值应同类型，双方均为数值时可跨数值类型。每条比较规则独立确定数值分类，不改变后续大小规则。

`timezone` 接受内置 IANA 2025b 数据中的精确标识。`timezone:all_with_bc` 包含兼容别名，`timezone:asia` 等参数限制地区，`timezone:per_country,CN` 按国家筛选。标识区分大小写，分组及国家参数不区分大小写。非法分组或国家参数长度返回 `InvalidRule`。

`active_url` 要求主机存在 DNS A 或 AAAA 记录，使用异步 DNS 查询，不因操作系统能解析数字 IP 就判为成功。`validator.set_dns_resolver(resolver)` 接受 Hickory 0.25 解析器，可指定域名服务器、超时及重试策略；DNS 查询失败即验证失败。测试时用 `validator.fake_dns_lookups(true)` 对当前验证器启用仅检查主机名的行为，传入 `false` 恢复实际 DNS 查询。开关按实例隔离，并发验证器互不影响。

## 修改验证器

用 `get_data`、`get_value` 和 `get_rules` 读取输入及展开后的规则。`get_rules` 按声明顺序返回 `(字段路径, 规则)`，路径使用原生转义。`set_value` 更新一个具体路径，拒绝通配符。`set_data` 替换输入，并恢复最近一次 `set_rules` 或工厂创建时的规则。`add_rules` 合并临时规则；`append_rules` 同时将合并后的声明作为下次重置的基准。

`set_custom_messages` 合并消息。`set_attribute_names`、`set_value_names` 替换对应映射；`add_custom_attributes`、`add_custom_values` 按字段合并。`get_translator`、`set_translator` 读取或替换错误消息使用的译器。修改后会丢弃缓存的成功结果和错误，需要重新执行 `validate().await`，再读取 `validated()` 或 `safe()`。

`get_exception`、`set_exception` 使用类型化函数 `fn(MessageBag, Option<&Translator>) -> ValidationException`，可定制实际拒绝响应的状态、错误包和重定向。固定返回类型替代运行期异常类名及继承，每个验证器独立保存工厂。

## 注册验证扩展

在服务提供者中用 `Validator::extend` 注册普通规则，`extend_implicit` 注册缺失或空值也必须执行的规则，`extend_dependent` 注册参数引用其他字段的规则。名称可用 `ProjectCode` 或 `project_code`，声明统一使用 `project_code`。工厂创建验证器时复制当前注册项，后续注册不会改变已创建的实例。

```rust
use openrust::support::facades::Validator;

Validator::extend("upper_code", |_, value, _, _| {
    Ok(value.and_then(serde_json::Value::as_str)
        .is_some_and(|value| value.chars().all(|ch| ch.is_ascii_uppercase())))
}, Some(":attribute 必须由大写字母组成。"))?;
```

回调接收字段名、`Option<&Value>`、参数和只读验证器，返回 `ValidationResult<bool>`。普通规则跳过缺失和空字符串；`nullable` 跳过 null，`sometimes` 仍限制缺失字段。隐式规则可检查缺失、空字符串及 null。依赖规则将 `items.*.other` 替换为当前项的具体路径。执行错误通过 `Err` 传播，不当作普通验证失败。异步规则继续使用 `Rule::custom` 和 `ValidationRule`。

用 `Validator::replacer` 注册消息替换回调，参数为消息、字段、规则名、参数列表及只读验证器，返回最终消息。先选择自定义或翻译消息并替换字段名称，再调用替换器；注册替换器后，由它处理 `:min` 等规则参数。`Factory::resolver` 接收并返回拥有的验证器，用于创建时的定制。Rust 使用闭包及显式解析的服务，不支持运行期 `Class@method` 字符串或动态验证器子类。

单个验证器可使用 `add_extension`、`add_implicit_extension`、`add_dependent_extension` 和 `add_replacer`；对应复数方法接受 `(String, Arc<回调>)`。这些修改会清除已有验证结果，需要再次验证。

工厂默认 `exclude_unvalidated_array_keys()`：父字段声明无参数 `array` 或 `list` 且有子字段规则时，只返回经过验证的子键。用 `include_unvalidated_array_keys()` 将之后创建的验证器切换为包含父字段全部键；调用 `exclude_unvalidated_array_keys()` 恢复默认。`array:name,email` 已显式限制允许键，不采用该裁剪策略。单个验证器的 `exclude_unvalidated_array_keys` 布尔字段可独立调整，`validated()` 和 `safe()` 随即采用新策略。

## 自定义消息与字段显示名 {#custom-validation-messages}

需要可选的消息和字段显示名时使用 `validate_with(rules, messages, attributes)`；具名错误包使用 `validate_with_bag_and_options(bag, rules, messages, attributes)`。Request 门面提供同样的入口；原有 `validate` 和 `validate_with_bag` 默认行为不变。Rust 用额外方法表达 PHP 的可选参数。

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

手动创建验证器时调用 `Validator::make_with(data, rules, messages, attributes)`。Form Request 可覆盖 `messages()` 和 `attributes()`，两者返回 `BTreeMap<String, String>`。这些入口复用应用验证工厂，保留 JSON 422、Web 重定向／错误包和 Precognition；文案只影响本次验证器，不修改共享翻译。

## 拒绝未知字段 {#failing-on-unknown-fields}

在 `FormRequest::fail_on_unknown_fields` 返回 `Some(true)`，拒绝正文中没有规则声明的字段。`Some(false)` 显式关闭此表单的策略，默认 `None` 继承应用默认值；应用默认关闭。

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

在 Provider 的 boot 阶段，通过 `app.container().make::<FormRequestDefaults>()?` 获取 `foundation::http::FormRequestDefaults`，调用 `fail_on_unknown_fields(true)`。这里以应用级状态替代上游进程静态开关，避免一个应用改变另一个应用。策略核对正文叶子键，支持嵌套、通配符声明和确认字段；查询参数、上传文件、`_token` 和 `_method` 不在此拒绝范围。只有 `array` 规则并不代表任意子键都被授权。失败复用已翻译的 `validation.prohibited`、JSON 422 或 Web 错误包；Precognition 也在成功响应前执行此检查。

具名错误包也可使用 `@error` 及其 `@else` 回退分支：

```blade
@error('title', 'article')
    <span>{{ $message }}</span>
@else
    <span>—</span>
@enderror
```

## 生成规则对象 {#creating-custom-rules}

使用不同名称，分别生成普通规则和隐式规则：

```sh
./artisan make:rule Uppercase
./artisan make:rule RequiredTitle --implicit
```

文件分别位于 `app/rules/uppercase.rs` 和 `app/rules/required_title.rs`。在 `app/rules/mod.rs` 声明模块，并在 `app/mod.rs` 导出 `rules` 模块；导入生成的类型后交给 `Rule::custom`。支持 `Admin/Uppercase` 等嵌套名称。`-i` 等同于 `--implicit`；`-f` / `--force` 覆盖已有文件。不带 force 的重名操作报告冲突并保留原文件。

执行 `./artisan stub:publish` 后，可通过 `stubs/rule.rs.stub` 与 `stubs/rule.implicit.rs.stub` 自定义生成模板。两种模板最初均返回空失败列表；隐式标志本身不代表字段必填。请在 `validate` 中实现拒绝逻辑，例如下文所示。模板使用模块限定导入，类型命名为 `RuleContext`、`Box`、`Option` 或 `Result` 也不会与内部名称冲突。

## 字段级闭包 {#using-closures}

直接向 `Rule::custom` 传入同步闭包，无需应用级注册。参数为当前规则上下文、已反转义的字段名和 `Option<&Value>`，返回 `RuleFailure` 列表。执行错误进入原有 HTTP 异常管线；规则对象保留既有异步 `ValidationRule::validate` 契约。

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

## 隐式规则对象 {#implicit-rules}

覆盖 `ValidationRule::is_implicit()`，在字段缺失、为空或 null 时也执行，包括 nullable 字段；`sometimes` 仍跳过缺失字段。标志本身不会拒绝输入，是否返回失败由规则决定。普通闭包跳过缺失字段、原始空字符串和 nullable null；正常 HTTP 的空串转 null 发生在请求验证前。`RuleContext::data` 借用本验证器的输入，不向共享规则对象复制状态。失败消息的翻译键使用验证器当前翻译器；本接口不提供ValidatorAware setter。

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

## 翻译规则失败消息 {#translating-validation-messages}

调用 `RuleFailure::translate`，传入当前 `RuleContext`、替换参数与可选语言；`None` 使用当前验证器语言。可使用 `lang/fr/rules.json` 中的 `rules.invalid` 等应用词条。`translate_choice(context, count, replacements, locale)` 额外通过现有翻译器选择复数分支。每条失败单独保存选择，不改变其他失败或请求语言。缺失词条保留键名，字面消息仍替换参数；显式翻译的消息不被通用规则消息覆盖。请使用 `RuleFailure::new` 构造失败，翻译标志不进入 HTTP 序列化响应。显式上下文借用原翻译器并立即解析消息，避免隐藏全局状态和延迟语言选择。

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

## 任一完整规则组 {#rule-any-of}

`Rule::any_of` 接收多组备选规则。一组中的全部规则通过即可，成功后不再执行后续组；`rules::AnyOf::new` 提供相同构造器。缺失字段也必须拒绝时，请在外层另加 `required`。

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

关联输入可传入由 `RuleSet` 构成的备选组，每组验证各自的字段映射。子验证器使用当前应用工厂，保留扩展、数据库存在性验证、自定义消息和字段显示名；嵌套组内的上传文件保留类型化句柄。检查规则组不会用子验证器过滤后的结果替换外层值；需要拒绝额外字段时，应声明允许的数组键。

此规则与其他验证门面一样，需要活动的应用门面作用域。空备选列表拒绝已提供的值；单个空规则组则通过。所有组失败时，在外层字段报告 `validation.any_of`，并支持该规则的自定义消息。

## 编译每项规则 {#accessing-nested-array-data}

`Rule::for_each` 返回 `NestedRules`。回调在创建验证器时执行，先于字段验证，接收当前值和展开后的属性名。返回 `Vec<Rule>` 表示当前值的规则，或返回字段相对于当前值的 `RuleSet`。替换整份数据或规则会重新编译，重复验证不会重复执行回调。

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

需要完整四个参数时，使用 `Rule::for_each_with` 或 `NestedRules::new`：当前值、属性名、收集后的扁平数据和父项上下文。扁平映射使用规范转义路径，字面点与嵌套路径不会互相覆盖。自定义编译器可实现 `contracts::validation::CompilableRules`，并调用 `Rule::compile`。返回列表中的数据库规则继续使用原工厂和存在性验证器。 需要按文件信息编译时，使用 `Rule::for_each_context` 或 `NestedRules::with_context`：`NestedRuleContext::file()` 读取原类型化文件，`data` 保留原始输入，`flattened` 保留当前收集的映射；其中 `attribute` 是规范转义路径。自定义编译器可通过 `CompilableRules::compile_context` 使用相同上下文。

编译后的字段保留通配符消息匹配、错误位置、Precognition 选择和 validated 字段过滤。既有不返回 Result 的 `set_data` / `set_rules` 会保留编译错误，下次 `get_rules` 或验证返回原错误种类；修正数据或规则后恢复。空的类型化映射仍序列化为 JSON 对象，不把 PHP 数组／对象歧义引入既有响应契约。
