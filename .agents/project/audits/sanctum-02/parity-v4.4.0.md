# Sanctum v4.4.0 对照与验收

基线为 [Sanctum v4.4.0 固定提交](https://github.com/laravel/sanctum/tree/a28ea5d4c2b123420562d6a3a233414f84b70774) 与项目锁定的 Laravel 13.40.0。复核范围为包内生产源码、配置、迁移与上游测试。OpenRust 的运行入口使用 Rust 编译装配。

| 上游契约 | OpenRust 对应与验证 |
|---|---|
| `Sanctum::actingAs` | `TestApp` 隔离、默认与命名守卫、零能力和通配符。MySQL 保存模型后验证 `wasRecentlyCreated` 清除、持久化状态保留。 |
| `currentApplicationUrlWithPort`、`currentRequestHost`、主机占位 | 缺省 URL、显式端口、IPv6、当前主机和错误端口均进入 HTTP 回归。 |
| `usePersonalAccessTokenModel`、`personalAccessTokenModel` | 应用内注册与查询 Rust 类型名，覆盖默认、自定义整数、UUID、ULID。 |
| `getAccessTokenFromRequestUsing`、`authenticateAccessTokensUsing` | 真实 HTTP 验证覆盖提取、接受和拒绝，以及默认校验结果传入回调。 |
| `HasAbilities`、`TransientToken` | 公开能力契约。会话授予临时能力，测试覆盖全量、任一和通配符。 |
| `HasApiTokens` 与 `tokens` / `tokenable` | 公开 owner trait，按 morph 别名、模型主键、已注册模型和连接签发、查询、反查与撤销。MySQL 验证默认模型和自定义整数、UUID、ULID 模型。覆盖链式查询、预加载、跨归属与大小写近似别名隔离，以及关系删除与实例删除的事件差异。 |
| `createToken`、`generateTokenString` | 通过 owner 方法签发，使用可覆写的生成方法。验证前缀、SHA-256、凭据格式、签发与拒绝。Rust 通过显式参数表达能力与到期时间。 |
| `currentAccessToken`、`withAccessToken`、`tokenCan`、`tokenCant` | 请求域内读取和替换，验证另一身份无法替换。 |
| `PersonalAccessToken`、`findToken`、`can` / `cant` | 摘要隐藏，标准属性转换、模型事件、作用域、自定义查找、错误摘要和无 ID 查找均有回归。 |
| `NewAccessToken::toArray` / `toJson` | 显式生成 `accessToken` 与 `plainTextToken`。嵌套令牌采用模型序列化，覆盖自定义主键、访问器、附加字段、可见性及摘要隐藏。Rust 通过 `Result` 返回序列化错误，JSON 字符串以 Zeroizing 返回。 |
| `Guard` | 默认守卫配置合并，按 Provider 或多态查找，模型显式实现并注册 `HasApiTokens`。真实 HTTP 验证具备和缺失令牌能力的模型。 |
| `TokenAuthenticated` | 使用 `OpenRust\Sanctum\Events\TokenAuthenticated`，携带 `token`，验证事件在最后使用时间写回之前派发。 |
| 能力与 Scope 中间件／异常 | 全部及任一匹配，401 与 403，包含能力或 Scope 的类型化错误。验证 HTTP 状态与默认文本。 |
| `CsrfCookieController`、`EnsureFrontendRequestsAreStateful`、`AuthenticateSession` | 首方 Cookie/Session/CSRF、选配中间件、密码变化后失效、会话优先于 Bearer、域名和端口匹配。密码指纹使用现行 Rust HMAC 格式。 |
| `stateful`、`guard`、`expiration`、`token_prefix`、`middleware`、`routes`、`prefix`、`last_used_at` | 配置接线均已复核。缺省守卫及 Provider、域名匹配、前缀、过期与中间件替换经 HTTP 和数据库验证。 |
| Provider、发布标签、默认迁移与清理命令 | 路由、Guard、命令由编译注册。Artisan 验证 Sanctum 及第三方 Provider、标签和交互选择，覆盖 `--all`、`--force`、`--existing`、首次迁移重命名及再次发布文件名稳定。MySQL 验证迁移、回滚及按显式到期和全局寿命清理，保留有效及作用域排除记录。 |
| Laravel 应用 `install:api` | 骨架源码生成、去重、覆盖和迁移提示由 Artisan 处理，见 [安装验收](../sanctum-03/README.md)。 |

## Rust 接口边界

- PHP 运行时类可直接决定关系返回值。Rust 通过应用注册选定模型，使用时显式指定类型。用 `tokensQuery::<T>` 构造 Eloquent 查询，用 `tokensRelation::<T>` 构造 `MorphMany` 预加载描述。MySQL 验证四种模型以及关系条件在用户添加 OR 时仍有效。
- PHP 的全局测试状态以隔离的 Rust 夹具表示。与令牌关联的身份绑定到请求和守卫。能力切换和跨用户隔离由 HTTP 测试验证。
- 第三方包通过 `ServiceProvider::publishes` 或 `publishesMigrations` 提供编译期资源及相对目标路径，替代 PHP 运行时源文件发现。交互菜单可按文本过滤，再按编号选择 Provider、标签或全部。新迁移发布时按时间重命名，应用先在迁移注册表接入再重新编译。已有迁移源文件及其名称保留，以匹配 Rust 的编译注册和迁移账本。同名重复发布使用已有文件名。文件与目录可通过 `publishesPaths` 注册。目录递归复制文件并保留相对路径。Rust 端使用逐行输入搜索词的终端菜单。为匹配本地文件适配器的安全边界，拒绝源树内和应用目标路径上的符号链接。
- Passport 安装选项属于 Laravel 的 API 安装器和 Passport 集成，继续单独跟踪。

## 本轮实际验收

- 隔离 MySQL 8.4.13，`cargo +1.94.0 test --locked --offline --features sanctum -p openrust-framework --test sanctum_authentication -- --skip bearer_and_opt_in_spa_over_http_with_sqlite` 通过 3 项。通过环境变量强制执行 MySQL 用例，默认及自定义模型均实际走 HTTP、数据库和 Eloquent。
- `cargo +1.94.0 test --locked --offline -p openrust-framework --test test_app` 通过 4 项，覆盖第三方 Provider 注册、Provider/标签交集、重新发布和应用间隔离。
- 启动实际应用二进制执行 `artisan vendor:publish`，用伪终端选择 `sanctum-migrations`，确认只写入迁移；随后直接执行标签发布、`--force` 和 `--existing`，确认只有一个文件。先把首次发布生成的带时间戳文件编译到独立应用，再用实际 Artisan 在 MySQL 8.4.13 验证首次迁移、重复执行无变更、回滚和重建。默认迁移账本和 `install:api` 的现有回归也各通过 1 项。
- Rust 1.94 格式检查通过，工作区全目标严格 Clippy 在默认功能和 `sanctum` 功能下均通过。VitePress 构建通过，三语在 `/` 和 `/docs/` 的桌面、手机浏览器验收共 12 项通过。


## `vendor:publish` 补充验收

- 对照固定 Laravel 13.40.0，逐标签处理注册映射。有注册项即在处理完成后派发 `VendorTagPublished`，包括所有文件均跳过或源路径缺失的情况。无匹配注册项时不派发。事件携带可空标签和原始源到目标映射，目录按注册根路径报告，嵌入文件用 `embedded/<Provider>/<路径>` 标识。 [固定命令](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Foundation/Console/VendorPublishCommand.php) · [固定事件](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Foundation/Events/VendorTagPublished.php)
- 实际应用二进制在隔离根目录通过伪终端验收。使用大小写不同的搜索词定位 Provider，再分别搜索配置标签、迁移标签和全部，核对每次生成的文件。
- 独立应用注册第三方目录和具名事件监听器，经实际 Artisan 验证相对路径复制、首次发布及再次跳过的事件载荷，并验证未知标签无事件。Rust 1.94 的 `test_app` 五项通过，覆盖二进制资源、隐藏文件、默认保留、`--force`、`--existing`、多个标签和缺失源。
- Rust 1.94 格式检查、默认与 Sanctum 功能的工作区全目标严格 Clippy、三语 VitePress 构建与 12 项浏览器检查通过。
