# OpenRust：Laravel + laravel-modules 的 Rust 1:1 实现计划

> 受众：设计、实现、验收 Agent
> 更新：2026-10-28
> 状态：整体方向已授权；本文件是目标设计与实施计划，R0 基线已交付，R1-S 正在收敛根应用与 app/example 过渡实现；最终模块位置按 ADR-074 采用 modules/example，尚未完成迁移；后继语义按阶段推进。

## 1. 目标与授权

正式定位为 **Laravel + laravel-modules = OpenRust**：以固定 tag/commit/文档基线，用 Rust 1:1 实现 Laravel 框架能力与 laravel-modules 模块化能力。Laravel 官方章节、源码与测试是功能、可观察语义、默认值、错误/安全边界、Container/Provider 生命周期、扩展方式和开发/CLI/生成/测试/部署流程的验收标准；Laravel 使用者应能沿用官方知识，仅转换必要 Rust 语法/类型/async 表达。Goravel 仅是跨语言实现参考，不能代替 Laravel 行为标准。允许重写目录、API、默认值与实现，无历史兼容义务。

核心必须全量对应，已纳入生态按可选模块落地；模块包是正式目标但不混同 Laravel 核心来源，也不建立平行模块内核。1:1 不要求执行 PHP 或源码字节兼容；不能以语言适配、困难或个人偏好隐藏适用行为的缺失。保留已有优秀增强与用户约定，差异必须透明。

“低学习成本”以常见任务的对照走查验收；Rust 语法、所有权和异步仍需学习，不承诺 PHP 源码直接运行或跨语言零知识成本。范围覆盖机制、默认体验、生成、文档、运维和故障语义，不能以目录改名完成本任务。

权威决策为 [ADR-070](../decisions/adr-070-laravel-first-rebuild.md)。当前代码与 Git 说明已实现事实，本计划说明获准的新目标。唯一活跃队列是 [todo.md](../todo.md)；原 M5/M6、DX1–DX8 与 F/G 专题被重排到本计划，不取消其有效能力和未完成验收。

本轮规划起点为 `7f964b8`，开始时工作区干净，只有一个 Git worktree；系统存在多个 Agent 进程，不能据此认定它们都在本仓库工作。执行前重新核对 HEAD、WIP、进程与代理任务，不依赖本快照。

### 逐项对应与差异分类

每个稳定 ID 至少维护：`上游章节/源码/测试与revision → Rust公开API/配置/操作流程 → 正常/错误/安全/生命周期等价测试 → modules/example实际演示 → 文档 → 适用真实服务证据 → 差异分类与状态`。既有矩阵条目暂未填齐这些字段时只是盘点输入，不能因旧说明写“适配”或“不适用”就豁免适用核心。

| 分类 | 判定与报告 |
| --- | --- |
| 语义等价 | 可观察行为与固定上游对应，Rust语法/类型映射公开；须有本contract实际测试，不靠同名API或旧测试推定 |
| 必要 Rust 差异 | 明确原行为、具体语言/运行时/编译约束、所选等价或最接近行为、可见差异与测试；不能只写Rust惯用。无法等价部分保留缺口，不称严格全一致 |
| 用户约定 | 单仓main、根应用结构、modules/<模块>、唯一modules/example与模块资源独占、Vue/uni-app、Redis策略及既有明确撤销范围；覆盖对应上游形式但不取消其底层机制 |
| 授权增强 | 统一响应等能力必须沿对应Laravel扩展机制实现与使用，共用内核、生命周期、配置和测试；可配置且不破坏协议；公开差异，不以增强为由自建独立体系或伪称上游原生默认 |
| 范围外 | 仅明确授权/产品边界，如PHP运行环境、闭源商业运营、未授权AI产品/真实付款；不是困难核心能力的出口 |
| 未闭合缺口 | 未盘点、未实现、行为不等价、未实测或外部条件缺失；保留责任/依赖/证据，不能标已验收 |

分类与完成状态是两条轴：必要适配或用户约定经过验证可报告其实际交付，但不能因此声称全部上游行为严格相同。每版报告核心、模块化、生态、增强和差异的具体已验收范围；未知/未测/未等价项仍列缺口。新增行为按固定范围差异流程进入后继批次，不追浮动 master 无限扩张。

