# ADR-048 稳定键有界分批读取

- 状态：Accepted
- 日期：2026-10-27
- 依据：[ADR-008 游标分页边界](adr-008-api-conventions.md#3-查询参数)、[M5 ORM 与查询体验](../plans/v1-capability-coverage.md#能力表)

## 背景

导出、重算、维护和队列任务需要逐批遍历大结果集。`PageParams` 的页码/offset 入口面向 HTTP 小页，不能支撑稳定的大数据处理；每页 offset 会随位置增长，也会在并发插入/删除后移动。SeaORM 2.0.4 已有适用于复合键的 keyset `Cursor`，因此框架应复用它并在其上限定批次、连接、授权和取消语义。

## 决策

- 新增独立 `BatchReader<E>`，输入 SeaORM 实体查询；受保护查询由 `AuthorizedSelect<E>` 显式转成 batch reader，保持 `QueryRule` 的资源范围与业务筛选 AND 条件。普通 SeaORM 查询可显式使用未授权入口。
- 游标由实体所有主键列按 SeaORM 声明顺序组成；只支持主键元组升序或降序。每批以 exclusive `after` 游标请求，不使用 offset；复合键由 SeaORM 构造字典序条件和对应排序，主键重复值不可能跨实体行。
- `BatchOptions` 批次大小范围为 1–1000，默认 100；每次至多物化一批实体模型。此上限约束行数，不约束单行大字段的字节大小。
- 遍历始终通过调用方 `Db` 的主库连接池执行，不自动路由只读副本、不切换 named instance、不隐式包裹跨批事务。应用可在每批 callback 中自行使用事务。
- callback 对一批记录返回成功后才推进报告 cursor；callback 错误立即停止。取消在批次边界检查并返回带有 `cancelled` 和最后完整批次 cursor 的部分报告，不中断正在执行的数据库查询或 callback。
- 取消前已完成批次的外部副作用不会回滚；callback 若部分执行后返回错误或外层取消，只能保证报告 cursor 停在上一完整批次，重试可能重复当前批次。调用方负责幂等与事务。
- 不提供跨批快照或 exactly-once 保证。各页在默认隔离下独立读取；遍历期间新插入且主键排在游标之后的行可能被包含，排在游标之前的行不会追溯读取；未访问行删除或业务筛选字段变化会影响结果。需一致快照的应用自行选择隔离事务并承担长事务成本。
- 这是内部批处理契约，不改变 API 的 `page/per_page`、响应 envelope 或 ADR-008 对 HTTP cursor pagination 的后续决策。

## 验收

- 双库真实隔离库覆盖单列及复合主键、升降序、多批无重复、业务过滤、授权范围逐页保留、1/1000/非法批次大小、部分报告/cursor 与取消后连接池可继续使用。
- 明确验证或记录并发 insert/delete/filter-update 的既定非快照语义；测试 callback 错误停止且不虚报后续页完成。
- 独立公开 API consumer 与默认/all-features workspace 门禁通过。fake、SQL 构造测试不能替代 PostgreSQL 18/MySQL 8.4 真实运行。

## 非目标

本 ADR 不提供 HTTP API cursor 参数、不在 callback 间自动开事务、不承诺一致快照或副作用 exactly-once，也不实现迁移互斥。跨进程迁移锁另属当前 todo 的后续功能簇。
