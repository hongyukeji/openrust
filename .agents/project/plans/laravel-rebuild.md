# Laravel 式 OpenRust 全面重构计划

> 受众：设计、实现、验收 Agent
> 更新：2026-10-28
> 状态：整体方向已授权；本文件是目标设计与实施计划，代码重构尚未开始。

## 1. 目标与授权

以 Laravel 的完整开发体验为目标，像 Goravel 一样让熟悉 Laravel 的开发者快速识别目录、术语、配置、路由、模型、认证、任务和测试流程。允许架构和代码推倒重来，无旧 API、数据或目录兼容要求；必须保留 Rust 类型安全、所有权、原生 async、trait、Cargo 与 IDE 能力。

“低学习成本”以常见任务的对照走查验收；Rust 语法、所有权和异步仍需学习，不承诺 PHP 源码直接运行或跨语言零知识成本。范围覆盖机制、默认体验、生成、文档、运维和故障语义，不能以目录改名完成本任务。

权威决策为 [ADR-070](../decisions/adr-070-laravel-first-rebuild.md)。当前代码与 Git 说明已实现事实，本计划说明获准的新目标。唯一活跃队列是 [todo.md](../todo.md)；原 M5/M6、DX1–DX8 与 F/G 专题被重排到本计划，不取消其有效能力和未完成验收。

本轮规划起点为 `7f964b8`，开始时工作区干净，只有一个 Git worktree；系统存在多个 Agent 进程，不能据此认定它们都在本仓库工作。执行前重新核对 HEAD、WIP、进程与代理任务，不依赖本快照。

## 2. 上游依据与范围冻结

参考入口（2026-10-27 读取；版本页不等于固定 commit）：

