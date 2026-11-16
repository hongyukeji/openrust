# 05-08 用户提供器、Guard 和会话认证

- 对照固定 Laravel 的用户提供器、手动认证、remember、登出及会话 Guard 测试。参考固定 Goravel 的认证服务与请求 Guard 装配。复用 `v0.x` 的请求级解析、会话优先再查 recaller、原始身份键与令牌失效规则。 [Laravel Authentication](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/authentication.md) · [Laravel SessionGuard](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/SessionGuard.php) · [Laravel Guard 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Auth/AuthSessionGuardTest.php) · [Goravel Auth](https://github.com/goravel/framework/tree/a554ced468cada50a269780692a0491e5f9e3a11/auth)
- 应用级 AuthManager 保存定义和工厂，Guard 借用当前 Request 并将已解析用户与选中 Guard 存在请求内。Query Builder 和 Eloquent 提供器保留字符串主键与密码重哈希，默认应用显式注册编译模型。支持请求解析器、Bearer Token、自定义提供器和 `auth`、`guest` 中间件，默认认证优先于模型绑定。
- Session 登录轮换 ID 和 CSRF token。remember 需要加密 Cookie，携带随机令牌及密码哈希指纹。登出清除当前 Guard 并轮换令牌，`logoutAndInvalidate` 继续清除整个 Session。限流复用原子缓存计数，应用负责账号和可信客户端地址的规范化。Rust 以显式 Request 借用代替动态请求解析，以静态注册代替 PHP 模型反射。HTTP Basic、认证事件、其他设备会话失效及密码确认仍待实现。
- 以 `b314a7f4` 为隔离基线。Rust 1.94.0 格式、457 项锁定离线测试、全目标严格 Clippy、`./artisan inspire` 通过。SQLite 与真实 MySQL 8.4 的实际 HTTP 覆盖错误登录、并发限流、429、成功登录、密码升级、会话及 CSRF 轮换、认证优先、remember 恢复、密码修改及登出失效、请求隔离和 Bearer 精确匹配。
- 三语示例从正文抽取，在两种数据库上通过 HTTP。VitePress 构建、12 项通用浏览器组合及认证正文和链接在根路径与子路径共 6 项通过。

## HTTP Basic

- 对照固定 [SessionGuard](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/SessionGuard.php#L374-L427) 的会话及单次 Basic 验证。支持 `auth.basic`、具名 Guard 和查找字段、已有会话直通、`Auth::onceBasic`、HTTP 401 及 `WWW-Authenticate: Basic`。旧版无可直接复用的 Basic 实现，复用已有凭据类型与 Guard。Rust 将 Base64 解码结果临时保存在归零缓冲区。
- Rust 1.94.0、macOS ARM64。真实 HTTP 覆盖无效凭据、冒号密码、会话持久化、指定用户名字段及单次请求隔离。隔离工作区格式与严格 Clippy 通过，既有 Sanctum 格式差异仅在隔离副本中整理。实际 Artisan `route:list --middleware auth.basic --json` 识别三语共用示例中的两种别名，VitePress 三语构建通过。

## 会话 Guard 认证事件

- 固定 SessionGuard 的 `Attempting`、`Validated`、`Login`、`Authenticated`、`Failed`、`Logout`、`CurrentDeviceLogout` 接入已有应用事件派发器。Rust 同步监听器按生命周期借用凭据和验证前的类型化用户。(当前设备登出使用异步 API 以等待应用监听器，Sanctum 调用点同步更新。)
- Rust 1.94.0、macOS ARM64。真实 HTTP 验证错误凭据的已知和未知用户、单次登录、持久登录、会话读取、remember 恢复、当前设备与普通登出的事件顺序和令牌行为，两项数据库独立的定向测试通过。隔离工作区格式与启用 Sanctum 的严格 Clippy 通过。实际 Artisan `event:list --event Login --json` 验证三语共用监听器的注册，三语构建通过。

## 其他设备会话失效

- 对照固定 [SessionGuard::logoutOtherDevices](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/SessionGuard.php) 和 [AuthenticateSession](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Session/Middleware/AuthenticateSession.php)。确认当前密码后强制重哈希，从提供器重读账户，更新当前指纹及适用的 remember Cookie，并派发 `OtherDeviceLogout`。`auth.session` 校验会话指纹，拒绝曾保存旧指纹的会话。复用现有会话机制与 Sanctum 的 HMAC 指纹格式，既有 recaller 校验沿用已验收实现。
- Rust 1.94.0、macOS ARM64。真实 HTTP 用两个独立会话验证错误密码保留两者，成功撤销后当前会话和新 remember Cookie 可用，旧会话和旧 Cookie 返回 401，并验证重登和事件载荷。三项数据库独立的定向测试、隔离格式及启用 Sanctum 的严格 Clippy 通过。真实 Artisan `route:list --middleware auth.session --json` 识别三语共用的示例路由与中间件顺序，三语构建通过。

## 密码确认

- 对照固定 [RequirePassword](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/Middleware/RequirePassword.php) 与 Session Store，复用旧版密码确认的会话时间戳、JSON 423 和命名路由跳转语义。`password.confirm` 支持配置和路由级覆盖。浏览器通过现有 Redirector 保存原目标，`Auth::confirmPassword` 使用当前账户的提供器校验后写入确认时间。Guard 新建或结束登录会话时清除旧确认时间，避免确认随重新登录继承。
- Rust 1.94.0、macOS ARM64。真实 HTTP 验证默认和具名确认页、原 URL 含查询参数的恢复、错误密码、成功确认、配置超时及单路由超时。四项扩展 HTTP 测试、八项 CSRF／视图回归及隔离 MySQL 8.4.13 的既有认证用例通过。MySQL 测试专用库和账号已回收。
- 隔离工作区格式及启用 Sanctum 的严格 Clippy 通过。真实 Artisan 使用三语共用 Rust 示例，`route:list` 分别验证中间件路由及 `password.confirm` 的命名目标。三语构建及四个认证章节锚点复核通过。
