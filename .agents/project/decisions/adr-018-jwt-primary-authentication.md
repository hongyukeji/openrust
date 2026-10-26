# ADR-018 API 默认 JWT 与可选 Session

- 状态：Accepted（JWT runtime、AuthFlows 与路由已实现；PostgreSQL 18 / MySQL 8.4 AuthFlows、Session/PAT 授权及生成项目 HTTP 矩阵通过）
- 日期：2026-10-25
- 替代：[ADR-004](adr-004-session-and-token-auth.md) 的默认认证方式；补充 [ADR-012](adr-012-auth-current-user-contract.md) 的 credential 路由 contract 与 [ADR-017](adr-017-generic-session-runtime.md) 的可选 Session contract。

## 背景

OpenRust 的主要场景是前后端分离 API 服务，因此用户登录后应以 Bearer JWT 访问 API。Cookie Session 仍是通用框架能力，可供服务端渲染网页、同源浏览器应用或明确需要服务端会话状态的项目选择。

## 决策

- v1 主要认证路径为账号登录签发短时效 JWT access token，客户端通过 `Authorization: Bearer <token>` 请求 API。路由默认认证模式和生成项目示例以该路径为主；缺失、格式错误、签名不合法、过期或受众不匹配的凭据均不得降级为匿名。
- JWT 使用严格配置的签名算法、issuer、audience、有效期和密钥 ID；验证端不得接受未配置算法、错误 issuer/audience、未生效或过期 token。私钥/密钥轮换复用 v1 通用加密与密钥轮换能力，集群节点使用一致的受控密钥集合。
- access token 默认短期有效。续期使用高熵、不透明 refresh token；数据库只保存哈希，刷新时轮换并撤销旧 token，检测重复使用并撤销对应 token family。登出、密码重置、账户禁用和安全事件按明确规则撤销 refresh token。
- JWT access token 在默认模式下不会逐请求查询数据库；因此单个 access token 的撤销最迟在其过期时生效。需要立即撤销时由应用启用共享存储的 denylist、账户 token-version 检查或等价能力；不能依赖单节点内存状态，也不能声称纯无状态 JWT 可立即全局撤销。
- Cookie Session 保留为显式可选模式，使用 [ADR-017](adr-017-generic-session-runtime.md) 的通用共享 Session/Cookie 运行时；通过 Session 认证的 unsafe method 必须校验 CSRF。选择 JWT 的 API 路由不要求 Session 或 CSRF Cookie。
- PAT 保留为与用户登录 JWT 不同的可选凭据类型，面向人工创建的自动化或服务集成密钥；只存哈希，支持能力、过期和撤销。API 登录默认路径与生成示例不得把 PAT 当作 JWT。
- `CurrentUser`、可选身份、授权 Policy、账户模型与注册/登录钩子继续复用现有认证 contract；所有认证模式输出同一已验证主体，来源类型清楚区分 `Jwt`、`Session` 与 `PersonalAccessToken`。
- ADR-012 当前的 credential 与 `AuthSource` 仅覆盖 Session/PAT；实施时必须扩展为 JWT，并为 claims、过期语义和 token abilities 明确定义验证边界，不能仅把 JWT 字符串当作 PAT 传入旧路径。
- 默认模式可按应用覆盖；公共 API 命名保持 Rust 惯例，协议语义遵循 Bearer token/JWT 标准。不为迁就某类前端将 JWT 放入 URL、日志或默认暴露给浏览器 JavaScript。

## 影响与迁移

- JWT、refresh token 轮换与撤销进入 v1 认证交付范围；Cookie Session 为可选能力，不再是默认路径。运行时、配置与 claims 的实现 contract 由 [ADR-022](adr-022-jwt-runtime-contract.md) 细化。
- 可复用的账号流程 service 与应用自有 HTTP 路由边界见 [ADR-024](adr-024-authentication-flow-service.md)；这项 contract 不代表流程已实现。
- 当前认证实现 WIP 的 Session 与 PAT 可以保留为能力实现输入，但生成流程、认证选择和验收必须以本 ADR 为准。不得把现有代码状态当作产品默认决策。
- 旧 ADR-004 的账户字段、密码哈希、内置注册与找回密码等决定在不冲突时继续有效；其 Cookie Session/PAT 默认顺序和 JWT 排除条款已由本 ADR 替代。

## 验收

- JWT 签发与验证覆盖算法混淆、伪造/篡改、issuer/audience、有效期、未来生效时间、key ID、轮换及未知密钥拒绝；secret 不进入日志、Debug 或响应。
- refresh token 哈希存储、单次轮换、并发刷新、重复使用检测、登出、重置密码、账户禁用和跨节点撤销均有 PostgreSQL/MySQL 集成证据。
- 多 API 实例验证一致签发与验证、密钥轮换窗口及共享撤销状态；access token 默认短时效与立即撤销可选路径的承诺分别测试。
- 生成项目的默认受保护 API 使用 JWT Bearer；Session 可独立启用并验证 Cookie 安全属性、CSRF 和共享存储；PAT 的能力与生命周期独立验收。
