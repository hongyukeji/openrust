# OpenRust 框架设计

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：OpenRust v1 版本框架底座的架构设计：设计原则、模块与应用目录布局、内核、配置、路由、错误、数据、认证、异步能力、CLI、测试工具与框架代码规范。目标、里程碑与验收见 [gen1-plan.md](gen1-plan.md)。

## 设计原则

框架定位与 v1 已确认需求以 [v1 计划](gen1-plan.md) 为准：提供高性能、开箱即用且可扩展的通用底座，让开发者集中于业务开发；性能结论必须有代表性场景证据。

OpenRust 追求的优雅是 Rust 语境下的优雅：靠类型、trait 和约定减少样板，不靠运行期魔法。

组件复用和开发体验以 [ADR-001](../decisions/adr-001-own-kernel-on-axum.md#成熟组件优先与-laravel-式开发体验) 为权威：优先整合成熟组件，以 Laravel 式一致约定和完整开发流程减少业务开发负担。框架承担整合与扩展契约；新抽象先证明其附加价值，已有稳定实现按实际问题迭代。

1. **约定优于配置，但约定可见**：目录、命名、路由、迁移和配置键有默认约定；每个约定在生成代码或文档中可查，不藏在宏展开里。
2. **显式依赖**：服务在 `Plugin` 中用 `AppBuilder::insert` 注册，处理函数用 `Inject<T>` 声明依赖，签名即依赖清单；不提供全局 Service Locator、运行期反射容器或静态 Facade。
3. **一件事一个明显做法**：每类能力只有一个官方入口（验证只用 `Validated<T>`，授权只用 `Policy`），不保留多套并存。
4. **错误是一等公民**：框架错误统一为 `openrust::Error`，自动映射为 `ApiResponse<T>` 统一包装响应；库代码禁止 `unwrap` / `expect` / `panic!`。
5. **宏只做派生，不做控制流**：`#[derive(...)]` 和少量属性宏用于消除样板；不用宏改变程序结构或隐藏调用关系。
6. **可替换但不过度抽象**：按能力提供最小公开契约和 driver；以已实现行为及已确认的第三方接入场景定义边界，不因当前只有内置实现就封闭扩展，也不创建万能 driver 或假想接口。
7. **生成即普通代码**：CLI 生成可读、可改的普通 Rust 代码；少量宏只减少样板，不隐藏控制流。
8. **可测试是默认值**：适合替换的外部能力提供显式 fake；业务单元测试可隔离外部服务，驱动和集群验收仍必须使用真实服务。
9. **最小默认，按需开启**：库默认 features 最小，具体依赖以 Cargo.toml 为准；数据库驱动、Redis、S3、OpenTelemetry 等通过 Cargo feature 开启。

领域差异按 [ADR-009](../decisions/adr-009-extensible-capability-drivers.md#可选领域插件) 由应用及可选插件承接；通用底座的优先级与开发成本评价见 ADR-001。[AI模型平台](ai-model-platform.md) 仅为后续领域案例，项目范围与启动门禁单独维护。

## 关键流程的公开扩展点

v1 应在数据库事务、注册/登录、文件上传等常规流程的合理边界提供公开、类型化的钩子或事件，让应用/第三方包通过注册扩展，不修改核心源码。现有 Plugin 构建钩子不能代替业务流程扩展点。

| 边界 | 最低语义 |
| --- | --- |
| 事务提交前 | 在可控事务仍有效时执行必要校验，失败可中止并回滚；不在持锁期间执行邮件/SMS 等不可回滚远程副作用 |
| 事务提交后 | 仅真实提交成功后通知；回滚和提交失败不触发；回调失败不能把已提交事务伪装为已回滚，错误/补偿策略明确 |
| 用户注册前 | 扩展业务校验与允许的资料转换，可明确拒绝；不泄漏凭据或绕过框架安全规则 |
| 用户注册成功后 | 用户数据提交后发出成功事实；欢迎消息、资料初始化等按是否需同事务/异步分别注册，不能模糊执行位置 |
| 登录前/成功/失败 | 登录前可增加风控校验；成功/失败通知在凭据验证与会话/令牌状态明确后触发，不允许监听器绕过认证；载荷脱敏 |
| 文件接收前 | 校验权限、配额、声明元数据和上传策略，可拒绝；仅凭元数据不能完成实际内容检测 |
| 内容可读取、发布前 | 向扩展提供有界流或受控临时对象，允许图片/敏感内容/恶意文件检测及处理；要求检测时，通过前不可公开访问 |
| 上传完成/失败 | 成功后可扩展缩略图、索引、审计或通知；失败/拒绝/取消清理临时资源，持久化与对外发布的成功状态分别明确 |
| 其他能力流程 | 邮件/短信、任务、存储等按真实扩展场景复用同类契约，避免为每个内部函数机械增加 before/after |

- 同步钩子用于会影响当前操作的校验/处理，事件用于已经发生的事实，HTTP 管道用于请求横切处理；三者职责明确，复用 Plugin/事件/队列机制，不建立三个重叠的全局扩展系统。
- 注册入口、载荷、可变字段、执行顺序、短路、失败传播、异步/取消与资源边界必须形成 contract。默认不注册扩展即可正常运行；提供独立应用注册示例，不依赖反射、字符串事件名或全局可变注册表。
- 数据库钩子围绕框架管理的事务边界，明确嵌套事务/savepoint、重试与回调执行次数。底层直连/原生 SQL 逃生口的覆盖范围必须说明，不能承诺拦截全部 ORM/驱动调用。
- 文件内容检测是可选扩展，框架交付注册、读取、拒绝、状态与清理机制，不强制内置特定审核算法或供应商。未注册检测时保持普通上传的简单路径。同步检测须有超时/并发上限；异步检测标为待处理，通过前私有隔离，超时/失败不能默认为通过。
- 检测结果必须绑定实际发布的对象版本/内容，检测后再次修改需重新校验；缓存/CDN、临时链接、下载接口及客户端直传完成路径也不得绕过发布门槛。集群异步检测需共享状态和幂等处理，不能依赖另一节点不可访问的本地临时文件。检测会消费流时，定义重读/暂存契约，不无界缓冲整个文件，也不向扩展泄漏存储密钥。
- after-commit 内存回调不等于可靠投递；需跨进程故障恢复的副作用复用事务内数据库任务或 outbox。集群下区分“提交该事务的节点执行一次”与“所有节点广播”，监听器可能重试时要求幂等。
- 验收包含公开注册/顺序、前置拒绝、提交/回滚/提交失败、后置错误、注册登录成功与失败、敏感载荷、重试及集群投递边界；上传另验证检测拒绝/超时、发布前不可读、版本绑定、直传与下载门槛、失败清理和集群恢复。真实事务语义需双库证据。具体 API 名称和布局先按 ADR-006 定稿。

### 开发 Agent 的自主扩展性判断

用户列举的事务、注册登录和上传场景用于说明意图，不是穷举清单或固定 API。每个功能簇在 Spec/自查中主动判断业务开发者可能需要替换、校验、转换或观察的节点，选择最小合适机制并记录取舍；不等待用户逐项指出。

| 常见场景 | 可考虑的扩展用途 |
| --- | --- |
| 应用启动/就绪/停机 | 初始化业务依赖、追加就绪检查、排空连接和释放资源 |
| HTTP 请求/响应 | 访问校验、业务上下文、语言、响应转换；优先复用既有中间件 |
| 数据保存/删除/事务 | 业务约束、变更观察、缓存失效、提交后任务；优先复用 ORM/事务公开边界 |
| 账号与会话 | 注册资格、风控、登录结果、退出、密码/权限变更后的审计与业务处理 |
| 文件上传/读取/删除 | 内容检测、图片处理、命名/元数据、访问规则、索引和清理 |
| 邮件/短信/通知 | 收件人/渠道策略、模板参数、发送前策略、结果与回执处理 |
| 任务与调度 | 执行上下文、失败分类、重试策略、完成通知和运行指标 |
| 缓存与共享状态 | key/命名空间、加载策略、失效观察、驱动故障与明确降级策略 |
| WebSocket/SSE | 握手/频道授权、业务消息处理、断开清理与投递结果 |
| 外部 HTTP 调用 | 签名/认证、请求与响应转换、故障分类、受控重试及观测 |
| 审计与 API 数据输出 | 字段白名单、脱敏、记录去向与业务 DTO 转换 |

此表是判断提示，不要求每行每阶段建立独立钩子。既有 trait、driver、事件或中间件已足够时直接复用；同一职责不增加第二个入口，已有行为边界仍需遵守。

- 判断问题：真实业务需求是什么？应用仅通过公开 API 能否做到？能否用既有机制组合？需要改变流程还是只观察事实？引入后普通用户是否增加配置负担？
- 选择机制：校验/转换且影响当前结果时评估同步钩子或策略 trait；事实通知评估事件；请求横切优先中间件/责任链；后端替换用 driver；构建配置在清晰时使用链式调用。无需将这些模式分别建成通用引擎。
- 明确可修改范围、顺序、错误、短路、超时、取消、重试与敏感载荷；结合事务、流式消费和集群语义判断，扩展不能绕过不可放宽的安全规则。
- 在 Spec 中简要记录“场景—现有机制—采纳/不新增理由—验证”。公共 contract 按角色分工设计后实施，日常局部选择自行推进，不因判断步骤重复请求用户确认。
- 提供有代表性的外部注册/实现示例，证明不改核心能扩展；测试正常、不注册扩展、多扩展组合、拒绝/失败和必要的并发/集群行为。没有真实需求的假想扩展点不预留，合理需求也不以“避免抽象”为由拒绝支持。

## 跨语言设计参考

OpenSpring 可作为设计参考，编程思想与问题解决方法可以跨语言复用；OpenRust 仍是独立 Rust 框架，不是 Java 项目的移植。

- 在相关功能簇设计时，按需只读查看 `/Users/Home/Codes/Java/openspring` 的实际规则、设计和代码；先定位相关模块，不先全仓扫描。不修改参考仓库，不依赖旧聊天或记忆认定其当前实现。
- 候选参考主题包括模块边界与聚合启动、能力接口与供应商适配、配置与启动校验、权限/审计、事务/事件/幂等、缓存一致性、任务调度与集群治理。这是待核查方向，不表示已证实 OpenSpring 均有可复用实现。
- 每次采纳需说明：来源路径与所解决问题、适用前提、OpenRust 既有对应能力、采用/调整/不采用的理由，以及 Rust 实现与验证方式。重要公共契约变化形成 ADR，普通局部设计写入对应 Spec，不复制执行流水。
- 使用 Rust 的类型、trait、所有权、显式依赖与异步模型表达设计；不机械复制 Java 继承层级、反射/AOP、全局服务定位或注解驱动控制流。已有成熟 Rust 机制足够时优先复用。
- 每类职责保留一个公共入口，先扩展既有能力，避免为“参考模式”新增平行框架。跨项目业务模型、命名、默认参数和历史约束不自动带入本仓库。
- v1 参考通用基础设施与多副本正确性；v2 再参考后台产品模块与操作流程。采纳后仍执行本项目双库、集群、安全、性能与易用性验收，不能用参考项目可用代替本项目证据。
- 借鉴思路不要求复制代码；实际复用代码时核对许可证、记录来源并按要求保留声明。

## 框架参考与对标

参考范围按版本划分：**v1 仅按 Loco、Laravel、Goravel、OpenSpring 的顺序参考；若依与芋道仅用于 v2。** v1 的四个框架按同一套维度综合评估：整体架构与模块边界、项目结构与启动体验、通用能力覆盖、扩展机制、测试与工具链、性能与资源边界、部署运维和安全性。Laravel 的设计思想与开发体验是明确偏好，不改变参考顺序，也不将任何一个框架限定为某一类设计的专属来源。最终以 OpenRust 的 Rust 语义、版本范围和自身验证结果决策。

| 参考对象（按指定顺序） | 项目类型与范围说明 |
| --- | --- |
| [Loco](https://github.com/loco-rs/loco) | Rust 应用框架；与其余参照对象按相同维度综合对照 |
| [Laravel](https://laravel.com/docs) | PHP 应用框架；与其余参照对象按相同维度综合对照 |
| [Goravel](https://www.goravel.dev/) | Go 应用框架；与其余参照对象按相同维度综合对照 |
| OpenSpring | Java 应用框架；需要借鉴具体实现时先只读核对当前源码，再按相同维度综合对照 |

综合评估时只提炼能解决 OpenRust 实际问题的设计，不直接照搬 PHP/Go/Java 运行时机制、术语或业务模块。

OpenRouter 是后续 AI模型平台的产品能力对标，不列入上述框架参考清单。

### v1 对标与补齐落点

依据上述四框架的一手文档、OpenSpring 本机源码及 OpenRust 当前代码/计划，补齐通用能力和跨能力组合契约。以下证据用于设计参考，不表示相关 OpenRust 能力已经实现或通过验收，也不构成性能排名。

| 参考框架与一手依据 | 对 OpenRust 的比较结论与落点 |
| --- | --- |
| Loco：[应用上下文](https://loco.rs/docs/explanation/appcontext-and-di/)、[调度](https://loco.rs/docs/how-to/schedule-jobs/)、[生成器模板](https://loco.rs/docs/how-to/override-templates/)、[租户作用域](https://loco.rs/docs/how-to/multi-tenancy/) | 复用现有 Plugin/AppState，补调用作用域、日历调度、应用模板覆盖和显式租户隔离；查询覆盖与逃生口须明确，不新增平行 Hooks |
| Laravel：[上下文](https://laravel.com/framework/docs/context)、[调度](https://laravel.com/framework/docs/scheduling)、[查询](https://laravel.com/framework/docs/queries)、[测试](https://laravel.com/framework/docs/mocking)、[认证脚手架](https://laravel.com/framework/docs/starter-kits) | 核对完整开发流程与可组合性，补调度、轻量测试数据、安全模板、国际化、缓存策略及可选身份能力；保留 Rust 的显式调用，不复制动态 Facade/ORM |
| Goravel：[gRPC](https://www.goravel.dev/the-basics/grpc.html)、[验证](https://www.goravel.dev/the-basics/validation.html) | gRPC 已纳入 v1；借鉴服务/客户端配置与生成体验，复用验证和授权。TLS 配置失败关闭，不静默降级明文 |
| OpenSpring：本地源码，使用前记录实际提交与相关路径 | 驱动、租户作用域、身份接入及通用导出契约可按真实边界参考；AI Provider 的隔离/熔断和 Webhook 地址检查仍是局部实现，不推广成全框架保证 |

OpenSpring 只读证据路径：`openspring-core/openspring-core-ai/src/main/java/com/openspring/core/ai/model/support/AiModelResilienceExecutor.java`、`openspring-core/openspring-core-notification/src/main/java/com/openspring/core/notification/channel/NotificationWebhookEndpointGuard.java`。这两项是局部实现参考，本轮没有执行其测试，也不把简单 DNS 预检查视为已完整防御重绑定。

新增必交范围、最小边界与验收集中维护于 [v1 底层补齐计划](v1-foundation-completion.md)，与 [能力覆盖](v1-capability-coverage.md) 共用同一实现归属，执行只在 `todo.md` 领取。gRPC 按 [ADR-040](../decisions/adr-040-grpc-integration.md) 完整交付。cron/IANA/DST、模板覆盖、显式轻量 factory/fixtures、安全邮件模板、复数/地区格式化、cache tags/SWR/singleflight、可信租户隔离、公共查询权限、外部 OAuth2/OIDC/SSO 客户端与 TOTP/恢复码、CSV/XLSX 均为 v1 必交机制；“可选启用”不表示可省略实现与验收。

Passkeys、独立 OAuth 授权服务器、更多数据库/MQ、搜索、运行时 feature flags 与 SSR/静态 Starter 按 [后续评估](../roadmap.md#框架对标后的候选评估) 处理。GraphQL、任意 DAG、分库分表与跨库事务的既有范围边界不变；通用租户隔离不包含组织管理或租户运营后台。

### v2 附加参考

v2 保持 Loco、Laravel、Goravel、OpenSpring、若依、芋道的原顺序，在通用层之上评估后台套件；若依与芋道的证据只用于本阶段，不作为 v1 设计依据。具体交付与候选见 [v2 计划](v2-admin-platform.md)。

| 附加参考与一手依据 | v2 参考边界 |
| --- | --- |
| RuoYi-Vue：[官方手册](https://doc.ruoyi.vip/ruoyi-vue/document/htsc.html)、[仓库](https://github.com/yangzongzhuan/RuoYi-Vue) | 组织/部门 DataScope、任务监控、业务表格与生成体验；只比较传统分离版，不混入 RuoYi-Cloud 或社区分支 |
| 芋道：[仓库](https://github.com/YunaiV/ruoyi-vue-pro)、[数据权限](https://doc.iocoder.cn/data-permission/)、[多租户](https://doc.iocoder.cn/saas-tenant/) | 后台数据权限、租户管理与套件操作流程；区分 Boot/Cloud、完整/精简版，复用 v1 通用机制，不整体移入商城、支付、CRM/ERP 或流程平台 |

## v1 通用能力补齐

国际化、WebSocket/SSE、gRPC、统一出站 HTTP、加密与密钥管理、审计和请求幂等按 [通用能力计划](v1-common-capabilities.md) 交付；后续确认的通用底层范围以 [底层补齐计划](v1-foundation-completion.md) 为入口。优先复用既有模块与成熟实现；公共 API、配置和依赖选型先定 contract，不自动为每项能力新增 crate。

## 框架仓库布局

框架实现集中在根 `openrust` package 的 `src/`，配置代码在 `src/config/`。目录迁移与源码 workspace 交付已完成，当前实际结构见 [architecture.md](../architecture.md)，验收入口见 [Spec](module-layout-migration-spec.md) 与 [ADR-021](../decisions/adr-021-loco-style-module-layout.md)。

工具 package 统一位于 `tools/{cli,macros,testing,performance}/`，旧 `crates/` 已移除。按 [ADR-039](../decisions/adr-039-source-workspace-distribution.md)，整套源码交付，内部包名仍与各工具目录末级同名，全部禁用发布；proc-macro 不合入普通 library。`performance` 单独承载性能基准且不进入默认成员。`examples/<name>/`、`tests/<capability>/` 采用明确的 Cargo target/harness，不能靠嵌套目录自动发现测试。`apps/` 保留应用约定；根目录集成宿主按 ADR-062 迁入根 package。

未实现的 cache/storage 等能力在对应里程碑以实际实现进入 `src/`；不预建空模块或为每种能力创建独立 package。事件当前事实见 [core/events.md](../core/events.md)。

## 生成的应用布局

默认在整套源码的 `apps/<name>/` 生成业务应用，其 `src/` 是业务代码，与框架根 `src/` 职责不同；应用与 migration 使用本地 workspace 依赖。当前生成器以 [CLI 事实](../core/cli.md) 和模板源码为准；以下是 v1 完成时的组织目标，不表示现有 `new` 已生成所有目录。

```text
apps/<app>/
├── Cargo.toml                 # package.metadata.openrust
├── .env.example               # .env 不入库
├── config/                    # default/development/test/production.toml
├── src/
│   ├── main.rs                # openrust::run::<App>()
│   ├── app.rs                 # 显式 Plugin/路由/命令组装
│   ├── config/                # 需要自定义类型化配置时建立
│   ├── controllers/
│   ├── models/_entities/
│   ├── services/
│   ├── policies/
│   ├── jobs/
│   ├── events/
│   ├── mailers/
│   └── middleware/
├── migration/                 # 使用数据库时的 workspace member
├── tests/                     # 按场景分目录并显式注册 targets
├── compose.yaml
├── Dockerfile
└── AGENTS.md
```

只生成所选能力实际需要的文件。默认认证/资源/job 示例、Redis、完整 dev 编排是 v1 黄金路径验收目标；当前 `--auth` 是显式选项，dev 代理运行服务，没有自动编排依赖与迁移。不得用目标树推断功能已交付。

## 应用内核与依赖注入

- `Application::build` 使用 `AppBuilder::plugin/insert/command` 显式组装；HTTP 通过 `AppBuilderHttpExt::routes` 注册。队列通过 `QueuePlugin::register_job` 与 schedule 方法配置；不存在通用 `.jobs()` / `.schedule()` builder API。
- `AppState` 是按类型索引的服务注册表；`Inject<T>` 读取服务。缺失服务返回受控错误，硬依赖在 plugin 构建阶段检查。
- 生命周期是配置引导、tracing、应用 build、所有 plugin build（含动态追加）、冻结、顺序 finish、命令执行；不提供反射容器或平行 Hooks。
- HTTP 服务与 worker/scheduler 命令各自持有并停止自己的任务；当前 Plugin 没有通用 shutdown 回调。取消、限时 drain、abort/join 与 lease 恢复遵守 [ADR-037](../decisions/adr-037-queue-worker-lifecycle.md)。HTTP/gRPC/实时流的有界运行和联合退出按 [生产补强](v1-performance-and-readiness.md#6-生产默认值与开发编排) 落地；仅调用 graceful shutdown 不等于所有后台任务都有退出截止。
- 协议无关验证与单次调用/任务的上下文边界按 [补齐计划](v1-common-capabilities.md#5-协议无关验证与调用作用域) 实施；复用现有结构，不把身份、语言和取消等可变调用状态放入共享 AppState。
- 运行时命令以应用 `help` 输出为准。`serve`、迁移/seed、queue/schedule 由注册 plugin 提供；routes/doctor 等目标命令未全部实现。

## 配置

- 唯一 ConfigLoader 合并默认值、根 `config/default.toml`、环境 TOML、非生产 `.env`、进程环境；进程变量优先，production 不注入 `.env`。
- 默认命名空间 `APP_`，启动前可定制；`APP_ENV`、`APP_CONFIG_DIR`、`APP_DATABASE__URL` 等规则见 [ADR-016](../decisions/adr-016-neutral-application-namespaces.md)。`DATABASE_URL` / `PORT` 是通用别名，同键应用命名空间优先；`REDIS_URL` 随缓存能力落地。
- 框架 Rust 配置类型集中在 `src/config/`，能力 plugin 通过 `section` 提取类型；业务通过 `Application::Config` 定义应用配置。旧类型路径用 re-export 保持必要兼容，不复制配置 loader。
- 启动验证完整配置键和错误来源；敏感值始终脱敏。`config show` 只读展示，`.env.example` 不含可用凭据。详见 [ADR-030](../decisions/adr-030-modular-configuration.md)。

## 路由与控制器

- 使用 `Routes`、`#[openrust::resource]` 和 `resource_with` 组合 handler；公共 API 以 [HTTP 事实](../core/http.md) 为准，不把模块名当 Rust 类型参数。
- 输入统一经 `Validated<T>`；响应使用统一错误包装及 `Created` / `NoContent` / `Paginated<T>`。认证用 `CurrentUser` / `OptionalUser`，主要路径为 JWT；业务事务在 service 层。
- 请求 ID、追踪、CORS、超时、压缩、panic 包装按当前 middleware 顺序执行。通用路由限流已有显式 `RateLimitLayer`，配置与边界见 [HTTP 事实](../core/http.md)；OpenAPI/非生产文档 UI 仍待补齐。
- 查询过滤/排序白名单、DTO 输出与显式软删除已有基础实现和示例，边界见 [HTTP 事实](../core/http.md) 与 [数据层事实](../core/db.md)。后续查询体验按 [能力覆盖](v1-capability-coverage.md) 补强，复用 SeaORM，不另造 query language。

## 错误模型

- 统一使用 `openrust::Error`，主要错误类别包括：`BadRequest` / `NotFound` / `Unauthorized` / `Forbidden` / `Validation` / `Conflict` / `RateLimited` / `Config` / `Custom` / `Internal`；业务错误可映射 Custom 或实现 From。
- JSON envelope 包括 `success/code/message/request_id` 和有业务载荷时的 `data`，校验错误有字段信息，分页元数据为明确类型；实际 HTTP 状态模式以 `[api] http_status` 为准。5xx 只输出通用消息，内部原因进入日志并脱敏。
- CORS preflight、流式/非 JSON 等协议例外遵守已验收契约，不能为了包装破坏协议。`NoContent` 返回省略 `data` 的成功 JSON 包装，并非 HTTP 204 空响应。

## 数据层

- SeaORM 2.x 与 migration 是唯一 ORM/迁移基础；直接 SeaORM connection 是逃生口，不承诺拦截全部底层 SQL。`Db::sqlx()` 当前不存在。
- 表名 resolver 已实现 `tb_` 默认、自定义/空前缀、完整映射，贯通迁移、查询与 TestDatabase；配置修改不自动重命名现有表。
- `Db::transaction` 控制提交/回滚，事务固定当前实例 primary；`read_connection()` 显式使用该实例的只读副本，`with_primary` 提供 task-local 写后读保护，无静默故障回退。独立 named connections 见 [ADR-045](../decisions/adr-045-named-database-instances.md)，不与 replicas 混为一谈。
- 跨数据库 SQL 在 dialect 边界；框架表使用可移植类型。连接池、超时和慢查询按实际配置校验。声明支持新 backend 必须补驱动、迁移、方言及真实能力验证，不能仅因底层 ORM 支持就宣称框架支持。
- TestDatabase 使用每用例隔离数据库；具体 PostgreSQL/MySQL 建库与清理实现见 [测试事实](../core/test.md)，PostgreSQL 已用模板克隆，MySQL 逐库运行迁移，不宣称已有 MySQL 迁移缓存。
- UUIDv7 与显式软删除生成选项已经实现，范围及双库验收见 [数据层事实](../core/db.md)。查询 scopes 和关联加载体验在既有实现上补强；事务扩展点遵守本文件公开流程规则。
- v1 补可信租户作用域、公共查询权限规则及通用 CSV/XLSX 数据交换；只提供可选机制，复用 ORM、验证、任务和存储，不固化组织模型。边界与双库/隔离验收见 [底层补齐计划](v1-foundation-completion.md)。

## 认证与授权

- JWT access/refresh 是 API 默认路径；Session/CSRF 显式可选，PAT 为自动化/集成凭据。共同身份模型、撤销、认证失败和安全边界以 [ADR-018](../decisions/adr-018-jwt-primary-authentication.md) 与 [auth 事实](../core/auth.md) 为准。
- `AuthPlugin::new()` 使用数据库认证器，`with_authenticator` 支持替换。已提供但无效的凭据不降级匿名；密码使用 Argon2id。
- `AuthFlows` 已含注册、登录、退出、密码重置、邮箱验证；后两者复用 VerificationCodes。独立 SignedLinks 提供用途隔离链接，不能把验证码流程写成已改用签名 URL。
- 当前 `Policy` 是基于数据库的授权服务结构体，提供角色/权限及 PAT ability 交集；不是已实现的资源 Policy trait。资源策略/生成器如需新公开 API，先基于现有入口设计，不建立第二授权系统。
- v1 必交外部 OAuth2/OIDC/SSO 客户端接入及可选 TOTP/恢复码，复用本地主体、JWT/Session、加密、限流与审计；不建设独立 OAuth 授权服务器。协议、账号绑定与恢复边界见 [底层补齐计划](v1-foundation-completion.md)，不将计划接口写成当前实现。
- 共享 Session、refresh、验证码、限流等已有双库证据，通用 HTTP 限流复用同一服务；注册/登录扩展已按 [ADR-041](../decisions/adr-041-transaction-and-auth-flow-events.md) 交付，完整跨节点组合仍待验收。JWT 即时撤销策略与 access token 存活窗口须明确，不默认声称纯 token 可即时全局撤销。

## 队列、调度与事件

- 当前 Job 契约为 `fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>>`；显式稳定 NAME 与 handler allowlist，参数用 serde。没有 `#[job]` 属性宏或 `Job::run`。
- `Queue::dispatch` / `dispatch_with_options` / `dispatch_in_transaction` 使用显式任务名和序列化参数；具体签名见源码与 [queue 事实](../core/queue.md)，不宣传未实现的 awaitable fluent dispatch builder。
- 官方数据库 driver 已实现。唯一键只限制同一 queue/key 的活动任务重叠，完成后释放；不是永久请求幂等。执行至少一次，handler 自行保持业务幂等；外部副作用不能与数据库提交原子化。
- worker 具有受界并发、重试/失败保留、超时、lease/ownership token、心跳和有限停机；任务链/有限批次已有实现，parent retry 可重跑成功子任务，失败策略和并发边界见 queue 事实；旧 token 接管与生产资源验收继续按 ADR-037。
- 调度已支持数据库 UTC 时钟、interval/daily slots、slot 与 job 同事务去重、run/once/list。现有首版不含 cron/IANA/DST；这些已纳入 v1 必交，按 [底层补齐计划](v1-foundation-completion.md) 先定 DST、错过触发和补跑上限，再实现并验收；不承诺业务副作用 exactly-once。当前事实见 [schedule](../core/schedule.md)。
- 类型化进程内有序事件与显式队列 listener 已实现，语义与边界见 [ADR-037](../decisions/adr-037-queue-worker-lifecycle.md) 及 [events 事实](../core/events.md)；不接外部事件总线。进程内通知可能因崩溃丢失，可靠路径复用事务内任务/outbox，不把单进程 listener 写成集群广播。
- 原规划可选 Redis queue 与 sync 测试执行在 v1 的 M5 Redis 能力接入后补交，ADR-037 只关闭数据库基础簇的范围。当前没有对应 feature；先明确持久化、事务派发及 worker 能力差异，不承诺 Redis 与业务数据库共享原子事务，也不混同默认 Redis 缓存。

## 缓存、存储、邮件与短信

- Cache 待实现：default/named stores 与自定义 driver 按 ADR-014，标准应用默认 Redis 按 ADR-015；memory/file 显式选用，库 feature 可选，无静默回退。原子锁/计数能力不能用普通 get/set 伪装。
- cache tags、SWR 与 singleflight 纳入 v1，须明确驱动支持、失效与共享边界；安全邮件模板、i18n 复数及地区格式化复用现有通信和语言入口。详细契约与验收见 [底层补齐计划](v1-foundation-completion.md)，不增加重复缓存、翻译或模板系统。
- Storage 待实现：复用 OpenDAL，local/S3-compatible、命名实例、流式传输、上传检测发布边界与扩展点；具体 API 名先定 contract，不把示意接口当现有实现。
- Mailer/公开 MailDriver 已有 SMTP/log/fake；LocalizedEmail 复用显式 Locale 和 i18n catalog。SmsSender/SmsDriver 支持文本/供应商模板与 Twilio adapter，production 禁止 log；本机 Mailpit 实收已有证据，Twilio 实发仍需授权账号与目标。
- M4 增加队列化发送和薄多渠道通知；验证码与短信传输职责分离，默认不因此开启短信登录。不引入第二套邮件模板引擎或重试循环。

## gRPC

[ADR-040](../decisions/adr-040-grpc-integration.md) 定义可选 gRPC 服务端/客户端与四类调用的 v1 交付契约。复用应用装配、验证、认证、错误类别、配置及观测，保留 gRPC 的 Status/metadata/流式协议；当前尚无实现，不把可选 feature 或示例名称写成现有入口。

## 可观测性

- 已有 tracing、text/JSON 日志、request ID、健康检查及 panic 包装。
- readiness、Prometheus/OTLP、资源上限与代表性性能基线仍按 [性能计划](v1-performance-and-readiness.md) 实施。探针须有时限并反映实际依赖，不用 liveness 代替 readiness。
- WebSocket/SSE 的在线跨节点投递复用共享传输；限额、鉴权、取消、背压、滚动停机与资源释放是交付条件，不只提供 Axum upgrade 包装。

## CLI

当前 new、dev/serve 代理、model/migration/mailer/policy 生成及运行时命令转发以 [CLI 事实](../core/cli.md) 为准。v1 补齐 job/controller/scaffold/command、routes、只读 doctor、完整 dev 编排，按原能力里程碑交付，M6 做全集复验。

生成器输出普通代码，默认不覆盖；字段 DSL、冲突、错误和 fmt 必须验证。模板当前使用内嵌文本/字符串渲染；是否使用 minijinja 取决于真实复杂度，未选用前不作为既定依赖。

v1 交付应用级生成器模板覆盖，开发者不修改框架源码即可定制；覆盖优先级、路径安全、冲突预览及生成物验收见 [底层补齐计划](v1-foundation-completion.md)。

## 测试工具

- 当前 `testing` 提供 TestDatabase；薄 TestApp/HTTP assertions 待 M6，复用 finalize/build_router/Tower oneshot，不复制运行时和数据库 fixture 系统。
- 显式轻量 factory/fixtures 已纳入 v1，复用测试数据库与普通 builder，支持确定性构造及关系数据组合，不引入隐式持久化或要求所有模型实现统一 Factory trait。契约与验收见 [底层补齐计划](v1-foundation-completion.md)。
- fake 必须显式注入，不能静默替换生产驱动；fake 验证业务逻辑，真实数据库/Redis/SMTP/对象存储验证适配与跨节点行为。
- 可控时钟只在存在时间边界测试需求时设计；队列/scheduler 的共享 UTC 数据库时钟不能由单进程冻结时间替代。

## 框架代码规范

以 [conventions.md](../conventions.md)、[stack.md](../stack.md) 和 `AGENTS.md` 为唯一工程规则入口。框架能力是 `openrust::<capability>` 模块；只有命令行工具、proc-macro 或测试依赖边界保留内部 package。公共异步 trait 根据对象安全/动态 driver 需要采用原生 async、BoxFuture 或现有适配方式，不机械统一一种签名。

已执行门禁与待建立发布门禁分别记录。基于已有源码版本的 API 兼容性检查、完整模板快照和 mdBook 不因列入目标就算 CI 已实现；发布前须实际补齐。文档、模板、示例和下游调用方必须随公共 API 同步。

## 相关

- [v1 版本计划](gen1-plan.md)
- [开发计划索引](README.md)
- [ADR 索引](../decisions/README.md)
