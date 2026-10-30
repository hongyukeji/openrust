# ADR-078：只读请求检查与内容协商

- 状态：已接受；C04-A 适用验收通过
- 日期：2026-10-29
- 对应：L019.02；不关闭完整 Request/Response 范围

## 决策

公开 `openrust::http::RequestInfo`，实现 `FromRequestParts<S>`，保存当前请求的只读元信息；与现有 `Json<T>`、`Query<T>`、`Path<T>`、`ClientIp` 组合使用。Rust body 只能被一个提取器拥有，因此不为读取 headers 而缓存或复制 body，不创建伪装成完整 Laravel Request 的空壳。原生 Axum Request 继续作为连接/正文逃生口；动态输入、Session、用户、文件与可变输入语义由后继 C04/R7 完成。

1. `path` 使用 OriginalUri（缺失时使用当前 URI），按 Laravel 去首尾斜线、根为 `/`，不自动百分号解码；`method` 返回 Rust Method，`is_method` 忽略大小写。`uri` 保留原始 URI 的公开只读访问。
2. `host` 复用域路由的实际 authority 校验，缺失、重复、畸形或 Host/URI 冲突为 400，不采信 Forwarded/X-Forwarded-Host。此簇不新增代理信任配置；IP 继续使用 ClientIp，URL 生成继续使用既有登记模板与 app.url。
3. `header(name) -> Result<Option<&str>>` 和 `has_header` 使用首个同名 header；`headers` 提供只读 HeaderMap 以保留二进制头部访问。文本值不是 UTF-8 时稳定返回 `http_request_header_invalid`，不回显值。`bearer_token` 对首个 Authorization 按固定上游提取，不验证凭据，也不替代 Auth；Debug 不输出 header、token、URI/query。
4. 内容协商方法归 RequestInfo：`acceptable_content_types`、`accepts`、`prefers`、`accepts_any_content_type`、`accepts_json/html/markdown`、`wants_json/markdown`、`expects_json`、`is_json`、`ajax`、`pjax`、`format(default)`。内部解析器不建立第二套公共协议接口。
5. 以 Laravel v13.39.0 方法行为及其允许的 Symfony HttpFoundation v8.0.0 解析为固定比较点：质量降序、相同质量保留声明顺序、带引号参数和重复项规范键；不自行丢弃 q=0。accepts/prefers 的匹配方向、空 Accept、通配符、结构化后缀、AJAX/PJAX、JSON 判定和默认 MIME format 表分别按来源对应，不能用“更符合 RFC”悄悄替换上游行为。prefers 支持默认格式别名；空候选返回 None，不 panic。
6. Rust 常驻进程明确限制快照 header 总量 64 KiB/128 个值、Accept 文本 16 KiB/128 项；超限为 431 `http_request_headers_too_large`，无截断或静默放宽。此为资源边界适配，记录与上游无限制 helper 的差异。无新增依赖、全局可变 format 表或新配置。

## 交付与验收

真实模块业务 endpoint 按协商返回 HTML/JSON 并发送 Vary: Accept；请求快照与 Json 同时提取，证明 body 未消费。生成项目公开 consumer、真实 TCP、资源限制/敏感 Debug、非法 authority、q/通配/参数/重复头与格式别名均有行为证据。原统一 JSON envelope 的默认/错误响应策略本簇保持已定 contract，不因增加协商 helper 自动改写全部异常渲染。FormRequest、Cookie/Session、redirect/flash、CSRF、文件验证和其他 L020 行为继续保留后继范围。

## 固定来源

- [Laravel Request v13.39.0](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Http/Request.php)
- [Laravel InteractsWithContentTypes v13.39.0](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Http/Concerns/InteractsWithContentTypes.php)
- [Laravel InteractsWithInput v13.39.0](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Http/Concerns/InteractsWithInput.php)
- [Symfony AcceptHeader v8.0.0](https://github.com/symfony/http-foundation/blob/v8.0.0/AcceptHeader.php)、[AcceptHeaderItem](https://github.com/symfony/http-foundation/blob/v8.0.0/AcceptHeaderItem.php)、[Request](https://github.com/symfony/http-foundation/blob/v8.0.0/Request.php)


## C04-D6：请求地址与查询串契约

D6限定子范围已通过实际验收；不关闭完整Request或Session范围。公开`RequestInfo`的`scheme`、`port`、`http_host`、`root`、`url`、`full_url`、`decoded_path`、`segments`、`segment`、`is`、`full_url_is`与`prefetch`；受管理`Request`经Deref复用。`full_url_with_query`、`full_url_without_query`属于拥有唯一当前query bag的`Request`，不在只读Info上伪造可变输入。

- `full_url`以`OriginalUri`原始查询串作HeaderUtils规范化：保顶层点/空格、同键后覆盖、仅顶层按PHP常规键比较排序，再RFC3986编码。with/without从已规范输入bag取值，GET/HEAD的merge/replace可见；正文输入不混入query。增参使用顶层array_merge与数字键重排，删参精确顶层键优先、再按点路径删除；不建第二持久bag。
- 传输安全在Parts阶段按既有`RequestSecurity`/可信peer与proto规则保存快照。绝对URI的https不是TLS证明，未信任转发头不生效。authority沿现严格Host/URI一致性校验并保留端口；没有Host和URI authority不凭app.url或localhost补值。可信代理Host/Port/Prefix的完整对应仍属后继TrustProxies，不能用当前仅proto规则冒称完成。
- `decoded_path`按raw百分号解码，`+`不变；segments解码后拆斜线、仅滤空段、索引从1开始，Rust用Option表达越界/缺省。非法UTF-8显式400而非有损替换，此为Rust字符串表示适配。原始`uri`继续提供未解码访问。
- query原文16KiB/1024字段与既有树限额继续生效；编码/完整URL上限128KiB，超限413；星号匹配最多128模式、合计16KiB，不引入正则回溯。以上是常驻进程资源边界适配，超出不声称与PHP无限制helper字节等价。
- prefetch仅在三个对应头任一首值与prefetch ASCII不敏感全值相等时成立。真实matched Route、Session previous及已确认Precognition均为后继；不从URL/命名生成器/用户header反推。

来源：固定[Laravel Request](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Http/Request.php)及其HttpRequestTest。该framework仅声明HttpFoundation `^7.4.0 || ^8.0.0`，未提供已解析依赖锁；独立固定[Symfony 7.4.0](https://github.com/symfony/http-foundation/blob/769c1720b68e964b13b58529c17d4a385c62167b/Request.php)与[8.0.0](https://github.com/symfony/http-foundation/blob/19f925ff62437970e56fcdf793aa93607622d8be/Request.php)，相关query/URI方法与实际测试文本核对一致。源码审计不等于执行过PHP测试。

D6实现复用原Input变量注册树：真实PHP参考证明根hex保护后嵌套Cookie前缀过滤仍生效，不保留额外UrlQuery模式。两线真实PHP差分、唯一模块/生成应用、完整门禁及真实双库/网络限定证据见[testing](../memory/testing.md#c04-d6-request-url限定子范围已验收)。
