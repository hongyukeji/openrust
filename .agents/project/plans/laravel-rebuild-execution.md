# 全面重构执行任务

> 受众：执行本次重构的 AI Agent
> 更新：2026-10-28
> 用途：用户明确要求的可复制执行任务说明；不保存聊天恢复日志。范围唯一来源是全面重构计划，状态唯一来源是 todo.md。

请在 OpenRust 仓库执行 `.agents/project/plans/laravel-rebuild.md`，按 ADR-070 将框架全面重构为 Laravel 式 Rust 应用框架。

## 已获授权

- 产品目标是用 Rust 忠实实现 Laravel，并长期跟进框架功能与行为更新；默认保持其目录、概念、命名、默认值、工作流和扩展方式。必要 Rust 适配与版本差异依总计划[长期上游跟进](laravel-rebuild.md#长期上游跟进)记录和验收，不自行降低为仅部分概念相似的框架。
- Laravel 是能力、概念、默认流程和开发体验的首要参照，Goravel 是跨语言落地参照。允许目录、架构、公共 API、默认值和实现推倒重来，无旧接口或历史数据兼容义务。
- Rust 类型、所有权、trait、async、Cargo 和 IDE 便利性必须保留。不得用动态字典、全局可变单例、隐式阻塞 I/O 或 PHP 解释器模拟来追求外形一致。
- 旧 ADR/规范中与新方向冲突的约束按 ADR-070 替代；当前代码是已实现事实，不能用“旧规定不允许”拒绝本次重构。普通设计决策自主完成并记录，不重复询问是否允许重写。
- 已有 WIP、他人工作、用户数据与共享服务必须保护；发布、推送、tag、凭据和项目外写操作仍遵守授权边界。

## 开始和推进

1. 读取 AGENTS.md、`.agents/project/README.md`、context.md、todo.md、plan.md、ADR-070 和全面重构计划；核对 Git HEAD/status、worktree、进程/其他代理、已完成提交和最近验收。
2. 根据 todo.md 领取当前功能簇。首次从 R0 开始，固定 Laravel/Goravel 版本和源码 commit，逐章建立能力/子行为/现有资产/差异/验收矩阵；随后直接进入 R1，不把完成计划当作完成重构。
3. 按 R0–R16 的依赖顺序连续推进；一次一个主功能簇，内部可分派不冲突的子任务。每簇执行 Spec → Implement → Verify → Commit，写明公共 contract、生命周期、事务、错误与资源边界。
4. 默认应用骨架向 app/bootstrap/config/database/routes/resources/public/storage/tests 职责目录收敛，框架实现独立；R1 实测 Cargo/IDE 成本后定稿。允许 Rust 合理适配，不能只搬目录而保留不一致的使用流程。
5. 完成交付矩阵的全部适用机制：内核/Provider/Container/Facade、HTTP/FormRequest/Resource、ORM/关系/Factory、认证授权、缓存/存储、视图/国际化/通信、事件/队列/调度、网络/广播、观测、Artisan 式 CLI、测试与生成器、生产集群、V2 和已纳入的生态可选包。子行为、排期和证据以总计划为准，不私自缩小范围。
6. 已有实现可复用、适配或重写；复用必须通过新 contract 测试。删除失效代码、旧别名和重复内核，迁移全部调用方、模板、示例、CI 与文档，不长期并存两套框架。

## 模型和协作

- 已定 contract 的实现和测试优先派给 `gpt-6.1-sol` 子智能体；普通任务使用适中推理，提供最小必要上下文和明确路径，节约 token。
- 按环境支持使用本地主控做轻量编排；指定模型不可用时如实说明并选成本合适的可用模型。不要仅凭风险词升级到最贵模型。
- 改码默认独立 worktree，任务基线、路径 owner、测试资源明确；worker 不递归派发、不修改整体 contract、不自动合并、不推送。高风险代码由不同模型或 Provider 独立审查。
- 按当前工具的异步任务规则汇报启动和完成；不虚报后台任务已运行，不把计划中的验证写成已执行。

## 必须实际验证

- 每个代码簇执行适用 fmt、workspace clippy/test/doc、cargo deny、feature/MSRV 门禁；失败先修复。生成项目执行 check/test 和真实启动/HTTP 流程。
- PostgreSQL 18/MySQL 8.4 LTS 双库；SQLite 单机支持按阶段纳入。真实复制验收延迟/追平/强制主库/故障恢复，不能用两个独立数据库替代。
- 真实 Redis、S3、SMTP、HTTP/WebSocket/SSE/gRPC、Collector；各数据库分别验收负载均衡、两个 API、多 worker/scheduler 和共享状态/文件/广播/恢复。外部 SMS/支付等仅用授权目标和凭据。
- 根目录普通 `cargo run` 和 RustRover 同一个 Command=`run` 的运行与断点调试均实际成功；额外 --bin/--package 或命令别名不能替代。
- 验证单机部署、完整集群、资源上限、性能、干净环境黄金路径与 Laravel 使用者任务对照；没有真人走查时注明 Agent 证据。

## 持续交付

每簇同步 todo/plan/progress、领域 ADR/core、用户文档、生成器和验收记录，只本地提交本簇变更；不 push、不发布、不打 tag。前台及时汇报有证据的进展，普通问题和设计分歧自行解决，不在分析阶段反复停下。

外部条件缺失时记录缺口并继续所有不依赖项，不虚报完成；遇到工具/授权硬边界如实报告。最终以总计划矩阵、真实验收、独立审查和交付证据判断完成，不能仅以“已重构”“已编译”“所有计划已更新”结束任务。
