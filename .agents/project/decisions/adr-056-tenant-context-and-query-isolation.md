# ADR-056：可信租户上下文与查询隔离

- 状态：Accepted
- 日期：2026-10-27

## 背景

V1 必须提供默认关闭的可选租户隔离。应用需从已验证身份或明确信任的解析器取得 tenant ID；请求自带的任意 `tenant_id` 不能直接成为授权依据。框架已有 `ExecutionContext`、Auth `CurrentUser`、`QueryRule` 与 `AuthorizedSelect`，应组合这些能力而不复制 ORM 或建立全局 SQL 改写层。

## 决策

- 新增不透明 `TenantId`，校验长度 1..=128 字节、字母数字及 `-_.:` 字符；不允许空白、控制字符或路径分隔符。
- 新增 task-local `TenantContext`，提供 `current`、失败关闭的 `require_current` 与显式 `with_tenant` future scope。task-local 不自动跨 `tokio::spawn`，异步边界必须显式传播。
- 新增 `TenantLayer`，由应用提供可信 resolver 从完整 HTTP request 提取并验证 `TenantId`。框架不提供从未验证 header/query 直接读取租户的 shortcut；resolver 缺失、拒绝或出错时请求失败关闭。Auth layer 必须在 TenantLayer 外侧先认证，供 resolver 读取可信身份。
- 新增 `TenantQueryRule<E>`，组合现有 `QueryRule<E>` 与应用声明的 tenant column；select、关联读取、批量更新及批量删除都会将 tenant predicate 与用户筛选和授权规则做 AND。缺失租户上下文、无效规则或范围错误均拒绝，不放宽成全量查询。
- 普通应用不启用 `TenantLayer` 或 `TenantQueryRule` 时无额外配置和 schema 约束。框架不自动加 tenant column，不接管原生 SQL、直接 SeaORM、未注册关系、公共资源、管理面绕过、自定义 driver 或不使用受保护 query facade 的代码；这些路径由应用显式标记并授权。
- F6 后续子簇再把租户作用域传播到缓存键、任务 envelope/重试、存储路径与审计。持久任务不得序列化原始请求或认证凭据；执行时仍由 handler 重新验证适用业务权限。跨 RPC/实时范围在相应 transport 启用时另行验收。

## 验收

- 用无数据库 HTTP 测试验证未启用时行为不变、resolver 成功时 task scope 覆盖 handler、缺失/拒绝时不执行 handler、请求之间不串 tenant。
- PostgreSQL 18 与 MySQL 8.4 各使用隔离数据库和两个真实 TCP API 节点验证列表、详情、关联和批量变更只影响当前 tenant；用户授权 scope 与 tenant scope 均以 AND 生效，缺 tenant context 时 fail closed。
- `TenantId`、嵌套 scope、取消/错误与 task-local 边界有回归测试；执行 workspace fmt/clippy/tests/docs/cargo deny 与 diff 检查。完整 F6 还需 F6-B 的 Cache/Queue/Storage/Audit 跨服务验收。
