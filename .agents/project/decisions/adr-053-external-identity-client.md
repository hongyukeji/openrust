# ADR-053：外部 OAuth2/OIDC 身份客户端

- 状态：Accepted
- 日期：2026-10-27

## 决策

- 外部身份作为可选 `auth-oidc` 能力，保持 JWT-first；不实现 OAuth 授权服务器、SAML 或 Passkeys。
- 使用 `oauth2` 提供授权码、S256 PKCE 与令牌交换，复用框架现有 `jsonwebtoken`（aws-lc-rs backend）校验 ID Token 签名；框架只实现必要的 OIDC Discovery、JWKS 选择/刷新、issuer/audience/azp/时效/nonce/`at_hash` 校验。两个库均通过自定义异步 HTTP client 接入命名 `DependencyClient`；禁止绕过 ADR-051/052。
- 不采用 `openidconnect` 4.0.1：该 crate 无条件依赖 `rsa` 0.9.x，`cargo deny check` 命中 RUSTSEC-2023-0071 与 `crypto-bigint` RUSTSEC-2026-0387。关闭默认 features 不能移除这条依赖；不通过忽略安全公告保留它。
- 仅允许应用静态配置 provider、issuer、client ID、redirect URI、scope 与密钥。Discovery 的 issuer 必须精确匹配配置 issuer；发现到的 endpoint 仍须通过目标策略；禁止 HTTP 重定向。每次登录共享一个总超时预算与有界响应正文。
- Provider 接口表达授权 URL 和经验证的外部 principal；provider key、issuer 与 subject 构成唯一外部身份。不得按 email 自动合并账户。新建本地账户只接受 IdP 明确验证的 email；email 已被本地账户占用时要求本地重新认证后显式绑定。
- state、nonce、PKCE verifier 作为短时一次性事务保存在数据库，以 state 摘要寻址并使用现有 Encryptor 加密秘密字段。另生成独立浏览器绑定随机值，仅保存其摘要；应用须将其作为十分钟、`HttpOnly; Secure; SameSite=Lax` Cookie 写回，并在回调时传回。回调校验后先原子消费事务；错误浏览器绑定/回调不会创建账户或身份绑定，重放拒绝。事务 TTL 有界且提供批量清理。
- 外部 principal 解析完成后复用 `AuthFlows` 签发既有 JWT/refresh 凭据及登录事件；身份行与新账户/登录审计在一个数据库事务内完成。协议 bearer、authorization code、state、nonce 和 verifier 不进入日志或错误。
- 双库必须验证 migration、并发唯一绑定、同邮箱冲突、事务回滚与回调重放。至少一个真实兼容 OIDC Provider 运行授权码+PKCE、issuer/nonce/PKCE 拒绝及 JWKS 轮换；fake 只覆盖单元边界。

## 依据与影响

复用现有 AuthFlows、JWT、加密、Database、Audit、RateLimiter、jsonwebtoken 的 aws-lc-rs 验签 backend 和 ADR-052 的出站隔离。Discovery、token 和 JWKS 读取同享单次登录 deadline，响应体上限 1 MiB；JWKS 成功缓存最多 5 分钟，过期必须刷新且刷新失败时拒绝登录，未知 `kid` 的主动刷新最多每 30 秒一次。新增依赖与 feature 关系记录在 `stack.md`。

实现先交付 OAuth2/OIDC 外部身份；TOTP 与恢复码作为后续独立 F7 子簇，不能由此 ADR 宣称已完成。
