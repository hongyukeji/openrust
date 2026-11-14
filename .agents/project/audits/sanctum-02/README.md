# Sanctum-02 令牌与 SPA 认证

## 令牌签发与存储

对照固定 Sanctum v4.4.0 的 `HasApiTokens`、`PersonalAccessToken` 与 `HasApiTokensTest`。复用 `v0.x` 的安全随机数、SHA-256 摘要及作用域撤销思路，使用当前连接的 Query Builder。Rust 将数据库连接、归属类型和整数 ID 显式传给 `PersonalAccessTokens`。签发凭据由可清零的字符串持有，数据库只保存摘要。凭据由前缀、40 位随机字母数字和 CRC32b 组成，返回时附加 `id|`。固定 Goravel 用作应用连接装配参考。 [HasApiTokens](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/HasApiTokens.php) · [PersonalAccessToken](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/PersonalAccessToken.php) · [Goravel 数据库 Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/service_provider.go)

SQLite 和独立 MySQL 8.4.13 用实际 Artisan 迁移后的表验证并发签发、凭据格式、按 ID 和直接摘要查找、错误凭据、能力与过期判断、归属隔离和重复撤销。三语示例编译。HTTP 守卫及 SPA 验收见下文。 [Laravel 签发 API 令牌](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/sanctum.md#issuing-api-tokens)

Rust 1.94.0 格式、启用 `sanctum` 的 618 项工作区测试、全目标严格 Clippy、三语隔离构建和 18 项浏览器检查通过。8 项进程夹具由对应集成测试启动。

## HTTP 守卫与 SPA

按固定 v4.4.0 守卫先解析请求中的已认证会话，再查找 Bearer 令牌。令牌归属、过期及账户存在性通过后派发 `TokenAuthenticated`，按配置记录最后使用时间。缓存的身份及令牌均按请求和 Guard 隔离。Rust 通过编译注册的 Provider 解析整数归属标识，默认使用 Web 守卫的 Provider。Laravel 的默认 Provider 为空，通过 Eloquent 的多态关系恢复任意已注册的模型。Rust 在请求上访问令牌，通过类型和作用域共享同一身份。 [固定 Guard](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/Guard.php)

`statefulApi()` 将来源识别放在 API 组的认证之前，匹配 Referer 或 Origin 后才运行 Cookie 加解密、Cookie 队列、Session 和 CSRF。会话 Cookie 使用 HttpOnly 和 SameSite=Lax，XSRF Cookie 保持可供前端读取。Laravel 的可替换中间件配置和 AuthenticateSession 密码变更失效机制仍有差异，后者依赖框架的 Session 中间件交付。Sanctum 的自定义令牌模型、取令牌／验证回调、定期清理命令和 Passport 继续单独跟踪。 [固定 SPA 中间件](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/Http/Middleware/EnsureFrontendRequestsAreStateful.php) · [Laravel SPA Authentication](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/sanctum.md#spa-authentication)

Rust 1.94.0 执行 `cargo test --locked --workspace --features sanctum`，报告 621 次通过、0 失败，含测试自行启动的进程夹具。SQLite 与隔离 MySQL 8.4.13 真实 HTTP 覆盖有效、错误、到期、最大年龄、撤销、缺失账户、错误归属、全部／任一能力、最后使用时间和认证事件。匹配来源且启用状态 API 时，实际登录后的会话优先于 Bearer，缺 CSRF 返回 419，带正确 XSRF 头返回 200。未匹配和关闭状态 API 时拒绝会话，第三方 Bearer 正常。实跑应用二进制的 `artisan migrate`、`artisan route:list` 和 `GET /sanctum/csrf-cookie`，路由返回 204 及 XSRF Cookie。Rust 格式、启用 Sanctum 的工作区全目标严格 Clippy、三语示例编译、文档构建与 18 项浏览器检查通过。提交前账户可用窗口剩余 94%，磁盘可用 60 GiB。下一单元为 Sanctum-03 API 安装。

## SPA 密码变更与中间件替换

固定 Sanctum 的 AuthenticateSession 在响应后保存会话守卫的 HMAC，并在后续请求核对。Rust 对当前设备清理登录及记住 Cookie、清空 Session 并返回 401。沿用项目的当前数据契约。前端管道的 Cookie 加密、CSRF 与会话校验可按上游配置键引用应用注册的中间件别名。 [上游会话校验](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/Http/Middleware/AuthenticateSession.php) · [上游前端管道](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/Http/Middleware/EnsureFrontendRequestsAreStateful.php)

Rust 1.94.0 用专用 MySQL 8.4.13 执行 `sanctum_authentication` 的 `bearer_and_spa_over_http_with_mysql`。真实 HTTP 验证修改密码后旧会话 401、随后 Bearer 可独立认证、默认及替换 CSRF 的拒绝和通过路径。启用 Sanctum 的工作区全目标严格 Clippy 通过。

三語 VitePress 構建及根路徑、`/docs/` 的 12 項瀏覽器檢查通過。

## 过期令牌清理

按固定 `PruneExpired` 注册 `sanctum:prune-expired`，默认保留 24 小时。按显式过期时间和可选的全局期限分别删除，沿用实际令牌数据库连接和时间格式。Rust 拒绝负数及超出可计算范围的保留期限。复核 v0.x 的令牌过期逻辑，本批复用当前 Query Builder 与令牌期限解析。 [固定清理命令](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/Console/Commands/PruneExpired.php)

Rust 1.94.0 用独立 MySQL 8.4.13 运行带 Sanctum 的 `migrations` 测试目标，精确筛选 `artisan_migrations_use_a_real_mysql_ledger`。实际 Artisan 验证默认 24 小时、全局期限、`--hours=0` 和保留有效令牌。启用 Sanctum 的工作区全目标严格 Clippy 通过。

## 自定义模型、回调和多态归属

对照固定 Sanctum v4.4.0 的 `Sanctum`、`Guard` 与前端中间件。旧版固定查询用户表，本批复用当前 Eloquent 模型注册、MorphMap 和令牌仓储。应用级 `usePersonalAccessTokenModel` 使用自定义模型的表与连接，令牌仓储沿用标准列和整数主键。模型事件与属性转换的后续接线见下节。未指定 Provider 时从归属类型解析已注册模型。显式 Provider 保留类型限制。提取回调替换 Bearer 提取，认证回调接收内置校验结果并决定是否接受，归属仍须存在。当前主机占位默认不启用，启用后按每个请求的主机和端口比对。 [固定 Sanctum](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/Sanctum.php) · [固定 Guard](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/Guard.php)

Rust 1.94.0 执行 `cargo test --locked --offline --features sanctum -p openrust-framework --test sanctum_authentication`。SQLite 和专用 MySQL 8.4.13 两项测试通过，覆盖真实 HTTP 自定义提取、有效令牌拒绝、已到期令牌覆盖放行、两个归属模型及显式 Provider、SPA 当前主机匹配及错误端口拒绝。自定义表和命名连接覆盖签发、认证、撤销及真实 Artisan `sanctum:prune-expired --hours=0`。清理后有效令牌仍可查找。Rust 1.94.0 格式检查、启用 Sanctum 的工作区全目标严格 Clippy 和三语 Rust 示例编译通过。三语 VitePress 构建及根路径、`/docs/` 下桌面和手机共 12 项浏览器检查通过。

## 自定义令牌模型生命周期

固定 Sanctum v4.4.0 使用 Eloquent 模型签发、查询及保存最后使用时间。单个实例撤销遵循模型删除事件，Artisan 清理通过带模型作用域的批量删除执行。自定义模型仍使用标准 Sanctum 字段和整数令牌标识。现有 Eloquent API 提供属性转换、访问器、修改器和模型事件，旧版令牌仓储没有对应的 Eloquent 模型接线。 [上游签发](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/HasApiTokens.php) · [上游认证](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/Guard.php) · [上游清理](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/Console/Commands/PruneExpired.php)

Rust 1.94.0 对隔离的 MySQL 8.4.13 执行 `cargo test --locked --offline --features sanctum -p openrust-framework --test sanctum_authentication bearer_and_spa_over_http_with_mysql -- --exact`。真实 HTTP 与 Artisan 验收通过，覆盖签发事件顺序与取消、名称访问器／修改器、能力转换、认证写回和时间转换、撤销事件与取消、清理全局作用域及批量删除无实例事件。以 Asia/Shanghai 验证自定义模型与清理的时间一致性。格式检查、启用 Sanctum 的工作区全目标严格 Clippy 和三语站点 12 项浏览器检查通过。

Passport 和安装器的 Passport 选项单独跟踪。
