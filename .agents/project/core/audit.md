# `openrust::audit`

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：显式审计事件、受限详情和可替换 sink；内置数据库 sink 支持同事务写入。

## 当前实现

- `AuditEvent` 创建时捕获当前可信 `TenantContext`，并显式提供 actor、action、object、outcome、时间、可选 request ID、可信来源 IP 和 details；`AuditRecord` 字段私有，必须经过 `AuditPolicy` 的详情白名单和大小校验。subject ID 应使用稳定非敏感标识，不得用凭据、邮箱或用户自由文本。
- 密码、token、secret、credential、code、authorization 等敏感名称的详情值固定写为 `[REDACTED]`；详情只接收有界 scalar，不接受嵌套 JSON。Event/Record Debug 隐藏 tenant ID、对象 ID、请求 ID 和详情值。
- `AuditSink` 提供第三方可实现的 append contract；`AuditPlugin::with_sink` 注册 custom sink，错误向调用方传播，不自动 fallback/retry。`AuditPlugin::database` 要求此前已注册 `DatabasePlugin`；`test-util` feature 提供 `audit::testing::FakeAuditSink`。
- `DatabaseAuditMigration` 由应用 migrator 显式注册，并遵循 `DatabaseTableNames`；`DatabaseAuditTenantMigration` 为既有表补充可空 tenant_id，新表的初始迁移直接含此列；`DatabaseAuditSink::append` 始终使用主库。`record_in_transaction` 在调用方的既有 SeaORM transaction 内插入，不提交或嵌套事务。
- `purge_before(cutoff, batch_size)` 删除主库中严格早于 cutoff 的至多指定批次；无后台自动清理、查询 HTTP、固定保留期或不可篡改保证。
- `Policy::grant_permission_in_transaction` 允许应用将角色权限变更与审计记录放在同一个 `Db::transaction`；`--auth` 模板注册数据库 audit plugin 并加入两项迁移。审计记录不自动添加查询 tenant filter。其他认证方法不自动全量审计。

## 事务和故障边界

- 原子审计必须与关键业务 SQL 共用数据库及事务；任何插入错误都应传播，使外层事务回滚。事务成功返回后才可确认审计和业务一并提交。
- 拒绝、失败及已回滚结果应在业务事务结束后另写。数据库不可用时无法保证失败事件落库；提交后 sink 失败也不会回滚已提交业务。
- 自定义远程 sink 与业务数据库事务不具备原子性；框架不承诺幂等、恰好一次或自动可靠投递。
- 来源 IP 由调用方传入已按可信代理策略解析的地址；request ID 只用于关联，不可用作授权依据。

## 验证

- 模块测试覆盖详情 allowlist、敏感名脱敏、Debug 脱敏、有界 scalar、sink 错误传播和 database/custom plugin 注册。
- `examples/demo/tests/server/auth/database.rs` 在 PostgreSQL 18 与 MySQL 8.4 真实隔离库验证迁移、映射表写入、角色权限和审计同事务提交、外层回滚双向撤销及有界重复清理。
