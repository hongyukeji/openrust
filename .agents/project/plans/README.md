# 开发计划索引

> 本目录保存 v1/v2 尚未完成的详细方案。代码、Git 与当前 crate 事实高于计划描述；实施前必须重新核对仓库。

- [OpenRust v1 版本开发计划](gen1-plan.md)：目标、关键决策、能力范围、黄金路径、M0-M6 里程碑、验收原则、质量门禁、风险与 AI 协作分工。
- [目录收尾 Spec](module-layout-migration-spec.md)：优先完成剩余工具 package 路径、examples/tests 分目录、apps/server 职责评估与测试/CI 入口。
- [OpenRust 框架设计](framework-design.md)：设计原则、模块与应用布局、内核、配置、路由、错误、数据、认证、异步、CLI、测试工具与框架代码规范。
- [v1 性能与开箱即用补强计划](v1-performance-and-readiness.md)：完整使用路径与性能验收、待决策问题、推荐方案及推进顺序。
- [v1 通用能力补齐计划](v1-common-capabilities.md)：WebSocket、gRPC、跨协议验证/调用作用域、出站安全/韧性、加密与密钥、请求幂等的范围与验收；gRPC 细则见 [ADR-040](../decisions/adr-040-grpc-integration.md)。
- [v1 通用能力覆盖与归属](v1-capability-coverage.md)：完整通用底座的补齐清单、单一职责归属、决策建议与完成定义。
- [v2 现成后台开发计划](v2-admin-platform.md)：复用 v1 的后台套件、核心模块、扩展候选、版本分工与 B1-B5 验收。
- [v1 集群部署与多副本一致性](v1-cluster-deployment.md)：共享状态、跨节点实时投递、任务竞争、滚动发布与必须通过的集群验收。

执行约束：

- 只把当前可在一次干净提交中闭合的功能簇提升到 `todo.md`，不复制计划正文。
- 里程碑完成后把稳定事实迁入 `architecture.md`、`core/*.md` 与 `docs/`，并从计划中删除已完成部分。
