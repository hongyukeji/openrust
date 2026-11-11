# 05-09 Gate 和 Policy

- 对照固定 Laravel 的 Gate、Policy、访客授权、路由授权和拒绝状态。参考固定 Goravel 的 Gate 上下文与回调组织。复用旧版的请求 Guard 作用域和原始身份键设计，旧版数据库 RBAC 继续留在其交付范围。 [Laravel 授权文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/authorization.md) · [Laravel Gate](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/Access/Gate.php) · [Laravel Authorize](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/Middleware/Authorize.php) · [Goravel Gate](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/auth/access/gate.go)
- 应用容器持有 Gate 注册表，请求通过当前 Guard 解析用户。策略按 Rust 类型显式注册，`PolicyTarget` 支持无实例策略。Gate 的 `before` 可短路，`after` 补齐未决结果。路由 `can` 排在认证与模型绑定之后。FormRequest 共用拒绝状态转换。
- Rust 适配采用显式 Request 和编译期类型，策略模块生成后由应用接入。PHP 的反射式策略发现、Blade 授权指令及 Gate 判定事件尚未接入。
- 以 `12bea048` 为隔离基线，Rust 1.94.0 格式检查、459 项锁定离线测试、全目标严格 Clippy 和 `./artisan inspire` 通过。SQLite 和真实 MySQL 8.4 的 HTTP 验收覆盖访客、已认证用户、认证先于模型查找、模型缺失 404、策略优先、回调、409 拒绝、隐藏为 404、请求并发与应用隔离。
- `./artisan make:policy` 实测有模型和无模型生成、拒绝重复写入、强制覆盖与拒绝非法名称。两份生成的源码接入应用服务提供者后通过编译。
- 三语文档中的同一 Rust 示例在锁定工作区编译并用 SQLite 和 MySQL 请求验证。VitePress 构建、12 项通用浏览器组合及授权正文和链接的 6 项检查通过。
