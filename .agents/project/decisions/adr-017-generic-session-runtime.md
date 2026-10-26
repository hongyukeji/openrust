# ADR-017 通用 Session 与 Cookie 运行时

- 状态：Accepted（M3 contract；PostgreSQL 18 / MySQL 8.4 数据库 Session store、认证登录/退出 helper 与生成项目 HTTP 流程已验证）
- 日期：2026-10-25
- 依据：[ADR-004](adr-004-session-and-token-auth.md)、[ADR-012](adr-012-auth-current-user-contract.md)、[v1 能力覆盖](../plans/v1-capability-coverage.md)

## 背景

v1 要求通用 Session/Cookie 与认证共用一个存储入口，并支持匿名会话、轮换、失效、CSRF 和集群撤销。当前 `AuthLayer` 把 Cookie ID 直接交给认证器，`openrust_sessions` 又要求 `user_id` 并只存认证摘要，不能承载匿名 Session 或通用键值数据。

## 决策

- 复用 `tower-sessions` 0.15 的 `Session`、`SessionManagerLayer` 与 `SessionStore` 契约；通过 OpenRust 公共 API 暴露请求内 Session，认证只在该 Session 中读写保留命名空间的数据。PAT 继续独立。
- 官方数据库 store 复用 `Db` / SeaORM，日常开发与 CI 基线为 PostgreSQL 18 / MySQL 8.4 LTS；暂定最低支持为 PostgreSQL 14+ 与 MySQL 8.0+，完整最低版本能力矩阵在 v1 发布前验证，默认共享数据库。Session ID 不以明文持久化；按规范 ID 字节计算 SHA-256 查询键。Session 数据、过期时间和失效状态由同一存储记录承载。
- `create` 采用唯一约束的原子插入，冲突报 store 错误；`save` 只更新仍存在且未撤销的记录，禁止 upsert；`load` 对过期或撤销记录返回未命中；`delete` 在数据库提交后立即对其他节点的新请求生效。
- 上游 store contract 不提供版本号或 CAS。同一 Session 的并发写入按最后成功写入覆盖；调用方不得把该行为当作原子计数或锁。`cycle_id` 旧记录删除与新记录持久化不是一个原子迁移，旧在途请求的 `save` 不得恢复旧 ID。
- 错误 Cookie 仍按 ADR-012 作为无效凭据处理，不能被上游中间件静默创建的匿名 Session 掩盖。Cookie 名、Secure、HttpOnly、SameSite、Path、Domain 与 Max-Age 由 OpenRust 配置管理；生产环境安全默认值由实现 contract 确定。
- Session store 故障不得伪装成未命中或匿名身份。中间件保存失败必须进入统一 JSON 500 响应。认证流程需轮换 ID；退出/全局撤销需删除或撤销共享记录。
- 认证和通用 Session 使用同一 `SessionManagerLayer`、Cookie 和记录，不建立第二套认证会话表或 Cookie 解析器。表逻辑名遵循 ADR-016，默认物理名为 `tb_sessions`；可定制前缀/映射须与数据库命名解析器集成后启用。

## 认证登录 helper contract

- JWT 仍是默认登录模式。Session 是可选模式，调用 `AuthFlows::login_session` 显式选择；它校验同一已验证账户凭据并使用共享登录限流，但不签发 JWT 或 refresh token。
- 凭据校验成功后，helper 轮换当前 Session ID，再写入 `openrust.auth.user_id` 与 CSRF token 的 SHA-256 摘要；Session 中其他应用数据保留。密码错误或账户不可登录时，不修改 Session。
- helper 返回 `SessionCredentials`，只暴露账户 ID 与一次性 CSRF token；`Debug` 脱敏，不实现序列化。CSRF token 使用 CSPRNG 生成 256-bit 随机值，以 URL-safe base64 无填充形式返回。
- `AuthFlows::logout_session` 删除认证命名空间并轮换 ID，保留应用自己的 Session 数据；旧 ID 删除失败、Session 读取/写入失败均返回错误，不能报告成功或回退匿名状态。
- Session middleware 负责持久化与 Cookie 响应。helper 成功但中间件持久化失败时，统一 HTTP 错误响应生效；旧 ID 在轮换后不能被在途请求保存复活。
- 双库验收验证有效/无效登录、Session ID 变化、CSRF 摘要与一次性 token、旧 ID/并发旧请求失效、登出后旧 Cookie 失效、保留应用 Session 数据，以及 store 错误传播。

## 适用边界

- 同步数据库 store 是 v1 默认共享实现。Redis Session adapter 可按覆盖计划提供，但不得引入绕过认证失效语义的读缓存。
- 事务写入、计数器、锁、全局限流等需要明确原子性时使用相应数据库事务或独立能力，不通过普通 Session `get` / `insert` 拼出原子操作。
- `tower-sessions` 只提供运行机制，不代表 OpenRust 已交付插件接入、默认配置、Cookie 安全默认值、错误包装或双库验收。

## 验收

- PostgreSQL/MySQL 与两个独立应用实例验证匿名 Session、读取、过期、删除/撤销、ID 冲突和轮换期间旧请求保存。
- 错误 Cookie、CSRF、PAT 独立性及存储故障统一响应均有契约测试；生成项目/`apps/server` 使用同一公开 Session 入口。
- 验证并发写入的最后成功写入语义和非原子能力边界；用 Rust 1.97 实际检查依赖 MSRV。
