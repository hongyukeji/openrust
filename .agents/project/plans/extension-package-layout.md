# 核心与扩展目录归属

状态：真人已批准。迁移负责人、当前状态及验收只在[唯一待办](../todo.md)维护；根应用与框架独立包迁移已由4463238限定验收，模块拆包和命名空间对应仍待。

## 根项目定位

根项目以固定 `laravel/laravel` 应用骨架为直接对照，承载应用代码、启动装配、配置、路由、迁移、资源、测试和开发入口。框架核心与扩展源码在同一 Git 仓库的 `vendor/` 下维护，不另建框架源码仓库，不把根目录继续组织成框架库仓库。独立 Cargo package 用于依赖与编译边界，不等于独立 Git 仓库。

目录职责、默认配置、安装／启动／命令／生成工作流均逐项核固定应用骨架与框架公开机制；仅保留 Rust 编译确需的 manifest、workspace、显式模块声明和薄 `src/main.rs` 等适配。根应用与 vendor 框架不各保留一套核心实现；唯一 `modules/example` 仍为普通业务模块。`tools/` 继续承担已授权开发工具职责，不为目录相似新增重复工具或强行模拟 PHP 运行时。

## 命名空间与开发者体验

真人明确目标：Laravel 开发者应能按固定官方文档的概念和操作步骤使用 OpenRust，主要学习差异限于 Rust 语法和确有必要的语言／运行时约束，不额外学习另一套领域命名、默认或生命周期。

- 框架公共前缀保持 `openrust::`，领域职责及规范公开路径逐项对应固定 `Illuminate` 源码；PHP 命名空间分隔符转为 Rust 路径、模块／方法采用 Rust 必要命名形式，类型职责尽量保持对应。依据官方职责拆分，不能给现有混合 `core` 直接换一个官方名字。
- 优先核对 Foundation、Container、Contracts、Support、Database、Routing、Http、Translation、Filesystem、Encryption 等领域；当前 `core`、`db/database`、`i18n` 等逐项建立官方类型／方法→Rust 唯一路径→行为与消费者证据映射。表中目标是待实现契约，不因文档改名宣称已有 API。
- 应用遵循 `app` 内 Controller／Model／Provider 职责；业务模块使用根 `modules`，模块系统与 Passport 等扩展使用各自独立 crate 命名空间，不能把扩展 API 继续混入框架核心作为第二权威入口。
- 每个行为明确规范公开入口，移除纯旧体系兼容、重复状态和无必要别名；有明确职责的 Facade／prelude／公开 re-export 按官方机制及 Rust 实际需要核定，不机械删除或通过别名掩盖实现未对应。
- 默认值、配置含义、参数／选项、路由前缀、错误与安全边界、Provider 生命周期和生成行为与命名共同验收。示例保留官方案例的值和步骤，仅作真实 Rust 语法适配；扩展增强明确来源，不能冒充官方默认。
- 同步实现及全部调用方、宏、CLI、模板、唯一示例、consumer、Rustdoc 和章节指南；提供按官方文档操作的实际验收。Rust 必要差异须写出具体约束、等价入口和证据，不能泛用“Rust 惯用”作为偏离理由。

公共 API 由总控一次收敛接口和受影响文件，worker 按互斥归属实施；当前已冻结迁移候选不无限追加。必要命名调整可在迁移冻结前纳入明确组合边界，其他对应在唯一待办逐簇交付。未验收范围继续标明缺口，不宣称当前已经只剩语法区别。

## 目录契约

| 目录 | 职责 |
| --- | --- |
| `src/main.rs` | 根应用薄启动入口，不再承载框架库 |
| `vendor/openrust/framework/` | 独立框架 Cargo 包，包根放 manifest、文档与测试 |
| `vendor/openrust/framework/src/` | 框架库入口 `lib.rs` 及 `core/`、`db/` 等核心源码 |
| `vendor/openrust/modules/src/` | 对应 laravel-modules 的独立模块化扩展源码，Cargo 包 `openrust-modules` |
| `vendor/openrust/<name>/` | 我们维护的独立扩展包，如 Passport、数据库翻译来源 |
| `vendor/<publisher>/<package>/` | 沿 Laravel／Composer 的发布者与包名组织第三方副本；仓库来源单独记录 |
| `app/`、`modules/` | 宿主应用和业务模块；唯一综合示例保持 `modules/example` |
| `tools/` | CLI、宏和测试等工具 |

