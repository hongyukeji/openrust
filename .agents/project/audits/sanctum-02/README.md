# Sanctum-02 令牌与 SPA 认证

## 令牌签发与存储

对照固定 Sanctum v4.4.0 的 `HasApiTokens`、`PersonalAccessToken` 与 `HasApiTokensTest`。复用 `v0.x` 的安全随机数、SHA-256 摘要及作用域撤销思路，使用当前连接的 Query Builder。Rust 将数据库连接、归属类型和整数 ID 显式传给 `PersonalAccessTokens`。签发凭据由可清零的字符串持有，数据库只保存摘要。凭据由前缀、40 位随机字母数字和 CRC32b 组成，返回时附加 `id|`。固定 Goravel 用作应用连接装配参考。 [HasApiTokens](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/HasApiTokens.php) · [PersonalAccessToken](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/PersonalAccessToken.php) · [Goravel 数据库 Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/service_provider.go)

SQLite 和独立 MySQL 8.4.13 用实际 Artisan 迁移后的表验证并发签发、凭据格式、按 ID 和直接摘要查找、错误凭据、能力与过期判断、归属隔离和重复撤销。三语示例编译。HTTP 守卫及 SPA 验收见下文。 [Laravel 签发 API 令牌](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/sanctum.md#issuing-api-tokens)

Rust 1.94.0 格式、启用 `sanctum` 的 618 项工作区测试、全目标严格 Clippy、三语隔离构建和 18 项浏览器检查通过。8 项进程夹具由对应集成测试启动。

## HTTP 守卫与 SPA

按固定 v4.4.0 守卫先解析请求中的已认证会话，再查找 Bearer 令牌。令牌归属、过期及账户存在性通过后派发 `TokenAuthenticated`，按配置记录最后使用时间。缓存的身份及令牌均按请求和 Guard 隔离。Rust 通过编译注册的 Provider 解析整数归属标识，默认使用 Web 守卫的 Provider。Laravel 的默认 Provider 为空，通过 Eloquent 的多态关系恢复任意已注册的模型。Rust 在请求上访问令牌，通过类型和作用域共享同一身份。 [固定 Guard](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/Guard.php)

`statefulApi()` 将来源识别放在 API 组的认证之前，匹配 Referer 或 Origin 后才运行 Cookie 加解密、Cookie 队列、Session 和 CSRF。会话 Cookie 使用 HttpOnly 和 SameSite=Lax，XSRF Cookie 保持可供前端读取。Laravel 的可替换中间件配置和 AuthenticateSession 密码变更失效机制仍有差异，后者依赖框架的 Session 中间件交付。Sanctum 的自定义令牌模型、取令牌／验证回调、定期清理命令和 Passport 继续单独跟踪。 [固定 SPA 中间件](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/Http/Middleware/EnsureFrontendRequestsAreStateful.php) · [Laravel SPA Authentication](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/sanctum.md#spa-authentication)

Rust 1.94.0 执行 `cargo test --locked --workspace --features sanctum`，报告 621 次通过、0 失败，含测试自行启动的进程夹具。SQLite 与隔离 MySQL 8.4.13 真实 HTTP 覆盖有效、错误、到期、最大年龄、撤销、缺失账户、错误归属、全部／任一能力、最后使用时间和认证事件。匹配来源且启用状态 API 时，实际登录后的会话优先于 Bearer，缺 CSRF 返回 419，带正确 XSRF 头返回 200。未匹配和关闭状态 API 时拒绝会话，第三方 Bearer 正常。实跑应用二进制的 `artisan migrate`、`artisan route:list` 和 `GET /sanctum/csrf-cookie`，路由返回 204 及 XSRF Cookie。Rust 格式、启用 Sanctum 的工作区全目标严格 Clippy、三语示例编译、文档构建与 18 项浏览器检查通过。提交前账户可用窗口剩余 94%，磁盘可用 60 GiB。下一单元为 Sanctum-03 API 安装。
