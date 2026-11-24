# 入门套件

## 支持状态 {#support-status}

**完整安装预设与 UI 尚未提供。** 尚未提供现成套件 UI、控制台页面或 React／Vue／Livewire 套件。原生 init 向导与 Fortify 提供应用装配和无界面认证，页面由应用自行实现。

本章保留框架导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

## 可用入口 {#available-entry-points}

原生接口及用法见 [Fortify](../packages/fortify.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

## 范围 {#scope}

OpenRust 不提供此集成的安装命令。不要因存在此导航入口就将框架Composer 或 Artisan 命令照搬到 OpenRust。

## 可选 JSON 响应 {#json-responses}

完整套件安装预设与 UI 尚未交付。已配置好 Fortify Action 与路由的应用，可显式使用响应适配。

在 Fortify Provider 之后注册 `openrust_fortify::starter::StarterKitServiceProvider`。HTML 请求保留原生重定向和闪存；JSON 登录、注册及双因素登录完成后，通过 `ApiResponse` 返回当前用户，注册使用201。更新省略空数据，退出与邮箱验证保留204空正文。消息跟随当前应用语言。

```rust
Box::new(openrust_fortify::starter::StarterKitServiceProvider),
```

默认 `UserResource` 只读取 `id`、`name`、`email`、`email_verified_at`，保留字符串主键，不枚举模型属性或读取凭据。应用可替换原有 Fortify 响应绑定来使用自己的资源。认证成功响应设置 `Cache-Control: no-store`。此适配不安装页面、不改变认证守卫，也不签发个人令牌。
