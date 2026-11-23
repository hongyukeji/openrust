# HTTP 请求

## 请求头 {#request-headers}

用 `request.header("X-Trace")?` 读取首个文本值，请求头缺失时返回 `None`。内核会将无效名称或非 UTF-8 值产生的错误渲染为 HTTP 400。用 `has_header` 检查是否存在，`headers()` 读取完整的请求头映射，`bearer_token()?` 从 Authorization 中提取 Bearer 令牌。

### 浏览器语言偏好 {#language-preferences}

`request.get_languages()` 按权重从高到低返回有效的 `Accept-Language` 范围，同权保留输入顺序。标签使用 BCP 47，例如 `en-US`、`zh-Hant`；`*` 保留为通配符。多个同名请求头合并处理，重复标签保留最高有效权重。无效标签、非法或重复的权重参数、`q=0` 条目会被忽略；权重必须在 0 到 1 之间，最多三位小数。没有有效范围时返回空列表。

```rust,ignore
let languages = request.get_languages();
let preferred = request.get_preferred_language(Some(&["en", "fr-FR"]))?;
```

`get_preferred_language` 按请求偏好依次匹配有序候选：先完整标签，再语言／文字体系／地区组合及基础语言。通配符或没有匹配时选择首个候选。传入 `None` 或空候选时，返回请求的首选范围，没有则为 `None`。候选使用其他语言接口相同的 BCP 47 校验；任意候选无效都会返回翻译错误，即使前面的候选已匹配。返回值是 `Result` 包装的独立 `Option<String>`。 这是偏好选择助手，不会自动返回 406；忽略的 `q=0` 条目不形成永久排除清单，无匹配时仍使用有序候选的首项。

