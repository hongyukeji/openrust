# HTTP 请求

## 请求头 {#request-headers}

用 `request.header("X-Trace")?` 读取首个文本值，请求头缺失时返回 `None`。内核会将无效名称或非 UTF-8 值产生的错误渲染为 HTTP 400。用 `hasHeader` 检查是否存在，`headers()` 读取完整的请求头映射，`bearerToken()?` 从 Authorization 中提取 Bearer 令牌。

## 请求路径、主机和 URL {#request-path-host-and-method}

`path()` 移除路径两端的斜杠，`decodedPath()` 再进行百分号解码。用 `segments()` 或从 1 开始的 `segment(index)` 读取路径段。`is(pattern)` 匹配解码后的路径，`routeIs(&["reports.*"])` 匹配当前路由名称，`isMethod("GET")` 检查生效的 HTTP 方法。

`host()?`、`httpHost()?` 和 `schemeAndHttpHost()?` 使用生效的连接信息和可信代理设置。HTTP 主机包含非默认端口。`root()?` 还包含可信代理基础路径。`url()?` 返回去掉查询串的当前 URL。`fullUrl()?` 和 `fullUrlIs(patterns)?` 使用规范化查询串。用 `fullUrlWithQuery(&serde_json::json!({"page": 2}))?` 或 `fullUrlWithoutQuery(&["page"])?` 根据请求查询数据生成地址。这些方法返回新的 URL 字符串。签名验证使用原始请求目标。

## 获取输入

### 获取全部输入 {#retrieving-all-input-data}

用 `request.all().await?` 一起读取普通输入和上传文件。`RequestValue` 表示 JSON 标量、带整数或字符串键的有序集合，以及类型化的 `UploadedFile`。

普通输入与上传文件同名时，优先使用普通输入，显式 null 同样优先。嵌套集合保留两边各自独有的字段。上传文件仍可从文件集合读取。

用 `request.allOnly(&["items.1.photo", "name"]).await?` 按路径选取字段，缺失字段保留为 null。用 `request.keys().await?` 依次列出普通输入键和文件键。两边都提供的键会出现两次。

### 获取单个输入值 {#retrieving-an-input-value}

用 `request.query(Some("key"))` 读取精确的顶层查询键。用 `request.input(Some("team.name")).await` 读取嵌套输入，传入 `None` 可读取整组输入。JSON 和表单的顶层字段优先于查询参数。只读取 JSON 正文时使用 `request.json`。

用 `request.post(None).await?` 读取正文参数集合，或用 `request.post(Some("name")).await?` 读取精确的顶层键。JSON 请求会读取解码后的正文。查询参数和 multipart 上传对象各自通过独立集合读取。显式 null 在完整集合中保留，按名称读取该值时返回 `None`。

用 `request.fluent(Some("user")).await?` 将一组普通输入包装为独立的 `Fluent` 属性集合。传入 `None` 可包装全部普通输入，字段缺失或为 null 时可用 `fluentOr("user", default)` 提供默认属性集合。标量无法构造属性集合，返回 HTTP 400。用 `fluentOnly(&["user.name", "document"]).await?` 获取保留上传对象的独立类型化快照。用 `get` 读取路径，用 `value` 读取精确的顶层键。

用 `scope("user")` 获取独立的子属性集合。标量与文件用索引 `0` 包装。用 `set("user.name", RequestValue::Value(serde_json::json!("Ada")))?` 修改类型化属性集合。通配路径更新已有成员，原请求的输入不受影响。

OpenRust 成功读取正文后保留原始字节，供后续读取。默认上限为 8 MiB。输入无效返回 400，超量返回 413。

### 请求边界 {#request-boundaries}

路径经百分号解码后若不是有效 UTF-8，HTTP 内核返回 400。声明的 `Content-Length` 超过 8 MiB 时，框架在读取正文前返回 HTTP 413。框架为输入解析或 `body_bytes()` 收集正文时，实际读取量也以 8 MiB 为上限，包括分块传输的请求。

