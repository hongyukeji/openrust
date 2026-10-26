# ADR-047 类型化资源查询授权

- 状态：Accepted（功能实现与双库专项验收完成；workspace 门禁进行中）
- 日期：2026-10-26
- 依据：[ADR-027](adr-027-policy-and-basic-rbac.md)、[ADR-008](adr-008-api-conventions.md)、[F5 查询授权规则](../plans/v1-foundation-completion.md#f5-查询授权规则)

## 背景

现有 `Policy::authorize` 只检查全局 RBAC 权限；业务 handler 仍须自行组合资源所有权过滤、分页、软删除、关联及批量写条件。仅在写操作前查一次所有权会留下并发 TOCTOU 窗口。既有 `SortParams`、`PageParams` 和 SeaORM 逃生口不能自动注入行级授权，也不应改写 SQL。

## 决策

- 在 `openrust::auth` 增加按 SeaORM `EntityTrait` 类型参数化的 `QueryRule<E>`。应用以闭包接收已认证 `CurrentUser` 并返回 `Result<QueryScope<E>, Error>`；`QueryScope` 必须以至少一个 SeaORM 表达式创建，可继续 AND 条件或构造非空的 AND-OR 组。规则同步、无副作用，不执行 I/O。
- 受保护入口在内部把 `QueryScope` 与可选用户筛选合并为顶层 `Condition::all()`；构造器不允许空范围，规则错误统一映射为禁止访问且不执行查询。业务筛选可使用嵌套 OR，但只作为 AND 的一侧。受保护 API 不暴露底层 builder 或可追加顶层 OR 的方法。分页拒绝空页、超过 500 行的页大小及偏移乘法溢出。
- `AuthorizedSelect<E>` 不暴露底层 `Select<E>`，只提供受保护的 `all`、`one`、分页、排序与 limit/offset。关联集合读取必须同时给出关联实体自己的 `QueryRule<R>`；父记录及每类关联记录分别受限，不承诺一定使用同一条 SQL。不能安全表达或验证的关联形态不由受保护入口提供。
- 多行更新与删除由 `QueryRule` 内部创建 mutation builder，只接收更新值、业务 `Condition` 和连接；授权条件在实际 UPDATE/DELETE SQL 中与业务条件相交。不接受预构造 builder，也不提供先读取、再无保护写入的快捷路径。授权影响记录数由数据库结果返回。
- `QueryRule` 负责资源行范围；现有 `Policy::authorize(user, "posts:update")` 等仍负责操作权限，调用方按需显式调用两者，不隐式授予 RBAC 权限。
- 既有裸 SeaORM 查询/写入、直接关联加载、原生 SQL、自定义 ORM/driver 仍是显式逃生口。框架不拦截所有数据库访问、不改写任意 SQL、不内置组织/部门规则或策略 DSL。raw SQL 和自定义 driver 必须由应用自行复现规则，并标明范围。
- 与 `Entity::not_deleted()` 等显式范围通过 AND 组合；不改变软删除默认语义、不新增 schema migration、不缓存授权结果。

## 验收

- 单元验证嵌套 OR 仍受 rule AND 约束、空规则和规则错误 fail-closed、更新/删除 SQL 带授权条件、分页及关联查询同时应用父子规则。
- PostgreSQL 18 与 MySQL 8.4 真实隔离数据库验证越权 ID、过滤扩大范围、分页、混合授权批量写入、关联泄漏、失败拒绝及软删除组合。
- 独立 consumer 仅依赖公开 `openrust::auth::QueryRule` / 受保护查询入口实现自有 row scope；示例明示 `Policy::authorize` 与 row scope 的分工及原生 SQL 边界。
- 不把单元构造 SQL、fake 或无数据库运行计为双库通过。

## 后续

稳定键有界分批读取与取消、迁移跨进程互斥/等待/崩溃恢复是 F5 同一 todo 行中的独立功能簇；本 ADR 不实现或验收它们。
