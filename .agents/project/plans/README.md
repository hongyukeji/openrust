# 计划入口

[ADR-001](../decisions/adr-001-framework-principles.md) 是唯一方向决策；[todo](../todo.md) 是唯一可领取队列。本目录保存范围、对应矩阵与专项实现约束，不单独分配任务。旧 M0–M6、V1 基础方案与重复执行提示已移除；其已实现事实和测试证据仍在 Git、`core/` 与 `memory/` 中。

- [Laravel 总计划](laravel-rebuild.md)：范围、差异分类、目标结构、依赖与验收。
- [全框架执行与接手](laravel-rebuild.md#全框架执行与接手)、[小型应用本地交付](local-app-starter.md)：主线与并行分工、交验步骤和本地里程碑。
- [固定上游基线](laravel-rebuild-baseline.md)、[官方全量交付规则](laravel-official-coverage.md)：版本和两阶段交付范围。
- [Laravel 行为矩阵一](laravel-rebuild-matrix-1.md)、[矩阵二](laravel-rebuild-matrix-2.md)、[矩阵三](laravel-rebuild-matrix-3.md)、[Laravel Modules 矩阵](laravel-modules-matrix.md)：逐项来源、状态及差异。
- [重构资产与回归入口](laravel-rebuild-assets.md)：旧实现与测试的迁移来源，不代表新语义验收。
- [AI SDK](ai-capabilities.md)、[MCP](mcp-extension.md)、[SMS](sms-delivery.md)、[API 正文加密](api-payload-encryption.md)、[运行时配置与动态调度](runtime-configuration-scheduling.md)：已授权能力的专项约束，实施仍依唯一 todo。
- [可选快速开发模块](optional-rapid-development-modules.md)、[AI 模型平台](ai-model-platform.md)：未启动产品边界；不因框架目标自动开工。
- [发布仓库整理](release-repository-consolidation.md)：完整实现及验收之后的独立任务，不属于当前文档收敛或代码门禁。
