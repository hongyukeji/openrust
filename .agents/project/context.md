# 项目上下文

> 当前代码与目标：源码已提交 `app/example/` 过渡结构；最终业务位置为 `modules/<模块名>/`，唯一示例为 `modules/example/`。目录和模块能力尚未完成验收，按 [ADR-074](decisions/adr-074-laravel-modules-directory.md) 推进，不能将目标路径当作已实现。

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：正式目标为 Laravel + laravel-modules 的 Rust 1:1 实现。计划与授权已记录，R0 已固定上游与资产，R1 正按根 Laravel 应用与唯一综合示例 contract 纠偏，新布局尚未验收；原 v1 专项证据作为复用/重写基线。

## 当前状态

| 项 | 当前事实与目标 |
| --- | --- |
| 当前代码基线 | 规划开始 HEAD 为 `7f964b8`；根 openrust 同 package 的框架 lib 与默认 server 已装配；应用职责回根，当前示例仍在 app/example，目标 modules/example；tools 与 consumers 保留；版本和依赖以 Cargo.toml 为准 |
| 当前阶段 | [全面重构计划](plans/laravel-rebuild.md) 已记录；R1-S结构、R2-A/A2/A3与R2-B统一Provider内核已提交；R2-C入口scope及受管清理、R2-D生产同路径TestApp已通过适用门禁；后继 R2-M 模块闭环与 R1-E 真实CRUD/当前能力示例尚未完成，唯一队列见 [todo.md](todo.md) |
| 目标授权 | [ADR-070](decisions/adr-070-laravel-first-rebuild.md)：Laravel固定基线为1:1行为标准，laravel-modules为正式模块化范围，Goravel仅跨语言实现参考；允许全面重写，无历史兼容义务 |
| 当前完成边界 | 原认证/数据/队列/缓存/存储/实时/gRPC/视图等有专项证据；完整生产/集群/最低版本仍有缺口，新目标不能继承为已验收 |
| 协作 | 已定 contract 优先 gpt-6.1-sol 子智能体；主控负责设计/协调；所有 Agent 直接 main，非重叠文件可并行实现；共享 manifest/lock/迁移/文档/提交由集成负责人串行协调，target/服务资源预先分配，审查并行只读；高风险改码不同模型或 Provider 审查；不推送 |

## 固定约束

- 当前及以后全部开发仅在本仓库 `main` 进行并创建本地提交，不新建源码仓库、开发分支或隔离 worktree；子代理遵守同一目录、明确文件所有权、共享文件串行协调和 WIP 保护规则。

- 正式定位为 Laravel + laravel-modules = OpenRust，以固定基线1:1实现功能与可观察语义，持续跟进其框架功能与行为演进；Laravel的目录、概念、默认值、错误/安全边界、Container/Provider register→boot与开发/CLI/生成/测试/部署流程是验收标准。逐项上游源码/测试→Rust API/配置→等价测试→modules/example→文档→真实服务证据，未等价/未实测保留缺口。版本基线、差异矩阵与必要 Rust 适配遵循[长期上游跟进](plans/laravel-rebuild.md#长期上游跟进)。
- 已有优秀机制保留并融合进 Laravel 对应职责；统一返回格式明确归 HTTP Response / API Resource 与异常渲染，提供便捷接入与公开定制，避免重复内核和协议破坏。规则见[融合计划](plans/laravel-rebuild.md#已有优秀机制的融合)。
- 用户明确要求从零设计并重写 Laravel 式框架核心，旧实现不构成设计或兼容约束；旧源码按功能簇替换、验证后清除，不批量抹除 WIP 和证据。
- 旧目录、API 名称、CLI 风格、默认值、交付方式和“已验证不可重写”等约束可被新 contract 替代。先校准领域 ADR 与调用方；当前实现事实不等于未来设计约束。
- 保留 Rust 类型、所有权、trait、async、Cargo 的便利性。采用熟悉的 Laravel 术语，语法用 Rust 惯用形式；不做 PHP 运行环境，也不引入不可测试的全局可变状态。
- 允许重构全部框架，但保护他人 WIP、进程、服务、数据和已存在证据；底层成熟组件按能力复用，不为全面重写重造密码算法/协议驱动。
- 对应新能力按 Spec → Implement → Verify → Commit 连续推进；一次一个主功能簇。旧 M0–M6/DX/F/G 范围作为矩阵输入，顺序由新计划重排，不能丢失原未完成验收。
- 第一方说明使用简体中文，技术标识符/原许可保持原文，见 [style.md](style.md)。代码安全、依赖记录和测试/提交规则见 [conventions.md](conventions.md)。

- 除 modules/example 外均为框架通用结构与代码；示例专有迁移/种子/资源/协议/生成脚本/测试 fixture 均归模块内，根只保留通用加载与公开挂载，不因示例新增 build.rs/build 依赖。

- 根 openrust 同 package 提供框架库与默认 server；default-members=["."]、default-run="server"，根默认启动，modules/example 是唯一综合示例；生成应用与真实 HTTP 验证继续执行。

必要Rust差异、用户约定和授权增强按[差异分类](plans/laravel-rebuild.md#逐项对应与差异分类)公开；单仓main、根结构、modules/<模块>、模块专属资源、Vue/uni-app与Redis策略不被字面1:1覆盖。核心全量，模块包独立来源但同内核，生态/商业/外部授权边界保持。

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

当前重构来源与 102 章/749 子节当前纳入范围盘点见 [固定基线](plans/laravel-rebuild-baseline.md)；未将旧证据继承为新 contract 通过。
