# v1 开发体验实施计划

> 受众：实施与验收 AI Agent
> 更新：2026-10-27
> 状态：待按 M6 执行；将 Laravel 借鉴建议落实为既有范围的交付与验收，不表示能力已实现。

## 范围与执行契约

- 按 [ADR-001](../decisions/adr-001-own-kernel-on-axum.md) 复用成熟组件与现有公开机制，以首次启动、业务开发、扩展、测试、诊断及升级的完整流程评估收益。
- 当前先闭合 [todo.md](../todo.md) 的 M5 功能簇，再进入本计划。活跃状态只在 `todo.md` 维护；每次领取一个可独立提交的子簇，执行 Spec → Implement → Verify → Commit。
- 本计划细化已有 M6、[F9/F10](v1-foundation-completion.md#f9-应用模板覆盖) 与 [生产交付计划](v1-performance-and-readiness.md)，不替代其安全、双库、集群和发布门禁。遇到已经实现的条目，核对源码/提交/验收后复用，不重新开发。
- 每簇 Spec 写明现有入口、实际缺口、最小改动、下游兼容、失败语义及验收命令；公开 API 尚未确定时先设计，再实现。以下能力描述不是已存在的方法签名。
- 源码与当前 ADR 高于历史分析。框架在 `src/`，工具在 `tools/`，应用模块由 `apps/mod.rs` 显式组合，综合示例在 `apps/example/`，独立 consumer/拓扑验收在 `tests/consumers/`；不恢复已移除目录。
- 本轮仅记录计划；不安装依赖、不修改 IDE 或运行默认值、不启动 V2 或独立产品。实现时保护既有 WIP，新增依赖登记 `stack.md`，提交仅包含本簇。

## 现有能力与借鉴边界

| 借鉴机制 | 现有基础与权威入口 | 本计划处理方式 |
| --- | --- | --- |
| Service Provider / DI | `Plugin::build/finish`、`AppBuilder`、`Inject<T>`；[core](../core/core.md) | 完善模板、诊断和生命周期用例，保留显式组装 |
| Form Request / Policy / API Resource | `Validated`、Policy、DTO、分页、查询授权；[HTTP](../core/http.md)、[Auth](../core/auth.md)、[DB](../core/db.md) | 生成完整业务流程；仅在重复样板有证据时补小型 API |
| Artisan / 模板覆盖 | `tools/cli/`、F9；[CLI](../core/cli.md) | 补剩余生成器、预览、覆盖、诊断与命令契约 |
| HTTP assertions / Factories | `TestDatabase`、公开 router、F10；[测试工具](../core/test.md) | 完成 TestApp、断言、显式 builder/fixtures |
| Jobs / Notifications / Context | 事件、事务派发、队列、Notifier、调用作用域；[事件](../core/events.md)、[队列](../core/queue.md)、[邮件](../core/mail.md) | 补组合示例、测试替换和保证边界，不重做运行机制 |
| Cache / Storage / Scheduler | 已有统一 driver、命名实例及调度；[缓存](../core/cache.md)、[存储](../core/storage.md)、[调度](../core/schedule.md) | 验证默认使用、替换、能力差异和故障诊断 |

保留 Rust 类型、所有权、异步与显式依赖；不建立动态反射容器、全局静态服务定位器、第二套 ORM/Provider/Hooks 或隐式持久化 Factory。自动模型绑定、动态属性、lazy loading、路由/配置缓存产物不作为必交实现；只有实际场景与测量证明需要时另定 Spec。默认 Redis 故障语义、显式软删除范围、租户/权限边界沿用已有 ADR。

## 依赖与领取顺序

| 编号 | 可交付功能簇 | 前置 | 主要位置 |
| --- | --- | --- | --- |
| DX1 | 默认启动与 IDE 调试 | 当前 M5 簇闭合；启动配置基线核对 | 根 manifest、启动/配置、必要的 IDE 项目配置及文档 |
| DX2 | TestApp 与行为断言 | DX1；公开 router/生命周期接口 | `tools/testing/`、测试 consumer |
| DX3 | 显式测试数据与可控时间 | DX2；F10 | `tools/cli/`、`tools/testing/`、示例 |
| DX4 | 业务生成与输入输出契约 | DX2/DX3；F9 | CLI 模板/生成器、HTTP/Auth/DB 公开组合 |
| DX5 | 插件与只读诊断 | DX4 的命令/模板基础 | CLI、Plugin、配置/路由/数据库诊断 |
| DX6 | 事务、通知与调用上下文组合 | DX2/DX3；既有事件/队列/租户机制 | 示例、consumer、既有模块的实际缺口 |
| DX7 | 本地开发进程编排 | DX1/DX5；既有 supervisor | CLI dev 与进程管理 |
| DX8 | 综合示例、文档与源码升级 | DX1–DX7 | `apps/example/`、中文文档、隔离升级夹具 |

按表顺序逐簇执行；DX4 内先完成 F9 模板覆盖，再交付业务生成。较大簇拆成连续小提交，前一子簇验证通过后再继续，不把全部门禁推迟到 DX8。

## DX1 默认启动与 IDE 调试

- **交付**：核对 `default-run`、默认 workspace 成员、bin/lib targets、工作目录和配置加载，消除启动歧义；保留完整 workspace 显式检查/测试以及 CLI 和性能工具的使用路径。
- **硬验收**：RustRover 同一个 Cargo 配置的 Command 始终只有 `run`；Run 正常提供 HTTP 服务，停止后 Debug 正常启动，请求接口命中业务断点并能继续执行。记录 IDE/工具链版本、配置与断点证据。
- 不能通过加入 `--bin`/`--package`、Cargo alias、包装脚本或另一份调试命令代替该验收。终端 `cargo run`、构建成功和配置文件存在均不能证明 IDE 调试通过。
- Cargo 配置变更前说明默认 build/check/test 与显式 `--workspace` 的差异，并验证 CLI、测试与工具 targets 未丢失。若版本缺陷导致无法满足，记录可复现条件与上游证据，本项保持未通过。
- 验证根宿主与生成应用的实际配置名称、端口、环境覆盖、应用工作目录隔离和正常退出；非法配置给出脱敏的配置键/来源/修复指引，不能暴露凭据。配置优先级以当前 contract 为准，不恢复旧策略。

## DX2 TestApp 与行为断言

- **交付**：基于已有 finalize/build_router/Tower oneshot 封装公共测试入口；提供状态、响应头、JSON 路径、统一错误/校验字段及分页断言。名称与返回类型在 Spec 确认。
- 身份与服务替换显式、按 TestApp 实例隔离；保留真实认证链测试路径，不能让测试登录辅助绕过所有认证验收。断言失败报告期望/实际差异并脱敏。
- 复用既有 mail/SMS/queue/HTTP fake 与记录能力，补调用次数、参数和未调用断言；未实现的能力明确拒绝。验证应用状态不串用、正文大小限制、异常/拒绝响应及后台资源释放。
- **验收**：独立 consumer 只用公开 API 完成成功、校验失败、未认证、无权限、404、分页和通知派发断言；生成应用直接运行测试。进程内 HTTP 验证与真实 TCP/跨节点验证分别记录。

## DX3 显式测试数据与可控时间

- **交付**：落实 F10 普通 Rust builder/fixtures，构造与保存分离；支持字段覆盖、状态组合、关系数据、批量上限和固定种子。明确锁定依赖版本内的随机可复现范围。
- 复用 TestDatabase 的隔离与清理；测试数据默认不触发真实通信供应商。迁移、外键和唯一约束由实际数据库验证。
- TTL、验证码、退避等边界优先使用纯时间参数或 Tokio 可控时间；必要时局部注入时钟，不为全部模块引入新时钟框架。数据库共享时钟和网络超时另外真实验证。
- **验收**：PostgreSQL 18/MySQL 8.4 各验证构造不写库、显式保存、关系/唯一冲突、数据隔离及清理；相同种子可复现；取消/超时和 fake 调用保持确定性。

## DX4 业务生成与输入输出契约

- **交付**：先完成 F9 模板清单、导出、预览、应用覆盖、变量/版本诊断和路径安全，再补 controller/scaffold/command 等缺失生成器。复用既有 model/migration/Policy 生成器，避免重复模板。
- 选择中性资源演示“请求解析 → 校验 → 资源授权 → 业务事务 → 显式响应 DTO”；生成迁移、模型、创建/更新 DTO、handler、路由接入、授权位置和行为测试，默认不覆盖已有文件。
- 明确允许输入/更新的字段、未知字段策略、服务端拥有字段、敏感字段输出规则。列表/详情/写入/批量/导出在各自支持范围内使用同一授权原则；HTTP 与 gRPC 使用既有共同校验/策略，不平行复制。
- SeaORM 查询、显式软删除和关联加载继续复用；排序/过滤使用白名单。序列化不触发隐式数据库访问；不把普通 `Resource` 路由登记与响应 DTO 混为同一种抽象。
- **验收**：无数据库生成路径及 PG18/MySQL8.4 数据路径分别生成后 check/test；数据路径实际执行迁移和 HTTP CRUD，覆盖越权字段、他人资源、非法排序、关系加载及敏感字段不泄漏。API 文档补齐时与真实错误/分页 schema 对照；生成快照只来自实际产物。

## DX5 插件与只读诊断

- **交付**：插件模板与独立 consumer 沿用 `Plugin::build/finish`、`AppBuilder` 和 `Inject<T>`；明确依赖、初始化失败、实例生命周期、可选 feature 和 driver 的公开扩展方式。
- 完成 routes、只读 doctor、配置来源/脱敏与 `db status`，复用已有 config show/migration status。诊断覆盖未注册依赖、重复/冲突注册、未知 store、缺 feature、配置错误和依赖不可达。
- 显式 `db reset` 单独子簇定义目标环境、数据范围和确认机制，只在隔离数据库验收；doctor/status/dev 不隐式 reset 或执行迁移。
- **验收**：自定义 driver 无需改核心；缺依赖可诊断；两个应用和并发请求的用户/租户/语言不串用；不让共享服务捕获请求或事务对象。只读命令前后数据/Schema 不变，输出无密钥；插件停用保留数据，兼容升级与失败退出明确。

## DX6 事务、通知与调用上下文组合

- **交付**：在综合示例及独立 consumer 中串联“资源写入 → 审计 → 事务派发任务/通知 → worker”；复用 Events、Queue、Notifier、现有 metadata/调用作用域，不新增工作流引擎或通知平台。
- 区分提交前校验短路、提交后内存通知和持久事务派发；同数据库同事务才声明业务/任务原子提交。数据库到 Redis/外部服务的失败窗口必须明确；没有可靠中继时不声称跨系统原子性。
- request/trace、locale、tenant 等元数据按已有 contract 显式传播；独立 `tokio::spawn`、任务重试和进程边界逐项核验。任务上下文中的身份不能绕过执行时必要授权，令牌/密码/验证码不进入任务载荷或日志。
- **验收**：双库提交前拒绝、回滚无任务、提交后 worker 可见、多渠道部分失败、重复执行与业务幂等；进程在提交边界异常退出后的恢复。验证事件顺序、短路、后置失败不会假称回滚已提交业务。Redis 路径只验证其真实支持保证；外部实发缺条件则保留缺口。

## DX7 本地开发进程编排

- **交付**：完善现有 dev，按显式应用配置编排 API、worker/scheduler 及 watch；先复用已有合适服务，仅创建本任务确实缺少的资源。依赖选择与配置错误通过 DX5 诊断。
- 生命周期复用现有 supervisor：定义启动顺序、子进程错误传播、端口冲突、重启策略、信号转发、超时/取消/join 和归属资源回收。迁移只有显式配置允许时执行；不得影响既有服务/数据。
- **验收**：实际启动/停止、编译失败后恢复、依赖不可用、重复端口、子进程崩溃、Ctrl-C/SIGTERM，无遗留任务和本任务占用端口；一个应用退出不停止其他应用。不能以代理 serve 代称完整编排。

## DX8 综合示例、文档与源码升级

- **交付**：在唯一 `apps/example/` 中演示资源开发、认证授权、数据库、缓存、队列通知、文件与已实现实时能力；复用真实 v1 API，页面/组织业务仍归 V2。
- 按 [生产交付计划](v1-performance-and-readiness.md#8-源码升级与维护约定) 在临时源码副本演练框架升级，保留用户 apps/配置/迁移，验证冲突、公开 API/任务载荷兼容和数据迁移回滚限制。
- 中文文档贯穿创建、Run/Debug、增加资源、替换驱动、测试、诊断、部署及升级；对外支持矩阵只按实际验证承诺。记录构建时间、操作步骤和阻塞点，不能用主观“优雅”替代证据。
- **验收**：未参与实现的走查者仅凭文档完成上述任务；Agent 代走查注明身份。PG18/MySQL8.4 分别提供生成应用和部署证据；全部关联 CI/consumer targets 实际被发现，完整集群门禁仍按专门计划执行。

## 每簇验收与收口

1. 核对 Git/WIP、当前 ADR 与上述基础能力；只补缺口。审计已有能力后无代码缺口的部分，记录可复验入口而不伪造新增实现。
2. 源码执行 `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo doc --workspace --no-deps`、`cargo deny check`，并运行相关可选 feature；精确矩阵按 [testing.md](../memory/testing.md) 和最新 AGENTS 执行。
3. 数据行为在 PG18/MySQL8.4 真实验证；生成器在生成项目 check/test；HTTP/IDE/进程/外部 driver/集群使用相应真实证据。fake、test skip、编译通过不能替代未执行的验收。
4. 更新受影响 core、模块 README、必要 ADR、todo/plan/progress 和测试记录；成功、失败、未执行和外部阻塞分别报告。使用仓库提交脚本创建仅本簇本地 commit，不推送、发布或打 tag。
5. 按构建产物规则回收本任务资源；其他任务使用共享 target 时延后清理。文档规划仅做 diff、链接与旧引用检查，不启动构建或清理其他任务产物。

## 官方机制参考

以下是设计依据；实施时复核适用版本，具体 Rust API 由当前 contract 决定。

- [Providers](https://laravel.com/framework/docs/13.x/providers)、[Container](https://laravel.com/framework/docs/container)：注册、初始化和作用域。
- [Validation](https://laravel.com/framework/docs/validation/latest)、[API Resources](https://laravel.com/framework/docs/13.x/eloquent-resources)：输入校验及对外输出。
- [Artisan](https://laravel.com/framework/docs/13.x/artisan)、[HTTP Tests](https://laravel.com/framework/docs/13.x/http-tests)：命令、模板与业务测试。
- [Queues](https://laravel.com/framework/docs/13.x/queues)、[Octane](https://laravel.com/framework/docs/13.x/octane)：提交边界与长驻服务生命周期。