### 执行偏离的纠正

- 每簇开工、设计审查和提交前核对实际生产调用链：上游行为、选定 Rust contract、启动/请求/任务入口、默认配置、示例及生成器必须一致。移动目录、改类型名、加别名或包装层，只能证明相应结构变化，不能证明行为迁移。
- 发现不符合已批准目标的实现，先停止在该偏离设计上继续新增能力，由当前负责人校准 contract，再在所属功能簇修正实现、调用方、模板、测试与文档；常规纠偏直接推进，不反复请求总体授权。依赖未就绪时将缺口、前置和责任范围登记到唯一 todo 队列，继续独立工作，不将缺口包装为已完成。
- 新内核必须在预定集成功能簇切换生产入口，并实际替代旧执行语义；Container 基元存在而请求仍查旧 Registry、Provider 仅转发旧 Plugin 等情况，不得报告内核迁移完成。允许明确排期的可构建过渡，不能长期保留两套生产内核或仅因旧测试通过宣称等价。
- 以能区分旧、新行为的高价值测试验收：例如全部 register 后 boot、scoped 生命周期隔离、显式配置路径失败、语义 HTTP 状态、模块启停的路由可见性。CRUD 须实际读写数据库；资源路由不能代替 API Resource 序列化，SeaORM Entity 不能直接作为 Eloquent 等价的证据。测试覆盖上游行为及授权差异，不要求为每个名称新增测试。
- 审查发现的偏离在修正并回归前阻断受影响行为的完成判定；已经误记完成的条目退回相符状态并修正进度说明。复用已有矩阵、todo 和领域 contract 跟踪，不另建平行队列；保护其他 Agent 的 WIP，由既定负责人收口修改。用户约定和统一响应等授权增强继续保留，不以纠偏为由删除。

### 开发者任务对照验收

| 用户要完成的任务 | 必交代码与证明 |
| --- | --- |
| 按 Laravel 文档配置应用 | 对应领域配置、env 映射、默认值与错误语义；不支持/差异明确列出 |
| 新建业务模块并定义 API | 在 modules/<业务> 编写控制器、请求、模型和路由；公开 Provider/声明清单接入，不编辑框架 src；临时独立业务 fixture 验证 |
| 完整 CRUD | 一个真实实体的列表/详情/新增/更新/删除，校验、分页、404、授权、事务，双库 HTTP 实测 |
| 调用框架全部当前能力 | modules/example 的逐能力代码/配置/运行/测试表，未覆盖项保持待办，后继新能力同簇补全 |
| 扩展或替换底层服务 | 公开 trait/driver/Provider 实现与注册、替换和错误测试，无框架私有接口依赖 |
| 引入第三方业务包 | 实际导入、Cargo 清单与锁图/features 检查、框架回归；不承诺 Provider 提供依赖隔离 |
| 移除或禁用模块 | 作用时点、路由/资源可见性、依赖冲突及安全停机有合同和测试；不能遗留全局注册 |
| 部署与扩展 | 默认单机及双库真实集群证据，所有功能不能只靠 fake/编译成功证明 |

当前一个主功能簇，内部按非重叠文件分派实现、测试、文档；共享清单/锁图/迁移/服务资源与提交由集成负责人协调。高风险代码由不同模型或 Provider 审查。所有任务保持同仓 main；计划更新不授权推送、发布、tag 或外部业务操作。

## 2. 上游依据与范围冻结

参考入口（2026-10-27 读取；版本页不等于固定 commit）：

