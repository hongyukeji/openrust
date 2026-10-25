# ADR-012 认证身份与路由契约

- 状态：Accepted（公共 contract；实现待完成）
- 日期：2026-10-25

## 背景

ADR-004 已确定 Cookie 会话与个人访问令牌为 v1 认证方式，但没有规定业务用户类型如何进入 handler、路由如何配置认证、可选身份如何处理无效凭据。现有 Axum `Routes` 只提供 Tower `.layer(...)`，认证中间件构建时无法直接捕获最终 `AppState`。

## 最小 Spec

- 保留 `Db`、`Plugin`、`Routes::layer` 和 Axum 提取器机制，不建立静态认证 Facade 或运行期 ID 转换注册表。
- `CurrentUser` 表示经过认证的身份，不代表应用的领域用户记录。
- 认证只从路由指定的 Cookie Session、Personal Access Token 或二者择一模式读取凭据。
- Required 与 Optional 由同一认证层提供；仅凭据完全缺失时 Optional 返回匿名身份。
- 身份 ID 固定为 `i64`，与 v1 用户表 `BIGINT` 约定一致；业务 service 根据 ID 加载自己的领域用户类型。
- `CurrentUser` 对 handler 暴露 ID、认证来源和令牌能力，不暴露原始 Cookie、Bearer token 或 session secret。
- 未安装认证层却提取身份是路由配置错误；错误凭据、过期或撤销凭据不能降级为匿名访问。
- 认证层从请求扩展取得 `AppState`；HTTP 构建器必须保证该扩展先于路由层可用。
- 契约测试覆盖凭据矩阵、CSRF、即时撤销、身份提取和统一错误响应。

## 决策

- 身份 API 为 `CurrentUser` 与 `OptionalUser(Option<CurrentUser>)`。二者实现 Axum `FromRequestParts<AppState>`，可与 `Inject<T>` 同时用于 handler，并在 body extractor 前提取。
- `CurrentUser` 提供 `id() -> i64`、`source() -> AuthSource` 与 `token_abilities() -> Option<&[String]>`。`AuthSource` 只有 `Session` 和 `PersonalAccessToken`；Session 返回 `None`，PAT 返回 `Some(abilities)`。空 PAT 能力列表不允许任何受能力限制的操作，能力按精确字符串匹配，不支持 `*` 通配符。身份字段不可由请求参数、JSON body 或客户端自报 header 构造。
- 业务用户类型由应用持有。handler 注入业务 `UserService` 并按 `CurrentUser::id()` 查询；OpenRust 不自动装载或强制领域用户实现 `AuthUser` / `UserResolver` trait，也不要求继承框架结构体。认证账户表使用 ADR-004 的通用字段并包含 nullable `disabled_at`；非空时账户不可认证。业务字段通过迁移扩展。
- 路由使用 `AuthLayer::required(AuthMode)` 或 `AuthLayer::optional(AuthMode)` 并通过现有 `Routes::layer` 应用。`AuthMode` 为 `Session`、`PersonalAccessToken`、`SessionOrToken`；每路由仅认证所选来源。
- Required 缺少所选凭据返回 401；Optional 缺少所选凭据返回 `OptionalUser(None)`。仅所选认证来源参与解析，未选来源的凭据忽略且绝不作为回退。所选来源提供无效、过期、撤销或账户 `disabled_at` 非空的凭据时一律返回 401，不把错误凭据视为匿名。`CurrentUser` 出现在 Optional 匿名请求时返回 401。
- `SessionOrToken` 请求若同时提供 Cookie session 与 Bearer token（无论其中一个是否有效），返回 400 `ambiguous_credentials`，不按顺序回退或合并身份；只有单一来源存在时才解析该来源。使用 Session 完成认证的所有 unsafe method 都必须通过 CSRF 校验，包括 `SessionOrToken` 选择 Session 的分支；PAT 请求不使用 Cookie session，故不做 CSRF 校验。该规则细化并取代 ADR-004 中“配合表单时启用”的条件式表述。Token 模式从 `Authorization: Bearer` 读取个人访问令牌。
- 若 handler 使用 `CurrentUser` / `OptionalUser` 但路由未安装认证层，返回内部配置错误，而不是伪造匿名或空身份。
- Policy 返回拒绝时用框架统一 403；PAT 能力与用户角色权限取交集，能力不足不能被角色权限覆盖。PAT/Session 撤销、登出或账户禁用事务提交后，之后开始的每个认证请求必须观察到失效；已在提交前通过认证的在途请求不撤销。角色/权限变更的跨节点传播时限由 M3 授权与集群 contract 另行定义。
- 认证层所需 `AppState` 通过 HTTP 全局 Tower 扩展提供，在每个 route layer 之前执行。具体实现应优先调整现有 `build_router` 与 middleware stack，不增加并行路由系统。当前 route builder 注册时取不到最终 `AppState`，因此缺少 `AuthService` 在认证路由首个请求时返回 500；不宣称启动时已校验。

## 跨语言参考

- 来源：`/Users/Home/Codes/Java/openspring/openspring-core/openspring-core-auth/src/main/java/com/openspring/core/auth/CurrentUser.java`、`AuthManager.java`、`AuthIdConverterRegistry.java`。
- 解决的问题：从认证上下文统一取得主体 ID，并与认证服务分离业务资源查询。
- 适用前提：业务用户表采用稳定 ID；OpenRust 当前约定使用 `BIGINT`，领域用户模型由应用扩展。
- OpenRust 已有对应能力：`AppState` / `Inject<T>`、Axum `FromRequestParts`、`Routes::layer` 和 Plugin 生命周期。
- 采用主体 ID 入口的思想，调整为请求作用域强类型提取器；不采用 OpenSpring `Object` ID、运行期转换注册表、静态 Facade 或 ThreadLocal 风格，以遵守 Rust 显式依赖和当前框架约束。
- 验证：同 handler 的身份与 `Inject<T>` 组合、路由凭据矩阵、错误与撤销语义的契约测试。

## 验收要求

- Cookie session / Bearer token 单独、缺失、伪造、过期、撤销、账户禁用和冲突凭据均有测试；Session-only/Token-only 忽略未选来源；Optional 仅对所选来源缺失返回匿名。
- Session 与 SessionOrToken 选择 Session 时 unsafe method 的 CSRF 缺失/匹配测试；PAT 空能力、精确能力与角色权限交集；PAT/Session 撤销、登出或禁用提交后新请求立即观察到失效。
- `CurrentUser` 与 `OptionalUser` 可和 `Inject<T>`、validated body extractor 组合；无认证层配置错误、必需身份 401、授权拒绝 403 均为统一 JSON 包装。
- 个人令牌能力与角色权限交集验证；审计与日志不包含 Cookie、Bearer token、session secret 或密码。

## 影响

- ADR-004 增补本 ADR 作为公共身份与路由 API 的依据。
- 该决策只定义身份提取与路由认证契约，不表示登录、会话、个人令牌或授权能力已实现。
- Session / CSRF 与共享存储实现继续遵循 v1 覆盖计划和集群验收；不得用黏性会话弥补节点间身份状态缺失。
