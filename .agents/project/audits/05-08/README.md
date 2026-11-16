# 05-08 用户提供器、Guard 和会话认证

- 对照固定 Laravel 的用户提供器、手动认证、remember、登出及会话 Guard 测试。参考固定 Goravel 的认证服务与请求 Guard 装配。复用 `v0.x` 的请求级解析、会话优先再查 recaller、原始身份键与令牌失效规则。 [Laravel Authentication](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/authentication.md) · [Laravel SessionGuard](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/SessionGuard.php) · [Laravel Guard 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Auth/AuthSessionGuardTest.php) · [Goravel Auth](https://github.com/goravel/framework/tree/a554ced468cada50a269780692a0491e5f9e3a11/auth)
- 应用级 AuthManager 保存定义和工厂，Guard 借用当前 Request 并将已解析用户与选中 Guard 存在请求内。Query Builder 和 Eloquent 提供器保留字符串主键与密码重哈希，默认应用显式注册编译模型。支持请求解析器、Bearer Token、自定义提供器和 `auth`、`guest` 中间件，默认认证优先于模型绑定。
- Session 登录轮换 ID 和 CSRF token。remember 需要加密 Cookie，携带随机令牌及密码哈希指纹。登出清除当前 Guard 并轮换令牌，`logoutAndInvalidate` 继续清除整个 Session。限流复用原子缓存计数，应用负责账号和可信客户端地址的规范化。Rust 以显式 Request 借用代替动态请求解析，以静态注册代替 PHP 模型反射。HTTP Basic、认证事件、其他设备会话失效及密码确认仍待实现。
- 以 `b314a7f4` 为隔离基线。Rust 1.94.0 格式、457 项锁定离线测试、全目标严格 Clippy、`./artisan inspire` 通过。SQLite 与真实 MySQL 8.4 的实际 HTTP 覆盖错误登录、并发限流、429、成功登录、密码升级、会话及 CSRF 轮换、认证优先、remember 恢复、密码修改及登出失效、请求隔离和 Bearer 精确匹配。
- 三语示例从正文抽取，在两种数据库上通过 HTTP。VitePress 构建、12 项通用浏览器组合及认证正文和链接在根路径与子路径共 6 项通过。

## HTTP Basic

- 对照固定 [SessionGuard](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/SessionGuard.php#L374-L427) 的会话及单次 Basic 验证。支持 `auth.basic`、具名 Guard 和查找字段、已有会话直通、`Auth::onceBasic`、HTTP 401 及 `WWW-Authenticate: Basic`。旧版无可直接复用的 Basic 实现，复用已有凭据类型与 Guard。Rust 将 Base64 解码结果临时保存在归零缓冲区。
- Rust 1.94.0、macOS ARM64。真实 HTTP 覆盖无效凭据、冒号密码、会话持久化、指定用户名字段及单次请求隔离。隔离工作区格式与严格 Clippy 通过，既有 Sanctum 格式差异仅在隔离副本中整理。实际 Artisan `route:list --middleware auth.basic --json` 识别三语共用示例中的两种别名，VitePress 三语构建通过。
