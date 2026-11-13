# 06-01 命名 URL、重定向和签名

- 核对固定 Laravel 的命名 URL、参数编码、签名与过期、重定向和本地临时文件机制。参考固定 Goravel 的 HTTP 重定向响应接口。复用 `v0.x` 的原始查询串验签、恒定时间比较、密钥轮换及固定 PHP 夹具，并移植重定向和签名 HTTP 用例的关键路径。 [Laravel URLs](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/urls.md) · [Laravel URL 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Routing/RoutingUrlGeneratorTest.php) · [Laravel 文件路由](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/FilesystemServiceProvider.php) · [Goravel Response](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/http/response.go)
- 路由装配后按生效路由建立应用级名称索引。请求上下文保留原始 URI，按需解析 Host，CLI 使用 `APP_URL`。支持具名、位置与嵌套参数、可选段、默认值、域名模板与模型主路由键。签名使用配置原值、查询顺序与前任密钥。常规验签失败返回 403。
- 重定向支持指定状态、命名路由、一次性 intended 会话状态与同源 Referer。开启 `serve` 的本地磁盘提供签名 GET 和 PUT，生产环境无效签名返回 404。未签名的现有公开静态文件仍按原挂载顺序服务。S3 临时链接由现有 AWS SDK 生成，上传时转发签名要求的请求头。
- Rust 使用有序类型表达 PHP 参数数组，并显式传入模型投影、时间和会话请求。缺少签名密钥时拒绝签发，`expires` 始终参与验签。HTTP URL 不读取转发头。S3 临时链接的通用选项和自定义磁盘签名回调尚未提供。
- 以 `407954e9` 为隔离基线。Rust 1.94.0 格式、464 项锁定离线工作区测试、全目标严格 Clippy 和 `./artisan inspire` 通过。`./artisan route:list --path=storage` 显示本地 GET／HEAD 与 PUT 路由。实际 HTTP 覆盖相对和绝对签名、篡改、过期、Host 改动、轮换、公开静态文件、局部磁盘上传下载、会话重定向和一次性 intended。分页链接在 SQLite 和真实 MySQL 8.4 上复验。
- 真实 RustFS 使用预签名 PUT 和 GET 完成无客户端凭据的上传及读取，去掉签名后读取失败。三语正文的同一 Rust 示例从文档抽取并通过实际 HTTP，VitePress 构建、12 项通用浏览器检查与 6 项专题浏览器检查通过。

## 重定向闪存

- 按固定 Laravel `RedirectResponse` 接入 `with`、`withInput`、`onlyInput`、`exceptInput` 和 `withErrors`。复用旧版 `v0.x` 的独立重定向类型和输入筛选契约。Rust 显式借用可变请求，以现有 Session Store 写入闪存。输入按需异步解析，上传文件由现有输入解析器单独存放。错误使用已有 `MessageBag`。处理器以 `.into()` 转换为 HTTP `Response`。 [Laravel RedirectResponse](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/RedirectResponse.php) · [Laravel 闪存重定向文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md#redirecting-with-flashed-session-data)
- 真实 HTTP 验收使用默认 Web 中间件和加密会话 Cookie。带 CSRF 令牌的请求验证嵌套字段选择和排除、显式输入、查询参数、多命名错误包、独立会话隔离、下一请求可见和再下一请求清理。multipart 文件不写入闪存。原有签名、临时本地文件、命名路由和 intended 回归同时通过。

## 控制器动作 URL

- 按固定 Laravel 的 `UrlGenerator::action`、`RouteCollection::refreshActionLookups` 与旧版动作索引实现。生效路由按应用建立动作索引，域名路由优先，同类取首条注册。命名路由与动作 URL 共用参数解析和编码。Rust 参考 Goravel 的编译型处理器注册，通过 `Route::setAction` 显式关联动作标识。此接口使用字符串标识。PHP 类／方法数组和根控制器命名空间推断仍有差异。 [Laravel UrlGenerator](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/UrlGenerator.php) · [Laravel RouteCollection](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/RouteCollection.php) · [Goravel Router](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/route/route.go)
- 真实 HTTP 验证绝对和相对动作 URL、参数编码、动作重定向及目标访问。额外核对域名优先、路由覆盖、缺少动作与参数、两个应用隔离。三语示例编译、站点构建、浏览器验收和专项严格 Clippy 通过。

## 模型 URL 的自定义绑定列

- 按固定 Laravel `RouteUrlGenerator` 的模型参数投影和路由测试，具名与位置模型参数在填入占位符时选择指定列，无指定列时使用默认键。旧版 `v0.x` 的 URL 生成器使用显式字符串参数，未找到对应的自动投影实现。Goravel 的编译型路由登记继续作为装配参考。 [Laravel RouteUrlGenerator](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/RouteUrlGenerator.php) · [Laravel 路由测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Routing/RoutingUrlGeneratorTest.php)
- Rust 参数保留已加载的标量属性快照与默认键。生成 URL 时无需再次查询数据库。字段缺失或非标量时，必填占位符报缺少参数。Eloquent 使用已加载的原始标量属性，PHP 动态访问器和延迟加载仍有差异。
- 实际 HTTP 与 SQLite、MySQL 8.4.13 覆盖默认 slug、自定义 id、具名与位置模型参数、动作 URL、缺失列、跟随生成链接、签名成功和篡改拒绝。原有 URL 与绑定回归同时运行。
