# 开发者文档边界整理：docs/zh_CN/packages/oidc.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_CN/packages/oidc.md (迁移前)
+++ docs/zh_CN/packages/oidc.md (开发者正文)
@@ -117,7 +117,7 @@

 ## 浏览器 SSO 与退出 {#sso-logout}

-API-O02 验证两个独立 RP 应用连接同一受控 OP：客户端 ID、应用密钥、数据库和本地用户各自隔离。首次在 OP 登录后，第二个 RP 可借助已有 OP 浏览器 Session 授权；每个 RP 仍拥有独立本地 Session，并分别解析 `(issuer, subject)`。`prompt("login")` 请求 OP 重新认证，`maxAge` 另行检查返回的 `auth_time`。
+为每个 RP 分别配置客户端 ID、应用密钥、数据库和本地用户绑定。首次在 OP 登录后，第二个 RP 可借助已有 OP 浏览器 Session 授权；每个 RP 仍拥有独立本地 Session，并分别解析 `(issuer, subject)`。`prompt("login")` 请求 OP 重新认证，`maxAge` 另行检查返回的 `auth_time`。

 RP 发起退出遵循 OpenID Connect RP-Initiated Logout 1.0（2022-09-12）。将 `OIDC_POST_LOGOUT_REDIRECT_URI`（`post_logout_redirect`）设为 OP 已登记的固定回调。在 `user()` 及本地账户绑定成功后，调用 `Oidc::provider("work", request)?.rememberLogout(&authentication)?`，显式将已验证的 ID Token 保存到服务端 Session。`LogoutHint` 是不透明类型，修改公开 token 字段不能替换捕获的已验证提示。Session 存储和发出的退出 URL 均按敏感数据处理。

@@ -169,9 +169,9 @@

 固定返回处理器调用 `Oidc::provider("work", request)?.completeLogout().await?`，再返回本地已退出页面。完成时校验 Session 绑定、配置、目标、期限与共享原子重放声明；失败同样消费本次尝试，且不会登录任何用户。此结果确认返回状态，不证明全部远端凭据均已撤销。

-OP 浏览器 Session 结束后，另一个 RP 的既有本地 Session 仍有效。已有 access／refresh token 遵循 OP 独立的撤销策略。本实现不提供 front-channel／back-channel 退出通知及全局单点退出。浏览器验收覆盖并发授权、强制重新认证、issuer／绑定拒绝、CSRF、本地／OP 退出、并发／过期／重放返回状态、OP 不支持及 bearer token 边界。夹具为受控测试 OP，不是生产身份提供方。
+OP 浏览器 Session 结束后，另一个 RP 的既有本地 Session 仍有效。已有 access／refresh token 遵循 OP 独立的撤销策略。本实现不提供 front-channel／back-channel 退出通知及全局单点退出。


 ## 测试与部署 {#testing}

-受控供应商测试使用真实 TCP HTTP、RSA 签名、同 kid 轮换、Session 中间件和 MySQL 8.4。两个 RP 副本在加载同一回调 Session 后，通过共享数据库缓存原子认领 state，只有一个请求调用 token 端点。另覆盖 PKCE／客户端认证、声明／Discovery／UserInfo 失败、过期或替换尝试、跨应用 Cookie、本地绑定和同邮箱不合并。真实第三方授权需供应商 issuer、已登记测试应用、客户端凭据、获准回调与测试账户。API-O02 浏览器结果与退出边界见上文。
+在身份提供方注册应用和回调，配置 issuer 与客户端凭据，并使用获该应用授权的账户。多个 RP 副本须共享 Session 存储和支持原子操作的数据库或 Redis 缓存，防止回调重放。
````
