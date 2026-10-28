# `openrust::tenant` 当前事实

## F6-A 已实现

- `TenantId::new` 校验 1..=128 字节安全 ASCII 标识符；Debug 遮蔽标识符。
- `TenantContext::current/require_current/scope` 提供显式 task-local 租户上下文；缺失时拒绝，嵌套调用不能更换租户。Tokio task-local 不自动传播至 `tokio::spawn`。
- `TenantLayer` 由应用提供可信 resolver。认证路由应让 `AuthLayer` 外包 `TenantLayer`，使 resolver 读取认证成功后加入请求扩展的 `CurrentUser`。未验证的 header/query 不可直接作为可信租户。
- `QueryRule::for_tenant(column)` 返回 `TenantQueryRule`，在列表、关联和批量写操作中将当前租户、用户规则与业务条件相交。框架不自动改 schema，也不覆盖裸 SeaORM、原生 SQL、未登记关系或自定义 driver。
- 用户用法见 [`src/tenant/README.md`](../../../src/tenant/README.md)，设计与尚未完成的跨资源传播见 [ADR-056](../decisions/adr-056-tenant-context-and-query-isolation.md)。

## F6-B 实现状态

- Cache facade 将 tenant 摘要加入物理 key、singleflight 协调键和 tag generation；无 context 时兼容既有 key。
- Queue facade 在 dispatch 时将 `TenantId` 写入经校验的 `ExecutionMetadata` 白名单；数据库/Redis envelope retry 保留 metadata，worker 每次 attempt 在 typed handler 外恢复 scope。损坏 envelope fail closed；身份/权限不传播，handler 需重新授权。自定义 driver 的 metadata-aware 默认入口会拒绝非空 metadata。
- Storage facade 将 tenant 摘要加入后端物理路径，逻辑路径仍返回给调用方和 UploadInspector。直接使用 backend 属于逃生口。
- `AuditEvent::new` 捕获当前 tenant；record 和数据库表新增可空 `tenant_id`，Debug 脱敏。既有库注册 `DatabaseAuditTenantMigration`，新库初始迁移已含该列。审计查询的 tenant filter 仍由应用实现。
- RPC、实时 transport、后台刷新 task-local、裸 ORM/SQL 和自定义绕过 facade 的路径需要应用显式传播/授权；两节点完整集群组合仍未验收。