### 类型化输入

用 `request.integer("value", 42, true).await?`、`request.float("value", 1.25, true).await?` 或 `request.boolean("value", true, true).await?` 读取。参数依次是输入键、默认值和严格模式，点号路径和输入优先级与 `request.input` 相同。

缺失值使用默认值。启用严格模式后，无效值也使用默认值。例如，`"01"` 无法作为严格整数读取，但可以作为浮点数读取。关闭严格模式后，数值按 PHP 强制转换，可读取字符串开头的数字部分。布尔读取忽略大小写，识别 `1`、`true`、`on`、`yes` 为真，`0`、`false`、`off`、`no` 为假。宽松模式下未识别的布尔输入返回 false。正文格式错误仍会返回输入错误。

用 `request.string("label", "fallback", true).await?.trim()` 获取可链式操作的字符串，`request.str` 是同义入口。严格模式接受文本和数字。`request.array(Some("items")).await?` 保留列表和关联对象，将标量包成单项列表，缺失值或 null 返回空列表。`request.collect` 返回保留原键的集合。传入 `None` 可读取整组输入。

实现 `BackedEnum`，并以 `String` 或 `i64` 为 backing 类型。用 `request.r#enum::<Stage>("stage", None).await?` 读取单项，用 `request.enums::<Stage>("list").await?` 读取集合。集合保留原键和重复项，跳过未知 case。单项遇到未知 case 时采用给定默认值。无法转换的 backing 类型返回 HTTP 400。

用 `request.date("when", None, None).await?` 按应用时区解析日期。用 `request.date("formatted", Some("!d/m/Y H:i:s.u"), Some("Asia/Tokyo")).await?` 指定 PHP 格式和时区。缺失值、null 和空白返回 `None`，无效日期返回 HTTP 400。解析结果保留实际时区和微秒。

