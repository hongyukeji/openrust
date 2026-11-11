# 06-01 命名 URL、重定向和签名

- 核对固定 Laravel 的命名 URL、参数编码、签名与过期、重定向和本地临时文件机制。参考固定 Goravel 的 HTTP 重定向响应接口。复用 `v0.x` 的原始查询串验签、恒定时间比较、密钥轮换及固定 PHP 夹具，并移植重定向和签名 HTTP 用例的关键路径。 [Laravel URLs](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/urls.md) · [Laravel URL 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Routing/RoutingUrlGeneratorTest.php) · [Laravel 文件路由](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/FilesystemServiceProvider.php) · [Goravel Response](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/http/response.go)
- 路由装配后按生效路由建立应用级名称索引。请求上下文保留原始 URI，按需解析 Host，CLI 使用 `APP_URL`。支持具名、位置与嵌套参数、可选段、默认值、域名模板与模型主路由键。签名使用配置原值、查询顺序与前任密钥。常规验签失败返回 403。
- 重定向支持指定状态、命名路由、一次性 intended 会话状态与同源 Referer。开启 `serve` 的本地磁盘提供签名 GET 和 PUT，生产环境无效签名返回 404。未签名的现有公开静态文件仍按原挂载顺序服务。S3 临时链接由现有 AWS SDK 生成，上传时转发签名要求的请求头。
- Rust 使用有序类型表达 PHP 参数数组，并显式传入模型投影、时间和会话请求。缺少签名密钥时拒绝签发，`expires` 始终参与验签。HTTP URL 不读取转发头。动态控制器动作查找、自定义模型绑定列的自动投影、重定向链式 flash、S3 临时链接的通用选项和自定义磁盘签名回调尚未提供。
- 以 `407954e9` 为隔离基线。Rust 1.94.0 格式、464 项锁定离线工作区测试、全目标严格 Clippy 和 `./artisan inspire` 通过。`./artisan route:list --path=storage` 显示本地 GET／HEAD 与 PUT 路由。实际 HTTP 覆盖相对和绝对签名、篡改、过期、Host 改动、轮换、公开静态文件、局部磁盘上传下载、会话重定向和一次性 intended。分页链接在 SQLite 和真实 MySQL 8.4 上复验。
- 真实 RustFS 使用预签名 PUT 和 GET 完成无客户端凭据的上传及读取，去掉签名后读取失败。三语正文的同一 Rust 示例从文档抽取并通过实际 HTTP，VitePress 构建、12 项通用浏览器检查与 6 项专题浏览器检查通过。
