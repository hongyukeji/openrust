# `openrust::data_exchange`

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：可选 `data-exchange` 提供 CSV/XLSX 有界解析与转换；`data-exchange-tasks` 复用 Queue、Storage、事务与请求幂等，提供任务化导入、导出和报告清理。

## 当前实现

- `ImportSchema` 将输入表头映射到应用字段和 `CellConverter`；可选的 `ImportRowValidator` 复用框架 `Validate` 与统一字段错误。CSV 支持 UTF-8 BOM，CSV/XLSX 都逐行/逐 cell 调用同步 callback。
- `ImportLimits` 限制输入字节、行列、单元格、错误数、ZIP entry、解压总量及 shared strings 字节/数量。XLSX 解析前预检 ZIP，拒绝重复/重叠归档项、宏、超限共享字符串与公式；不会执行公式或读取其缓存值。
- callback 是唯一保存边界。callback 报错立即中止，但解析器不管理事务、不回滚已经执行的应用副作用；批次事务由调用方管理。
- `write_csv` 按显式字段顺序写出，默认转义可能触发表格公式的文本前缀。`write_xlsx` 使用 constant-memory worksheet，文本写成字面字符串，数字和布尔值保留类型；两者校验有界输出。XLSX 不接受数组/对象 cell。
- 独立 consumer 示例位于 `examples/data-exchange/`；根 `tests/server/` 通过 `data-exchange` feature 提供集成测试入口。
- `data-exchange-tasks` 提供 `ImportTaskInput`、有界 `import_storage_task` 和 `export_storage_task`。导入将源文件流式复制到自动回收临时文件，经有界 channel 解析；每个有效批次在数据库事务中执行应用 callback，并与 `RequestIdempotency` 完成记录原子提交。批次重试跳过已完成批次，文件级状态按 tenant 与导入 ID 隔离，记录保留 24 小时。
- 报告与导出结果由应用选择 Storage disk/基础路径；框架按 job ID、attempt（导入报告另加 import ID）派生每次尝试唯一的实际路径，报告结构返回路径。默认报告保留 7 天、上限 30 天，QueuePlugin 自动注册延迟清理任务。只有对象完整发布后才写保留标记并排期清理。数据库 `SharedLocks` 串行化路径发布和清理，每 5 分钟续租，单次续期超时为 30 秒；续租失败时任务报错并让队列重试，但不丢弃可能已提交的 Storage future，因此外部 Storage 不承诺 exactly-once。旧尝试迟到发布仍只写入其独立路径。调用方取消后等待 Storage 完成/中止边界再释放锁；清理会移除对象和标记；框架不提供下载路由，应用负责鉴权和租户检查。
- `.openrust-data-exchange-retention/` 是内部 key 前缀，不能作为应用源/报告/导出对象路径。
- `export_storage_task` 接收应用异步 producer 和有界 `ExportRowSink`；应用必须使用 `AuthorizedSelect` 或等价受保护查询，框架不推断业务权限、不处理 SQL。数据库每批事务原子，不承诺整文件原子或外部副作用 exactly-once。取消后已提交导入批次保留，报告持久化并返回 `queue_job_cancelled`。
- 幂等记录与自动重试安全窗口为 24 小时；应用应让 `import_id` 绑定输入、schema 和业务写入语义。超过窗口的恢复须依赖应用唯一约束或人工核对。
- `tests/server/data_exchange_tasks.rs` 在 PostgreSQL/MySQL 真实隔离库验证分批提交、重复执行去重、不同源冲突、失败报告、已提交批次恢复、进度与本地 Storage 版本失败关闭/上传导出及旧清理任务保护。

## 验证入口

- `cargo test -p openrust --features data-exchange --lib data_exchange::tests`
- `cargo test -p openrust --features data-exchange --test data_exchange`
- `cargo test -p openrust --features 'data-exchange-tasks storage-local db-postgres' --test data_exchange_tasks`
- `cargo test -p openrust --features 'data-exchange-tasks storage-local db-mysql' --test data_exchange_tasks`
- `cargo check --manifest-path examples/data-exchange/Cargo.toml --locked`
- `cargo test --manifest-path examples/data-exchange/Cargo.toml --locked`
- 完整默认/all-features workspace 门禁与 `cargo deny check` 按 `memory/testing.md` 执行。