测试中，`TestApp::travelTo` 和冻结时间方法为相对日期及格式中省略的字段提供参考时刻。例如，`tomorrow` 从所用时区的冻结日期计算。格式以 `!` 开头时，省略的字段会重置。详见[控制测试时间](../testing/mocking.md#interacting-with-time)。

用 `request.interval("duration", None).await?` 读取 ISO 8601 区间或 `2 hours 30 minutes` 等表达式。字段以指定单位提供值时，使用 `request.interval("seconds", Some("second")).await?`。结果保留日历分量和微秒，显式传入的 90 秒仍在秒分量中。缺失或空白输入返回 `None`，无效区间或单位返回 HTTP 400。

### 通过动态属性读取输入 {#retrieving-input-via-dynamic-properties}

用 `request.field("name").await?` 执行 Laravel 动态请求属性的查找顺序。先读取合并后的输入和文件，显式 null 也算命中。字段缺失时检查当前匹配路由的同名绑定模型或原始参数。匹配 `RequestField::RouteModel(model)` 后用 `model.downcast::<T>()` 读取绑定模型。普通字段、上传文件和原始路由参数使用 `RequestField::Value`。

### 获取部分输入 {#retrieving-a-portion-of-the-input-data}

用 `request.only(&["user.name", "document"]).await?` 从普通输入和上传文件中选取字段。缺失路径会被省略，显式 null 和类型化文件会保留。`request.except(&["document"]).await?` 移除字段并保留剩余数字键。用 `arrayOnly` 或 `collectOnly` 将选中字段读为类型化数组或集合。

## 内容协商 {#content-negotiation}

用 `request.getAcceptableContentTypes()` 读取按质量权重排序的 `Accept` 条目。用 `request.accepts(&["text/html", "application/json"])` 检查是否接受任一类型，或用 `request.prefers(&["html", "json"])` 选择最佳候选。缺少该请求头或候选均不匹配时，`prefers` 返回 `None`。`request.format("html")` 返回首个识别出的响应格式，找不到时返回传入的默认值。

`acceptsAnyContentType()`、`acceptsJson()`、`acceptsHtml()` 和 `acceptsMarkdown()` 检查是否接受对应类型。`wantsJson()` 和 `wantsMarkdown()` 检查排序第一的条目。`expectsJson()` 也识别 `Accept` 为空或通配的非 PJAX XMLHttpRequest。`isJson()` 读取请求正文的 `Content-Type`。

重复发送 `Accept` 请求头时，按固定 Laravel 实现使用首个字段。需要发送多种类型时，请在一个字段中用逗号分隔。

### 原生 HTTP 请求 {#native-http-requests}

HTTP 服务将每个原生请求交给 `Kernel::handleTransport`，由它调用 `Request::from(native)` 建立框架请求。可信代理处理和方法覆盖都在本次请求的生命周期内执行。

与 Rust HTTP 库互操作时，用 `Request::from(native)` 接收拥有所有权的 `http::Request<Body>`，用 `into()` 转回原生请求。所有权转换会移动当前正文流、请求头、协议版本和扩展。在路由中用 `request.toHttpRequest().await?` 创建带缓冲正文的独立原生请求。它使用框架输入的 8 MiB 限额，框架请求仍可读取正文。

原生请求携带 `originalUri()` 返回的传输 URI，以及 `method()` 返回的生效方法，包括方法覆盖后的结果。用 `uri()` 读取可信代理处理后的框架 URI，用 `getRealMethod()` 读取原始方法。`version()` 和 `extensions()` 读取对应的传输元数据。

返回原生类型的写法见[原生 HTTP 响应](./responses.md#native-http-responses)。

### 检查输入是否存在 {#input-presence}

用 `request.has(&["profile.name"]).await?` 检查键是否存在，显式设置为 `null` 的键也算存在。用 `hasAny` 检查任意一个键，用 `missing` 检查是否有键缺失。这些检查也识别上传文件。

用 `filled`、`isNotFilled` 和 `anyFilled` 检查输入值。缺失值、`null` 和空白字符串算空。布尔值、零和空数组算已填写。只有上传文件的字段可用 `hasFile` 检查。

用 `whenHas`、`whenFilled` 或 `whenMissing` 在条件成立时运行异步回调。`whenHas` 和 `whenFilled` 将合并后的字段作为 `Option<RequestValue>` 传给回调。匹配 `RequestValue::File(file)` 即可操作上传文件。回调执行时返回 `Some(result)`，可用 `unwrap_or_else` 提供默认结果。

### 合并额外输入 {#merging-additional-input}

用 `request.merge(serde_json::json!({"profile.name": "Ada"})).await?` 按点路径设置输入。`mergeIfMissing` 保留请求已有的键，包括显式 `null`、查询回退值和上传文件。用 `replace` 传入对象或数组可替换当前输入来源。

JSON 请求更新 JSON 输入。非 JSON 的 GET 或 HEAD 更新查询数据。其他请求更新表单输入，并继续使用查询回退。这些方法保留原始 URI、原始正文和上传文件。

## 旧输入 {#old-input}

在使用会话中间件的路由中，用 `request.flash().await?` 保存当前输入供下一次请求读取。用 `request.flashOnly(&["profile.name", "page"]).await?` 选取字段，或用 `request.flashExcept(&["password", "password_confirmation"]).await?` 排除字段。字段选择支持点路径。上传文件保存在文件数据中。

用 `request.old(Some("profile.name"))` 读取旧输入的一个字段，或用 `request.old(None)` 获取整袋数据。先匹配字面键，再解析点路径。显式空值保留为 `Some(Value::Null)`，缺失时可用 `Option` 方法提供默认值。

用 `request.flush()?` 将旧输入替换为空。会话内的其他值仍可读取。旧输入在下一次请求结束后过期。

## Cookie {#cookies}

用 `request.cookie(Some("theme"))?` 读取指定 Cookie，用 `request.cookie(None)?` 读取整个 Cookie 集合。`request.hasCookie("theme")?` 检查 Cookie 是否存在，空字符串也算存在。

启用 `EncryptCookies` 后，受保护的 Cookie 会在进入处理器前解密并校验。校验失败时，整个集合仍保留该键，其值为 null。按名称读取会返回 `None`，调用方可用 `Option` 提供默认值。

## 输入修剪与规范化 {#input-trimming-and-normalization}

全局中间件先修剪查询、JSON 和表单中的字符串，再把空字符串转为 `null`。顶层 `current_password`、`password` 和 `password_confirmation` 跳过修剪，其中的空字符串仍会转为 `null`。原始 URI、正文和上传文件仍可读取。

在 `bootstrap/app.rs` 配置字段模式，或为特定请求跳过某一层中间件：

```rust
.withMiddleware(|middleware| {
    middleware
        .trimStrings(["rows.*.secret"])
        .skipTrimStringsWhen(|request| request.is("hooks/*"))
        .convertEmptyStringsToNull(|request| request.is("hooks/*"));
})
```

字段模式使用点路径和 `*` 通配符。例外仅对配置它的应用生效。

## 文件 {#files}

### 读取上传文件 {#retrieving-uploaded-files}

用 `request.file("attachments.0").await?` 读取单个 multipart 上传文件。`UploadedFile` 提供文件字节，以及客户端给出的文件名和媒体类型。保存前应校验文件内容。

用 `request.fileValue("photos.*.image").await?` 读取一组文件或通配符匹配结果，用 `request.allFiles().await?` 读取整个文件集合。两者用 `UploadedFileValue` 表示文件和带键集合。没有文件名的上传字段会被移除，有文件名的零字节文件仍可读取，保留其余文件的数字索引。

`request.hasFile("attachments").await?` 检查单个文件或集合的直接成员。要检查更深层的文件，请指定嵌套字段或用通配符选取。

输入读取和 JSON 响应的完整示例见[控制器](./controllers.md)。

## 配置可信代理 {#configuring-trusted-proxies}

在应用装配时配置可信代理地址。来自非可信对端的转发头不会生效。策略接受 IPv4 和 IPv6 的精确地址与 CIDR。头部掩码指定允许代理设置的字段。

```rust
use openrust::http::TrustedProxyHeaders;

.withMiddleware(|middleware| {
    middleware
        .trustProxies(&["192.0.2.10", "10.20.0.0/16"], TrustedProxyHeaders::DEFAULT)
        .expect("valid trusted proxy policy");
})
```

默认掩码 `TrustedProxyHeaders::DEFAULT` 对应 X-Forwarded 字段。RFC 7239 使用 `FORWARDED`，AWS 负载均衡字段使用 `AWS_ELB`。同时启用的两组头部出现冲突时返回 HTTP 400。

`Request::ip` 返回链中离应用最近的非可信客户端地址，`Request::ips` 返回解析出的地址。`Request::secure` 反映有效协议。URL 生成使用转发后的来源与前缀，路由匹配使用实际收到的路径。

应用装配未指定代理策略时，HTTP 启动从配置仓库读取 `trustedproxy.proxies`，支持逗号分隔字符串或数组。可选的 `trustedproxy.headers` 支持数值掩码或 `HEADER_X_FORWARDED_AWS_ELB` 等 Laravel 预设名。显式 `trustProxies` 优先，包括空地址列表。

## 配置可信 Host {#configuring-trusted-hosts}

启用 `trustHosts` 可限制原始和转发后的主机名。模式使用不区分大小写的 PCRE 表达式，匹配去掉端口的主机名。未通过校验时，在进入路由前返回 HTTP 400。

```rust
.withMiddleware(|middleware| {
    middleware
        .trustHosts(&[r"^edge\.example\.com$"], true)
        .expect("valid host patterns");
})
```

启用 `subdomains` 可同时信任 `app.url` 的主机及其子域。用 `trustHostsWith` 可在每次请求时异步获取模式。环境名恰好为 `local` 或 `testing` 时跳过 Host 校验。
