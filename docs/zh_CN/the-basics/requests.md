# HTTP 请求

## 获取输入

用 `request.query(Some("key"))` 读取精确的顶层查询键。用 `request.input(Some("team.name")).await` 读取嵌套输入，传入 `None` 可读取整组输入。JSON 和表单的顶层字段优先于查询参数。只读取 JSON 正文时使用 `request.json`。

OpenRust 成功读取正文后保留原始字节，供后续读取。默认上限为 8 MiB。输入无效返回 400，超量返回 413。

### 类型化输入

用 `request.integer("value", 42, true).await?`、`request.float("value", 1.25, true).await?` 或 `request.boolean("value", true, true).await?` 读取。参数依次是输入键、默认值和严格模式，点号路径和输入优先级与 `request.input` 相同。

缺失值使用默认值。启用严格模式后，无效值也使用默认值。例如，`"01"` 无法作为严格整数读取，但可以作为浮点数读取。关闭严格模式后，数值按 PHP 强制转换，可读取字符串开头的数字部分。布尔读取忽略大小写，识别 `1`、`true`、`on`、`yes` 为真，`0`、`false`、`off`、`no` 为假。宽松模式下未识别的布尔输入返回 false。正文格式错误仍会返回输入错误。

用 `request.string("label", "fallback", true).await?.trim()` 获取可链式操作的字符串，`request.str` 是同义入口。严格模式接受文本和数字。`request.array(Some("items")).await?` 保留列表和关联对象，将标量包成单项列表，缺失值或 null 返回空列表。`request.collect` 返回保留原键的集合。传入 `None` 可读取整组输入。

实现 `BackedEnum`，并以 `String` 或 `i64` 为 backing 类型。用 `request.r#enum::<Stage>("stage", None).await?` 读取单项，用 `request.enums::<Stage>("list").await?` 读取集合。集合保留原键和重复项，跳过未知 case。单项遇到未知 case 时采用给定默认值。无法转换的 backing 类型返回 HTTP 400。

用 `request.date("when", None, None).await?` 按应用时区解析日期。用 `request.date("formatted", Some("!d/m/Y H:i:s.u"), Some("Asia/Tokyo")).await?` 指定 PHP 格式和时区。缺失值、null 和空白返回 `None`，无效日期返回 HTTP 400。解析结果保留实际时区和微秒。

测试中，`TestApp::travelTo` 和冻结时间方法为相对日期及格式中省略的字段提供参考时刻。例如，`tomorrow` 从所用时区的冻结日期计算。格式以 `!` 开头时，省略的字段会重置。详见[控制测试时间](../testing/mocking.md#interacting-with-time)。

用 `request.only(&["user.name", "user.note"]).await?` 选取嵌套输入，并保留显式 null。`request.except` 从输入副本移除指定路径。`arrayOnly` 和 `collectOnly` 分别对应 `array` 和 `collect` 的多键读取。这些方法选择输入值，上传文件用 `request.file` 读取。

用 `request.interval("duration", None).await?` 读取 ISO 8601 区间或 `2 hours 30 minutes` 等表达式。字段以指定单位提供值时，使用 `request.interval("seconds", Some("second")).await?`。结果保留日历分量和微秒，显式传入的 90 秒仍在秒分量中。缺失或空白输入返回 `None`，无效区间或单位返回 HTTP 400。

## 文件

用 `request.file("attachments.0").await` 读取 multipart 上传文件，用 `request.hasFile("attachments").await` 检查字段是否包含文件。上传对象提供文件字节以及客户端给出的文件名和媒体类型。保存前应检查文件内容。

输入读取和 JSON 响应的完整示例见[控制器](./controllers.md)。

## Cookie

用 `request.cookie(Some("theme"))?` 读取指定 Cookie，用 `request.cookie(None)?` 读取整个 Cookie 集合。启用 `EncryptCookies` 后，受保护的 Cookie 会在进入处理器前解密并校验。密文无效或名称绑定不符时，读取到的值为 null。

## 旧输入

用 `request.old(Some("profile.name"))` 读取嵌套旧输入，用 `request.old(None)` 读取整个旧输入集合。闪存值在下一次请求可用，显式 null 会保留。

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
