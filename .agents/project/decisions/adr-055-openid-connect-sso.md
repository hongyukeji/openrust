# ADR-055：OIDC 双应用 SSO 与统一退出

- 状态：Accepted
- 日期：2026-10-27

## 决策

- 继续复用 ADR-053 的静态 `ExternalIdentityProvider` 与 `ExternalIdentityFlows`，不新增 OAuth 授权服务器、通用身份中心或独立 session 管理器。每个应用分别配置自己的 OIDC client、redirect URI、密钥和本地用户；共享 IdP 浏览器会话只免除重复 IdP 认证，不共享应用 JWT、Cookie、角色或签名密钥。
- `VerifiedExternalIdentity` 增加可选 IdP `sid`；只持久化其摘要，不保留原始 ID Token。公开 Debug、日志和审计均不得输出 `sid`。无 `sid` 的兼容 provider 仍可登录，但无法区分 IdP session 粒度的退出。
- 新增 `ExternalLoginResult` 与 `ExternalLoginHandle`。保留现有 `complete` 返回类型以兼容已交付 OIDC 调用；新增 `complete_with_handle` 为集成应用返回经认证的 `LoginAttempt` 和不透明关联句柄。MFA challenge 也与关联句柄原子保存，只有完成 MFA 后才允许建立应用 Session。
- 应用必须将待处理 MFA 状态与关联句柄存入受保护的服务端 Session；多副本时 Session 存储必须跨节点共享。应用可从其安全存储的暴露值通过 `ExternalLoginHandle::from_exposed` 恢复句柄；进程内状态与粘性会话不构成跨节点保证。
- 新增 `attach_session(user_id, handle, Session)`：验证 Session 中已有账户 ID 与调用者一致，轮换后保存 Session ID 的加密值。`SessionPlugin` 注册共享 `SessionStoreHandle`，Back-Channel Logout 通过该公开窄接口删除关联 Session；数据库与 Redis 自定义 Session store 均沿用实际 store，不创建第二套存储。
- `begin_logout(user_id, handle, post_logout_redirect_uri)` 要求应用先用其 JWT/Session 授权路由保护，并验证句柄属于该 user/provider。它先在共享数据库事务撤销该关联的 refresh family、MFA challenge、Session 并写审计，再返回经过 Provider 校验的 RP-Initiated Logout URL。Logout endpoint、client ID 与退出回调必须是固定 provider 配置；不接受用户自选 URL。未配置 IdP logout endpoint 时只报告本地退出。
- `ExternalIdentityProvider` 增加默认拒绝的 Back-Channel Logout Token 验证入口。内置 `OidcProvider` 独立校验签名算法/JWKS、issuer、client audience、`iat`、`jti`、events、禁止 nonce、`sid`/`sub` 约束；Logout Token 不复用要求 nonce 的 ID Token 验证入口。自定义 Provider 必须明确实现相同验证 contract 才能启用退出接收。
- 新增共享关联和 replay migration：provider 名称绑定静态 client 配置，数据库按 provider、issuer 与 `sid` 或 subject 哈希匹配；`jti` 摘要唯一保证跨节点幂等。sid logout 仅撤销该 IdP session，只有无 sid 的标准 subject logout 才撤销该 provider/client 下该 subject 的所有关联会话；不撤销其他 provider、其他账户、PAT 或无关设备。MFA challenge hash 与关联行一同保存，并与 challenge completion 使用相同的“关联行→challenge 行”锁顺序。
- 并发退出统一按 logout scope → 外部登录关联行 → MFA challenge → 账户行 → refresh family token 行的顺序。callback 依序锁 sid（如有）与 subject scope 后再创建关联；MFA challenge 完成先锁关联行，再消费 challenge 并绑定 refresh family。refresh 轮换与 family 撤销都先锁所属账户行，保证撤销事务不会因语句快照漏掉并发插入的后继 token。退出先提交时，旧回调/challenge 不得重新签发有效 refresh；完成先提交时，退出随后撤销完整 refresh family。退出事务同时删除已关联的 MFA challenge。批量关联按句柄哈希顺序加锁。重复/已处理 jti 返回幂等成功；若 Session store 删除未完成，重放通知仅重试该清理，不重复撤销其他副作用。
- 本地退出及 Back-Channel Logout 撤销 refresh、MFA challenge 与 Session；stateless JWT 继续本地验签，最大访问残留窗口为应用 `[auth.jwt].max_valid_ttl_secs`，配置上限 3600 秒，默认上限 3600 秒、默认实际 access TTL 900 秒。V1 不宣称 JWT 即时撤销；需要拒绝访问时限更短的业务可使用 Session 或在应用授权层增加共享检查。
- OIDC logout 请求、state 与 callback 使用标准 HTTP 状态，不套用 `HttpStatusMode::AlwaysOk` JSON envelope；JWT、logout token、ID Token、state、association handle 与 Session ID 不进日志。回调 state 一次消费、按显式 URI allowlist 验证且有 TTL；退出失败不能恢复已撤销的本地凭据。
- Session store 删除在数据库提交后执行。RP 删除失败时，同一授权用户可用同一关联句柄重试清理；后通道删除失败时，IdP 可重放同一 `jti` 重试清理。成功 callback 会释放尚未成为撤销墓碑的 scope 行；过期墓碑按索引分批、每批最多 100 行回收。

