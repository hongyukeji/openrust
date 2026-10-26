# ADR-029：数据库显式只读副本路由

- 状态：Accepted（已实现，双库真实复制拓扑验收有记录）
- 日期：2026-10-26
- 背景：SeaORM `DatabaseConnection` 是 `Db` 的公开逃生口。框架无法可靠判断裸连接上的 SQL 语义，也不能安全自动重放查询或写入。
- 决策：保留 `[database].url` 为主库，使用可选 `read_replicas` 配置同后端副本；不解析 SQL、不做自动故障回退或重试。
- API：`connection()` / `primary_connection()` 和事务始终使用主库；`read_connection()` 显式轮询副本，未配置副本则返回主库；`with_primary(future)` 以 Tokio task-local scope 让当前工作单元的读入口固定主库。
- 初始化：官方插件验证所有 URL、feature、后端并连接全部实例；任一副本不可用则启动失败。自定义连接插件通过 `initialize_with_replicas` 使用相同后端约束。
- 边界：`read_connection()` 的调用者须保证只读且能接受复制延迟；`tokio::spawn` 不继承主库 scope。绕开框架入口的连接操作不受路由控制。
- 资源：每个实例单独使用已有连接池上限和超时；无副本的默认路径不增加连接资源。
- 验收：契约测试覆盖单库、副本轮询、主库 scope、backend 拒绝；2026-10-26 在 PostgreSQL 18.6 / MySQL 8.4.13 的真实复制拓扑上，复制暂停时副本返回旧值、两个 App 的主库 scope 返回新值且互不污染、恢复后副本追平均通过。
- 取舍：本 crate 尚无路由指标系统，指标并入通用可观测性功能簇；计划中的可配置副本故障回退暂不纳入 API；通过原始 SeaORM 连接无法可靠地区分连接故障、SQL 故障及可安全重试的只读操作。若未来改为受控查询接口，应另行评估。
