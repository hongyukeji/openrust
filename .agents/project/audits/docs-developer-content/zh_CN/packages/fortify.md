# 开发者文档边界整理：docs/zh_CN/packages/fortify.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_CN/packages/fortify.md (迁移前)
+++ docs/zh_CN/packages/fortify.md (开发者正文)
@@ -2,7 +2,7 @@

 ## 简介

-Fortify 提供认证路由和应用扩展契约，不包含用户界面。本页介绍按 Fortify 1.43.0 验收的 API-F01 认证与 API-F02 Passkeys 生命周期。
+Fortify 提供认证路由和应用扩展契约，不包含用户界面。认证 API 对应 Fortify 1.43.0。

 ## Fortify 与 Sanctum

@@ -74,7 +74,7 @@

 ## Passkeys

-API-F02 生命周期按 Laravel Passkeys 服务端 0.3.0 实现，并对接已验收的 `@laravel/passkeys` 0.4.1 客户端。`fortify:install` 发布 `passkeys` 表迁移，为默认 User 添加 `PasskeyAuthenticatable`，并在生成 Provider 中注册 `Passkeys::useUserModel::<User>()`。保留 `fortify.features` 中的 `passkeys`，重新构建并执行迁移。自定义 User 实现 `openrust_fortify::passkeys::PasskeyAuthenticatable`，并在应用 Provider 中显式注册。
+Passkeys API 对应 Laravel Passkeys 服务端 0.3.0，配合 `@laravel/passkeys` 0.4.1 客户端使用。`fortify:install` 发布 `passkeys` 表迁移，为默认 User 添加 `PasskeyAuthenticatable`，并在生成 Provider 中注册 `Passkeys::useUserModel::<User>()`。保留 `fortify.features` 中的 `passkeys`，重新构建并执行迁移。自定义 User 实现 `openrust_fortify::passkeys::PasskeyAuthenticatable`，并在应用 Provider 中显式注册。

 配置 `fortify.passkeys.relying_party_id`、`allowed_origins`、`user_handle_secret` 与 `timeout`。默认从 `APP_URL` 推导 RP ID 和精确 Origin，密钥读取 `PASSKEYS_USER_HANDLE_SECRET` 并回退到 `APP_KEY`，超时为 60000 毫秒。生产环境使用 HTTPS；开发支持 HTTP localhost。Origin 包含端口，不包含尾部斜杠或路径。轮换应用密钥时保持用户句柄密钥稳定：句柄为模型表名和主键派生的 64 字节十六进制 HMAC-SHA256；更换密钥会改变新注册的句柄。

````