## 当前实现边界

- 框架提供事务服务 API，不自动注册应用 HTTP logout/callback 路由；应用负责把协议端点映射到标准 HTTP status、授权层和配置。Back-Channel Logout 的 Session store 删除与数据库事务无法原子提交，依靠相同 `jti` 的安全重试完成残留清理。
- PostgreSQL 18/MySQL 8.4 provider fake 隔离库覆盖关联、refresh/Session/challenge 撤销、重放、过期 callback、审计及未登记退出 URI；真实 Keycloak 26.8.1 验收则由两个隔离应用、每应用两个独立 API 子进程、loopback TCP round-robin 与 Redis 8 共享 Session 构成。双库均通过双 client SSO、真实应用 callback/RP logout/Logout Token、跨节点 TOTP completion、退出通知撤销 pending MFA challenge 后拒绝挂起的 completion、refresh/logout 并发、退出后各子节点拒绝读取 Session/轮换 refresh、节点重启后重放幂等及 15 秒 access JWT 到期窗口。refresh/logout 竞争修复为 family 撤销与轮换共用账户行锁。Redis driver 上另注入一次 delete error 并验证同句柄重试成功。真实 Redis 容器停机时 Back-Channel Logout 返回 503；Redis 恢复后重启两个应用的四个 API 子进程，以相同 Keycloak Logout Token 重放成功，PostgreSQL 18 与 MySQL 8.4 均通过。

## 验收

- Keycloak 两 client、两个独立应用各至少两个 API 实例；同一浏览器先登录 A 再登录 B 不再输入凭据，分别保留应用用户/JWT/Session 隔离。
- PostgreSQL 18、MySQL 8.4 均以两个 API 子进程经真实 TCP 负载均衡验收 callback、MFA completion、RP/IdP logout、refresh 与 Session；logout 期间挂起的 MFA completion 被 challenge 撤销后拒绝，挂起的 OIDC callback 在退出后返回 401。refresh/logout 并发、退出后两节点直接拒绝共享 Session/refresh、节点重启重放、伪造与错误 client token 拒绝及 15 秒 JWT 窗口均通过。另以隔离 Redis 容器执行真实停机，验证通知返回 503；恢复服务并重启四个 API 子进程后，相同真实 Logout Token 重放成功，PostgreSQL 18 与 MySQL 8.4 均通过。M6 仍按完整集群计划复验。
- PostgreSQL 18、MySQL 8.4 均以两个 API 子进程经真实 TCP 负载均衡验收 callback、MFA completion、RP/IdP logout、refresh 与 Session；logout 期间挂起的 MFA completion 被 challenge 撤销后拒绝，挂起的 OIDC callback 在退出后返回 401。refresh/logout 并发、退出后两节点直接拒绝共享 Session/refresh、节点重启重放、伪造与错误 client token 拒绝及 15 秒 JWT 窗口均通过。另以隔离 Redis 容器执行真实停机，验证通知返回 503；恢复服务并重启四个 API 子进程后，相同真实 Logout Token 重放成功，PostgreSQL 18 与 MySQL 8.4 均通过。M6 仍按完整集群计划复验。
- 以配置的 `max_valid_ttl_secs` 验证刚签发 JWT 的最后有效窗口；报告实测值和不能即时撤销的边界。至少出现 API/worker 进程重启后重复或重放通知仍保持幂等的证据。
- 缺少 Keycloak、负载均衡或真实双库环境时可继续实现及其他可行验证，但必须将该外部集群验收留为未完成，不能以 fake 或双副本进程冒充通过。
