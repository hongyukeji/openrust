# 版本路线

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：v1 完整通用框架，v2 官方现成后台；当前执行任务只写 `todo.md`，具体 API 和选型按对应计划决策。

## v1：完整通用底座

- 交付可独立开发 API 与服务的 Rust 框架，单机起步、按需扩展集群；标准应用默认 Redis（见 [ADR-015](decisions/adr-015-redis-default-calibration.md)），保留显式 memory/file 选择，见 [ADR-013](decisions/adr-013-deployment-and-redis-cache-default.md) 与 [ADR-014](decisions/adr-014-cache-driver-and-store-contract.md)。通用基础能力开箱即用、按需组合并可扩展。
- 数据层包含默认单库、按需启用的基础读写分离，范围与真实复制验收见 [专题计划](plans/v1-database-read-write-splitting.md)；已纳入 v1，尚未实现。
- 包含数据、认证/授权/会话、验证码、国际化、本地加密与轮换、签名、审计、幂等、限流、缓存/锁/多实例、存储/上传下载、邮件/SMS/薄通知层、队列/常用编排/调度、WebSocket/SSE、CLI/生成器/测试与部署。
- 集群部署是必须项，包含多 API/worker/scheduler、共享状态、跨节点在线投递和滚动/故障恢复，按 [集群计划](plans/v1-cluster-deployment.md) 双库多副本验收。性能基线、资源边界、第三方扩展与干净环境完整路径同为完成条件。
- 入口：[v1 总计划](plans/gen1-plan.md)、[能力覆盖与归属](plans/v1-capability-coverage.md)。v1 不承担后台业务 UI、组织管理与部门数据权限的固定模型。

## v2：现成后台

- 交付可运行、可生成业务模块的官方后台套件，复用 v1 公共能力，后端为 Rust，前端栈在 B1 决策。
- 核心包含用户/角色/菜单/按钮权限、部门/岗位/DataScope、字典/参数、日志与在线会话、公告/收件箱、文件、任务/监控页面、前后端代码生成及基础导入导出。
- OAuth/SSO/MFA、JWT、多租户、更多数据库、缓存 tag、持久离线消息/重放、移动推送、SSR Starter、扩展目录等归 v2 评估候选，采用门槛与建议见 [v2 后台计划](plans/v2-admin-platform.md)。
- v2 不重做 v1 的认证、缓存、队列、通信或存储，不使纯 API 应用强制依赖后台。

## 基于框架的独立产品

- [AI 平台开发计划](plans/ai-platform-product.md)：暂称 AI Platform，首版模型网关、计费账本与客户/运营闭环，最终按 [完整对标计划](plans/ai-platform-openrouter-parity.md) 复刻 OpenRouter 公开功能与流程，P4–P8 为必交付阶段；独立于框架 v1/v2，待两版框架完全体成熟后再考虑立项，平台完全体验收后才上线，禁止阶段性对外开放，门禁见 [产品主计划](plans/ai-platform-product.md#启动开发与上线硬性门禁)。尚未启动，产品开发必须使用 `projects/*` 分支与独立 worktree，产品代码不回灌框架主线，通用改进按框架流程单独验证和审查。

## 不作为两个版本核心承诺

任意工作流引擎、完整聊天平台、动态插件市场、云平台一键部署及外部密钥管理服务默认集成。出现实际需求后独立评估，不以通用能力完整为由无限追加业务系统或供应商。

## 维护规则

- 当前执行顺序仍以 v1 活跃待办为准，v1 收口后启动 v2；准备设计不能打断当前功能簇。
- 核心交付、评估候选、已实现与已验收明确区分；不保存逐日历史，事实与完成记录由代码、crate 文档和 Git 追溯。
- 同一职责只有一个 contract 与实现归属；跨版本复用优先，不能创建平行能力。
