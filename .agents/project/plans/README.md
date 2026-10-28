# 开发计划索引

- [Laravel 式全面重构计划](laravel-rebuild.md)：当前主计划，覆盖上游矩阵、目标架构、R0–R16、真实验收与 gpt-6.1-sol 分工。
- [全面重构执行任务](laravel-rebuild-execution.md)：可交给执行 Agent 的任务说明。

下列旧计划的能力/证据继续作为映射输入，设计冲突和阶段顺序按 ADR-070 处理。

> 本目录保存 v1/v2 尚未完成的详细方案。代码、Git 与当前 crate 事实高于计划描述；实施前必须重新核对仓库。

- [OpenRust v1 版本开发计划](gen1-plan.md)：目标、关键决策、能力范围、黄金路径、M0-M6 里程碑、验收原则、质量门禁、风险与 AI 协作分工。
- [目录收尾 Spec](module-layout-migration-spec.md)：根 package 统一 server 入口、apps 业务职责及 tests/config/tools/CI 迁移与验收。
- [OpenRust 框架设计](framework-design.md)：设计原则、模块与应用布局、内核、配置、路由、错误、数据、认证、异步、CLI、测试工具与框架代码规范。
- [v1 开发体验实施计划](v1-developer-experience.md)：将 Laravel 借鉴建议细化为 DX1–DX8，复用 M6/F9/F10；明确依赖、实现边界和实际默认启动、双库、生成项目验收。
- [v1 性能与开箱即用补强计划](v1-performance-and-readiness.md)：完整使用路径与性能验收、待决策问题、推荐方案及推进顺序。
- [v1 通用能力补齐计划](v1-common-capabilities.md)：WebSocket、gRPC、跨协议验证/调用作用域、出站安全/韧性、加密与密钥、请求幂等的范围与验收；gRPC 细则见 [ADR-040](../decisions/adr-040-grpc-integration.md)。
- [v1 通用能力覆盖与归属](v1-capability-coverage.md)：完整通用底座的补齐清单、单一职责归属、决策建议与完成定义。
- [V1 通用底层能力补齐](v1-foundation-completion.md)：分版本参考规则、F1–F10 必交范围/依赖/验收，以及后续适配清单；可选启用不等于可省略交付。
- [v1 SQLite 轻量后端完整支持计划](v1-sqlite-backend.md)：SQLite 文件数据库面向单机轻应用的完整官方支持范围、实施顺序、通用能力验收与部署边界；当前尚未实现。
- [v2 现成后台开发计划](v2-admin-platform.md)：复用 v1 的后台套件、核心模块、扩展候选、版本分工与 B1-B5 验收。
- [v1 集群部署与多副本一致性](v1-cluster-deployment.md)：共享状态、跨节点实时投递、任务竞争、滚动发布与必须通过的集群验收。

后续领域套件（独立立项，不进入当前框架任务）：

- [AI模型平台](ai-model-platform.md)：唯一 AI 项目计划，集中维护完整能力矩阵、网关与平台职责、同仓 main 的模块目录内源码组合、工程实施与 G0–G3 门禁；只有用户明确要求开发才启动。

执行约束：

- 只把当前可在一次干净提交中闭合的功能簇提升到 `todo.md`，不复制计划正文。
- 里程碑完成后把稳定事实迁入 `architecture.md`、`core/*.md` 与 `docs/`，并从计划中删除已完成部分。