- [Laravel 13.x 文档目录](https://laravel.com/docs/13.x)：逐章建立能力和子行为清单。
- [Laravel 应用骨架](https://github.com/laravel/laravel)、[框架源码](https://github.com/laravel/framework)：核对实际默认行为、测试、依赖及许可证。
- [Container](https://laravel.com/docs/13.x/container)、[Eloquent](https://laravel.com/docs/13.x/eloquent)、[Queues](https://laravel.com/docs/13.x/queues)：生命周期、数据与异步 contract 的首批依据。
- [Goravel](https://www.goravel.dev/)、[目录](https://www.goravel.dev/getting-started/directory-structure.html)、[框架源码](https://github.com/goravel/framework)：跨语言概念、配置、目录和统一入口的参照。

R0 固定 Laravel 文档版本、`laravel/laravel`、`laravel/framework`、Goravel 源码 tag/commit 与读取日期；不能按浮动 master 的变化无限扩张开发。每个功能簇再读对应官方文档、实现和测试，保留来源与差异说明。仅参考设计可自行实现；复制代码或文档须遵守原许可并保留必要声明。

建立逐项矩阵：`ID / 上游章节与源码锚点 / 子行为 / OpenRust 目标 / 现有实现与证据 / 复用或重写 / 依赖 / 测试 / 状态 / 差异理由`。矩阵必须覆盖固定版本完整导航和官方包列表，以下清单是启动范围，不是用来跳过新发现章节的封闭简表。

状态只允许：待盘点、contract 已定、实现中、待真实验收、已验收、外部阻塞、不适用（有理由）。复用必须通过新 contract 回归；不适用必须解释语言/平台/产品边界，不能用于掩盖实现困难。

### 生态边界

| 层级 | 处理 |
| --- | --- |
| Laravel 框架核心、Goravel 通用基础 | 全面映射；适用机制进入必交矩阵，Rust 等价实现必须有体验与行为证据 |
| Sanctum/Fortify/Socialite、Scout/Pennant、Reverb、Horizon/Telescope/Pulse、Sail/Envoy/Dusk/Octane 等 | 功能拆入认证、搜索/特性、实时、可观测、开发/测试/部署；运行机制归底座，管理视图归 V2；不能仅写“生态包”而丢弃能力 |
| Passport、Cashier、Nova 等领域或产品套件 | R0 单列独立能力与依赖。授权服务器、支付订阅等不混入核心；形成可选包实施阶段，禁止将文档列项算成交付；支付实测使用授权 sandbox，生产扣款不在授权内 |
| Blade、Livewire、Inertia、Vite 等前端体验 | 视图组件/布局/转义/表单状态与资产构建纳入必交映射；Vue 适配保留熟悉流程。PHP 模板字节级兼容与 PHP 执行器不属于目标 |
| Cloud/Forge/Vapor/Nightwatch 等托管商业服务 | 提供部署/遥测集成契约和可自托管替代路径，不复制第三方商业运营平台；单独记录覆盖与差异 |
| AI SDK/MCP/Boost | 可复用的框架开发辅助和通用适配先形成独立包设计；不启动既有 AI模型平台/OpenRouter 产品。是否实现通用 AI 适配在 R0 依当前新版范围自主决策并记录，禁止悄悄混入核心 |
| SQL Server/MariaDB/MongoDB 及额外驱动 | 官方支持面逐项盘点，按模型适配/后端语义建立可选驱动阶段；连接成功不代表 ORM、认证、队列、事务均支持。缺服务保留验收缺口 |

整体“全部完成”必须列明底座、V2、可选包和不适用项；只完成底座不得声称已复刻整个 Laravel 生态。

## 3. 目标架构

### 3.1 面向应用的默认目录

推荐采用 Laravel 职责目录作为默认源码交付入口；R1 通过 Cargo/IDE 原型后形成最终目录 ADR，并迁移全部受影响调用方。

```text
openrust/
├── app/                    # 应用业务 Rust 模块
│   ├── http/               # controllers、requests、middleware、resources
│   ├── models/             # 模型、关系、casts、scopes
│   ├── providers/          # 应用服务提供者
│   └── ...                 # jobs、events、listeners、mail、notifications 等按需生成
├── bootstrap/              # 应用装配与 provider 注册
├── config/                 # app/database/cache/auth/mail 等领域配置
├── database/               # migrations、factories、seeders
├── routes/                 # web.rs、api.rs、console.rs、channels.rs（按需）
├── resources/              # views、语言/前端源资源（按能力创建）
├── public/                 # 可公开的构建产物和静态文件
├── storage/                # app、logs、framework 的运行数据
├── src/main.rs             # 唯一默认进程入口，尽量薄
├── tests/                  # 当前应用的 unit/feature/真实服务测试
├── framework/              # 框架 library 源码、Cargo.toml、自身 tests
├── tools/                  # CLI、proc-macro、测试/性能工具的独立编译边界
├── examples/               # 多个独立示例各占子目录，按实际需要增加
├── .env.example
└── Cargo.toml
```

这是目标布局，当前框架仍在根 `src/`。不先复制整套实现后长期保留两份；按可构建的功能簇迁移，旧入口和别名在替代验收后删除。现有 `apps/example`、`tests/consumers` 的行为证据需迁入对应应用/框架测试职责，不能随目录一起丢弃。

Rust 根 `src/main.rs` 用明确的编译期模块声明连接职责目录；允许薄层 `#[path]` 或 Cargo target path，禁止运行时扫描 Rust 源码。R1 必须比较顶层职责目录与 `src/` 内职责模块的 IDE/测试成本；若顶层形式妨碍 Cargo/IDE，则自主采用 `src/app`、`src/bootstrap`、`src/routes` 等等价布局，并更新 ADR、生成器和本文，不能牺牲语言工具便利性。

默认应用只有一个启动 bin；框架库与维护 CLI 是独立 package，默认 workspace 成员明确。无参数 `cargo run` 启动应用，RustRover 同一配置 Command=`run` 普通运行和断点调试都成功。不把命令别名、另存带 `--bin/--package` 的调试配置当作验收通过。

支持干净源码分发与独立生成应用；独立应用依赖在本地可解析，不依赖未发布的 crates。源码交付、可打包边界与更新方式在 R1 定稿，创建外部项目的验收在 `/tmp` 或仓库隔离目录进行，不默认授权写用户其他项目。

### 3.2 内核和语言适配

| Laravel 心智模型 | OpenRust 目标与边界 |
| --- | --- |
| Container、ServiceProvider | 类型化注册/解析；singleton、scoped、transient；register/boot 顺序、缺失和环依赖启动失败。HTTP、job、console 都有明确 scope；禁止持锁 await 和跨请求状态泄漏 |
| Facade、依赖注入 | 熟悉的能力入口与可测试替换；底层为容器绑定的类型化 handle，可显式注入。若提供上下文便捷入口，须定义无 scope 错误、spawn 传播与并发隔离，不能依赖不可替换全局单例 |
| 自动装配、发现 | 显式 provider 列表、derive 或构建期可检查注册；保留 IDE 跳转和编译错误，不引入 PHP 反射解释器 |
| Eloquent | 熟悉模型/关系/查询/事件/工厂语义，typed builder + derive/trait + 成熟数据库组件；数据库 I/O 显式 async，禁止属性读取隐式网络调用 |
| fluent API | 返回类型表达链式状态和错误，使用 snake_case；不用字符串万能字典抹掉类型信息。集合优先 Iterator/Vec，公共包装需有业务价值 |
| HTTP/异常 | `Result` 和类型化响应；语义 HTTP 状态默认；JSON Resource、分页、redirect、view、stream 各自正确，不强制所有响应套统一 200 包装 |
| Web/API 认证 | web/第一方 SPA 的 Cookie Session + CSRF，API/PAT 和可选 JWT guard；密码、轮换、撤销和授权共享核心，不能让一种 guard 绕过另一种限制 |
| 生命周期 | 长驻异步运行时按请求/任务重置 scope；所有后台任务有 owner、cancel、drain、join 和超时，不模拟 PHP 每请求进程重置 |
| 配置与缓存 | 领域配置、`.env` 和类型化校验；运行时配置读取一致，环境优先级与敏感值脱敏明确。优化命令适应编译型语言，不能照搬无效 PHP 缓存操作 |

现有 Plugin 与新 Provider 必须收敛为一套装配内核；现有事件/事务扩展纳入统一语义，禁止保留第二套等价生命周期。底层 Axum/Tower/Tokio、SeaORM/SeaQuery 等先按契约评估，满足目标就复用；不能为“全面重写”自行重造密码算法、HTTP 协议或数据库驱动。

## 4. 能力交付矩阵

下表所有条目为待新 contract 验收；已有代码不是本次新目标的完成证明。R0 为每行细化上游子行为、真实测试名和差异。

| ID | 必须覆盖的行为 | 主阶段 |
| --- | --- | --- |
| C01 | 应用创建、职责目录、启动/错误处理、配置优先级、环境/密钥、维护模式、优雅停机 | R1–R2 |
| C02 | 容器绑定/别名/生命周期/依赖诊断、Provider、Facade/显式 DI、包扩展与配置资源发布 | R2 |
| C03 | web/api 路由组、middleware alias/group、命名/URL、资源路由、模型绑定、domain/prefix、fallback、签名 URL | R3 |
| C04 | Request/Response、Cookie/Session、redirect/flash、CSRF/CORS、可信代理、FormRequest 验证授权、文件验证 | R3/R7 |
| C05 | Controller `index/create/store/show/edit/update/destroy`、API resource 子集、资源序列化、条件字段/关系、分页 | R3/R5 |
| C06 | 错误报告与渲染、内容协商、日志 channels、结构化上下文、脱敏、可观测请求 ID | R3/R12 |
| C07 | 默认/named 数据库、查询 builder、参数绑定、事务/savepoint/重试、锁、提交前后、读写分离/粘主与逃生口 | R4 |
| C08 | Model CRUD、属性/类型转换、hidden/visible、fillable/guarded、timestamps、主键、软删、scope、模型事件/observer | R5 |
| C09 | 一对一/多/多对多、pivot、多态关系、预加载/聚合、N+1 检查、有界 chunk/lazy/cursor、分页、tenant/query policy | R5 |
| C10 | Schema/migration、锁/批次/rollback/status、seed、factory states/relations/固定随机种子、隔离测试库 | R4–R6 |
| C11 | PostgreSQL 18/MySQL 8.4 LTS 全能力；SQLite 单机完整支持；最低版本与其他驱动按独立矩阵 | R4/R6/R14 |
| C12 | web/SPA/API guard/provider、注册登录退出、邮箱验证、重置密码、PAT/JWT、refresh 轮换撤销、MFA/OIDC/SSO | R7 |
| C13 | Gate/Policy、角色权限、认证上下文、共享 Session/限流、哈希容量与升级、本地加密/轮换/签名/审计 | R7 |
| C14 | default/named cache stores、TTL、remember、tags、原子增减、锁、SWR/singleflight、driver 扩展、故障显式 | R8 |
| C15 | Storage disk、本地/S3、私有下载/临时 URL、流式上传验证/授权、生命周期扫描扩展、共享边界 | R8 |
| C16 | View 模板布局/组件/局部/转义/错误、表单状态、语言 plural/fallback/locale、Vue/Vite 资产 manifest | R9 |
| C17 | Mail/Mailable、模板/附件/多 mailer、SMS、Notifications 多通道/按需/locale/队列/数据库与自定义通道 | R9/R10 |
| C18 | 有序同步事件、listener/subscriber、queued listener、短路/异常、model/auth/upload/transaction 扩展 | R10 |
| C19 | 数据库/Redis queue、失败任务、retry/backoff、timeout、unique/middleware、chain/batch/cancel/progress、after_commit/outbox | R10 |
| C20 | Schedule/cron/timezone/DST、去重/锁/lease、单节点执行策略、多实例/故障补跑、worker/scheduler 安全停机 | R10 |
| C21 | HTTP client、认证/重试/并发池/超时/流/测试替换；Process 有界输出/取消/进程组回收；上下文传播 | R11 |
| C22 | 请求幂等、持久去重、事务边界、通知/任务/上传组合；外部副作用幂等及补偿责任 | R10/R11 |
| C23 | WebSocket/SSE、公共/私有/presence channel、广播授权/事件、连接限额/撤销/背压/跨节点 transport | R11 |
| C24 | gRPC 四类调用、TLS/mTLS、认证/status、deadline/取消/背压、health/reflection、生成与跨节点 | R11 |
| C25 | 日志/metrics/OTel、health/readiness、错误诊断、队列/任务观测；采样与低基数/资源上限 | R12 |
| C26 | Artisan 风格命令、make:*、route:list、migrate:*、db:seed、queue:*、schedule:*、config:*、doctor、交互/进度/退出码 | R1/R13 |
| C27 | TestApp、HTTP/session/auth assertions、各能力 fake/spy、factory/seed、时间控制、真实 DB 与浏览器测试 | R2/R6/R13 |
| C28 | 模板覆盖/stubs、资源发布、第三方 provider/driver、公开 consumer、文档/Rustdoc/IDE、干净环境生成与升级 | R13 |
| C29 | Search 索引/同步/队列/查询、运行时 feature flags、图片处理有界接口；真实 driver 与失败语义 | R12 |
| C30 | CSV/XLSX 流式数据交换、任务化进度/恢复、tenant/context 隔离等现有通用能力的适配回归 | R11 |
| C31 | 单机部署、服务复用、dev/watch 编排、Compose、迁移发布、备份恢复、构建成本、性能/故障/集群 | R14 |
| C32 | V2 Rust Admin + Vue/pure-admin + uni-app，PC 仅设计；运维视图与业务权限均通过公开 API | R15 |
| C33 | 官方生态可选包的逐项等价能力、独立应用接入和实际验收；领域模型不进核心 | R16 |

## 5. 按依赖实施

每阶段必须拆成能在一次局部提交中闭合的功能簇；下表不是允许把整个阶段塞进一个 commit。每簇先 Spec，再代码/模板/测试/文档，再 Verify，再本地 Commit。只做规划不足以完成任何实现阶段。

| 阶段 | 前置 | 交付与退出条件 |
| --- | --- | --- |
| R0 基线和全量对照 | 本计划 | 固定上游；逐章矩阵；资产/回归清单；旧规则替代清单；每项归属/测试/外部条件；优先识别破坏性共享 contract，完成后立即进入 R1 |
| R1 骨架与启动 | R0 | 最终目录/分发 ADR，最小可运行应用，框架迁移，生成器起步，`cargo run` 与 IDE run/debug，应用/框架测试可发现，无旧双入口 |
| R2 内核与测试底座 | R1 | Container/Provider、scope、Facade/DI、配置/日志/停机统一；先交 TestApp 最小路径、测试替换和独立扩展 consumer，为后续簇提供回归工具 |
| R3 HTTP 与 Web | R2 | 路由、控制器、FormRequest、response/resource 基础、middleware、Session/CSRF 基础、异常；默认错误码与 HTML/JSON 行为在真实 HTTP 验收 |
| R4 数据基础 | R2–R3 | 公开连接/QueryBuilder/Schema/迁移、事务/读写路由和 before/after 边界；PG/MySQL 真实复制拓扑和故障/恢复 |
| R5 Model 与 Resource | R4 | Eloquent 式关系、scope、cast、mass assignment、软删、observer、分页；真实双库 N+1/并发/批量资源边界 |
| R6 工厂与 SQLite | R5 | factories/seed/关系 fixtures 与 SQLite 单机全能力所需基础；后续能力分别加入 SQLite 验收，不能此阶段就宣布全能力支持 |
| R7 身份安全 | R3–R6 | 多 guard、完整认证 HTTP、授权/MFA/SSO、Session/限流、加密审计；双库与跨请求 scope 隔离；独立安全审查 |
| R8 Cache/Storage | R2/R4/R7 | 统一 stores/disks 与驱动、并发/锁/私有文件/流式上传、第三方扩展；真实 Redis/S3 和跨节点文件验收 |
| R9 视图与通信 | R3/R7/R8 | 模板/国际化/资产、Mailable/SMS/Notification 同步路径，真实 SMTP；供应商实发缺条件如实保持缺口 |
| R10 异步与事务组合 | R4–R9 | events/queue/schedule/queued notifications、chain/batch/幂等/outbox；双库/Redis、多 worker/scheduler、强杀恢复和安全停机 |
| R11 网络与组合能力 | R7–R10 | HTTP/Process/context、广播/WebSocket/SSE/gRPC、数据交换；真实网络、跨节点、事务组合和资源压力 |
| R12 搜索/特性与观测 | R8–R11 | search/feature flags/图片接口、完整观测；真实驱动、索引一致性/重建、探针/Collector/资源退化 |
| R13 完整开发体验 | R1–R12 | 补全命令/生成/stubs、测试 assertions/fakes、docs、示例、开发编排、独立扩展与更新体验；各簇生成器应随能力交付，不全部推迟到这里 |
| R14 底座最终验收 | R1–R13 | 底座 C01–C31 的适用矩阵项完成（不包含 C32/C33）；双库完整集群、SQLite 单机、最低版本、性能资源、干净环境全链路、Laravel 对照走查及独立审查 |
| R15 V2 后台/App | R14 | 按既有 B1–B5 全面适配新 contract；Vue Admin 与 uni-app 实机，PC 只规划；观测/队列等管理视图复用底座 |
| R16 生态可选包 | R14；UI 依赖 R15 | R0 已纳入的领域包逐个实现和验收；缺外部条件保留证据缺口；C32/C33 及 R0 固定范围的适用子项全部通过后才可报告整个计划完成 |

R0–R16 是依赖路线，`todo.md` 只维护当前可领取项。旧观测审查、TestApp、复制验收、最低版本、供应商实发和集群缺口逐项关联，不得因重构重置为“无需验证”。

## 6. 功能簇 contract 模板

每簇创建/更新领域 ADR 或 Spec，至少包含：

1. 上游版本/源码与选定子行为；Laravel、Goravel、Rust 三者的映射。
2. 普通开发者示例、默认配置/命令、公共类型/trait/错误；示例须进入可编译测试或生成项目。
3. 生命周期、所有权、并发、scope、事务、超时/取消/关闭、故障与恢复语义。
4. 复用、适配、重写和删除的文件/能力；下游/模板/CI/文档清单；无历史兼容层。
5. 至少一个外部 consumer 自定义 provider/driver 的真实接入，用公开构造与注册 API。
6. 正常、错误、越权、竞争、依赖故障/恢复、资源上限测试；适用数据库与真实服务拓扑。
7. 当前完成证据与外部缺口；下一簇依赖，不用编译或假测试替代真实验收。

语言映射出现困难时，按 Laravel 行为目标、Goravel 解决方式、Rust 惯用实现自主决策。一次决定一个公共契约，避免每个 worker 自造命名。常规决策不反复请求用户批准。

## 7. 验收门禁

### 每个代码功能簇

- `cargo fmt --all --check`。
- `cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo doc --workspace --no-deps`；关键 features、all-features、MSRV 按改动与 CI 矩阵补测，记录实际 feature/忽略测试。
- `cargo deny check`；新增依赖登记 `stack.md`，许可证和安全问题不能靠关门禁解决。
- 数据相关能力实际执行 PostgreSQL 18、MySQL 8.4 LTS 集成；SQLite 从支持落地起加入适用单机矩阵。最低版本在最终验收确定和验证，PG16 历史记录不充当当前基线。
- 模板/生成命令生成干净应用后 `cargo check` + `cargo test`，实际启动 HTTP 与新增流程；IDE 默认 run/debug 在 R1 及最终验收保留证据。
- 检查公开扩展 consumer、真实服务失败和恢复、文档/Rustdoc、所有下游和废弃路径引用；只提交本簇变更。

文档-only 功能簇只做 diff、链接/锚点和旧口径扫描；本次规划不运行全量代码门禁，也不增加“代码已通过”的记录。

### 真实服务与生产边界

- 读写分离使用真实复制，验证暂停/延迟/追平、强制主库、事务锁读、写后读、回退与恢复、池连接/延迟开销；两个独立库不算复制。
- 每个主要数据库各自通过：负载均衡 + 至少两个 API + 多 worker/scheduler + 共享状态；覆盖 session/权限/cache/限流、幂等/lease、A 上传 B 下载、A 连接 B 触发广播、摘除/滚动/扩缩容/依赖故障。
- 至少一次队列不承诺外部副作用恰好一次；outbox、幂等键、ACK/lease fencing、事务提交与补偿责任分别验证。SQLite 不冒充共享集群数据库。
- 本地存储、进程 memory 与断联 Redis 不得静默变成集群权威；所有 fallback 需显式配置且报告一致性变化。
- 真实 SMTP 收件、授权 SMS/供应商目标、S3、Redis、OTLP Collector、搜索 driver 和真实 TCP/WebSocket/SSE/gRPC 分层记录。缺凭据继续独立实现，不能标记投递成功。
- 新密文使用当前本地密钥，历史密钥仅解密；不强制云 KMS/Vault。共享环境/用户数据不属于“无历史包袱”的可删除对象。
- 基准保存构建/硬件/数据/并发/拓扑，测 p50/p95/p99、吞吐、CPU/RSS、连接/任务/队列上限、干净与增量构建。记录与既有基线/最小 Axum 路径的开销，实测制定回归阈值。

### Laravel 使用者走查

给只懂 Laravel 概念且具备 Rust 基础的独立走查者一份概念对照，完成：创建/调试应用、resource CRUD、关系分页、FormRequest/Policy、迁移/factory、认证、Mailable、queued job、schedule、disk 上传、广播、fake/HTTP 测试、自定义 driver、单机部署。

记录每项实际步骤、文档跳转、样板量、耗时、误解和缺失机制；修复阻碍后重跑。Agent 走查明确注明 Agent，不能冒充真实 Laravel 开发者；没有真人数据时不宣称“已证明零学习成本”。

## 8. 子智能体与额度安排

- 本地主控可用时按环境规则负责分类、派发、进度汇总；已定 contract 的实现/测试优先用用户指定的 `gpt-6.1-sol`，常规 `medium`，只有具体难题才单次提高推理强度。不可用时记录真实原因并选择可用且成本合适的模型，不伪称使用指定模型。
- 一个功能簇内按互不写同一文件的子任务分配：实现、配套测试/文档、独立审查；小任务由一个 worker 闭合，避免每个文件一个代理。不要复制全仓/完整会话给每个 worker，只提供路径、Spec、必要事实和验收命令。
- 改码 worker 默认独立 worktree，目标任务目录、分支基线和 owner 明确；不自动 merge/main、不推送。只读审查可以读当前目录。共享 ADR/计划/锁文件由主控指定唯一维护者。
- worker 不递归派发、不擅改 contract、不顺手扩大范围；返回文件清单、commit/未提交 diff、实际命令与结果、外部条件、待审查问题。测试环境/端口/库名/构建目录隔离，禁止多个 agent 同时清理共享 target。
- 同模型独立会话可做普通回归审查；认证、密钥、事务、并发/分布式等高风险修改必须不同模型或 Provider 独立审查。首选任务适配且成本可控的方案，不自动升级到最高模型。
- 长任务派发后前台给出任务名、模型、范围和输出位置；异步运行，收到完成通知再收集证据。额度不足/服务不可用时保存下一步与已完成证据，不把中断写成完成。

## 9. 文档、删除与交付

每簇同步 `todo.md`、`plan.md`、`progress.md`、领域 ADR、`core/`、用户文档、测试/验收记录和生成模板；稳定事实只有一个权威入口，旧规范标注替代关系后清理失效正文。`AGENTS.md` 和技能不得继续否定已批准的 Laravel 术语/目录目标。

删除已替代代码、兼容别名、旧模板/脚本/目录走 `git rm`，先核对 WIP 与行为覆盖。不得清理他人 worktree、进程或共享服务。新结构可改变全部历史约束，但不能通过复制旧实现再长期维护两套核心完成重构。

最终报告提供矩阵关闭情况、真实测试/拓扑/性能证据、生成项目与 IDE 证据、独立审查结论、提交范围及外部缺口；本地 commit 后不 push、不发布、不打 tag。

执行任务入口见 [重构执行说明](laravel-rebuild-execution.md)。本计划记录完成不代表 R0 盘点或 R1 代码已完成。
