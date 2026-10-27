# ADR-058 数据交换任务与批次语义

- 状态：Accepted
- 日期：2026-10-27
- 依据：[ADR-057 通用 CSV/XLSX 数据交换](adr-057-data-exchange-contract.md)、[ADR-042 共享锁与请求幂等](adr-042-shared-locks-and-idempotency.md)、[ADR-047 查询授权](adr-047-typed-query-authorization.md)、[ADR-050 文件存储](adr-050-storage-drivers.md)

## 决策

- 以可选 `data-exchange-tasks` feature 提供任务工作流；它显式组合 `data-exchange`、`queue` 与临时文件能力，不进入默认 feature。应用继续定义自己的 `Job`、Schema、数据库实体和授权策略，不另建任务注册/Hook 系统。
- 导入从已授权的 `StorageDisk` 对象读取有界流，复制到权限受限的临时文件，再由 `spawn_blocking` 中既有 CSV/XLSX parser 处理。容量受输入限制、单行/批次预算和有界 channel 约束；临时文件在成功、失败、取消及 future drop 时回收。
- 有效行按有界批次交给应用 callback，框架用 `Db::transaction` 包裹每批写入。无效行不传给写入 callback，进入有界错误报告。前批提交后后批失败会形成部分成功；不承诺整文件原子性。
- 批次使用 `RequestIdempotency` 记录稳定 `import_id`、源文件摘要、批次序号和行内容指纹；业务写入与批次完成记录在同一事务中。重试跳过已提交批次，拒绝同键不同内容。调用方须保证 `import_id` 同时绑定输入、schema 和业务写入语义；安全自动重试窗口为 24 小时，超过窗口后需由应用业务唯一约束或人工核对避免重复写入。完成记录保留 24 小时，遵循现有幂等 API 上限；文件报告可按配置保留至 30 天。依赖应用已安装 `CoordinationMigration`。这不覆盖 callback 内数据库之外的副作用，也不构成跨批 exactly-once。
- Queue at-least-once、worker lease 与合作取消语义保持不变。任务取消时已完成批次保留，先持久化取消报告，再返回 `queue_job_cancelled` 触发既有重试。源对象应使用不可变 key 或精确 version；任务计算内容摘要避免把替换后的文件误作原输入。
- 导入错误报告和导出文件写入应用显式选定的 Storage disk 与 opaque 基础路径；框架按 queue job ID、attempt 及导入 ID 派生每次尝试唯一的实际对象路径，并在报告中返回路径。旧尝试迟到发布也只影响自己的路径，不会覆盖新尝试的对象。完整发布对象后才写入按 disk/path 隔离的有界保留标记并派发清理 Job；因此未完成上传期间不会有清理任务删除对象。清理 Job 携带对应到期代际，遇到更新后的标记只重新安排最新到期任务，不删除较新对象。标记读写与发布/清理由同一 `SharedLocks` 数据库租约串行化，租约每 5 分钟续期，单次续期最多等待 30 秒；关键区在达到 Storage 发布/中止边界前不因调用方 future 被丢弃而提前释放锁。续租失败时操作继续运行至 Storage future 返回，以免丢弃后端可能已提交的操作；整个任务返回错误交由 Queue 重试。每尝试独立路径隔离迟到副作用；不承诺数据库不可用期间 Storage 操作 exactly-once。worker 请求取消时，导出在上传开始前检查取消，并等待已开始的上传返回后再清理并返回取消错误。锁冲突让队列按既有重试策略重试；协调数据库失效时操作报错，依赖恢复后重试。清理成功后移除对象和标记；标记损坏或存储错误时失败关闭并由 Queue 重试。框架不新增匿名下载路由；应用每次下载仍须校验身份、授权与租户。
- 导出接收应用提供的异步 row producer；producer 应从 `AuthorizedSelect` 或等价已授权查询逐批发送行。框架使用有界 channel 和既有 CSV/XLSX writer 写入临时文件，再流式上传 Storage；取消不发布半成品对象。框架不推断业务授权，不提供通用 SQL 或模型反射映射。

## 验收

- PostgreSQL 18 与 MySQL 8.4 真实隔离库验证每批事务、相同任务重试/已完成批次跳过、不同源冲突、租户作用域、取消报告、callback 故障及进度；真实 local Storage 验证版本参数失败关闭、报告/导出流、应用授权责任、延迟清理、重试更新对象后旧清理任务不能提前删除。
- 独立 Job consumer 仅使用公开 API；测试证明有界 channel 背压、临时文件取消清理、压缩/行上限、CSV/XLSX 输出与报告大小限制。
- 完成默认/all-features workspace fmt、Clippy、tests、warnings-denied Rustdoc、no-default-features 与 `cargo deny check`。

## 边界

数据库只保证每个成功批次内的业务写入与幂等完成记录原子提交；对象存储、邮件及其他外部副作用不参与数据库事务。报告/文件下载仍由应用负责授权；local Storage 仅当前节点可见，集群须选择共享 Storage driver。