真人最终批准采用标准 Cargo 布局：将根 `src/` 内的框架源码迁入 `vendor/openrust/framework/src/`，框架包根保留 `Cargo.toml`、`README.md` 与 `tests/`。此前平铺源码、不加 `src/` 的方案已被替代；本计划不表示迁移已经完成。核心与扩展按固定官方职责划分，不以默认启用、并行负责人或代码量划分。SQLite 仍是承诺支持的核心数据库后端。基础国际化及公共接口属于核心，数据库翻译来源属于显式可选扩展。

## Cargo 包与依赖方向

### 第三方来源目录

真人最终批准按 Laravel 使用的 Composer 命名思想采用 `vendor/<发布者>/<包名>/`，替代此前按 GitHub 仓库及仓库内路径分层的中间方案。目录统一小写；Rust 对应包名使用 crate 名，同一仓库内的独立 crate 在发布者目录下平级放置。发布者命名空间属于本项目归属约定，来源仓库 URL 保留真实拼写，不将这些 Rust crate 称为 Composer 包。本规则仅用于确需随仓维护的源码副本，不复制全部 Cargo registry 缓存，也不自动跟随远端更新。

| 当前路径 | 批准的目标路径 | 来源仓库 |
| --- | --- | --- |
| `vendor/sea-orm/` | `vendor/seaql/sea-orm/` | `https://github.com/SeaQL/sea-orm` |
| `vendor/sea-orm-migration/` | `vendor/seaql/sea-orm-migration/` | `https://github.com/SeaQL/sea-orm` |
| `vendor/sqlx-mysql/` | `vendor/launchbadge/sqlx-mysql/` | `https://github.com/launchbadge/sqlx` |

迁移保留原版本、上游 commit／crate 校验值、许可证和本地补丁说明；发布者、包名及真实仓库映射须核原始 manifest／VCS 来源，不表示副本一定是完整 Git checkout。各包来源清单独立维护，不改原始校验事实。不得在当前冻结测试快照中直接改路径。总控在目录迁移簇中统一调整根 patch、workspace exclude、依赖内部路径、consumer、生成器复制及依赖校验、来源清单和文档，交独立测试验证实际 metadata、构建及生成项目，无动态主仓引用或补丁失效。

