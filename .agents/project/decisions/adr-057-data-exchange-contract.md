# ADR-057 通用 CSV/XLSX 数据交换

- 状态：Accepted
- 日期：2026-10-27
- 依据：[v1 底层补齐计划 F8](../plans/v1-foundation-completion.md#f8-通用数据交换)、[F5 查询授权](adr-047-query-authorization.md)、[F6 租户隔离](adr-056-tenant-context-and-query-isolation.md)

## 决策

- 在根 `openrust` package 增加显式 `data-exchange` feature 和 `openrust::data_exchange` 模块；默认构建不引入表格依赖。
- 采用成熟格式库而不自写格式解析：`csv` 逐记录读写，`calamine` XLSX streaming cell reader，`rust_xlsxwriter` 写 XLSX。只支持 CSV/XLSX，不支持 XLS、宏或公式执行。
- `ImportLimits` 限制输入字节、行、列、单元格和错误记录；解析器逐行产出 `ImportRow`，将表头映射、类型转换及统一字段错误交给显式 `ImportSchema` 与自定义 converter。解析不执行数据库写入，调用方可将有效批次交给既有 `BatchReader`、授权查询、租户规则或事务 API。
- CSV 输入按 UTF-8 处理并识别 UTF-8 BOM；编码或语法错误以安全格式错误中止。重复、空或超限表头失败关闭。缺失字段/类型转换由 schema 规则明确报告；每行最多收集配置上限错误。
- XLSX 使用 ZIP 元数据预检与逐 cell reader；在 calamine 解析前限制 entry 数、解压总量、单项大小及 shared strings 声明数量/大小。工作表声明维度和实际 cell 位置/数量均校验。任意公式 cell 拒绝，不使用其缓存结果；宏文件拒绝。
- 同步导入将 callback 作为唯一保存边界；callback 错误立即停止且不声称回滚已发生的应用副作用。异步任务、事务边界、失败报告存储和重试策略属于后续 F8 任务化簇，不另建队列。
- CSV 导出默认执行 spreadsheet formula injection 防护：对首个非空白危险前缀 `=`, `+`, `-`, `@`, TAB、CR 或 LF 的文本字段加 apostrophe；XLSX 的文本始终走字符串 writer API，不把文本转换为公式，数字/布尔值保持原生单元格类型。原始无保护 CSV 文本导出须显式选择不用于电子表格的格式策略。
- XLSX 输出采用有界行、列、单元格及总文本预算；XLSX 内存模式采用 writer constant-memory 路径和临时文件，不在框架堆内累积整个工作簿。
- 所有解析、转换、格式、限制和 callback 错误使用稳定错误码；不回显整行或敏感单元格内容。

## 依赖

- `csv 1.4`（MIT OR Unlicense，MSRV 1.73）。
- `calamine 0.37`（MIT，MSRV 1.88；streaming `XlsxCellReader` 与公式 metadata API）。
- `rust_xlsxwriter 0.101`（MIT OR Apache-2.0，MSRV 1.88；使用有界 constant-memory worksheet 与 writer API）。
- `zip 9.x` 与 `quick-xml 0.42` 直接用于预检归档元数据及校验受限 shared strings XML；关闭不需要的压缩/加密 feature，沿用 calamine 的 deflate 依赖。

## 验收边界

- 单元/集成测试覆盖 BOM、类型、空值、重复表头、输入/行/错误限制、formula cell、宏、ZIP 展开预算、callback 失败与导出注入防护。异步任务取消在后续任务集成簇验证。
- 在真实 PostgreSQL 18/MySQL 8.4 上验证后续任务化导入的事务/批次、权限/租户过滤、失败报告及可重试语义；通用解析 API 与数据库保存分离。
- 格式组件的许可证、MSRV 和 feature 记录进入 `stack.md`；`cargo deny check` 为依赖门禁。独立 consumer/example 仅使用公开 API。

## 非目标

不提供业务导入页面、报表设计器、通用 ORM 映射、隐式事务、跨批 exactly-once、XLS 兼容、公式/宏执行或无界的整工作簿物化 API。
