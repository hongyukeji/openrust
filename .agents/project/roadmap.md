# 版本路线

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：先完整验收 v1 通用框架，再按现有授权继续 v2 官方后台；AI 平台仍待另行启动。当前执行任务只写 `todo.md`，具体 API 和选型按对应计划决策。

## v1：完整通用底座

- 交付可独立开发 API 与服务的 Rust 框架，单机起步、按需扩展集群；标准应用默认 Redis（见 [ADR-015](decisions/adr-015-redis-default-calibration.md)），库层 Redis feature 可选，保留显式 memory/file 选择且不静默回退，见 [ADR-013](decisions/adr-013-deployment-and-redis-cache-default.md) 与 [ADR-014](decisions/adr-014-cache-driver-and-store-contract.md)。缓存仍待 M5 实现，这里的默认是交付契约。
- 数据层包含默认单库、按需启用的显式只读副本路由与主库 scope；已在 PostgreSQL/MySQL 真实复制拓扑验证。完整集群验收仍按 [专题计划](plans/v1-database-read-write-splitting.md) 与集群计划执行。
- 包含数据、认证/授权/会话、验证码、国际化、本地加密与轮换、签名、审计、幂等、限流、缓存/锁/多实例、存储/上传下载、邮件/SMS/薄通知层、队列/常用编排/调度、WebSocket/SSE、gRPC server/client、CLI/生成器/测试与部署。gRPC 已正式纳入 v1，范围与完整验收见 [ADR-040](decisions/adr-040-grpc-integration.md)。
- 集群部署是必须项，包含多 API/worker/scheduler、共享状态、跨节点在线投递和滚动/故障恢复，按 [集群计划](plans/v1-cluster-deployment.md) 双库多副本验收。性能基线、资源边界、第三方扩展与干净环境完整路径同为完成条件。
- 入口：[v1 总计划](plans/gen1-plan.md)、[能力覆盖与归属](plans/v1-capability-coverage.md)。v1 不承担后台业务 UI、组织管理与部门数据权限的固定模型。
- M0-M3 的局部验收与 M4 队列/调度基础不代表上述全部范围完成；目录/验收宿主调整、M4 剩余能力及 M5/M6 依赖顺序在 `todo.md` 领取，不在路线图创建另一份完成清单。

## v2：现成后台

- 交付可运行、可生成业务模块的官方后台套件，复用 v1 公共能力：Rust 服务端、pure-admin 精简国际化版 + Element Plus 管理界面和 uni-app App。PC 端仅规划设计、暂不开发；具体端侧版本、组件方案与验收矩阵见 [v2 计划](plans/v2-admin-platform.md)。
- 已授权在 v1 完整验收通过后继续 v2；当前 v2 尚未实施，不提前跨过真实双库、完整集群、性能和生产路径门槛。
- 核心包含用户/角色/菜单/按钮权限、部门/岗位/DataScope、字典/参数、日志与在线会话、公告/收件箱、文件、任务/监控页面、前后端代码生成及基础导入导出。
- OAuth/SSO/MFA、多租户、更多数据库、缓存 tag、持久离线消息/重放、移动推送、SSR Starter、扩展目录等归 v2 评估候选；JWT 已调整为 v1 的主要 API 认证方式（ADR-018）。v2 候选采用门槛与建议见 [v2 后台计划](plans/v2-admin-platform.md)。
- v2 认证继续 JWT-first，Session 显式可选、PAT 面向机器身份；不重做 v1 的认证、缓存、队列、通信或存储，不使纯 API 应用强制依赖后台。

## 文档语言与后续国际读者支持

当前默认文档、Rustdoc 和源码说明优先服务简体中文开发者，具体规则见 [语言规范](style.md#语言规范)。已有英文说明的整改属于当前开发任务，见 `todo.md`；框架成熟后再按受众需求评估完整英文文档及其同步维护方式，不预建双语文档树，不把英文翻译列为 v1/v2 必交付门槛。运行时国际化能力继续按既有版本计划实现。

## 基于框架的独立产品

- [AI 平台开发计划](plans/ai-platform-product.md)：暂称 AI Platform，首版模型网关、计费账本与客户/运营闭环，最终按 [完整对标计划](plans/ai-platform-openrouter-parity.md) 复刻 OpenRouter 公开功能与流程，P4–P8 为必交付阶段；独立于框架 v1/v2，待两版框架完全体成熟后再考虑立项，平台完全体验收后才上线，禁止阶段性对外开放，门禁见 [产品主计划](plans/ai-platform-product.md#启动开发与上线硬性门禁)。尚未启动，产品开发必须使用 `projects/*` 分支与独立 worktree，产品代码不回灌框架主线，通用改进按框架流程单独验证和审查。

## 不作为两个版本核心承诺

任意工作流引擎、完整聊天平台、动态插件市场、云平台一键部署及外部密钥管理服务默认集成。出现实际需求后独立评估，不以通用能力完整为由无限追加业务系统或供应商。

## 框架对标后的候选评估

按 Loco、Laravel、Goravel、OpenSpring、若依、芋道综合评估，职责划分与当前缺口见 [框架设计「框架参考与对标」](plans/framework-design.md#框架参考与对标)。在进入 v2 规划前可评估：

- GraphQL：V1 明确不包含；按客户端按需组合数据的实际需求，评估是否需要独立集成，不能仅因参考框架支持而纳入。
- 显式数据库测试工厂/fixtures：V1 使用普通 builder，`fake::Dummy` 按实际需要评估、当前尚未引入；只在集成测试重复样板明显时评估轻量工具，不建立隐式 Factory trait 或 ORM 替代品。
- 可选服务端页面渲染：V1 保持 API 优先；按目标用户对 Rust 服务端页面/表单的实际需求评估独立 starter 或 feature。

以上是候选，不构成 v1/v2 必交付范围；OpenRouter 仅作为独立 AI 平台产品参考。

## 维护规则

- 当前执行顺序仍以 v1 活跃待办为准，v1 完整验收后继续 v2；准备设计不能打断当前功能簇，开发授权不等于发布/上线授权。
- 核心交付、评估候选、已实现与已验收明确区分；不保存逐日历史，事实与完成记录由代码、crate 文档和 Git 追溯。
- 同一职责只有一个 contract 与实现归属；跨版本复用优先，不能创建平行能力。
