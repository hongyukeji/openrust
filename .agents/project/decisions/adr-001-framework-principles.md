# ADR-001：Laravel + Laravel Modules = OpenRust

- 状态：接受；目标授权，不代表代码已实现或通过验收
- 来源：用户确认的产品方向；固定上游版本与逐项来源见[基线](../plans/laravel-rebuild-baseline.md)

## 唯一设计目标

OpenRust 用 Rust 逐项对应固定基线的 Laravel 框架与 Laravel Modules 模块化能力，并持续跟进上游。Laravel 官方功能、可观察行为、默认值、错误与安全边界、Container / ServiceProvider 生命周期、扩展契约，以及创建应用、CLI、生成、测试、部署流程是验收标准。Laravel 开发者应能按官方章节完成等价操作。这里的 1:1 不要求执行 PHP 或实现字节兼容；Rust 语法、类型、所有权、trait、async、Cargo 等语言与工具优势保留。

Goravel 只用于跨语言实现参考，不改变 Laravel 的行为标准。Laravel Modules 是正式目标，但与 Laravel 核心分开追溯来源；两者复用唯一 Container、Provider、路由和配置机制，不建立第二套模块内核。当前与目标有差异时，以目标指导重构，以代码和 Git 说明现状；旧设计不能阻止已批准的目录、API、默认值或实现重写，已验证行为及证据仍须保留并重新验收。

## 范围与应用结构

