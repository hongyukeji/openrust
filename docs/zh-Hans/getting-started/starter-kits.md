# 入门套件

## 支持状态 {#support-status}

显式用户套件预设可安装原生 Fortify Action、安全用户响应和基础 Blade 认证页面。完整账户中心、个人令牌登录协议和可选因素 UI 仍在开发，不能据此视为已完成。不会生成 React、Vue 或 Livewire 前端。

## 可用入口 {#available-entry-points}

```shell
./artisan init --starter-kit=full --extensions=api,fortify
# 仅 API，并保留邮件链接所需的最小网页：
./artisan init --starter-kit=api --extensions=api,fortify
```

先配置应用数据库和邮件。命令发布资源、注册 Provider、重建并处理迁移；自动化可在配置完成后使用 `--no-interaction`。`all` 不会自动安装用户套件，两种预设只能选择一种。

两种预设都启用第一方 stateful API 认证。完整预设增加登录、注册和本人页面；API 预设仅保留邮件链接落地页面。API 路径遵循 `config/api.rs` 和 `API_PREFIX`：受保护用户接口默认为 `/api/user`，设置 `API_PREFIX=api/admin` 后为 `/api/admin/user`。原有公开 API 信息路由保持可用。

Cookie 客户端须先获取 CSRF Cookie，并发送最新的 `X-XSRF-TOKEN`，登录导致会话轮换后也要更新。请求 JSON 不代表签发个人令牌。参阅 [Sanctum](../packages/sanctum.md) 和 [Fortify](../packages/fortify.md)。

## 配置与已有应用 {#scope}

生成的 `config/user.rs` 控制 `USER_STARTER_PROFILE`、注册、邮箱验证和可选因素。注册和验证默认启用，额外因素默认关闭。原有 Guard 和认证前缀保持有效。使用验证与密码找回前，应配置可用的邮件传输。

重复安装保留已发布文件。`--force` 显式替换所选生成资源，执行前应检查业务修改。无法安全识别的自定义 Provider 列表、bootstrap 中间件或 API 用户处理器需要手动接线，不会静默覆盖。标准用户处理器之外的业务路由会保留。

## 可选 JSON 响应 {#json-responses}

生成的应用 Provider 自动安装 `openrust_fortify::starter::StarterKitServiceProvider`。已有 Fortify 应用也可仅注册响应适配，不安装页面：

```rust
Box::new(openrust_fortify::starter::StarterKitServiceProvider),
```

HTML 保留原生重定向和闪存。JSON 登录、注册与双因素登录完成后，通过 `ApiResponse` 返回当前用户，注册使用201。更新省略空数据；退出和邮箱验证保留204空正文。消息跟随应用语言，认证成功响应使用 `Cache-Control: no-store`。

`UserResource` 只读取 `id`、`name`、`email`、`email_verified_at`，保留字符串主键，不枚举属性或读取凭据。自定义资源可替换原有 Fortify 响应绑定。单独使用响应适配不会签发令牌或改变 Guard。

## 邮件链接与密码会话 {#mail-and-sessions}

套件使用 Fortify 原生的 `POST /forgot-password`、`POST /reset-password`、`PUT /user/profile-information` 和 `PUT /user/password`。JSON 客户端发送 `Accept: application/json`；Cookie 认证须同时携带当前 CSRF Cookie。成功更新返回200，字段或重置凭据无效返回422。验证链接要求本人已认证、当前邮箱哈希匹配、签名有效且未过期。JSON 验证成功使用204空正文，重复验证保持幂等。更换邮箱会清除验证状态并向新地址发送链接。

API 预设也提供最小登录、密码重置与验证网页。未登录浏览器打开验证邮件后，登录仍回到原始签名 URL。验证收据读取数据库状态，添加 `?verified=1` 不能让账户变成已验证。依赖邮件投递前须配置传输。设置 `USER_EMAIL_VERIFICATION=false` 会关闭验证路由及这类验证邮件，密码找回保持可用。

已安装套件在登录和有效 remember Cookie 恢复时立即保存现有密码指纹，Guard 在后续认证读取中拒绝缺失或过时指纹。修改密码保留当前浏览器会话并拒绝其他密码会话；重置还会轮换 remember token。独立认证的默认行为不变。

## 凭据生命周期 {#credential-lifecycle}

显式安装套件提供两份迁移模板（发布时按 Laravel 机制生成时间戳文件名）：`0001_01_01_000020_add_credential_version_to_users_table.rs` 和 `0001_01_01_000021_add_credential_stamp_to_personal_access_tokens_table.rs`，跟随所配置的用户及令牌模型表名和连接。升级应用开始服务前先执行正常迁移。启用此策略后，既有会话须重新认证，既有个人令牌须重新签发；不要手动重置凭据版本。

生成的 User 保留 Sanctum 的 `create_token` 入口。在已认证的应用作用域内使用，先完成所需的近期认证及额外因素；此服务不会替调用方执行这些 HTTP 检查：

```rust
use openrust::sanctum::HasApiTokens;

// `user` is the authenticated application User; authenticate before issuing.
let token = user.create_token(
    &sanctum,
    &database,
    "phone",
    &["account:read"],
    None,
).await?;
let plaintext = token.plain_text_token;
```

`user.token_expiration_days` 默认30天。显式传入更短期限会保留，更长期限会被限制；Sanctum 全局期限可以更早拒绝令牌。`user.token_abilities` 是服务端能力白名单，未知或空能力会报错。明文只返回一次，数据库仅保存摘要及隐藏的凭据标记，勿写入日志。过期记录继续用 `sanctum:prune-expired` 清理，认证不等待清理才拒绝过期令牌。

生成的密码 Action 在修改密码的同一事务内递增持久化用户凭据版本，保留当前浏览器登录，使其他会话及个人令牌失效；密码重置使全部旧登录失效。普通密码哈希升级及原生“退出其他浏览器会话”不会撤销个人令牌。自定义 Action 应调用 `openrust_fortify::starter::credentials::change_password`，直接写数据库密码不会自动推进此策略。`credentials::revoke_all` 也支持无本地密码账户，调用方须先取得有效的近期认证。

修改凭据与签发令牌会在提交前锁定并重新核对当前身份／版本，要求用户和令牌使用同一数据库连接身份；独立配置的令牌连接会明确失败，不假装具有原子安全保证。自定义用户模型实现 `Clone` 并保留 `credential_version`；自定义令牌模型保留 `credential_stamp` 并在序列化中隐藏。含瞬态字段的令牌模型可覆写 `PersonalAccessTokenModel::clone_token_model` 返回 `self.clone()`，使属性访问器保留这些状态。

以上为生命周期服务和生成的密码 Action。设备令牌登录／挑战接口及完整账户中心仍在开发。仅注册响应适配器不会启用凭据策略。