这两个方法始终读取标准 `Accept-Language`，不受 `app.locale_header` 影响，不改变应用语言，也不依中文地区推断文字体系。业务、翻译和页面需要当前实际语言时使用 `App::current_locale()?`。全局 `SetLocale` 另按配置来源选择语言，更换其请求头不会改变 Request 标准方法；可通过 `middleware.set_locale(...)` 覆盖候选或关闭识别，见[应用语言协商](../digging-deeper/localization.md#locale-header)。标准解析结果按请求缓存，通过 `parts_mut()` 修改请求头时失效；自定义协商头复用解析器，但不使用该标准头缓存。

## 请求路径、主机和 URL {#request-path-host-and-method}

`path()` 移除路径两端的斜杠，`decoded_path()` 再进行百分号解码。用 `segments()` 或从 1 开始的 `segment(index)` 读取路径段。`is(pattern)` 匹配解码后的路径，`route_is(&["reports.*"])` 匹配当前路由名称，`is_method("GET")` 检查生效的 HTTP 方法。

`host()?`、`http_host()?` 和 `scheme_and_http_host()?` 使用生效的连接信息和可信代理设置。HTTP 主机包含非默认端口。`root()?` 还包含可信代理基础路径。`url()?` 返回去掉查询串的当前 URL。`full_url()?` 和 `full_url_is(patterns)?` 使用规范化查询串。用 `full_url_with_query(&serde_json::json!({"page": 2}))?` 或 `full_url_without_query(&["page"])?` 根据请求查询数据生成地址。这些方法返回新的 URL 字符串。签名验证使用原始请求目标。

## 获取输入

### 获取全部输入 {#retrieving-all-input-data}

用 `request.all().await?` 一起读取普通输入和上传文件。`RequestValue` 表示 JSON 标量、带整数或字符串键的有序集合，以及类型化的 `UploadedFile`。

普通输入与上传文件同名时，优先使用普通输入，显式 null 同样优先。嵌套集合保留两边各自独有的字段。上传文件仍可从文件集合读取。

用 `request.all_only(&["items.1.photo", "name"]).await?` 按路径选取字段，缺失字段保留为 null。用 `request.keys().await?` 依次列出普通输入键和文件键。两边都提供的键会出现两次。

### 获取单个输入值 {#retrieving-an-input-value}

字段名可以直接传入，已有 `Some(key)` 和 `None` 调用仍可使用。默认值分别用 `request.input_or("name", "访客").await?`、`request.query_or("page", "1")?` 和 `request.post_or("name", "访客").await?`。`input_or` 保留显式 null；`query_or`、`post_or` 在缺失或 null 时使用默认值。空字符串、零和 false 均保留。解析错误仍向上传播。读取查询参数优先用 `query`，不另加含义重叠的 `get` 别名。

```rust
let name = request.string_or("name", "访客").await?;
let page = request.integer_or("page", 1).await?;
let remember = request.boolean("remember").await?;
let keyword = request.query_or("keyword", "")?;
let title = request.post_or("title", "").await?;
```

用 `request.query("key")` 读取精确的顶层查询键。用 `request.input("team.name").await` 读取嵌套输入，传入 `None` 可读取整组输入。JSON 和表单的顶层字段优先于查询参数。只读取 JSON 正文时使用 `request.json`。

用 `request.post(None).await?` 读取正文参数集合，或用 `request.post("name").await?` 读取精确的顶层键。JSON 请求会读取解码后的正文。查询参数和 multipart 上传对象各自通过独立集合读取。显式 null 在完整集合中保留，按名称读取该值时返回 `None`。

用 `request.fluent(Some("user")).await?` 将一组普通输入包装为独立的 `Fluent` 属性集合。传入 `None` 可包装全部普通输入，字段缺失或为 null 时可用 `fluent_or("user", default)` 提供默认属性集合。标量无法构造属性集合，返回 HTTP 400。用 `fluent_only(&["user.name", "document"]).await?` 获取保留上传对象的独立类型化快照。用 `get` 读取路径，用 `value` 读取精确的顶层键。

用 `scope("user")` 获取独立的子属性集合。标量与文件用索引 `0` 包装。用 `set("user.name", RequestValue::Value(serde_json::json!("Ada")))?` 修改类型化属性集合。通配路径更新已有成员，原请求的输入不受影响。

OpenRust 成功读取正文后保留原始字节，供后续读取。默认上限为 8 MiB。输入无效返回 400，超量返回 413。

### 请求边界 {#request-boundaries}

路径经百分号解码后若不是有效 UTF-8，HTTP 内核返回 400。声明的 `Content-Length` 超过 8 MiB 时，框架在读取正文前返回 HTTP 413。框架为输入解析或 `body_bytes()` 收集正文时，实际读取量也以 8 MiB 为上限，包括分块传输的请求。

### 类型化输入

用 `request.integer("page").await?`、`request.float("price").await?`、`request.boolean("remember").await?` 或 `request.string("name").await?` 读取。缺失时分别使用 `0`、`0.0`、`false` 和空字符串。自定义默认值用 `integer_or("page", 1)`、`float_or("price", 1.25)`、`boolean_or("remember", true)` 或 `string_or("name", "访客")`，再加 `.await?`。显式 null 不会被普通默认值替换，而是按原有规则转换。

严格转换用 `integer_strict("value", 42)`、`float_strict("value", 1.25)`、`boolean_strict("value", true)` 或 `string_strict("label", "fallback")`。缺失或类型无效时采用默认值；严格模式不是表单验证，不会因此返回 422。点号路径和输入优先级保持不变。`str`、`str_or`、`str_strict` 分别是对应字符串方法的同义入口。

缺失值使用默认值。启用严格模式后，无效值也使用默认值。例如，`"01"` 无法作为严格整数读取，但可以作为浮点数读取。关闭严格模式后，数值按 PHP 强制转换，可读取字符串开头的数字部分。布尔读取忽略大小写，识别 `1`、`true`、`on`、`yes` 为真，`0`、`false`、`off`、`no` 为假。宽松模式下未识别的布尔输入返回 false。正文格式错误仍会返回输入错误。

用 `request.string_strict("label", "fallback").await?.trim()` 获取可链式操作的字符串，`request.str` 是同义入口。严格模式接受文本和数字。`request.array(Some("items")).await?` 保留列表和关联对象，将标量包成单项列表，缺失值或 null 返回空列表。`request.collect` 返回保留原键的集合。传入 `None` 可读取整组输入。

实现 `BackedEnum`，并以 `String` 或 `i64` 为 backing 类型。用 `request.r#enum::<Stage>("stage", None).await?` 读取单项，用 `request.enums::<Stage>("list").await?` 读取集合。集合保留原键和重复项，跳过未知 case。单项遇到未知 case 时采用给定默认值。无法转换的 backing 类型返回 HTTP 400。

用 `request.date("when", None, None).await?` 按应用时区解析日期。用 `request.date("formatted", Some("!d/m/Y H:i:s.u"), Some("Asia/Tokyo")).await?` 指定 PHP 格式和时区。缺失值、null 和空白返回 `None`，无效日期返回 HTTP 400。解析结果保留实际时区和微秒。

测试中，`TestApp::travel_to` 和冻结时间方法为相对日期及格式中省略的字段提供参考时刻。例如，`tomorrow` 从所用时区的冻结日期计算。格式以 `!` 开头时，省略的字段会重置。详见[控制测试时间](../testing/mocking.md#interacting-with-time)。

用 `request.interval("duration", None).await?` 读取 ISO 8601 区间或 `2 hours 30 minutes` 等表达式。字段以指定单位提供值时，使用 `request.interval("seconds", Some("second")).await?`。结果保留日历分量和微秒，显式传入的 90 秒仍在秒分量中。缺失或空白输入返回 `None`，无效区间或单位返回 HTTP 400。

### 通过动态属性读取输入 {#retrieving-input-via-dynamic-properties}

用 `request.field("name").await?` 执行框架动态请求属性的查找顺序。先读取合并后的输入和文件，显式 null 也算命中。字段缺失时检查当前匹配路由的同名绑定模型或原始参数。匹配 `RequestField::RouteModel(model)` 后用 `model.downcast::<T>()` 读取绑定模型。普通字段、上传文件和原始路由参数使用 `RequestField::Value`。

### 获取部分输入 {#retrieving-a-portion-of-the-input-data}

用 `request.only(&["user.name", "document"]).await?` 从普通输入和上传文件中选取字段。缺失路径会被省略，显式 null 和类型化文件会保留。`request.except(&["document"]).await?` 移除字段并保留剩余数字键。用 `array_only` 或 `collect_only` 将选中字段读为类型化数组或集合。

## Request 门面 {#request-facade}

无请求参数的路由回调可直接使用 `openrust::support::facades::Request`。门面借用当前的原始请求，读取和修改同一份输入、上传、用户与 Session，不会提前消费或复制请求体。

```rust
use openrust::{http::HttpError, support::facades::{Request, Route}};
use serde_json::json;

Route::post("/profile", || async {
    let name = Request::input("name").await?;
    Request::merge(json!({"verified": true})).await?;
    Ok::<_, HttpError>(json!({"name": name}))
})?;
```

控制器动作不需要请求参数时，用 `ControllerDispatcher::without_request(Controller::show)` 注册；其返回类型仍是 `HttpFuture<'_, Result<T, HttpError>>`。动作及其调用的服务可直接使用门面。显式接收 `&mut http::Request` 的处理器已独占借用请求，可以直接使用实例；需要调用使用门面的服务时，先用 `request.with_container(async { /* 调用服务 */ }).await` 借出请求，结束或取消后自动恢复。不要在同一次 `RequestHandle::with_mut`／`with_async` 访问中再借用门面，使用回调收到的请求。

| 功能 | 入口示例 |
| --- | --- |
| 元数据与 URL | `method`、`uri`、`path`、`header`、`full_url`、`get_port` |
| 输入与上传 | `input`、`all`、`only`、`file`、`all_files`、`merge`、`replace` |
| 类型化输入 | `integer`、`string`、`r#enum`、`date`、`interval`、`clamp` |
| 内容协商 | `accepts`、`prefers`、`get_format`、`get_preferred_format` |
| 会话与闪存 | `session`、`old`、`flash`、`flash_only`、`flush` |
| 用户与路由 | `user`、`route`、`route_parameter`、`set_user_resolver`、`set_route_resolver` |
| 验证与签名 | `validate`、`validate_with_bag`、`has_valid_signature` |
| 扩展 | `r#macro`、`mixin`、`has_macro`、`flush_macros`、`call` |

`session(|store| ...)`／`get_session(|store| ...)` 在一次安全借用内访问 Store；不要把引用带出回调。`route()` 返回可拥有的 `ResolvedRoute`，包含名称、元数据、参数和模型绑定；按名称读取参数或模型用 `route_parameter(name)`。`UserResolver` 和 `RouteResolver` 是显式类型的请求级回调。用户解析器接收可变请求及可选 Guard，返回异步 `HttpFuture<AuthResult<...>>`，可等待业务查询。`set_json` 只接受对象或数组，校验失败时保留原输入。`when_has`、`when_filled`、`when_missing`、`when_enum` 在分支未选中时返回 `None`，并在执行业务回调前释放内部请求借用。

`instance()` 和通用 `Facade::get_facade_root()` 返回 `RequestHandle`。用 `with`、`with_mut`、`with_async` 在受保护的借用内访问实例；句柄可在同一 HTTP 请求的多次借用之间复用，包括构造注入，但仅在请求被借出时可访问；HTTP 请求结束后失效。容器的 `request` 名称和 `RequestHandle` 类型也解析到当前句柄，不缓存跨请求状态。`Facade::swap(Arc::new(RequestHandle::from_request(request)))` 替换当前借用的请求，适用于显式重建和测试，不修改其他请求的容器绑定。

`get_payload()` 读取原始表单或 JSON 载荷，不混入 URL 查询参数；`get_content()` 返回有大小限制的原始字节。`uri()` 返回完整地址的 `support::Uri`，传输层的原始 URI 仍通过请求实例的 `uri()` 读取。请求语言设置使用 BCP 47，独立于 `App::set_locale` 的应用翻译语言；格式扩展也按请求隔离。`dump` 输出选定数据；`dd` 通过 HTTP 异常终止当前响应，不退出服务进程。

### 原生服务的配置与构造边界 {#request-facade-native-boundaries}

PHP 进程全局设置不复制成多请求共享的可变静态状态。对应的显式入口如下：

| 原机制 | OpenRust 入口 |
| --- | --- |
| 全局变量捕获、全局工厂、回写全局变量 | 服务器显式提供 `http::Request<Body>`，交给 `Request::capture`／`create`／`create_from_base`；自定义工厂使用普通函数或闭包，不读取或修改 PHP 全局变量 |
| 从框架请求构造 | `create_from` 转移 `http::Request` 的所有权，保留输入缓存、上传和 Session |
| 复制／重新初始化 | `duplicate().await` 导出独立 `http::Request<Bytes>` 传输快照；`initialize(native_request)` 替换当前借用的请求。快照保留原始请求体，不复制后续输入覆盖、会话、路由模型或中间件状态；需要原框架请求的状态时使用句柄，不把快照当作完整框架请求克隆 |
| 可信代理和主机的进程级设置及读取 | 在 `bootstrap/app.rs` 的 `with_middleware` 中持有并配置 `trust_proxies`／`trust_hosts`，或使用已有 `trustedproxy` 配置；不添加全局 Request 静态开关 |
| 方法覆写开关和允许列表 | 在装配闭包外用 `MethodOverride::new(parameter, allowed)` 构造并检查策略，再调用 `middleware.method_override(policy)`；`None` 允许现有安全规则下的目标方法，空列表关闭所有覆写。`parameter=false` 只关闭 `_method` 参数，允许列表仍约束覆写请求头；GET、HEAD、CONNECT、TRACE 不可加入允许列表 |

`get_http_method_parameter_override()` 和 `get_allowed_http_method_override()` 可读取当前应用的有效覆写策略。策略在每次请求路由匹配前应用，不会串到其他应用。没有 CGI 脚本入口的原生服务器中，`get_script_name()` 返回空字符串，基础路径来自可信代理前缀。`server(key)` 返回显式传输元数据，`REMOTE_ADDR` 保留原始连接地址；客户端地址用 `ip()`。

## 客户端 IP 地址 {#request-ip-address}

用 `request.ip()` 读取第一个解析出的客户端地址，用 `request.ips()` 读取地址链。直连时使用传输层提供的对端地址。配置可信代理后，地址链会过滤可信节点，从最近的非可信地址开始。若所有节点均可信，则返回链中的第一个可信地址。见[可信代理配置](#configuring-trusted-proxies)。

在当前 HTTP 请求内的控制器和服务中，也可以通过 IP 门面读取：

```rust
use openrust::support::facades::Request;

let ip = Request::ip()?;   // Option<std::net::IpAddr>
let ips = Request::ips()?; // Vec<std::net::IpAddr>
```

`Request::get_client_ip()` 和 `Request::get_client_ips()` 提供对应的 IP 入口，请求实例也提供这两个别名。它们读取 HTTP 内核按可信代理策略解析的连接信息。HTTP 请求缺少传输地址时返回 `None`／空列表；不在当前 HTTP 请求内时，门面返回 `BindingResolutionError::FacadeNotSet`。独立启动的任务不会继承此上下文，后台任务需要的地址应显式传递。其他入口与借用边界见 [Request 门面](#request-facade)。

## 内容协商 {#content-negotiation}

用 `request.get_acceptable_content_types()` 读取按质量权重排序的 `Accept` 条目。用 `request.accepts(&["text/html", "application/json"])` 检查是否接受任一类型，或用 `request.prefers(&["html", "json"])` 选择最佳候选。缺少该请求头或候选均不匹配时，`prefers` 返回 `None`。`request.format("html")` 返回首个识别出的响应格式，找不到时返回传入的默认值。

`accepts_any_content_type()`、`accepts_json()`、`accepts_html()` 和 `accepts_markdown()` 检查是否接受对应类型。`wants_json()` 和 `wants_markdown()` 检查排序第一的条目。`expects_json()` 也识别 `Accept` 为空或通配的非 PJAX XMLHttpRequest。`is_json()` 读取请求正文的 `Content-Type`。

重复发送 `Accept` 请求头时，按固定框架实现使用首个字段。需要发送多种类型时，请在一个字段中用逗号分隔。

### 原生 HTTP 请求 {#native-http-requests}

HTTP 服务将每个原生请求交给 `Kernel::handle_transport`，由它调用 `Request::from(native)` 建立框架请求。可信代理处理和方法覆盖都在本次请求的生命周期内执行。

与 Rust HTTP 库互操作时，用 `Request::from(native)` 接收拥有所有权的 `http::Request<Body>`，用 `into()` 转回原生请求。所有权转换会移动当前正文流、请求头、协议版本和扩展。在路由中用 `request.to_http_request().await?` 创建带缓冲正文的独立原生请求。它使用框架输入的 8 MiB 限额，框架请求仍可读取正文。

原生请求携带 `original_uri()` 返回的传输 URI，以及 `method()` 返回的生效方法，包括方法覆盖后的结果。用 `uri()` 读取可信代理处理后的框架 URI，用 `get_real_method()` 读取原始方法。`version()` 和 `extensions()` 读取对应的传输元数据。

返回原生类型的写法见[原生 HTTP 响应](./responses.md#native-http-responses)。

### 检查输入是否存在 {#input-presence}

用 `request.has(&["profile.name"]).await?` 检查键是否存在，显式设置为 `null` 的键也算存在。用 `has_any` 检查任意一个键，用 `missing` 检查是否有键缺失。这些检查也识别上传文件。

用 `filled`、`is_not_filled` 和 `any_filled` 检查输入值。缺失值、`null` 和空白字符串算空。布尔值、零和空数组算已填写。只有上传文件的字段可用 `has_file` 检查。

用 `when_has`、`when_filled` 或 `when_missing` 在条件成立时运行异步回调。`when_has` 和 `when_filled` 将合并后的字段作为 `Option<RequestValue>` 传给回调。匹配 `RequestValue::File(file)` 即可操作上传文件。回调执行时返回 `Some(result)`，可用 `unwrap_or_else` 提供默认结果。

### 合并额外输入 {#merging-additional-input}

用 `request.merge(serde_json::json!({"profile.name": "Ada"})).await?` 按点路径设置输入。`merge_if_missing` 保留请求已有的键，包括显式 `null`、查询回退值和上传文件。用 `replace` 传入对象或数组可替换当前输入来源。

JSON 请求更新 JSON 输入。非 JSON 的 GET 或 HEAD 更新查询数据。其他请求更新表单输入，并继续使用查询回退。这些方法保留原始 URI、原始正文和上传文件。

## 旧输入 {#old-input}

在使用会话中间件的路由中，用 `request.flash().await?` 保存当前输入供下一次请求读取。用 `request.flash_only(&["profile.name", "page"]).await?` 选取字段，或用 `request.flash_except(&["password", "password_confirmation"]).await?` 排除字段。字段选择支持点路径。上传文件保存在文件数据中。

用 `request.old(Some("profile.name"))` 读取旧输入的一个字段，或用 `request.old(None)` 获取整袋数据。先匹配字面键，再解析点路径。显式空值保留为 `Some(Value::Null)`，缺失时可用 `Option` 方法提供默认值。

用 `request.flush()?` 将旧输入替换为空。会话内的其他值仍可读取。旧输入在下一次请求结束后过期。

## Cookie {#cookies}

用 `request.cookie(Some("theme"))?` 读取指定 Cookie，用 `request.cookie(None)?` 读取整个 Cookie 集合。`request.has_cookie("theme")?` 检查 Cookie 是否存在，空字符串也算存在。

启用 `EncryptCookies` 后，受保护的 Cookie 会在进入处理器前解密并校验。校验失败时，整个集合仍保留该键，其值为 null。按名称读取会返回 `None`，调用方可用 `Option` 提供默认值。

## 输入修剪与规范化 {#input-trimming-and-normalization}

全局中间件先修剪查询、JSON 和表单中的字符串，再把空字符串转为 `null`。顶层 `current_password`、`password` 和 `password_confirmation` 跳过修剪，其中的空字符串仍会转为 `null`。原始 URI、正文和上传文件仍可读取。

在 `bootstrap/app.rs` 配置字段模式，或为特定请求跳过某一层中间件：

```rust
.with_middleware(|middleware| {
    middleware
        .trim_strings(["rows.*.secret"])
        .skip_trim_strings_when(|request| request.is("hooks/*"))
        .convert_empty_strings_to_null(|request| request.is("hooks/*"));
})
```

字段模式使用点路径和 `*` 通配符。例外仅对配置它的应用生效。

## 文件 {#files}

### 读取上传文件 {#retrieving-uploaded-files}

用 `request.file("attachments.0").await?` 读取单个 multipart 上传文件。`UploadedFile` 提供文件字节，以及客户端给出的文件名和媒体类型。保存前应校验文件内容。

用 `request.file_value("photos.*.image").await?` 读取一组文件或通配符匹配结果，用 `request.all_files().await?` 读取整个文件集合。两者用 `UploadedFileValue` 表示文件和带键集合。没有文件名的上传字段会被移除，有文件名的零字节文件仍可读取，保留其余文件的数字索引。

`request.has_file("attachments").await?` 检查单个文件或集合的直接成员。要检查更深层的文件，请指定嵌套字段或用通配符选取。

输入读取和 JSON 响应的完整示例见[控制器](./controllers.md)。

## 配置可信代理 {#configuring-trusted-proxies}

在应用装配时配置可信代理地址。来自非可信对端的转发头不会生效。策略接受 IPv4 和 IPv6 的精确地址与 CIDR。头部掩码指定允许代理设置的字段。

```rust
use openrust::http::TrustedProxyHeaders;

.with_middleware(|middleware| {
    middleware
        .trust_proxies(&["192.0.2.10", "10.20.0.0/16"], TrustedProxyHeaders::DEFAULT)
        .expect("valid trusted proxy policy");
})
```

默认掩码 `TrustedProxyHeaders::DEFAULT` 对应 X-Forwarded 字段。RFC 7239 使用 `FORWARDED`，AWS 负载均衡字段使用 `AWS_ELB`。同时启用的两组头部出现冲突时返回 HTTP 400。

`Request::secure` 反映生效的协议。URL 生成使用转发来源和前缀，路由匹配使用入站路径。

应用装配未指定代理策略时，HTTP 启动从配置仓库读取 `trustedproxy.proxies`，支持逗号分隔字符串或数组。可选的 `trustedproxy.headers` 支持数值掩码或 `HEADER_X_FORWARDED_AWS_ELB` 等框架预设名。显式 `trust_proxies` 优先，包括空地址列表。

## 配置可信 Host {#configuring-trusted-hosts}

启用 `trust_hosts` 可限制原始和转发后的主机名。模式使用不区分大小写的 PCRE 表达式，匹配去掉端口的主机名。未通过校验时，在进入路由前返回 HTTP 400。

```rust
.with_middleware(|middleware| {
    middleware
        .trust_hosts(&[r"^edge\.example\.com$"], true)
        .expect("valid host patterns");
})
```

启用 `subdomains` 可同时信任 `app.url` 的主机及其子域。用 `trust_hosts_with` 可在每次请求时异步获取模式。环境名恰好为 `local` 或 `testing` 时跳过 Host 校验。
## 请求元数据

`ajax()` 检查 `X-Requested-With: XMLHttpRequest`，`pjax()` 检查 `X-PJAX`。`prefetch()` 对 `X-Moz`、`Purpose`、`Sec-Purpose` 与 `prefetch` 进行忽略大小写的完整比较。预取请求不会覆盖 Session 上一页地址和路由。`user_agent()` 返回可选请求头值。`fingerprint()` 要求已匹配路由，按方法、域名、URI 和客户端 IP 生成摘要。通过 `user(guard).await` 使用指定 Guard 解析当前用户。
