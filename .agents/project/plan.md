# 当前计划

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：v1 版本共 M0-M6 七个里程碑；按用户最新要求，当前优先闭环框架底层调整（根 package 模块迁移 → M2 读写分离 → 依赖它们的集群能力），M3 认证 WIP 暂停但保留。详细交付与验收见 [v1 版本计划](plans/gen1-plan.md)「里程碑」。

## 当前里程碑

M0、M1、M2 基线已完成；新增 M2 读写分离仍未实现、现为当前功能簇。框架 package 已迁至仓库根 `openrust`，模块包含 `src/config/`、`src/core/`、`src/http/`、`src/db/`、`src/auth/`、`src/mail/`；工作区保留 CLI、proc-macro、测试工具和聚合 server 四个有明确边界的 package。迁移后 workspace、features、PostgreSQL 16 / MySQL 8.4 集成、生成项目与 HTTP 示例均已复验；`apps/server` 暂不移除。日常开发和 CI 数据库基线为 PostgreSQL 16 / MySQL 8.4 LTS，运行时 PostgreSQL 接受 16+、MySQL 接受 8.4+，最低正式支持版本仍待 v1 发布前确定。

实现与生成器命名遵循 [ADR-006](decisions/adr-006-rust-native-naming.md)：项目采用 `controllers/`、`models/_entities/` 和独立 `migration/` crate；全局 CLI 为 `new`、`dev`、`generate` / `g`，运行期命令由项目二进制提供。

## 执行校准

- **底层调整总览必读**：当前按 `todo.md` 顺序进行根模块迁移独立提交 → M2 基础读写分离 contract/实现 → 后续 M4/M5/M6 能力与集群共享验收。集群部署贯穿这些功能，并须完成双库多副本验收。专题计划中的能力在代码、测试和验收证据齐备前都仍属未完成。
- 根 `openrust` package 模块迁移按 [ADR-021](decisions/adr-021-loco-style-module-layout.md) 已独立实施并完成 workspace、feature、双库、生成项目、示例、文档和旧路径验证。ADR-019 的逐能力目录改名与 `apps/server` 包改名取消；根 tests/examples 尚未覆盖聚合宿主所有验收，故保留 server。
- Loco 应用 Hooks、CLI 与测试体验的边界按 [ADR-023](decisions/adr-023-loco-application-experience-evaluation.md)：不增加平行 Hooks；M4 worker/scheduler 实施前定义取消及停机生命周期，M6 落地只读 doctor 与薄 TestApp/HTTP assertions；现有生成器按 gen1-plan 的里程碑执行。
- 命名校准按 [ADR-016](decisions/adr-016-neutral-application-namespaces.md)：应用配置默认 `APP_`，表默认 `tb_`，均可定制。`APP_` 已在 core/bootstrap、CLI、模板和说明中实现；表名 resolver 已贯通 DatabaseConfig/Db、迁移、测试隔离及认证持久化查询，并通过 PostgreSQL/MySQL 真实映射矩阵验证。M3 仍有功能簇验收待补；禁止直接改生产表。

- AI 平台仅为独立产品规划，不进入当前框架功能簇；须先完成框架 v1/v2 完整体成熟验收再由开发者决定是否立项，平台完全体 P0–P8 及 [上线门禁](plans/ai-platform-product.md#启动开发与上线硬性门禁) 全通过才可上线，不允许阶段性对外开放。收到明确启动指令后，产品代码只在 `projects/*` 分支及独立 worktree 开发，执行 [产品隔离规则](plans/ai-platform-product.md#分支与工作区硬性约束)，不为产品妥协框架契约。

- 标准应用默认 Redis，由开发者要求并在 [ADR-015](decisions/adr-015-redis-default-calibration.md) 确认；memory/file 显式选择，库默认 feature 保持可组合。不得沿用 ADR-014 原 memory 默认或隐式 memory fallback；此为待 M5 实现的 contract。

- `todo.md` 当前功能簇必须与实际代码 WIP 一致；新需求先合入既有范围，当前簇闭环后再切换。已提交的成果不因旧提示词再次成为开发任务。
- 基础读写分离为新增 v1 必交付范围，尚未实现；按 [专题计划](plans/v1-database-read-write-splitting.md) 作为当前 M2 增补开发。默认单库，认证/幂等/队列等关键读取使用主库；不沿用旧 v2 候选表述。
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
| M1 | 内核与 HTTP | `openrust::core`、`openrust::http`、`apps/server`、`openrust-cli`（`new` / `dev` / `serve`） |
| M2 | 数据层 | `openrust::db`、`openrust-test`、模板 `--db` |
| M3 | 邮件、短信、认证及通用安全能力 | `openrust::mail`、`openrust::auth`；国际化/HTTP/安全/审计等见专题计划 |
| M4 | 队列、调度与事件 | `openrust::queue`、`openrust::events` |
| M5 | 缓存、存储与可观测性 | `openrust::cache`、`openrust::storage`、`openrust::http` 可观测性 |
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
