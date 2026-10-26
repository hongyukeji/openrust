# ADR-054：TOTP 多因素认证与恢复码

- 状态：Accepted
- 日期：2026-10-27

## 决策

- `auth-mfa` 是可选的本地认证能力，复用 `AuthFlows`、`Encryptor`、Argon2id `Passwords`、共享 `RateLimiter`、`RefreshTokens` 与事务内数据库审计；不实现 Passkeys、WebAuthn 或短信 MFA。
- 由 `MfaPlugin` 显式启用，要求先注册数据库、认证、加密、共享限流与数据库审计服务；应用同时注册 `AuthMfaMigration`。未启用该插件的应用保持现有 JWT-first 行为。
- Enrollment 与因子管理要求本地密码重新认证和可信请求来源。新 TOTP seed 由 `getrandom` 生成，使用 `Encryptor` 当前密钥加密后存储；历史密钥仅用于解密。Enrollment 先保存有时限的待确认 seed，只有成功验证 TOTP 后才激活。provisioning URI 和恢复码只在对应操作的响应中一次性返回，不进入日志、Debug 或审计详情。
- TOTP 遵循 RFC 6238：HMAC-SHA1、30 秒时间步、6 位十进制码、当前时间步前后各一个窗口。数据库事务锁定因子并记录最后接受的时间步，同一时间步及更早时间步不能再次使用。时钟回拨期间可能暂时拒绝较早窗口的有效码，这是防重放的预期失败语义。
- 登录对 MFA 账户采用两步流程：密码验证产生随机、不透明、仅存 SHA-256 摘要的 challenge；challenge 绑定账户、可信请求来源摘要、用途，5 分钟过期且最多 5 次验证。一次性消费、TOTP 重放状态、refresh 凭据与成功审计在同一数据库事务提交。失败或过期不签发任何完整权限凭据。
- 已有 `AuthFlows::login` 与 Session 登录在 MFA 已启用时 fail closed；新增统一登录结果供应用处理直接登录成功或 MFA challenge。OIDC 登录使用相同结果与 challenge，不允许外部身份路径绕过因子。
- 恢复码由操作系统 CSPRNG 生成至少 160 bit 熵，仅存 SHA-256 摘要并只显示一次；高熵随机恢复码无需使用面向低熵密码的慢哈希。恢复码、TOTP challenge 都以数据库条件更新/事务行锁确保跨节点单次消费；共享限流保护登录 challenge、验证和因子管理。
- 启用、禁用与恢复码重置均要求本地密码和第二因子重新认证；成功后撤销该账户全部 refresh token，并与事务内审计原子提交。无状态 access JWT 在其原有效期内无法即时撤销，最长窗口由应用 JWT access TTL 控制。
- 任意实现若缺少 MFA migration、加密/审计/限流服务或不支持 PostgreSQL/MySQL，应在启动时失败；不降级为内存或明文存储。MFA 对已有 JWT 与 Cookie Session 的失效边界按上述短期 access token 说明，不宣称即时全局撤销。

## 验收

- RFC 6238 独立已知向量、格式/时间窗边界、错误尝试上限、时间步重放与并发竞争。
- Enrollment 未确认前不影响账户状态；当前/历史密钥轮换解密；恢复码哈希落库、一次展示、并发仅一方消费。
- password-only、Session、OIDC 及 MFA 前置路由不能绕过 MFA；challenge 过期、来源不匹配、重放与错误耗尽均不签发凭据。
- PostgreSQL 18 与 MySQL 8.4 各执行真实数据库 HTTP 集成，覆盖事务内审计、refresh 撤销、挑战并发和恢复码并发；workspace 门禁通过。
