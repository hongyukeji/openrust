# `openrust::data_exchange`

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：可选 `data-exchange` feature 提供 CSV/XLSX 有界逐行解析、显式转换和流式导出；任务化持久化尚未接入。

## 当前实现

- `ImportSchema` 将输入表头映射到应用字段和 `CellConverter`；可选的 `ImportRowValidator` 复用框架 `Validate` 与统一字段错误。CSV 支持 UTF-8 BOM，CSV/XLSX 都逐行/逐 cell 调用同步 callback。
- `ImportLimits` 限制输入字节、行列、单元格、错误数、ZIP entry、解压总量及 shared strings 字节/数量。XLSX 解析前预检 ZIP，拒绝重复/重叠归档项、宏、超限共享字符串与公式；不会执行公式或读取其缓存值。
- callback 是唯一保存边界。callback 报错立即中止，但解析器不管理事务、不回滚已经执行的应用副作用；批次事务由调用方管理。
- `write_csv` 按显式字段顺序写出，默认转义可能触发表格公式的文本前缀。`write_xlsx` 使用 constant-memory worksheet，文本写成字面字符串，数字和布尔值保留类型；两者校验有界输出。XLSX 不接受数组/对象 cell。
- 独立 consumer 示例位于 `examples/data-exchange/`；`apps/server` 通过 `data-exchange` feature 提供集成测试入口。
- 本能力不提供异步导入任务、队列重试/进度、授权导出、报告存储或清理；这些属于 F8-B，须复用现有 Queue、事务、QueryRule、TenantContext 和 Storage。

## 验证入口

- `cargo test -p openrust --features data-exchange --lib data_exchange::tests`
- `cargo test -p server --features data-exchange --test data_exchange`
- `cargo check --manifest-path examples/data-exchange/Cargo.toml --locked`
- `cargo test --manifest-path examples/data-exchange/Cargo.toml --locked`
- 完整默认/all-features workspace 门禁与 `cargo deny check` 按 `memory/testing.md` 执行。
