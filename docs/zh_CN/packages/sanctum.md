# Sanctum

## 安装 API 路由 {#installation}

运行 `./artisan install:api`，创建并注册 `routes/api.rs`，同时把 Sanctum 加入应用的 Cargo 默认功能。交互终端会询问是否执行待运行的迁移，确认后会先重新构建再迁移。CI 可使用 `--without-migration-prompt`，再单独运行 `./artisan migrate`。使用 `--force` 覆盖已有 API 路由。 安装器无法修改自定义启动文件时，会保留生成的路由并提示手动接线。生成的路由返回已注册认证模型的可见属性。可选迁移失败时，安装器显示错误并保留脚手架。自动化需要迁移退出码时，单独运行 `./artisan migrate`。

默认应用骨架不预置 Sanctum 专属配置或用户模型注册。启用 Cargo 功能后，由包提供默认配置。新应用执行 `install:api` 会发布令牌表迁移，并接入发布的配置和默认用户模型。自定义用户模型须实现 `HasApiTokens`，并在 `config/auth.rs` 中用 `openrust::auth::model!(YourUser, sanctum)` 引用；显式运行时扩展也可用 `AuthManager::registerSanctumModel`。安装器更新模型能力和配置中的类引用，保留 `AppServiceProvider` 原样。手动启用功能时，可用 `vendor:publish --tag=sanctum-migrations` 发布迁移，用 `vendor:publish --tag=sanctum-config` 发布可编辑配置。手动发布后，在 `config/mod.rs` 声明模块并将载入结果注册到 `sanctum` 配置组。

在终端运行 `vendor:publish` 可交互选择服务提供者或标签。第三方包在服务提供者的 `boot` 中调用 `publishes` 或 `publishesMigrations` 注册编译进程序的资源。启用 `database.migrations.update_date_on_publish` 时，新迁移会使用新的时间戳。关闭配置，或在服务提供者中调用 `VendorPublishCommand::dontUpdateMigrationDates`，可保留源文件日期。重复发布沿用已有文件名，保持迁移账本稳定。迁移发布到 `database/migrations/` 后，重新构建即可将其纳入编译清单。

## 令牌表迁移 {#token-table-migration}

发布后的迁移属于应用。重新构建会将它纳入迁移清单，即使关闭 Cargo 的 `sanctum` 功能也能执行或回滚。

```sh
./artisan install:api --without-migration-prompt
./artisan migrate
./artisan migrate:status
```

迁移创建多态归属字段、唯一的 64 字符令牌哈希列、可空的能力和时间字段，以及过期时间索引。该功能同时注册 Sanctum 守卫和 CSRF Cookie 路由。

发布的迁移使用 `table.morphs("tokenable")`，默认创建整数型 `tokenable_id`。归属模型使用 UUID 或 ULID 时，在首次迁移前，从 `openrust::database::schema` 导入 `Schema`，并在应用服务提供者的 `boot` 中调用 `Schema::morphUsingUuids()?` 或 `Schema::morphUsingUlids()?`。该设置适用于当前应用后续的 `morphs` 调用。令牌表自身的主键仍为整数。

## 签发个人访问令牌 {#issuing-api-tokens}

使用 `Sanctum::personalAccessTokens` 解析出的令牌仓储创建令牌，传入归属类型、归属 ID、名称、能力和可选的过期时间。签发时将返回的 `plain_text_token` 交给客户端。数据库保存 SHA-256 摘要。`findToken` 查找凭据，`revokeToken` 按归属删除令牌。`can` 支持精确能力及通配符 `*`。HTTP 守卫在认证请求前检查过期时间。 签发、认证时写回及单条撤销都走 Eloquent 生命周期。默认令牌响应包含 `updated_at`。

```rust
use openrust::{auth::AuthResult, database::DatabaseManager, sanctum::{NewAccessToken, Sanctum}};

async fn issue(sanctum: &Sanctum, database: &DatabaseManager, user_id: u64) -> AuthResult<NewAccessToken> {
    sanctum.personalAccessTokens(database, "")?
        .createToken("app::models::User", user_id, "browser", &["reports:read"], None)
        .await
}
```

在服务提供者中通过应用的 `Sanctum` 服务调用 `usePersonalAccessTokenModel::<M>()`，选择 Eloquent 模型的表和连接。应用迁移负责创建自定义表，使用标准 Sanctum 属性以及模型声明的主键。签发时应用模型的属性修改器和转换，并触发 Eloquent 创建事件。Bearer 认证通过模型的访问器和转换读取属性，再通过同一模型实例保存最后使用时间。`revokeToken` 按归属删除模型实例，遵循模型删除事件。Artisan 清理应用模型全局作用域并批量删除，不逐条触发模型事件。

使用 UUID 或 ULID 主键时，在迁移中声明对应的列，并在模型中用 `ModelMetadata::uniqueId(UniqueId::Uuid)` 或 `UniqueId::Ulid` 配置生成策略。若主键列名不是 `id`，用 `primaryKey` 声明。凭据格式仍为 `<主键>|<密钥>`。令牌与归属标识使用 `AuthIdentifier` 保留整数或字符串类型。归属主键是字符串时，迁移须使用匹配的 `tokenable_id` 列类型，签发和撤销时传入该归属标识。

可用 `SANCTUM_TOKEN_PREFIX` 为新签发的令牌密钥设置应用专用前缀。

自定义模型须实现 `PersonalAccessTokenModel`。默认 `findToken` 校验 SHA-256 摘要。覆写方法收到请求凭据和已配置的 Eloquent 查询，可在追加条件或解码凭据后调用 `findTokenByDigest`。覆写方法负责验证其接受的凭据。

