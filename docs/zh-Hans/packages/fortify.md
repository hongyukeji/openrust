# Fortify

## 简介

Fortify 提供认证路由和应用扩展契约，不包含用户界面。认证 API 对应 Fortify 1.43.0。

## Fortify 与 Sanctum

Fortify 管理注册、Session 登录和凭据变更。[Sanctum](sanctum.md) 的 API 认证能力见对应章节。Fortify 复用现有的认证、会话、CSRF、哈希、验证器和通知服务。

## 安装

在应用 `Cargo.toml` 的 `[features].default` 列表中加入 `fortify`，保留已有功能，然后使用源码入口：

```sh
./artisan fortify:install
```

命令发布 `config/fortify.rs`、`app/actions/fortify/`、应用 Provider、双因素字段与 Passkeys 迁移，注册 Rust 模块、配置和 Provider，并为默认 User 接入所需能力。已有发布文件会保留；无法识别的源码布局会明确要求手工注册。安装不会执行迁移。

修改发布配置：默认 User 须启用 `email-verification`；配置视图回调或将 `views` 设为 `false`。随后运行迁移；源码入口会在需要时重新编译应用：

```sh
./artisan migrate --no-interaction
```

自动发现的 `FortifyResourcesServiceProvider` 仅提供安装资源。显式注册的 `openrust_fortify::FortifyServiceProvider` 启用运行时认证，生成的应用 Provider 连接动作、模型和命名限流。编译型应用在源码变更后需要重新构建。

## 配置与视图

安装默认使用 `web` 守卫、`users` 密码代理、`email` 用户名和邮件字段、用户名小写转换、空前缀和 `/home` 首页。这些是应用模板默认值，与包内默认配置不同。通过 `fortify.features` 选择功能，通过 `fortify-options.two-factor-authentication` 设置 `{"confirm":true,"confirmPassword":true}`；安装器会加载这一单独的 Rust 选项组。

`fortify.views=false` 移除界面路由，保留已启用的动作接口；Session Cookie 和 CSRF 仍有效。须提供名为 `password.reset` 的前端路由，或配置原生重置通知的 URL 生成，才能构建重置邮件。视图回调包括 `login_view`、`register_view`、`request_password_reset_link_view`、`reset_password_view`、`verify_email_view`、`confirm_password_view` 和 `two_factor_challenge_view`。

## 认证

`POST /login` 接收配置的用户名、`password` 和可选 `remember`。普通 JSON 登录成功返回 `200` 和 `two_factor:false`，需要双因素挑战时返回 `two_factor:true`。认证成功会轮换 Session。`POST /logout` 使其失效并重新生成 CSRF 令牌。浏览器变更请求须携带会话 Cookie 和 CSRF 头。JSON 验证失败返回 `422`，命名限流返回 `429`。

通过 `Fortify::authenticate_using` 定制身份查询，`confirm_passwords_using` 定制密码确认，`authenticate_through` 替换登录流水线。默认流水线处理限流、用户名规范化、双因素跳转、认证和会话准备。Web 响应可通过 `fortify.redirects` 配置跳转。

安装的 `login` 限流器按转写后的小写用户名和 IP 每分钟允许五次请求，通过 `RateLimiter::for_async` 读取真实请求输入。配置命名限流后会替换 Fortify 内置的失败计数限流。命名策略可返回 `Limit`、多个配额、`Unlimited` 或响应。

## 双因素认证

确认密码后，`POST /user/two-factor-authentication` 创建加密的 TOTP 密钥和八个恢复码。通过 `GET /user/two-factor-qr-code` 读取二维码 SVG，通过 `GET /user/two-factor-secret-key` 读取密钥；启用确认时，将 TOTP `code` 提交到 `POST /user/confirmed-two-factor-authentication`。

`GET /user/two-factor-recovery-codes` 列出恢复码，同一路径的 `POST` 重新生成恢复码。`DELETE /user/two-factor-authentication` 关闭该能力。默认模型在序列化时隐藏加密密钥和恢复码。

登录进入挑战后，使用同一 Session 将 `code` 或 `recovery_code` 提交到 `POST /two-factor-challenge`。兑换恢复码会替换该码。挑战状态、重放检查、命名限流和原生认证守卫均保留在请求流程中。应用可替换 `TwoFactorAuthenticationProvider` 契约。

