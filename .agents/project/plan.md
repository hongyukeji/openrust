# 当前计划

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：v1 版本共 M0-M6 七个里程碑，当前在 M3。详细交付与验收见 [v1 版本计划](plans/gen1-plan.md)「里程碑」。

## 当前里程碑

M0、M1、M2 已完成。M1 生成项目的 `cargo test` 验收通过。M2 `openrust-db` 连接池、事务辅助、SeaORM 迁移 runner、UTC/JSON 方言列 helper、应用 seed 命令、`openrust-test` 每用例数据库隔离、`openrust new --db postgres|mysql`、`generate model` / `generate migration` 均已实现；数据库扩展沿用 SeaORM `DatabaseConnection`、`Db` 与 Plugin 生命周期，外部自定义连接可复用迁移、seed 和事务入口。`DatabasePlugin` 检查 PostgreSQL 14+ / MySQL 8.0+；MySQL 使用强制 TLS。两库连接、事务、`SKIP LOCKED`、迁移、列类型、seed 命令、隔离和生成项目编译/测试均已验证。当前 M3 认证 API contract 已定义，运行能力实现待完成，见 `todo.md`。

实现与生成器命名遵循 [ADR-006](decisions/adr-006-rust-native-naming.md)：项目采用 `controllers/`、`models/_entities/` 和独立 `migration/` crate；全局 CLI 为 `new`、`dev`、`generate` / `g`，运行期命令由项目二进制提供。

## 执行校准

- AI 平台仅为独立产品规划，不进入当前框架功能簇；须先完成框架 v1/v2 完整体成熟验收再由开发者决定是否立项，平台完全体 P0–P8 及 [上线门禁](plans/ai-platform-product.md#启动开发与上线硬性门禁) 全通过才可上线，不允许阶段性对外开放。收到明确启动指令后，产品代码只在 `projects/*` 分支及独立 worktree 开发，执行 [产品隔离规则](plans/ai-platform-product.md#分支与工作区硬性约束)，不为产品妥协框架契约。

- 标准应用默认 Redis，由开发者要求并在 [ADR-015](decisions/adr-015-redis-default-calibration.md) 确认；memory/file 显式选择，库默认 feature 保持可组合。不得沿用 ADR-014 原 memory 默认或隐式 memory fallback；此为待 M5 实现的 contract。

- `todo.md` 当前功能簇必须与实际代码 WIP 一致；新需求先合入既有范围，当前簇闭环后再切换。已提交的成果不因旧提示词再次成为开发任务。
- 基础读写分离为新增 v1 必交付范围，尚未实现；按 [专题计划](plans/v1-database-read-write-splitting.md) 在当前认证簇闭环后作为 M2 增补领取。默认单库，认证/幂等/队列等关键读取使用主库；不沿用旧 v2 候选表述。
- M0-M2 等阶段完成仅代表对应已验收范围；后来补入的事务钩子、集群等要求仍需按依赖补做，不能被旧阶段标签遮蔽。
- 每次领取/切换功能簇，对照 [v1 能力覆盖](plans/v1-capability-coverage.md)、[通用能力](plans/v1-common-capabilities.md)、[性能补强](plans/v1-performance-and-readiness.md) 和 [集群要求](plans/v1-cluster-deployment.md) 定位相关未完成项；不能只沿最初 M0-M6 简表推进。
- M3 不限于身份提取器：短信、国际化、HTTP、安全、审计、验证码、通用 Session/限流和流程扩展按专题计划逐簇推进。认证 contract 现在就覆盖共享会话、跨节点撤销、全局配额/验证码与公开业务扩展点，不能留到 M6 才补单机假设。
- 在既有 `progress.md` / `core/` / `memory/testing.md` 分别记录契约、实现、实际验证与提交边界，不另建平行任务队列或历史状态表。fake/编译检查不替代真实 SMTP/短信、双库或集群验收；外部条件缺失如实标注。
- 旧排队消息仅作设计输入，已被更新的称谓、范围和措辞以当前仓库为准。收到消息后先去重合并，不将“已排队”当成“已接收”，不重复应用已有提交。
- 每个功能簇按 [扩展性自主判断](plans/framework-design.md#开发-agent-的自主扩展性判断) 主动评估接入场景并记录取舍；继续只实施 v1，v2 后台不得提前展开。

## 里程碑顺序

| 里程碑 | 主题 | 主要 crate |
| --- | --- | --- |
| M0 | 决策与基线 | workspace、CI、spikes |
| M1 | 内核与 HTTP | `openrust-core`、`openrust-http`、`apps/server`、`openrust-cli`（`new` / `dev` / `serve`） |
| M2 | 数据层 | `openrust-db`、`openrust-test`、模板 `--db` |
| M3 | 邮件、短信、认证及通用安全能力 | `openrust-mail`、`openrust-auth`；国际化/HTTP/安全/审计等见专题计划 |
| M4 | 队列、调度与事件 | `openrust-queue`、`openrust-events` |
| M5 | 缓存、存储与可观测性 | `openrust-cache`、`openrust-storage`、`openrust-http` 可观测性 |
| M6 | 开发者体验与 v1 发布 | `docs/`、`examples/blog`、模板、生成器全集 |

## 路由

| 任务类型 | 入口 |
| --- | --- |
| 架构 / crate 边界 | [architecture.md](architecture.md)、[框架设计](plans/framework-design.md)、`decisions/` |
| 编码 / Git / 验证 | [conventions.md](conventions.md) |
| 依赖 / 版本 | [stack.md](stack.md) |
| 里程碑范围与验收 | [v1 版本计划](plans/gen1-plan.md) |
| v1 版本之后 | [roadmap.md](roadmap.md) |

## 验证入口

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check
```

数据库相关 crate 另按 [memory/testing.md](memory/testing.md) 在 PostgreSQL 与 MySQL 上各跑一次。