应用服务通过 `personalAccessTokenModel()` 返回注册的模型类型名。用 `currentApplicationUrlWithPort()` 读取应用 URL 的主机与显式端口。返回值带前导逗号，可追加到有状态域名配置。

## 模型令牌关系 {#model-token-relationship}

在 Eloquent 归属模型上实现 `HasApiTokens`。用 `user.tokens(&sanctum, &database)?` 查询或撤销该模型的令牌，用 `user.createToken(&sanctum, &database, name, abilities, expires_at)` 签发。关系使用已配置的令牌模型以及归属模型的多态别名和主键。`deleteToken` 和 `delete` 按关系范围批量删除。令牌可以通过 `tokenable` 解析归属模型。归属模型可通过 `currentAccessToken`、`tokenCan` 和 `tokenCant` 检查当前请求的令牌及能力。

用 `user.tokensQuery::<DefaultPersonalAccessToken>(&sanctum, &database)?` 链式查询。用 `user.tokensRelation::<DefaultPersonalAccessToken>(&sanctum)?` 取得关系，再用 Eloquent 的 `with("tokens", relation)` 预加载。使用自定义模型时，替换为已注册的模型类型。签发响应的嵌套令牌采用模型的属性转换、访问器、附加字段和可见性，并始终排除摘要。直接序列化自定义模型时，应在模型中隐藏 `token` 字段。

## 保护路由与检查能力 {#protecting-routes}

在受保护的路由上加上 `auth:sanctum`，用 `Authorization: Bearer <token>` 传入凭据。凭据无效、过期或已撤销时返回 HTTP 401。`abilities:reports:read,reports:write` 要求具备两项能力，`ability:reports:read,reports:write` 要求具备任意一项。令牌已通过认证但缺少能力时返回 HTTP 403。

```rust
use openrust::{http::ApiResponse, routing::RoutingError, support::facades::Route};

fn register() -> Result<(), RoutingError> {
    Route::getWith("/reports", || ApiResponse::success(["quarterly"]), |route| {
        route.middleware_name("auth:sanctum").middleware_name("abilities:reports:read");
        Ok(())
    })
}
```

签发时将已注册的 Provider 模型名用作令牌归属类型。默认应用使用 `app::models::User`。可在 `auth.guards` 为自定义 Sanctum 守卫设置 `provider`，省略时通过已注册的 Eloquent 模型和 `MorphMap` 解析归属。用 `AccessToken::currentAccessToken(request)` 查看当前请求凭据。`sanctum.expiration` 以分钟限制令牌最长有效期，与单个令牌的到期时间同时生效。Bearer 认证成功后派发 `TokenAuthenticated` 事件并更新 `last_used_at`，可设 `sanctum.last_used_at=false` 关闭。

用 `getAccessTokenFromRequestUsing` 替换 Bearer 提取逻辑。用 `authenticateAccessTokensUsing` 决定已查到的令牌是否有效，回调接收令牌及内置过期和 Provider 检查结果。令牌归属仍须成功解析。

在认证管理器中用 `AuthManager::registerSanctumModel` 注册支持令牌的模型。签发时可显式调用 `NewAccessToken::toJson()` 或 `toArray()` 返回明文凭据。嵌套令牌不包含数据库摘要，认证事件载荷可从 `TokenAuthenticated::token` 读取。

## SPA 认证 {#spa-authentication}

在 `bootstrap/app.rs` 现有的 `withMiddleware` 闭包内调用 `middleware.statefulApi();`。用 `SANCTUM_STATEFUL_DOMAINS` 配置以逗号分隔的第一方前端主机，非默认端口须一并写入。使用默认路由配置时，先请求 `GET /sanctum/csrf-cookie`，再通过应用的 Web 会话登录入口登录。后续 API 请求携带 Cookie。需要 CSRF 校验时，把 URL 解码后的 `XSRF-TOKEN` Cookie 值放入 `X-XSRF-TOKEN` 请求头。

跨源第一方前端需要为具体来源开启带凭据的 CORS。匹配的 Referer 或 Origin 会启用 Session、Cookie 和 CSRF 中间件。Sanctum 优先检查已认证的会话，再检查 Bearer 令牌。第一方会话会通过令牌能力检查，用户操作权限由应用授权策略决定。

第一方请求成功记录密码指纹后，修改密码会使该会话在下次请求时失效。要替换前端中间件，先通过应用的 `alias` 方法注册，再把别名填入 `sanctum.middleware.encrypt_cookies`、`sanctum.middleware.validate_csrf_token` 或 `sanctum.middleware.authenticate_session`。

在 `SANCTUM_STATEFUL_DOMAINS` 中加入 `__SANCTUM_CURRENT_REQUEST_HOST__`，或在逗号分隔的域名字符串末尾追加 `Sanctum::currentRequestHost()`，即可匹配当前请求的主机和端口。

## 清理过期令牌 {#pruning-expired-tokens}

启用 Sanctum 功能后运行以下命令。默认保留已过期令牌 24 小时。清理检查令牌的显式过期时间，也检查配置了 `sanctum.expiration` 时按创建时间计算的过期时间。应用支持通过 `SANCTUM_EXPIRATION` 设置有效期，单位为分钟。

```sh
./artisan sanctum:prune-expired --hours=24
```

## 测试 {#testing}

在测试中传入拥有所有权的 Eloquent 用户，用 `Sanctum::actingAs(&mut app, user, &["reports:read"], None)` 为 `TestApp` 的后续请求设置身份和能力。默认选择 Sanctum 守卫。可传入 `Some("guard-name")` 指定守卫，或传入 `["*"]` 授予全部能力。测试夹具直接提供身份和能力，无需签发数据库令牌。

返回的共享用户已清除 `wasRecentlyCreated` 状态。