## 注册与账户更新

`POST /register` 调用 `CreatesNewUsers`。安装的 `CreateNewUser` 验证姓名和邮件地址，通过 User 模型检查邮件唯一性，验证默认密码策略和确认字段，哈希密码并保存账户。JSON 成功返回 `201`。

`PUT /user/profile-information` 调用 `UpdatesUserProfileInformation`；邮件地址变化会清除验证状态并发送新的原生通知。`PUT /user/password` 调用 `UpdatesUserPasswords`，验证 `current_password:web`、哈希新密码、删除重置令牌并发出更新事件。安装的 `update_from_request` 独立读取真实 `web` 守卫，与待更新用户区分。直接调用默认动作而不提供请求时会明确失败；已有自定义动作仍可实现原 `update` 方法。

可在应用的 Fortify 实例上通过 `create_users_using`、`update_user_profile_information_using`、`update_user_passwords_using` 和 `reset_user_passwords_using` 替换动作。安装的密码规则为 `required`、`string`、`Password::default()` 和 `confirmed`。资料与密码的 Web 验证使用命名错误包。

## 密码重置

`POST /forgot-password` 接收配置的邮件字段，通过选定的密码代理发送原生重置通知。将 `token`、邮件地址、`password` 和 `password_confirmation` 提交到 `POST /reset-password`。安装的 `ResetsUserPasswords` 动作验证、哈希并保存代理提供的实际用户。完成后轮换 remember 令牌并发出 `PasswordReset`，不会自动登录。动作输入不合法时可以重试；成功消费的令牌不能再次使用。

JSON 重置结果包含翻译后的消息，失败使用验证错误；Web 结果使用跳转和 Session 状态。可实现密码响应契约替换这些响应，其 Rust `to_response` 方法显式接收代理状态。

## 邮件验证

为实现 `MustVerifyEmail` 的用户启用 `email-verification`。注册和资料邮件变更使用原生验证通知，`POST /email/verification-notification` 重新发送。带签名的 `GET /email/verify/{id}/{hash}` 链接验证当前用户和当前邮件地址，更新验证状态并发出验证事件。可通过已有的 `verified` 中间件保护应用路由。实际投递须配置真实邮件传输；默认 `log` 邮件器仅记录消息。

## 密码确认

`POST /user/confirm-password` 验证 `password` 并记录确认时间。在有效期内，`GET /user/confirmed-password-status` 返回 `{"confirmed":true}`。配置要求时，凭据管理使用 `password.confirm`。应用可通过现有容器替换 `LoginResponse`、`RegisterResponse`、`PasswordUpdateResponse` 和双因素响应等契约；每个应用独立持有自己的替换实现。

## Passkeys

Passkeys API 配合 `@laravel/passkeys` 0.4.1 客户端使用。`fortify:install` 发布 `passkeys` 表迁移，为默认 User 添加 `PasskeyAuthenticatable`，并在生成 Provider 中注册 `Passkeys::use_user_model::<User>()`。保留 `fortify.features` 中的 `passkeys`，重新构建并执行迁移。自定义 User 实现 `openrust_fortify::passkeys::PasskeyAuthenticatable`，并在应用 Provider 中显式注册。

配置 `fortify.passkeys.relying_party_id`、`allowed_origins`、`user_handle_secret` 与 `timeout`。默认从 `APP_URL` 推导 RP ID 和精确 Origin，密钥读取 `PASSKEYS_USER_HANDLE_SECRET` 并回退到 `APP_KEY`，超时为 60000 毫秒。生产环境使用 HTTPS；开发支持 HTTP localhost。Origin 包含端口，不包含尾部斜杠或路径。轮换应用密钥时保持用户句柄密钥稳定：句柄为模型表名和主键派生的 64 字节十六进制 HMAC-SHA256；更换密钥会改变新注册的句柄。

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| GET | `/user/passkeys/options` | 当前登录用户的注册选项 |
| POST | `/user/passkeys` | 验证并保存 `name` 和 `credential` |
| DELETE | `/user/passkeys/{passkey}` | 删除自己的凭据 |
| GET | `/passkeys/login/options` | 可发现凭据登录选项 |
| POST | `/passkeys/login` | 验证 `credential`、可选 `remember` 并登录 |
| GET | `/passkeys/confirm/options` | 限定当前账户的确认选项 |
| POST | `/passkeys/confirm` | 验证并记录密码确认时间 |

