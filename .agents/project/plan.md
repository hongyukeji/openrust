# 当前计划

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：v1 版本共 M0-M6 七个里程碑；根 package 模块迁移、M2 读写分离及模块化配置与 `.env` 机制已完成。当前按依赖顺序继续 M3 及后续 v1 未完成工作。详细交付与验收见 [v1 版本计划](plans/gen1-plan.md)「里程碑」。

## 当前里程碑

M0、M1、M2 当前已验收范围完成；根模块迁移与读写分离均已独立闭环。框架 package 已迁至仓库根 `openrust`，模块包含 `src/config/`、`src/core/`、`src/http/`、`src/db/`、`src/auth/`、`src/mail/`；工作区保留 CLI、proc-macro、测试工具和聚合 server 四个有明确边界的 package。读写分离已在 PostgreSQL 18.6 / MySQL 8.4.13 真实复制拓扑验证暂停、主库新读、副本追平及双 App scope 隔离；完整集群验收仍未完成。日常开发基线为 PostgreSQL 18 / MySQL 8.4 / Redis 8，数据库 CI 使用 PostgreSQL 18 / MySQL 8.4；运行时 PostgreSQL 接受 16+、MySQL 接受 8.4+，最低正式支持版本仍待 v1 发布前确定。

实现与生成器命名遵循 [ADR-006](decisions/adr-006-rust-native-naming.md)：项目采用 `controllers/`、`models/_entities/` 和独立 `migration/` crate；全局 CLI 为 `new`、`dev`、`generate` / `g`，运行期命令由项目二进制提供。

## 执行校准

- **底层调整总览必读**：根模块迁移、M2 基础读写分离及 ADR-030 模块化配置与 `.env` 整理已实现并验证；按 v1 依赖顺序继续未完成能力。M3/M4/M5/M6 能力与最终集群共享验收仍未完成。专题计划中的能力在代码、测试和验收证据齐备前都仍属未完成。
- 根 `openrust` package 模块迁移按 [ADR-021](decisions/adr-021-loco-style-module-layout.md) 已独立实施并完成 workspace、feature、双库、生成项目、示例、文档和旧路径验证。ADR-019 的逐能力目录改名与 `apps/server` 包改名取消；根 tests/examples 尚未覆盖聚合宿主所有验收，故保留 server。
- Loco 应用 Hooks、CLI 与测试体验的边界按 [ADR-023](decisions/adr-023-loco-application-experience-evaluation.md)：不增加平行 Hooks；M4 worker/scheduler 实施前定义取消及停机生命周期，M6 落地只读 doctor 与薄 TestApp/HTTP assertions；现有生成器按 gen1-plan 的里程碑执行。
- 命名校准按 [ADR-016](decisions/adr-016-neutral-application-namespaces.md)：应用配置默认 `APP_`，表默认 `tb_`，均可定制。`APP_` 已在 core/bootstrap、CLI、模板和说明中实现；表名 resolver 已贯通 DatabaseConfig/Db、迁移、测试隔离及认证持久化查询，并通过 PostgreSQL/MySQL 真实映射矩阵验证。M3 仍有功能簇验收待补；禁止直接改生产表。

- AI 平台仅为独立产品规划，不进入当前框架功能簇；须先完成框架 v1/v2 完整体成熟验收再由开发者决定是否立项，平台完全体 P0–P8 及 [上线门禁](plans/ai-platform-product.md#启动开发与上线硬性门禁) 全通过才可上线，不允许阶段性对外开放。收到明确启动指令后，产品代码只在 `projects/*` 分支及独立 worktree 开发，执行 [产品隔离规则](plans/ai-platform-product.md#分支与工作区硬性约束)，不为产品妥协框架契约。

- 标准应用默认 Redis，由开发者要求并在 [ADR-015](decisions/adr-015-redis-default-calibration.md) 确认；memory/file 显式选择，库默认 feature 保持可组合。不得沿用 ADR-014 原 memory 默认或隐式 memory fallback；此为待 M5 实现的 contract。

- `todo.md` 当前功能簇必须与实际代码 WIP 一致；新需求先合入既有范围，当前簇闭环后再切换。已提交的成果不因旧提示词再次成为开发任务。
- 基础读写分离已按 [ADR-029](decisions/adr-029-database-read-routing.md) 实现：默认单库、显式只读副本入口、task-local 主库 scope、无静默故障回退；真实双库复制拓扑测试见专题计划。完整性能/集群验收仍按其各自计划推进。
- M0-M2 等阶段完成仅代表对应已验收范围；后来补入的事务钩子、集群等要求仍需按依赖补做，不能被旧阶段标签遮蔽。
- 每次领取/切换功能簇，对照 [v1 能力覆盖](plans/v1-capability-coverage.md)、[通用能力](plans/v1-common-capabilities.md)、[性能补强](plans/v1-performance-and-readiness.md) 和 [集群要求](plans/v1-cluster-deployment.md) 定位相关未完成项；不能只沿最初 M0-M6 简表推进。
- M3 不限于身份提取器：通信供应商、安全、审计、验证码、通用 Session/限流和流程扩展按专题计划逐簇推进。ADR-031 统一出站 HTTP client 与 ADR-032 请求级国际化已独立完成；下一项按 `todo.md` 推进邮件/短信统一供应商能力。认证 contract 现在就覆盖共享会话、跨节点撤销、全局配额/验证码与公开业务扩展点，不能留到 M6 才补单机假设。
- M3 认证已有能力已在当前基线补验：PostgreSQL 18 与 MySQL 8.4 的认证数据库流程各 4 项通过；两种新生成 `--auth` 项目分别通过格式检查、`cargo check` 和真实 TCP HTTP 数据库流程。CLI 生成包名的格式敏感断言已修复；国际化与出站 HTTP 已独立完成，M3 通信供应商、安全及真实发送验收仍待推进。
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
