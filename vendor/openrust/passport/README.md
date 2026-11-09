# OpenRust Passport

Laravel Passport 的独立 OAuth 2.0 扩展，复用 OpenRust 的 Container、Provider、模型元数据、Hashing 与数据库连接。固定对应 Passport v13.10.0，并以 League oauth2-server 9.4.1 作为协议兼容参考。该参考版本独立于上游 Composer 锁定结果。

当前实现覆盖客户端模型与读取仓储、授权码与 PKCE、Scope、刷新轮换、Token HTTP 适配、RSA 访问令牌签名、认证密文封装和事务持久化。应用公开路由、完整客户端管理与模型保存生命周期、生产认证权威、TokenGuard、生成应用及端到端接线仍待交付。OIDC Provider、ID Token、Discovery、UserInfo 和双应用单点登录属于后续功能簇。

## 配置与身份

`GrantConfig` 默认授权码有效十分钟，访问与刷新令牌有效一个 UTC 日历年，刷新时撤销旧刷新令牌，默认 Scope 为空。`GrantLifetime::Seconds` 接受显式秒数，日历年保留闰日进位。PKCE 支持 `plain` 和 `S256`，公开客户端必须提供挑战。

认证中心默认沿用官方 Web 和 Session 机制。显式 API＋Bearer JWT 模式复用同一 Guard、UserProvider 与 Fortify，并要求身份与同意流程全程使用无 Session 或 Cookie 的适配。应用应分别验证用户身份和授权同意。第一方客户端或已有 JWT 都不足以证明用户同意。

默认 `DatabaseGrantRepository` 要求应用绑定 `DatabaseGrantAuthority`。它是 `GrantAuthority<TransactionContext<'tx>>` 的生命周期泛型对象别名。权威实现应使用 `context.is_from(&expected_db)` 核对数据库句柄来源，再用 `context.transaction()` 查询真实主体、客户端和父凭据。应用可显式绑定自己的 `dyn GrantStorage`，并在其原子上下文内实现同等末检。只有从当前认证与同意流程取得的绑定才能传给授权编排。`GrantAuthorityBinding` 本身仅承载未验证的不透明值。

Provider 惰性解析签发和验证密钥，并在请求作用域内解析唯一授权服务器及存储。四表迁移工厂由应用显式纳入迁移配置。

## 持久化与密钥

客户端模型工厂决定表、主键和连接。应用按实际 Schema 选择整数、文本或 UUID 用户键。自定义工厂须保证协议 ID 唯一且可精确反解。

授权码、访问令牌与刷新令牌复用官方表的关联。存储保留记录与绑定信息。授权码与刷新 bearer 由框架 ContextualEncrypter 使用不同的认证上下文封装，密文格式为 OpenRust 所用。默认仓储在同一主库事务内重新读取当前客户端，使用条件更新消费凭据，并将令牌写入与消费一起提交。提交结果未知时，调用方须将其作为独立失败状态处理，避免自动重试签发。

`OAuthAccessSigner::from_pem(private)` 和 `OAuthAccessVerifier::from_pem(public)` 分别构造签发与验证能力。`validate_key_pair(private, public)` 可显式预检配对。`verify_at(token, expected_client, now)` 适用于绑定目标客户端的显式验证。生产 ResourceServer 的默认校验及撤销检查仍待接入。签发的 JWT 将唯一客户端写为字符串 `aud`。显式验证可读取字符串或字符串数组，并要求其中恰有一项且等于预期客户端。时间字段精度与密钥材料加载阶段仍待对齐。

## Token HTTP 适配

应用可以将同一请求作用域解析出的 `Arc<AuthorizationServer>` 传给 `AccessTokenController::new`。`issue_token(&Request)` 读取 Core 已提取的 JSON、表单或 multipart 文本字段，使用第一条 Authorization 请求头的原始字节，并直接返回 OAuth 协议响应。授权及同意路由由应用注册。

HTTP Basic 的密码解析及校验保留原始字节。客户端 ID 和已编码哈希使用文本载体，客户端秘密写入的二进制载体仍待接入。Laravel 空 JSON bag 的原文回读、畸形输入、标量、大整数与数字键合并仍待对齐。