- [Laravel 13.x 文档目录](https://laravel.com/docs/13.x)：逐章建立能力和子行为清单。
- [Laravel 应用骨架](https://github.com/laravel/laravel)、[框架源码](https://github.com/laravel/framework)：核对实际默认行为、测试、依赖及许可证。
- [Container](https://laravel.com/docs/13.x/container)、[Eloquent](https://laravel.com/docs/13.x/eloquent)、[Queues](https://laravel.com/docs/13.x/queues)：生命周期、数据与异步 contract 的首批依据。
- [Goravel](https://www.goravel.dev/)、[目录](https://www.goravel.dev/getting-started/directory-structure.html)、[框架源码](https://github.com/goravel/framework)：跨语言概念、配置、目录和统一入口的参照。

固定基线与逐章矩阵见 [R0 基线](laravel-rebuild-baseline.md)。R0 固定 Laravel 文档版本、`laravel/laravel`、`laravel/framework`、Goravel 源码 tag/commit 与读取日期；不能按浮动 master 的变化无限扩张开发。每个功能簇再读对应官方文档、实现和测试，保留来源与差异说明。仅参考设计可自行实现；复制代码或文档须遵守原许可并保留必要声明。

建立逐项矩阵：`ID / 上游章节与源码锚点 / 子行为 / OpenRust 目标 / 现有实现与证据 / 复用或重写 / 依赖 / 测试 / 状态 / 差异理由`。矩阵必须覆盖固定版本的正式纳入导航和官方包列表，以下清单是启动范围，不是用来跳过新发现章节的封闭简表。

状态只允许：待盘点、contract 已定、实现中、待真实验收、已验收、外部阻塞、不适用（有理由）。复用必须通过新 contract 回归；不适用必须解释语言/平台/产品边界，不能用于掩盖实现困难。

laravel-modules正式范围单独固定v13.2.0/commit `59714840708a2aece19e810764473f53227c35fe`，来源、35个官方文档导航入口与待盘点责任见[模块矩阵](laravel-modules-matrix.md)。它与Laravel749子节粒度不同，不拼接数量冒充全量完成；模块子行为/源码测试映射尚未闭合。两者同Provider/Container，R2/R13按依赖实现模块注册、配置/路由/资源/迁移/测试、依赖顺序、启停管理与生成，不另造模块内核。

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

### 长期上游跟进

- Laravel 是持续跟进的上游，不是本轮重构后就停止对照的参考资料。每次启动框架版本规划时，以及处理相关上游安全公告或行为缺陷时，核对官方版本说明、升级指南、文档和源码差异。
- 每个 OpenRust 交付版本固定 Laravel tag/commit、文档版本与读取日期，并公开该版本的能力对应、必要差异和未验收项；OpenRust 版本号不自动表示与 Laravel 同版本或已完整同步。
- 差异进入同一能力矩阵，记录上游来源、功能/默认行为/修复变化、受影响模块、归属、依赖、验收和交付批次。安全修复优先处理，其他更新按依赖实施；新上游发布不使当前交付范围无限扩张，未纳入项保留在后继批次。
- 沿用 Laravel 的概念、默认行为和扩展契约。无法直接表达的部分记录具体 Rust 约束、最接近的实现、开发者可见差异和回归证据；不能因实现困难直接标为不适用。
- 框架核心持续实现对应能力；官方可选包按生态边界独立跟进。PHP 执行器、托管商业平台与独立 AI 产品等既有边界保持明确，不能借“同步”将领域系统混入核心。
- 每批更新闭环为上游差异盘点 → contract/升级说明 → 实现与调用方更新 → 行为和真实服务回归 → 本地提交。发布时据矩阵说明同步范围，不承诺未经验证的即时、自动或零延迟同步。

## 3. 目标架构

### 已有优秀机制的融合

R0 为已有增强记录对应 Laravel 职责及上游扩展机制、Rust 公开入口、生产调用链、开发价值、接入/定制方式、上游行为差异和回归证据，区分上游等价、必要 Rust 适配与 OpenRust 通用增强。保留能力价值不等于保留旧 API 或执行体系；符合目标才复用，否则适配或重写并回归有效行为。增强的实现和使用方式同样遵循 Laravel，共用 Provider、生命周期、配置、事件和测试机制；没有相同内置能力时使用相应公开扩展，不另造独立机制、不要求修改核心，也不以移动目录或增加 Provider 外壳冒充完成。

统一返回格式作为明确保留项，归 C05/C06、R3/R5 的 Response / API Resource 与异常渲染：成功、分页、校验和错误复用共同契约，保持无载荷省略 `data` 等已确认行为，并覆盖元信息和嵌套资源只包装一次。提供便捷接入与公开定制，应用仍可选择原生响应；具体默认值在 R3 定稿。HTML、下载、SSE/gRPC、无正文等响应保持各自协议语义，统一格式不等于统一 HTTP 200。生成器、OpenAPI、测试 assertions 与文档同步；其他增强也按实际价值和对应职责融合，避免功能堆积。

### 3.1 最终工程边界与模块目录

目标以 [ADR-074](../decisions/adr-074-laravel-modules-directory.md) 为准。以下为目标树；当前源码仍有 `app/example/` WIP，`modules/example/` 尚未迁移和验收。根 openrust package 保留 src/lib.rs 与唯一默认 server；default-members=["."]、default-run="server"，根 cargo run 启动应用。

```text
openrust/
├── app/                  # 通用应用级代码与装配
├── bootstrap/            # 启动与 Provider/模块清单接入
├── config/               # 完整领域配置与公开配置入口
├── database/             # 通用迁移/种子设施
├── public/
├── resources/
├── routes/               # 通用路由入口，不写示例业务
├── storage/
├── modules/
│   └── example/          # 唯一综合业务模块
├── tests/                # 框架、集成与公开 consumer 验收
├── src/lib.rs            # 框架核心
├── src/main.rs           # 唯一 server 薄入口
├── tools/                # CLI、宏、测试/性能工具
├── .env.example
├── Cargo.lock
└── Cargo.toml
```

框架与应用始终同仓、同 main；不创建额外源码仓库、开发分支或 worktree，不保留根 examples 或第二 demo 应用。业务统一在 modules/<模块名>，根 app/ 保留通用应用级职责。框架核心不依赖具体业务类型；示例专用配置、迁移、资源、协议、生成脚本、fixture 均留模块内，不为示例增加根 build.rs 或构建依赖。

模块按职责组织，只有存在实际实现时才创建相应目录：

```text
modules/example/
├── mod.rs
├── providers/
├── routes/               # api/web 路由定义
├── http/                 # controllers、requests、resources
├── models/
├── services/             # 仅需要业务编排时
├── config/               # 模块默认值与说明，非第二配置系统
├── database/             # migrations、seeders、factories
├── resources/            # views、语言与模块资源
├── jobs/                 # events/listeners/notifications 等按实际能力增加
├── extensions/           # 自定义公开 driver/服务实现
├── tests/
└── README.md
```

上述树是职责约定，不要求每个简单模块复制空目录。模块没有独立 server、容器或整套 framework/bootstrap；复用唯一 ServiceProvider/Container、路由和配置机制。协议生成等特殊演示只在确有功能时增加模块内目录。

开发者路径为：创建 modules/orders → 定义本模块 CRUD/API/校验/模型 → 在 Provider 中绑定业务服务并注册路由与资源 → 通过应用清单及 Rust 编译声明接入 → 测试与根启动验证。注册清单和依赖清单是公开应用配置，允许开发者修改；新增业务不得要求修改 src 框架实现。声明/生成式注册与运行时启用须分别定义，不声称可热加载未经编译的 Rust 源码。

唯一综合示例必须提供真实完整 CRUD，以及当前已实现能力的使用和公开扩展代码。缓存、存储、认证、邮件、事件、队列等不能仅装配 Plugin 就算演示；按矩阵提供代码、请求/配置和测试，未实现的后继能力同簇补入。数据/外部依赖缺失明确报错或标未验收，不静默回退。

第三方业务依赖教程覆盖清单声明、workspace 共享版本、features、锁图和回归检查；普通模块与根 package 共享依赖解析，Provider 不提供 Cargo 版本隔离。先复用已登记依赖，不为教学无理由新增框架依赖；真正需要独立 package 时先设计单向依赖，不形成根与业务循环。

CLI 模板与生成器保持唯一维护来源；生成用户应用是框架功能，仅在 /tmp 验证，不代表维护第二源码仓库。所有现有消费者和真实服务测试保留，旧路径证据不当作新模块系统验收。

### 3.2 内核和语言适配

R2 配置补强以现有 Figment/ConfigLoader 和 section<T> 为唯一来源，吸收已经完成的 Goravel 定向审计：补齐领域配置、中文说明、env 映射和安全占位；保留具名数据库/缓存能力。Provider 在最终读取前声明模块 defaults/schema/validator，定义命名空间、覆盖优先级、动态 driver options、命名 map 合并和数组替换。显式配置目录不存在/不可读不能静默跳过；未知键按已注册 section 检查，不能误拒合法动态键；错误给安全字段路径，不回显密码/DSN。当前默认运行值不因审计而改变，Redis 默认和禁止静默本地回退保持。审计是输入，设计草稿和代码验收分别记录。

| Laravel 心智模型 | OpenRust 目标与边界 |
| --- | --- |
| Container、ServiceProvider | 类型化注册/解析；singleton、scoped、transient；register仅容器绑定、所有provider完成register后boot装配routes/events/resources；缺失和环依赖启动失败。现有Plugin不视为已等价，R2统一时同步ExampleServiceProvider与应用provider清单，不建双内核，Cargo依赖隔离不由运行时provider保证。HTTP、job、console 都有明确 scope；禁止持锁 await 和跨请求状态泄漏 |
| Facade、依赖注入 | 熟悉的能力入口与可测试替换；底层为容器绑定的类型化 handle，可显式注入。若提供上下文便捷入口，须定义无 scope 错误、spawn 传播与并发隔离，不能依赖不可替换全局单例 |
| 自动装配、发现 | 显式 provider 列表、derive 或构建期可检查注册；保留静态可检查性和编译错误，不引入 PHP 反射解释器 |
| Eloquent | 熟悉模型/关系/查询/事件/工厂语义，typed builder + derive/trait + 成熟数据库组件；数据库 I/O 显式 async，禁止属性读取隐式网络调用 |
| fluent API | 返回类型表达链式状态和错误，使用 snake_case；不用字符串万能字典抹掉类型信息。集合优先 Iterator/Vec，公共包装需有业务价值 |
| HTTP/异常 | `Result` 和类型化响应；语义 HTTP 状态默认；在 Response / Resource / 异常渲染中融合统一返回格式增强；分页、redirect、view、stream 各自正确，不强制所有响应套统一 200 包装 |
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
| C05 | Controller `index/create/store/show/edit/update/destroy`、API resource 子集、资源序列化、条件字段/关系、分页；融合可定制统一返回格式，无载荷省略 data、避免重复包装 | R3/R5 |
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
| C20 | Schedule/cron/timezone/DST；每任务显式 onOneServer / withoutOverlapping、命名锁存储与身份；去重/lease、多实例/故障补跑及安全停机；现有统一时段去重不算公开策略已交付 | R10 |
| C21 | HTTP client、认证/重试/并发池/超时/流/测试替换；Process 有界输出/取消/进程组回收；上下文传播 | R11 |
| C22 | 请求幂等、持久去重、事务边界、通知/任务/上传组合；外部副作用幂等及补偿责任 | R10/R11 |
| C23 | WebSocket/SSE、公共/私有/presence channel、广播授权/事件、连接限额/撤销/背压/跨节点 transport | R11 |
| C24 | gRPC 四类调用、TLS/mTLS、认证/status、deadline/取消/背压、health/reflection、生成与跨节点 | R11 |
| C25 | 日志/metrics/OTel、health/readiness、错误诊断、队列/任务观测；采样与低基数/资源上限 | R12 |
| C26 | Artisan 风格命令、make:*、route:list、migrate:*、db:seed、queue:*、schedule:*、config:*、doctor、交互/进度/退出码 | R1/R13 |
| C27 | TestApp、HTTP/session/auth assertions、各能力 fake/spy、factory/seed、时间控制、真实 DB 与浏览器测试 | R2/R6/R13 |
| C28 | 模板覆盖/stubs、资源发布、第三方 provider/driver、公开 consumer、文档/Rustdoc、干净环境生成与升级 | R13 |
| C29 | Search 索引/同步/队列/查询、运行时 feature flags、图片处理有界接口；真实 driver 与失败语义 | R12 |
| C30 | CSV/XLSX 流式数据交换、任务化进度/恢复、tenant/context 隔离等现有通用能力的适配回归 | R11 |
| C31 | 单机部署、服务复用、dev/watch 编排、Compose、迁移发布、备份恢复、构建成本、性能/故障/集群 | R14 |
| C32 | V2 Rust Admin + Vue/pure-admin + uni-app，PC 仅设计；运维视图与业务权限均通过公开 API | R15 |
| C33 | 官方生态可选包的逐项等价能力、独立应用接入和实际验收；领域模型不进核心 | R16 |
| C34 | MQTT独立可选服务端/Broker与客户端；Laravel式Provider/配置/Manager/Contract，默认编译和运行关闭，按角色隔离依赖与真实协议验收 | R11/R14 |

## 5. 按依赖实施

每阶段必须拆成能在一次局部提交中闭合的功能簇；下表不是允许把整个阶段塞进一个 commit。每簇先 Spec，再代码/模板/测试/文档，再 Verify，再本地 Commit。只做规划不足以完成任何实现阶段。

| 阶段 | 前置 | 交付与退出条件 |
| --- | --- | --- |
| R0 基线和全量对照 | 本计划 | 固定上游；逐章矩阵；资产/回归清单；旧规则替代清单；每项归属/测试/外部条件；优先识别破坏性共享 contract，完成后立即进入 R1 |
| R1-S 根结构与普通模块挂载 | R0 | ADR-073根职责、同package lib/server、静态模块公开注册、资源内聚、无双应用；实际代码/生成/双库/HTTP门禁及审查通过可限定本地提交。仅结构子交付，不关闭完整示例 |
| R2 基础及后继内核/测试子簇 | R1-S；不依赖R1-E全部示例 | 先收敛Container/ServiceProvider register→boot、模块配置与TestApp最小公开路径，形成示例稳定API前置；scope/Facade等按子簇继续，不把全部R2塞首批 |
| R2-M 模块最小闭环与迁移 | R2 基础 | 正式 modules/<模块> 注册/资源/配置/依赖最小契约，将既有示例迁至 modules/example；验证启用/禁用、冲突、失败与实际 HTTP，不以目录搬迁替代模块能力 |
| R1-E 真实CRUD与当前能力示例 | R1-S + R2基础 + R2-M；不要求未来框架全功能已完成 | modules/example真实实体列表/详情/新增/更新/删除，校验/分页/404/授权/事务，两库HTTP闭环；当前已实现能力逐项实际调用/扩展与测试，未覆盖保留活跃任务；R1整体仅在适用子项实交后关闭 |
| R3 HTTP 与 Web | R2 | 路由、控制器、FormRequest、response/resource 基础、middleware、Session/CSRF 基础、异常；默认错误码与 HTML/JSON 行为在真实 HTTP 验收 |
| R4 数据基础 | R2–R3 | 公开连接/QueryBuilder/Schema/迁移、事务/读写路由和 before/after 边界；PG/MySQL 真实复制拓扑和故障/恢复 |
| R5 Model 与 Resource | R4 | Eloquent 式关系、scope、cast、mass assignment、软删、observer、分页；真实双库 N+1/并发/批量资源边界 |
| R6 工厂与 SQLite | R5 | factories/seed/关系 fixtures 与 SQLite 单机全能力所需基础；后续能力分别加入 SQLite 验收，不能此阶段就宣布全能力支持 |
| R7 身份安全 | R3–R6 | 多 guard、完整认证 HTTP、授权/MFA/SSO、Session/限流、加密审计；双库与跨请求 scope 隔离；独立安全审查 |
| R8 Cache/Storage | R2/R4/R7 | 统一 stores/disks 与驱动、并发/锁/私有文件/流式上传、第三方扩展；真实 Redis/S3 和跨节点文件验收 |
| R9 视图与通信 | R3/R7/R8 | 模板/国际化/资产、Mailable/SMS/Notification 同步路径，真实 SMTP；供应商实发缺条件如实保持缺口 |
| R10 异步与事务组合 | R4–R9 | events/queue/schedule/queued notifications、chain/batch/幂等/outbox；双库/Redis、多 worker/scheduler、强杀恢复和安全停机 |
| R11 网络与组合能力 | R7–R10 | HTTP/Process/context、广播/WebSocket/SSE/gRPC、数据交换与独立可选MQTT服务端/客户端；真实网络、跨节点、事务组合和资源压力 |
| R12 搜索/特性与观测 | R8–R11 | search/feature flags/图片接口、完整观测；真实驱动、索引一致性/重建、探针/Collector/资源退化 |
| R13 完整开发体验 | R1-S、R2–R12与对应R1-E交付 | 补全命令/生成/stubs、测试 assertions/fakes、docs、示例、开发编排、独立扩展与更新体验；各簇生成器应随能力交付，不全部推迟到这里 |
| R14 底座最终验收 | R1-S、R1-E、R2（含R2-M）–R13 | 底座 C01–C31 与 C34 的适用矩阵项完成（不包含 C32/C33）；双库完整集群、SQLite 单机、最低版本、性能资源、干净环境全链路、Laravel 对照走查及独立审查 |
| R15 V2 后台/App | R14 | 按既有 B1–B5 全面适配新 contract；Vue Admin 与 uni-app 实机，PC 只规划；观测/队列等管理视图复用底座 |
| R16 生态可选包 | R14；UI 依赖 R15 | R0 已纳入的领域包逐个实现和验收；缺外部条件保留证据缺口；C32/C33 及 R0 固定范围的适用子项全部通过后才可报告整个计划完成 |

R1-S → R2基础 → R2-M模块闭环 → R1-E的CRUD/当前能力贯通是当前可完成主链；R2后继与R3–R12按依赖逐簇推进，每新增/改写能力同簇更新综合示例，不要求R1先演示尚未实现的全部未来能力，也不把当前CRUD缺失藏到最后。R1-S代码提交、GET基础信息路由或旧测试通过都不代表R1整体/1:1已完成。

R0–R16 是依赖路线，`todo.md` 只维护当前可领取项。旧观测审查、TestApp、复制验收、最低版本、供应商实发和集群缺口逐项关联，不得因重构重置为“无需验证”。

### R10 明确待办：每任务调度执行策略

用户已明确要求 V1 提供 Laravel `onOneServer()` 对应的每任务可选链式配置；当前所有数据库调度统一按时段去重只是已有事实，不能算本项完成。此项归 R10，不改变当前功能簇顺序。

- 以固定上游 scheduling 源码、测试及默认行为实现：未启用与启用单服务器策略分别验收；同一任务同一次触发由任意一个节点获得资格，不代表固定某台服务器。任务名称及不同参数组合的锁身份须明确隔离。
- `withoutOverlapping` 独立控制前次执行未结束时的后续触发，可与 `onOneServer` 组合；锁存储选择、过期/清理及失败行为对应上游。跨节点使用共享数据库/Redis等具备实际原子锁能力的公开驱动；本地file/memory不能冒充集群锁，缺条件明确报错。
- 复用统一 Cache/锁、Scheduler、Queue 与 Provider；实现前评估现有 schedule_runs 全局唯一槽如何表达按节点与全局两种策略，避免仅增加无效开关或自建第二调度器。排队任务的调度派发锁与worker执行互斥分别定义，不能把入队结束当作业务执行完成。
- 验收至少覆盖多独立scheduler同次竞争、未启用策略的上游默认、任务/参数命名隔离、组合不重叠、节点故障与锁过期、共享驱动失败；在PostgreSQL/MySQL及适用Redis上实际验证，并同步modules/example、生成器与文档。
- 邮件等外部副作用仍需业务幂等与供应商能力；单服务器调度不承诺崩溃重试下外部副作用恰好一次。依据：[Laravel scheduling](https://github.com/laravel/docs/blob/13.x/scheduling.md)，具体revision以R0固定基线为准。

集群自动发现、数据库发现后节点直连等方案仍仅为讨论，未授权实施；不得将其作为本项依赖或借机增加注册中心、节点网络及第二套集群系统。

### R11 明确待办：MQTT 可选服务端与客户端

用户已授权将两种角色纳入V1：服务端作为Broker接受外部客户端，客户端连接外部Broker；两者可独立启用、同时使用或均不使用。此为Laravel包/扩展机制下的通用增强，不称Laravel原生MQTT对应项；不与集群自动发现、调度或缓存强行串联，不改变当前任务顺序。

- 统一ServiceProvider/Container、配置仓储与.env、默认及命名连接、Manager与公开Contract；外部驱动可公开注册替换，不改src核心。Facade与DI（如提供）共用同一Manager，不建立第二容器或业务事件系统；服务端认证/主题授权通过既有公开机制扩展。
- 复用成熟组件，客户端首选评估rumqttc；Broker评估rumqttd/RMQTT等实际嵌入与部署边界。协议版本、QoS、会话/遗嘱/保留消息、TLS及资源/持久性边界须逐项核实；未支持不虚报。实施时才在stack.md登记选定版本、许可证、MSRV、维护情况及构建成本，不在本轮增加Cargo依赖。
- 候选feature名为`mqtt-client`与`mqtt-server`，均不在default，独立依赖以`optional = true`和`dep:`显式关联，按需控制第三方default-features。HTTP默认入口不传递启用；仅客户端不编译Broker专属依赖，反之亦然（公共依赖复用除外）。不得以.env运行开关代替编译隔离；工作区feature合并及all-features构建仍会显式引入对应依赖。
- 可选依赖仍可能进入共用Cargo.lock及版本解析，不能承诺锁图/版本约束完全无影响；验收关注未启用角色的专属依赖不进入默认编译/链接及原生构建脚本。若实际解析冲突无法隔离，先记录证据再评估最小独立可选集成包，不为假设拆大量crate。依据：[Cargo features](https://doc.rust-lang.org/cargo/reference/features.html)、[dependency resolution](https://doc.rust-lang.org/cargo/reference/resolver.html)。
- 运行也显式启用：默认不开端口、不连接Broker、不启动消费循环或自动部署服务；复用现有受管任务/取消/关闭，长驻入口不阻塞Provider boot。配置错误和连接失败明确传播，限队列/连接/消息/重试与背压，消息内容及凭据脱敏；发布入队、Broker确认和业务处理完成分别定义。
- 门禁覆盖默认关闭、client-only、server-only、两者同时启用与无默认feature；检查cargo tree/build/Rustdoc/生成应用及默认运行无MQTT副作用。真实Broker/独立客户端验证收发、认证/ACL、声明的协议与QoS、重连/会话、并发背压和有界关闭；modules/example展示公开接入并有fake/真实行为证据，MQTT默认关闭时示例不反向启用依赖。

## 6. 功能簇 contract 模板

每簇创建/更新领域 ADR 或 Spec，至少包含：

1. 上游版本/源码与选定子行为；Laravel固定源码/测试、Rust公开API与等价证据的映射；Goravel仅作实现参考。
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
- 模板/生成命令生成干净应用后 `cargo check` + `cargo test`，实际启动 HTTP 与新增流程；默认 cargo run 与真实 HTTP 保留证据。
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
- 当前及以后所有 worker 直接在本仓库 main 开发，不创建额外源码仓库、开发分支或隔离 worktree。同一功能簇内，主控明确划分互不重叠的文件所有权后可并行实现；存在依赖的任务按顺序推进。Cargo manifests/lock、大规模迁移、共享 ADR/计划与最终提交由集成负责人协调，同一共享文件不得并发写入；target、端口和服务资源预先分配。不推送、不发布。
- worker 不递归派发、不擅改 contract、不顺手扩大范围；返回文件清单、commit/未提交 diff、实际命令与结果、外部条件、待审查问题。测试环境/端口/库名/构建目录隔离，禁止多个 agent 同时清理共享 target。
- 同模型独立会话可做普通回归审查；认证、密钥、事务、并发/分布式等高风险修改必须不同模型或 Provider 独立审查。首选任务适配且成本可控的方案，不自动升级到最高模型。
- 长任务派发后前台给出任务名、模型、范围和输出位置；异步运行，收到完成通知再收集证据。额度不足/服务不可用时保存下一步与已完成证据，不把中断写成完成。

## 9. 文档、删除与交付

每簇同步 `todo.md`、`plan.md`、`progress.md`、领域 ADR、`core/`、用户文档、测试/验收记录和生成模板；稳定事实只有一个权威入口，旧规范标注替代关系后清理失效正文。`AGENTS.md` 和技能不得继续否定已批准的 Laravel 术语/目录目标。

删除已替代代码、兼容别名、旧模板/脚本/目录走 `git rm`，先核对 WIP 与行为覆盖。不得清理他人 worktree、进程或共享服务。新结构可改变全部历史约束，但不能通过复制旧实现再长期维护两套核心完成重构。

最终报告提供矩阵关闭情况、真实测试/拓扑/性能证据、生成项目 check/test、默认 cargo run 与真实 HTTP 证据、独立审查结论、提交范围及外部缺口。本地 commit 后不 push、不发布、不打 tag。

执行任务入口见 [重构执行说明](laravel-rebuild-execution.md)。本计划记录完成不代表 R0 盘点或 R1 代码已完成。
