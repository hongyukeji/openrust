# Sanctum v4.4.0 对照与验收

基线为 [Sanctum v4.4.0 固定提交](https://github.com/laravel/sanctum/tree/a28ea5d4c2b123420562d6a3a233414f84b70774) 与项目锁定的 Laravel 13.40.0。复核范围为包内生产源码、配置、迁移与上游测试。OpenRust 的运行入口使用 Rust 编译装配。

| 上游契约 | OpenRust 对应与验证 |
|---|---|
| `Sanctum::actingAs` | `TestApp` 隔离、默认与命名守卫、零能力和通配符。MySQL 保存模型后验证 `wasRecentlyCreated` 清除、持久化状态保留。 |
| `currentApplicationUrlWithPort`、`currentRequestHost`、主机占位 | 缺省 URL、显式端口、IPv6、当前主机和错误端口均进入 HTTP 回归。 |
| `usePersonalAccessTokenModel`、`personalAccessTokenModel` | 应用内注册与查询 Rust 类型名，覆盖默认、自定义整数、UUID、ULID。 |
| `getAccessTokenFromRequestUsing`、`authenticateAccessTokensUsing` | 真实 HTTP 验证覆盖提取、接受和拒绝，以及默认校验结果传入回调。 |
| `HasAbilities`、`TransientToken` | 公开能力契约。会话授予临时能力，测试覆盖全量、任一和通配符。 |
| `HasApiTokens` 与 `tokens` / `tokenable` | 公开 owner trait，按 morph 别名、模型主键、已注册模型和连接签发、查询、反查与撤销。MySQL 验证四种模型、跨归属拒绝、关系删除和实例删除的事件差异。 |
| `createToken`、`generateTokenString` | 通过 owner 方法签发，使用可覆写的生成方法。验证前缀、SHA-256、凭据格式、签发与拒绝。Rust 通过显式参数表达能力与到期时间。 |
| `currentAccessToken`、`withAccessToken`、`tokenCan`、`tokenCant` | 请求域内读取和替换，验证另一身份无法替换。 |
| `PersonalAccessToken`、`findToken`、`can` / `cant` | 摘要隐藏，标准属性转换、模型事件、作用域、自定义查找、错误摘要和无 ID 查找均有回归。 |
| `NewAccessToken::toArray` / `toJson` | 显式生成 `accessToken` 与 `plainTextToken`，测试验证摘要不在输出中。Rust 以 JSON Value 表示对象，JSON 字符串以 Zeroizing 返回。 |
| `Guard` | 默认守卫配置合并，按 Provider 或多态查找，模型显式实现并注册 `HasApiTokens`。真实 HTTP 验证具备和缺失令牌能力的模型。 |
| `TokenAuthenticated` | 使用 `OpenRust\Sanctum\Events\TokenAuthenticated`，携带 `token`，验证事件在最后使用时间写回之前派发。 |
| 能力与 Scope 中间件／异常 | 全部及任一匹配，401 与 403，包含能力或 Scope 的类型化错误。验证 HTTP 状态与默认文本。 |
| `CsrfCookieController`、`EnsureFrontendRequestsAreStateful`、`AuthenticateSession` | 首方 Cookie/Session/CSRF、选配中间件、密码变化后失效、会话优先于 Bearer、域名和端口匹配。密码指纹使用现行 Rust HMAC 格式。 |
| `stateful`、`guard`、`expiration`、`token_prefix`、`middleware`、`routes`、`prefix`、`last_used_at` | 配置接线均已复核。缺省守卫及 Provider、域名匹配、前缀、过期与中间件替换经 HTTP 和数据库验证。 |
| Provider、发布标签、默认迁移与清理命令 | 路由、Guard、命令由编译注册。Artisan 验证 `sanctum-config`、`sanctum-migrations` 发布及自定义文件保护。MySQL 验证迁移、回滚及按显式到期和全局寿命清理，保留有效及作用域排除记录。 |
| Laravel 应用 `install:api` | 骨架源码生成、去重、覆盖和迁移提示由 Artisan 处理，见 [安装验收](../sanctum-03/README.md)。 |

## Rust 接口边界

- 上游通过 PHP 类和可动态替换的关系返回类型装配。Rust 令牌模型由编译时 trait 和应用级注册装配。运行时切换模型时，`TokenRelation` 提供归属限制、反查、签发、列举和按 ID 或全部删除。通用 Eloquent 关系上的任意 query-chain 组合、预加载和模型自定义字段序列化未纳入这套令牌关系。标准字段的签发序列化与关系操作有真实 MySQL 覆盖。
- PHP 的全局测试状态以隔离的 Rust 夹具表示。与令牌关联的身份绑定到请求和守卫，不挂在跨请求共享的模型实例上。能力切换和跨用户隔离由 HTTP 测试验证。
- PHP/Composer package discovery、PHP 文件发布、运行时路由和配置缓存采用 Rust 的 Cargo 功能、Rust 源码模板及编译注册处理。发布命令当前覆盖 Sanctum 两个标签，通用发布框架的第三方注册、无参数交互式选择和 Laravel 时间戳重命名仍有差异。默认迁移标识固定在已编译的迁移账本中。
- Passport 安装选项属于 Laravel 的 API 安装器和 Passport 集成，继续单独跟踪。

## 本轮实际验收

- 隔离 MySQL 8.4.13，`sanctum_authentication` 运行 3 项，覆盖主应用与自定义模型的 HTTP、关系、测试身份及发布，SQLite 用例按定向范围过滤。
- `artisan_migrations_use_a_real_mysql_ledger` 与 `artisan_installs_optional_routes_without_overwriting_or_duplication` 各通过 1 项。
- Rust 1.94 格式与工作区全目标严格 Clippy 通过，分别覆盖默认功能和 `sanctum`。VitePress 构建及三语浏览器 12 项通过。
