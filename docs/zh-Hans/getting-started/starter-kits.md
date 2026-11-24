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
