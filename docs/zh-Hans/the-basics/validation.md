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

## 消息包 {#message-bags}

使用 `MessageBag` 收集有序字段错误，`ViewErrorBag` 管理命名包。`add` 去重，`merge(&other)` 保留重复。`first(key, format)` 在没有消息时返回空字符串。`get(key, format)` 返回 `MessageLookup::Exact` 或保留字段名的通配映射；仅在确实需要扁平消息时调用 `intoMessages()`。

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

命名包计数及转发的只读方法只针对 `default` 包。读取缺失包不会登记它；用 `getBagMut` 修改已有包，或用 `put` 添加包。JSON 保存原始消息，不受显示格式影响；反序列化恢复默认格式。消息列表使用连续的 Rust 索引，JSON 选项遵循 Serde，不提供 PHP 数组索引、JSON 位标志或运行期动态方法分派。


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

`active_url` 要求主机存在 DNS A 或 AAAA 记录，使用异步 DNS 查询，不因操作系统能解析数字 IP 就判为成功。`validator.setDnsResolver(resolver)` 接受 Hickory 0.25 解析器，可指定域名服务器、超时及重试策略；DNS 查询失败即验证失败。测试时用 `validator.fakeDnsLookups(true)` 对当前验证器启用仅检查主机名的行为，传入 `false` 恢复实际 DNS 查询。开关按实例隔离，并发验证器互不影响。

## 修改验证器

用 `getData`、`getValue` 和 `getRules` 读取输入及展开后的规则。`getRules` 按声明顺序返回 `(字段路径, 规则)`，路径使用原生转义。`setValue` 更新一个具体路径，拒绝通配符。`setData` 替换输入，并恢复最近一次 `setRules` 或工厂创建时的规则。`addRules` 合并临时规则；`appendRules` 同时将合并后的声明作为下次重置的基准。

`setCustomMessages` 合并消息。`setAttributeNames`、`setValueNames` 替换对应映射；`addCustomAttributes`、`addCustomValues` 按字段合并。`getTranslator`、`setTranslator` 读取或替换错误消息使用的译器。修改后会丢弃缓存的成功结果和错误，需要重新执行 `validate().await`，再读取 `validated()` 或 `safe()`。

`getException`、`setException` 使用类型化函数 `fn(MessageBag, Option<&Translator>) -> ValidationException`，可定制实际拒绝响应的状态、错误包和重定向。固定返回类型替代运行期异常类名及继承，每个验证器独立保存工厂。

## 注册验证扩展

在服务提供者中用 `Validator::extend` 注册普通规则，`extendImplicit` 注册缺失或空值也必须执行的规则，`extendDependent` 注册参数引用其他字段的规则。名称可用 `ProjectCode` 或 `project_code`，声明统一使用 `project_code`。工厂创建验证器时复制当前注册项，后续注册不会改变已创建的实例。

```rust
use openrust::support::facades::Validator;

Validator::extend("upper_code", |_, value, _, _| {
    Ok(value.and_then(serde_json::Value::as_str)
        .is_some_and(|value| value.chars().all(|ch| ch.is_ascii_uppercase())))
}, Some(":attribute 必须由大写字母组成。"))?;
```

回调接收字段名、`Option<&Value>`、参数和只读验证器，返回 `ValidationResult<bool>`。普通规则跳过缺失和空字符串；`nullable` 跳过 null，`sometimes` 仍限制缺失字段。隐式规则可检查缺失、空字符串及 null。依赖规则将 `items.*.other` 替换为当前项的具体路径。执行错误通过 `Err` 传播，不当作普通验证失败。异步规则继续使用 `Rule::custom` 和 `ValidationRule`。

用 `Validator::replacer` 注册消息替换回调，参数为消息、字段、规则名、参数列表及只读验证器，返回最终消息。先选择自定义或翻译消息并替换字段名称，再调用替换器；注册替换器后，由它处理 `:min` 等规则参数。`Factory::resolver` 接收并返回拥有的验证器，用于创建时的定制。Rust 使用闭包及显式解析的服务，不支持运行期 `Class@method` 字符串或动态验证器子类。

单个验证器可使用 `addExtension`、`addImplicitExtension`、`addDependentExtension` 和 `addReplacer`；对应复数方法接受 `(String, Arc<回调>)`。这些修改会清除已有验证结果，需要再次验证。

工厂默认 `excludeUnvalidatedArrayKeys()`：父字段声明无参数 `array` 或 `list` 且有子字段规则时，只返回经过验证的子键。用 `includeUnvalidatedArrayKeys()` 将之后创建的验证器切换为包含父字段全部键；调用 `excludeUnvalidatedArrayKeys()` 恢复默认。`array:name,email` 已显式限制允许键，不采用该裁剪策略。单个验证器的 `excludeUnvalidatedArrayKeys` 布尔字段可独立调整，`validated()` 和 `safe()` 随即采用新策略。