路由沿用 Fortify 的前缀、域名、中间件和 Guard。注册与删除默认要求近期密码确认；`fortify-options.passkeys.confirm_password=false` 可关闭此管理门禁。生成的 `passkeys` 命名限流对选项按 Session、对提交按凭据 ID 每分钟允许十次。包没有提供 GET 列表路由：在应用已认证的控制器中使用原生关系 `user.passkeys().get_results(&user).await?`，只返回界面所需字段。

固定官方客户端执行真实 WebAuthn 操作：

```js
import { Passkeys } from '@laravel/passkeys';
await Passkeys.register({ name: 'My device' });
await Passkeys.verify({ remember: true });
await Passkeys.verify({ routes: {
  options: '/passkeys/confirm/options', submit: '/passkeys/confirm'
} });
```

页面须提供 `csrf-token` meta 标签并携带 Session Cookie；配置前缀时覆盖客户端路径。注册 JSON 包含 `status`、`id`、`name`，验证 JSON 包含 `redirect`。Web 请求重定向并闪存注册／删除状态。通过原生容器替换 `passkeys::contracts` 中的四种响应契约。`Passkeys::authorize_login_using` 提供应用内异步授权回调，在密码学验证后执行。原生 `PasskeyRegistered`、`PasskeyVerified`、`PasskeyDeleted` 事件携带用户和已持久化模型的上下文。

注册要求常驻凭据与用户验证，支持 ES256／RS256，证明类型为 none。服务端 Session 将挑战绑定到操作和账户，并强制执行截止时间；原子缓存声明拒绝并发重用，MySQL 行锁保护独立挑战之间的计数器更新与回退检查。多进程部署必须共享 Session 存储以及支持原子 `add` 的缓存；array 驱动覆盖单进程。可发现凭据登录还会检查已存储的用户句柄。服务端状态丢失后须重新发起操作。

在应用 Provider 中调用 `Passkeys::use_passkey_model::<CustomPasskey>()` 选择实际 Eloquent 模型。为实现 `Model + Clone` 且具备凭据转换和必要列的类型实现 `PasskeyModel`；查询、插入、行锁验证及删除均使用其表名、主键、访问器和观察者。`Passkeys::passkey_model()` 与 `user_model()` 返回所选类型名。Rust 使用 `PasskeyRelation` 提供 `get_results`、`exists` 与 `create`，返回 `PasskeyRecord`。记录的 `get_key`、`get_attribute_value` 和 `to_array` 为异步方法；`record.model::<CustomPasskey>()` 返回异步互斥锁中的实际共享模型。调用其他记录方法前先释放该锁。事件的 `passkey::<CustomPasskey>(context)` 读取类型化快照，事件的 `passkey` 记录保留实际共享模型。

默认模型附加 `authenticator`，按注册 AAGUID 查询名称。`Aaguids::label_for`、`icon_for(id, "light")`、`all` 与 `flush` 使用固定上游资源。需要图标时，在模型 appends 中显式加入 `authenticator_icon_light` 与 `authenticator_icon_dark`。未知或缺失 AAGUID 返回 null，旧存储凭据缺少此字段仍可使用。none 证明模式下，名称和图标仅展示所报告的 AAGUID，不认证制造商身份。

独立发布资源时，在 Provider 列表中显式注册 `openrust_passkeys` 的 `PasskeysResourcesServiceProvider` 并重新构建，然后执行：

```sh
cargo run -- artisan vendor:publish --tag=passkeys-config
cargo run -- artisan vendor:publish --tag=passkeys-migrations
```

发布会保留自定义文件并避免重复迁移。将生成的 Rust 配置与迁移模块接入应用加载器及迁移列表，再重新构建并迁移。资源 Provider 只登记可发布文件；独立认证还需配置 `PasskeysServiceProvider`、用户模型、Guard 和中间件。Fortify 安装继续使用生成的 Fortify 配置。

PHP 凭据 JSON 不能直接作为 Rust 凭据存储格式。

安装器支持默认模型使用 `User::PASSWORD`、`User::REMEMBER_TOKEN` 的隐藏字段列表，也兼容等价的字符串写法。未知自定义结构仍保留原文件并要求手动配置。
