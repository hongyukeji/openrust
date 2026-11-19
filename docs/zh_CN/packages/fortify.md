# Fortify

## 简介

Fortify 提供认证路由和应用扩展契约，不包含用户界面。本页介绍按 Fortify 1.43.0 验收的 API-F01 实现。Passkeys 及其迁移仍待 API-F02；启动运行时 Provider 前，须从发布配置的功能列表中移除 `passkeys`。

## Fortify 与 Sanctum

Fortify 管理注册、Session 登录和凭据变更。[Sanctum](sanctum.md) 的 API 认证能力见对应章节。Fortify 复用现有的认证、会话、CSRF、哈希、验证器和通知服务。

## 安装

启用 `fortify` Cargo 功能后运行编译后的命令：

```sh
cargo +1.94.0 build --locked --features fortify --bin main
./target/debug/main artisan fortify:install
```

命令发布 `config/fortify.rs`、`app/actions/fortify/`、应用 Provider 和双因素字段迁移，注册 Rust 模块、配置和 Provider，并为默认 User 接入所需能力。已有发布文件会保留；无法识别的源码布局会明确要求手工注册。安装不会执行迁移。

修改发布配置：移除 `passkeys`；默认 User 须启用 `email-verification`；配置视图回调或将 `views` 设为 `false`。随后重新编译并迁移：

```sh
cargo +1.94.0 build --locked --features fortify --bin main
./target/debug/main artisan migrate --no-interaction
```

自动发现的 `FortifyResourcesServiceProvider` 仅提供安装资源。显式注册的 `openrust_fortify::FortifyServiceProvider` 启用运行时认证，生成的应用 Provider 连接动作、模型和命名限流。编译型应用在源码变更后需要重新构建。

## 配置与视图

安装默认使用 `web` 守卫、`users` 密码代理、`email` 用户名和邮件字段、用户名小写转换、空前缀和 `/home` 首页。这些是应用模板默认值，与包内默认配置不同。通过 `fortify.features` 选择功能，通过 `fortify-options.two-factor-authentication` 设置 `{"confirm":true,"confirmPassword":true}`；安装器会加载这一单独的 Rust 选项组。

`fortify.views=false` 移除界面路由，保留已启用的动作接口；Session Cookie 和 CSRF 仍有效。须提供名为 `password.reset` 的前端路由，或配置原生重置通知的 URL 生成，才能构建重置邮件。视图回调包括 `loginView`、`registerView`、`requestPasswordResetLinkView`、`resetPasswordView`、`verifyEmailView`、`confirmPasswordView` 和 `twoFactorChallengeView`。

## 认证

`POST /login` 接收配置的用户名、`password` 和可选 `remember`。普通 JSON 登录成功返回 `200` 和 `two_factor:false`，需要双因素挑战时返回 `two_factor:true`。认证成功会轮换 Session。`POST /logout` 使其失效并重新生成 CSRF 令牌。浏览器变更请求须携带会话 Cookie 和 CSRF 头。JSON 验证失败返回 `422`，命名限流返回 `429`。

通过 `Fortify::authenticateUsing` 定制身份查询，`confirmPasswordsUsing` 定制密码确认，`authenticateThrough` 替换登录流水线。默认流水线处理限流、用户名规范化、双因素跳转、认证和会话准备。Web 响应可通过 `fortify.redirects` 配置跳转。

安装的 `login` 限流器按转写后的小写用户名和 IP 每分钟允许五次请求，通过 `RateLimiter::forAsync` 读取真实请求输入。配置命名限流后会替换 Fortify 内置的失败计数限流。命名策略可返回 `Limit`、多个配额、`Unlimited` 或响应。

## 双因素认证

确认密码后，`POST /user/two-factor-authentication` 创建加密的 TOTP 密钥和八个恢复码。通过 `GET /user/two-factor-qr-code` 读取二维码 SVG，通过 `GET /user/two-factor-secret-key` 读取密钥；启用确认时，将 TOTP `code` 提交到 `POST /user/confirmed-two-factor-authentication`。

`GET /user/two-factor-recovery-codes` 列出恢复码，同一路径的 `POST` 重新生成恢复码。`DELETE /user/two-factor-authentication` 关闭该能力。默认模型在序列化时隐藏加密密钥和恢复码。

登录进入挑战后，使用同一 Session 将 `code` 或 `recovery_code` 提交到 `POST /two-factor-challenge`。兑换恢复码会替换该码。挑战状态、重放检查、命名限流和原生认证守卫均保留在请求流程中。应用可替换 `TwoFactorAuthenticationProvider` 契约。

## 注册与账户更新

`POST /register` 调用 `CreatesNewUsers`。安装的 `CreateNewUser` 验证姓名和邮件地址，通过 User 模型检查邮件唯一性，验证默认密码策略和确认字段，哈希密码并保存账户。JSON 成功返回 `201`。

`PUT /user/profile-information` 调用 `UpdatesUserProfileInformation`；邮件地址变化会清除验证状态并发送新的原生通知。`PUT /user/password` 调用 `UpdatesUserPasswords`，验证 `current_password:web`、哈希新密码、删除重置令牌并发出更新事件。安装的 `updateFromRequest` 独立读取真实 `web` 守卫，与待更新用户区分。直接调用默认动作而不提供请求时会明确失败；已有自定义动作仍可实现原 `update` 方法。

可在应用的 Fortify 实例上通过 `createUsersUsing`、`updateUserProfileInformationUsing`、`updateUserPasswordsUsing` 和 `resetUserPasswordsUsing` 替换动作。安装的密码规则为 `required`、`string`、`Password::default()` 和 `confirmed`。资料与密码的 Web 验证使用命名错误包。

## 密码重置

`POST /forgot-password` 接收配置的邮件字段，通过选定的密码代理发送原生重置通知。将 `token`、邮件地址、`password` 和 `password_confirmation` 提交到 `POST /reset-password`。安装的 `ResetsUserPasswords` 动作验证、哈希并保存代理提供的实际用户。完成后轮换 remember 令牌并发出 `PasswordReset`，不会自动登录。动作输入不合法时可以重试；成功消费的令牌不能再次使用。

JSON 重置结果包含翻译后的消息，失败使用验证错误；Web 结果使用跳转和 Session 状态。可实现密码响应契约替换这些响应，其 Rust `toResponse` 方法显式接收代理状态。

## 邮件验证

为实现 `MustVerifyEmail` 的用户启用 `email-verification`。注册和资料邮件变更使用原生验证通知，`POST /email/verification-notification` 重新发送。带签名的 `GET /email/verify/{id}/{hash}` 链接验证当前用户和当前邮件地址，更新验证状态并发出验证事件。可通过已有的 `verified` 中间件保护应用路由。实际投递须配置真实邮件传输；默认 `log` 邮件器仅记录消息。

## 密码确认

`POST /user/confirm-password` 验证 `password` 并记录确认时间。在有效期内，`GET /user/confirmed-password-status` 返回 `{"confirmed":true}`。配置要求时，凭据管理使用 `password.confirm`。应用可通过现有容器替换 `LoginResponse`、`RegisterResponse`、`PasswordUpdateResponse` 和双因素响应等契约；每个应用独立持有自己的替换实现。

## Passkeys
