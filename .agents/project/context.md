# 项目上下文

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：当前方向为 Laravel 式全面重构。计划与授权已记录，R0 已固定上游与资产，R1 骨架迁移待实施；原 v1 专项证据作为复用/重写基线。

## 当前状态

| 项 | 当前事实与目标 |
| --- | --- |
| 当前代码基线 | 规划开始 HEAD 为 `7f964b8`；根 `openrust` library + server、tools、apps/example 已有实现；版本和依赖以 Cargo.toml 为准 |
| 当前阶段 | [全面重构计划](plans/laravel-rebuild.md) 已记录；下一功能簇 R1 骨架与启动，唯一队列见 [todo.md](todo.md) |
| 目标授权 | [ADR-070](decisions/adr-070-laravel-first-rebuild.md)：Laravel 为首要概念/行为/体验参照，Goravel 为跨语言参照；允许全面重写，无历史兼容义务 |
| 当前完成边界 | 原认证/数据/队列/缓存/存储/实时/gRPC/视图等有专项证据；完整生产/集群/最低版本仍有缺口，新目标不能继承为已验收 |
| 协作 | 已定 contract 优先 gpt-6.1-sol 子智能体；主控负责设计/集成；高风险改码不同模型或 Provider 审查；不自动合并/推送 |

## 固定约束

- 旧目录、API 名称、CLI 风格、默认值、交付方式和“已验证不可重写”等约束可被新 contract 替代。先校准领域 ADR 与调用方；当前实现事实不等于未来设计约束。
- 保留 Rust 类型、所有权、trait、async、Cargo 和 IDE 便利性。采用熟悉的 Laravel 术语，语法用 Rust 惯用形式；不做 PHP 运行环境，也不引入不可测试的全局可变状态。
- 允许重构全部框架，但保护他人 WIP、进程、服务、数据和已存在证据；底层成熟组件按能力复用，不为全面重写重造密码算法/协议驱动。
- 对应新能力按 Spec → Implement → Verify → Commit 连续推进；一次一个主功能簇。旧 M0–M6/DX/F/G 范围作为矩阵输入，顺序由新计划重排，不能丢失原未完成验收。
- 第一方说明使用简体中文，技术标识符/原许可保持原文，见 [style.md](style.md)。代码安全、依赖记录和测试/提交规则见 [conventions.md](conventions.md)。

## 服务与验收

- 日常 PostgreSQL 18、MySQL 8.4 LTS、Redis 8；先检查复用合适现有服务，测试数据隔离。不得停止/删除/重建他人容器、卷和配置。临时资源有明确 owner，只清理本任务资源。
- SQLite 单机完整支持仍是必交规划；额外后端按新能力矩阵分阶段。最低版本须在最终门禁确定并实测；历史 PG16 记录不充当当前日常基线。
- 读写分离须真实复制；双库须真实数据库；集群须真实负载均衡、多 API/worker/scheduler 与共享状态。已具备 Docker 服务、fake 或编译通过均不能替代实际验收。
- 真实邮件/SMS/供应商发送只用已授权凭据与目标；当前 Twilio 实发条件未具备。外部缺口保留为未验收，继续其他独立工作。
- 加密在框架进程内完成，新密文用当前密钥，历史密钥仅解密；不强制第三方 KMS/Vault。
- 单机为普通应用默认部署形态，集群同属必交；共享能力不得在依赖失败时静默降级到本机权威状态。具体默认 store/guard/目录在对应新 contract 定稿。

## 版本与外部边界

- 新底座 R14 完整通过后进入 [V2 后台](plans/v2-admin-platform.md)：Rust 服务端、Vue Admin（pure-admin 精简国际化版 + Element Plus）、uni-app App；PC 仅规划。
- [AI模型平台](plans/ai-model-platform.md) 仍为独立产品，本次不启动；Laravel 生态中的 AI 开发辅助不自动授权该产品。
- 发布、推送、tag、修改凭据/系统配置、项目外持久写操作不从重构授权推断。所有阶段仅创建限定范围的本地 commit。

当前重构来源与 102 章/750 子节盘点见 [固定基线](plans/laravel-rebuild-baseline.md)；未将旧证据继承为新 contract 通过。
