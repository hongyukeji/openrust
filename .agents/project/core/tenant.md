# `openrust::tenant` 当前事实

## F6-A 已实现

- `TenantId::new` 校验 1..=128 字节安全 ASCII 标识符；Debug 遮蔽标识符。
- `TenantContext::current/require_current/scope` 提供显式 task-local 租户上下文；缺失时拒绝，嵌套调用不能更换租户。Tokio task-local 不自动传播至 `tokio::spawn`。
- `TenantLayer` 由应用提供可信 resolver。认证路由应让 `AuthLayer` 外包 `TenantLayer`，使 resolver 读取认证成功后加入请求扩展的 `CurrentUser`。未验证的 header/query 不可直接作为可信租户。
- `QueryRule::for_tenant(column)` 返回 `TenantQueryRule`，在列表、关联和批量写操作中将当前租户、用户规则与业务条件相交。框架不自动改 schema，也不覆盖裸 SeaORM、原生 SQL、未登记关系或自定义 driver。
- 用户用法见 [`src/tenant/README.md`](../../../src/tenant/README.md)，设计与尚未完成的跨资源传播见 [ADR-056](../decisions/adr-056-tenant-context-and-query-isolation.md)。

## 后续

F6-B 负责 Cache key、持久 Queue/retry、Storage path 与 Audit 的安全传播及真实双库验收；在此之前当前 TenantContext 不自动隔离这些资源。RPC/实时启用后按能力补组合验收。