固定基线内的 Laravel 核心、Laravel Modules 和[官方可选生态](../plans/laravel-official-coverage.md#全量交付授权与两阶段)均已授权全量对应，按依赖分两阶段交付。可选指开发者按需启用，不表示开发 Agent 可省略实现。官方工具自身必要界面与前端配套属于官方包交付；若依、芋道式业务后台和独立 AI 产品继续冻结，不能借此目标自动启动。唯一活跃任务与依赖顺序见[待办](../todo.md)，不能从本 ADR 推断某项已完成。

根项目对应固定laravel/laravel应用骨架；框架与扩展同仓维护，不新建独立框架Git仓库。根应用Cargo包openrust依赖openrust-framework，依赖键与库导入仍为openrust；独立Cargo包不等于独立仓库。最终批准采用标准 Cargo 布局：框架为 `vendor/openrust/framework/src/lib.rs` 及同层领域模块，包根放 manifest、文档和测试；根应用通过薄 `src/main.rs` 提供默认二进制 `app`，使用openrust包名并以openrust依赖别名引用openrust-framework，公开导入保持 `openrust::`。Laravel Modules对应包为vendor/openrust/modules/src（Cargo openrust-modules），按AR-M01后继拆分；默认集成不改变其第三方扩展身份。第一方可选扩展置于 `vendor/openrust/<扩展>/`，应用→扩展→框架及应用→框架，不允许框架反向依赖应用或扩展。框架与第一方扩展均参与适用 workspace 门禁；第三方 vendor 单独分类，目标沿 Laravel/Composer 的 `vendor/<发布者>/<包名>` 思想组织，同仓独立crate平级且包内保原布局；这是受控源码目录约定，Cargo并不自动安装到vendor，来源中保留真实 repository、许可证、版本与原始SHA，不按目录结构推断上游仓库独立性；迁移前保全当前源码与补丁。迁移已开始实际改码，尚未通过新布局验收；不保留两套框架，不以搬迁宣称已验收；具体依赖及迁移验收见[包布局](../plans/extension-package-layout.md)。`app/bootstrap/config/database/public/resources/routes/storage/tests` 承担 Laravel 应用职责。根 `app/` 支持正常的 Controller、Model、Provider 等应用开发；`modules/<模块名>/` 是可选且可并存的业务组织，两者共用公开框架入口。唯一综合示例是普通模块 `modules/example/`，模块专有迁移、种子、模板、协议、脚本与测试 fixture 留在其中；框架核心不依赖示例类型，不建第二 demo 或完整模块子应用。`bootstrap` 负责启动装配，`app/mod.rs` 只承担必要 Rust 模块导出。Rust 源文件与模块入口需显式编译声明，运行时启停不能冒充免编译加载源码。

默认应用配置采用可编辑的 `config/*.rs`，`.env` 提供部署值；dotenv 加载后，显式编译 manifest 中的配置函数通过同一 `ConfigRepository` 与 Provider 生命周期求值。模块配置复用此机制，原始值、类型化值与公开工厂共用仓储，工厂资源仍由现 Container 管理。旧 TOML 仅可作为显式可选来源；缓存遇不可序列化类型或工厂须明确拒绝，不能静默丢弃。管理员 DBSettings 是独立领域，不建立第二配置内核。

## 官方文档对照与开发体验验收

每项能力先对照固定版本 Laravel 官方文档，确认开发者应使用的目录、文件、命令、配置、API 与操作步骤，再核对同版本官方源码和测试所定义的默认值、生命周期、错误及扩展边界。文档、源码与测试共同构成对应依据；发生不一致时明确记录并核实，不能混用不同版本或凭记忆选取方便的行为。

验收必须能按对应官方章节完成同等操作；章节顺序、示例路径与参数值（例如 `api/admin`）、默认值和配置职责也属于对应契约，不以效果近似擅自替换。仅替换必要的 Rust 语法和工具调用；不要求开发者为了绕过框架缺失另学一套自创流程。例如 Provider 注册位置、生成命令是否自动登记，都须对应固定版本官方文档，不能因文件名相似就判定完成。每项在既有行为矩阵中关联官方章节、实现入口及验收证据；缺少环节明确保留缺口，不另建平行清单。

遇到确切语言差异时，先核 Laravel 官方行为，再参考 Goravel 的跨语言实现；不能用 Goravel 的不同默认、旧版 Laravel 文档、第三方教程或个人经验替代当前固定官方基线。官方文档没有覆盖的边界，以同版本官方源码和测试补齐。

## 实现与差异

用户明确采用单进程默认启动作为小型应用部署体验增强。根应用和生成项目的无参数入口通过同一应用生命周期运行 HTTP、已配置的队列 worker 和适用调度，已启用的 WebSocket 使用应用 HTTP 端口。各独立命令继续承担单独部署角色；此启动默认是 OpenRust 扩展，调度和队列仍按领域逐项对照固定 Laravel 基线。

成熟第三方 Rust 组件可以承担底层能力，但公开体验、生命周期和扩展行为仍按 Laravel 对应机制提供；不能凭 Tokio、同名 API、目录相似或上游 PHP 实现声明 Rust 版本已交付。仅当 Rust 语言、运行时或工具有确切约束时适配，记录原行为、具体限制、替代行为与等价证据。实现困难、个人偏好或笼统“Rust 惯用”不能豁免适用功能；不能等价的行为明确保留缺口。

已有优秀能力作为经授权增强保留其价值，但必须融入相应 Laravel 职责及同一内核。例如统一响应通过 HTTP Response、API Resource 与异常渲染的公开扩展接入，不另造响应分发与错误生命周期；HTML、文件、流和无正文响应遵守原协议。仅改名、搬目录或包一层 Provider 不算融合。用户明确选择的 Redis 等运行策略和其他特约，记录在相应配置规范与差异矩阵中，并透明标示与 Laravel 默认值的差异；不能误写为官方默认，也不能擅自取消。

每个对应项至少追溯固定上游源码与测试、Rust API / 配置、可区分旧行为的等价测试、唯一示例的真实调用、文档及适用真实服务证据。名称登记、静态盘点、旧测试、fake 或编译通过都不单独构成验收。目标、现状、待验证与无法对应的差异分开报告；详见[总计划的分类和纠偏规则](../plans/laravel-rebuild.md#逐项对应与差异分类)、[Laravel 行为矩阵](../plans/laravel-rebuild-matrix-1.md)及[模块矩阵](../plans/laravel-modules-matrix.md)。固定版本之后的功能和安全更新进入后续跟进批次，不使当前验收范围随浮动分支漂移。

Agent 在每项 Spec、实现和审查时主动核对固定官方源码、默认值、生命周期、公开扩展与开发流程，不能等用户指出偏离才纠正。Goravel 不能覆盖官方标准；现有代码、历史决策和旧测试通过不能为偏离背书。发现不符时主动整改并映射唯一 todo，不能只改名、换目录、套 Provider 或自创第二机制后声称 1:1。当前执行允许先完成全部已授权代码及配套，再统一运行验证；这不豁免编码前 contract 对照及静态自审。汇报分别说明已编码、对标差距、已验证与已提交，未闭合行为保留缺口，不能伪造完整对应证据。

第一方文档、Rustdoc、注释和模板说明以简体中文为主；专有名词、API 标识符及上游来源可保留英文。工程验证、WIP 保护、真实证据与对外授权边界继续遵守仓库 `AGENTS.md`。本 ADR 是唯一方向决策；领域细节在当前源码、矩阵、计划、`core/`、`stack.md` 和测试事实中维护，不另立互相竞争的 ADR。

开发者体验以按固定Laravel章节完成同一操作为验收：命名空间职责、默认配置、参数、错误/安全边界、Provider生命周期与CLI/生成流程逐项对应，仅具体Rust约束作必要适配并给等价证据。现core混合与db/database/i18n等入口尚未完整对齐，按AR-N01有限簇整改；不以目录搬迁、名称别名或近似自测宣称只剩语法差异，冻结中的AR-P01不因此无限扩张。