规范依据：[Composer 包名](https://getcomposer.org/doc/04-schema.md#name)与[安装路径](https://getcomposer.org/doc/01-basic-usage.md#installing-dependencies)。借鉴目录命名不改变 Cargo 依赖机制；本仓已批准的第一方源码和必要补丁需纳入 Git，不能照搬 Composer 应用通常忽略整个 vendor 的做法。

### 第一方包依赖

- 包身份按 Laravel 的应用骨架／框架／扩展对应：根应用包 `openrust` 对应 `laravel/laravel`，框架包 `openrust-framework` 对应 `laravel/framework`，扩展包 `openrust-<name>` 对应 `laravel/<name>`。这是 Cargo 不使用 Composer 斜杠命名的显式适配，替代中间方案 `openrust-app`／框架包 `openrust`。
- 根目录保留应用 package 和 workspace，默认运行 `server`；框架包位于 `vendor/openrust/framework/Cargo.toml`。依赖使用 `openrust = { package = "openrust-framework", ... }` 保留业务 `openrust::...` 导入，根应用与框架包身份不同，不产生自依赖。
- 框架检查命令和 tools／consumer 的 package 选择须改为准确的 `openrust-framework`；不能机械保留 `-p openrust --lib` 而误选根应用。生成应用使用自身项目包名，框架仍通过上述依赖别名接入。同步 lock、workspace 依赖和 feature 转发，不改已提交历史回执中的原命令事实。
- 框架采用标准 `src/lib.rs` 库入口，不使用平铺 `lib.rs` 的自定义路径；同步相对资源路径、测试 target 与工具路径。根应用仍保留 `src/main.rs`，业务导入保持 `openrust::...`。
- 依赖方向固定为“应用 → 框架”“应用 → 扩展 → 框架”；框架不反向依赖宿主业务或具体扩展。共用原公开 Container／Provider／配置契约，不保留重复框架副本。
- 第一方框架与扩展为实际 workspace members，进入匹配的 fmt／Clippy／测试／文档和最低 Rust 版本检查；第三方 vendored crate 单独分类。
- 根统一管理锁文件、必要第三方 patch 与构建配置；生成项目和独立消费者必须验证实际依赖解析，不能只迁路径后沿用旧结果。

## 实施约束

### 模块化扩展拆分

真人已批准将模块专属发现、清单、依赖、启停、资源注册和模块命令等实现收敛为 `vendor/openrust/modules/` 独立 Cargo 包，与 framework 平级，不放在 `framework/modules/`。默认集成不改变其扩展归属；真实来源为固定 laravel-modules，对外仍以 OpenRust／Laravel 定位，不冒称 Laravel 官方内置模块系统。

框架保留通用 Container／Provider、配置、路由、资源和命令公开契约；扩展通过这些契约实现模块能力，依赖方向为“应用 → modules → framework”，不让 framework 反向依赖 modules 或应用。不另建 Container／Provider，不为拆包保留重复实现；必要注册扩展点按实际调用收敛，不创建无需求的插件平台。

根 `modules/example/` 仍是普通业务模块，与 vendor 中实现模块机制的包明确区分。拆分同步根装配、配置、模块生成器／CLI、资源与依赖入口、公开消费者、模板和文档，保留默认综合示例的正常启动及模块生命周期／安全行为。包内部采用 `Cargo.toml`、`src/lib.rs`、测试与说明，第一方适用门禁不可豁免。

总控在唯一待办安排独立模块拆分簇并明确依赖；AR-P01已限定验收提交4463238，接续AR-M01时先确定公共hooks与有限组合边界再编码。模块拆分完整候选交独立测试仓，运行日常 MySQL 8.4 与适用非数据库门禁，其他数据库运行留发布前；验收通过后及时提交推送。

- 扩展通过同一公开 Container／Provider、配置和资源机制接入；按需启用，不另建核心体系，不新增 `packages/` 平行扩展目录。
- 第一方扩展源码和必要第三方修复纳入版本管理，不整体忽略 `vendor/`；正常 Cargo registry 依赖无需全部复制。第一方扩展必须进入适用验收，不因所在目录被当作第三方而跳过门禁。
- 包化按上述依赖方向消除原根 library／server 同 package 下的循环依赖风险；保持默认 `cargo run`、IDE 和生成项目可用，不要求额外准备脚本。独立目录不等于独立可交付扩展。
- 目录迁移按有限功能簇执行，保全所有 WIP，更新调用方、配置、生成器、唯一示例和文档；当前冻结受验输入先闭环，不追加全仓搬迁拖延提交。
- 总控和开发负责人编码，SQLite／官方生态／CLI／文档按互斥范围并行；完成编码与官方契约静态核对即交独立 `openrust-dev` 快照验收，总控继续下一独立簇。失败交总控修复，再同步复验。
- 适用验收通过并核准最终提交树后，及时限定提交、快进推送和核对远端；测试产物按资源政策收尾，日志和来源证据保留。

国际化扩展的功能契约见[统一国际化与数据库来源](localization-sources.md)。正式架构由总控同步到唯一 ADR-001 与架构说明，执行状态不在本计划重复维护。

## 迁移退出条件

当前已通过功能簇先提交推送，保全其余 WIP 后单列本布局迁移。同步框架源码、根装配、features、宏／测试／CLI 工具、consumer、生成模板、唯一模块和路径文档；不复制维持两套实现。导出稳定候选给独立测试仓，验证完整适用组合门禁、默认根启动、实际生成 check／test 与后端相关调用，并核对最终提交树后提交推送。旧架构证据保留，但不替代新布局验收。
