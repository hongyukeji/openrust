# 认证模型类引用与 Laravel 装配对齐

2026-11-21：撤销此前的 `AUTH_MODELS`／`withAuthModels` 目录与启动接线。`bootstrap/app.rs`、`app/models/mod.rs`、默认 `AppServiceProvider` 及 ApplicationBuilder 不增加认证专用装配；模型在 `config/auth.rs` 中以类引用选择，框架在解析 Eloquent 用户提供器时取得类型工厂。

## 固定源码与成熟方案调研

| 参考 | 核实的机制 | 采用与取舍 |
| --- | --- | --- |
| [Laravel CreatesUserProviders](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/CreatesUserProviders.php) 与 [EloquentUserProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/EloquentUserProvider.php) | 配置选择提供器驱动及模型类；框架创建 Eloquent 提供器，`createModel()` 根据配置类名实例化。 | 保留配置结构、默认类名、框架职责与按需解析时机；不在应用 Provider 或入口登记每个模型。自定义 provider 回调用于新提供器驱动，不应要求默认 Eloquent 模型使用它。 |
| [Laravel 应用 config/auth.php](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/config/auth.php) | `env('AUTH_MODEL', App\Models\User::class)`。 | Rust 配置以 `model!(crate::app::models::User)` 表达类引用，环境变量仍选择类名。 |
| [inventory 0.3.25](https://github.com/dtolnay/inventory/tree/ffc228473d7aa20ca0790ddccc894a20682489ae) | 收集最终程序链接进来的静态类型描述；无需中心登记列表。MIT／Apache-2.0，MSRV 1.68。 | 直接依赖成熟的收集机制，只增加工厂描述和一个声明式宏；不自写链接器区段、构造器或源码扫描器。 |
| [typetag 0.2.23](https://github.com/dtolnay/typetag/tree/645445aa46b793d0f7777d998e1a649f295fa222) | 用 inventory 收集 trait 实现的反序列化工厂，并通过名称选择；依赖 erased-serde、serde、once_cell 与过程宏。 | 借鉴名称到工厂的桥接；它解决 Serde trait object 序列化，认证模型不应为此新增序列化约束和过程宏，不直接引入整包。 |
| [linkme 0.3.38](https://github.com/dtolnay/linkme/tree/59b5f4bd1328c47c3b091cc6b50cd819f5230d5e) | 通过 distributed slice 与平台链接区段收集静态描述，不使用运行时初始化。 | 同类可选机制；本次选择 inventory，与 typetag 的名称／类型工厂实践一致，只保留一个收集依赖，不并用。 |

已在线读取固定 Laravel 原始源码，并从 crates.io 正式发布包读取 README、依赖声明和 `.cargo_vcs_info.json`，核实版本、许可证、MSRV 与源码提交。研究材料现位于 `target/tmp/app-provider-boot-20261121/research/`。先前核对的固定 Goravel auth 配置与 Laravel 提供器契约不同；旧版 `v0.x` 的 RegisterContext／AuthDrivers 也不适合直接复用。

## 必要的 Rust 差异

Rust 不支持 PHP 的运行时类加载，不能仅凭任意字符串实例化一个类型。`openrust::auth::model!` 表达配置里的类引用，返回可缓存的类名，同时让链接器保留对应工厂。配置缓存绕过加载函数时，工厂也必须可用；此宏的登记是静态元数据，不是运行配置函数产生的可变全局副作用。

- 默认引用返回 `app::models::User`；可显式提供第二个参数设置稳定别名。新增类引用须重新编译，`AUTH_MODEL` 只选择已引用并链接进程序的模型；不承诺运行时加载源码或未链接的类型。
- 类型描述供程序内各应用实例使用，如同 PHP 可加载的类；配置、模型序列化回调、显式运行时扩展和请求状态仍按应用实例隔离。
- 框架按配置类名解析工厂，继续复用现有 EloquentUserProvider 和 registerModel 内部机制。没有用户表查询发生在类引用声明或工厂登记时。未知类名报错；同一别名对应不同模型时明确失败，同类型的重复引用不依赖链接顺序。
- `install:api` 只在显式安装时接入 `HasApiTokens`，并为 `config/auth.rs` 的类引用加入 Sanctum 能力与 feature 开关。重复安装保留定制，Provider 和模型模块入口保持原样。自定义布局保留人工接入提示。
- 不引入自动源码扫描、额外构建生成阶段、过程宏 crate、认证专用 Builder 或新的应用登记文件。新增 inventory 已固定版本并写入 Cargo.lock，其他锁定依赖不升级。

## 验收状态

2026-11-21，Rust 1.94.0，macOS arm64。以下均为当前类引用实现的实际验收，不沿用已撤销目录方案的结果。

- 配置缓存绕过配置加载器、应用 Provider 注册阶段解析默认模型、同类型重复引用、不同类型别名冲突、未知模型错误、手工注册兼容和应用实例隔离均已通过。启动和登记不要求存在 users 表。
- 自定义模型检索与序列化分别在 SQLite、隔离 MySQL 8.4.13、隔离 PostgreSQL 18.7 实际通过；MySQL／PostgreSQL 使用独立精确测试和 REQUIRE 变量，未将缺环境变量时的空跑计作数据库验收。证据：旧工作流 `class-auth-final.log`、`class-mysql-final.log`、`class-postgres-final.log`。
- Sanctum 的 SQLite／HTTP／资源序列化回归及 MySQL `bearer_and_spa_over_http_with_mysql` 通过。证据：`class-sanctum-final.log`、`class-mysql-final.log`。
- 应用骨架 2 项、命令入口 14 项、配置测试及安装器定向回归通过；全 workspace／all targets 严格 Clippy（含 Sanctum）通过。证据：`class-feature.log`、`class-config-final.log`、`class-startup-final.log`、`class-clippy-final.log`。
- 三语正文检查、站点构建及根路径／子路径、桌面／移动端共 12 项浏览器检查通过。证据：`class-docs-content.log`、`class-docs-build.log`、`class-docs-browser.log`。
- 迁移验收目录后的复验已通过应用骨架、认证模型与命令入口；数据库专项仍以前述真实连接结果为据。新工作流 `tests-final.log` 中缺连接变量的两个数据库入口不计作新的数据库通过记录。

原工作流目录由用户迁至 `target/tmp/app-provider-boot-20261121/`；目录中的源码、日志与缓存已定位，未丢失。恢复期间建立的工作流亦已迁至 `target/tmp/auth-model-class-20261121/`，后续使用该目录内的独占 target、runtime 和日志。迁移后复验时，包发现生成名称与旧快照不一致导致一次编译失败；同步并行工作已提交的包发现构建输入后，相关回归通过。认证改动不重新设计或接管该包发现入口。

此前磁盘触及32 GB停线时，确认编译退出后取得 Cargo 排他锁并保留锁 inode，按项目规则清理全部项目编译产物。实际释放 78.38 GiB，清理后可用 112.67 GB（104.93 GiB）；源码、日志、依赖下载及数据库和数据卷保留。证据现位于旧工作流 `class-cleanup-result.json`。

生成应用已通过 `install:api --force --without-migration-prompt` → `cargo fmt --all` → 再次强制安装的检查，配置字节保持幂等，保留 `auth_model` 局部变量名、Provider 和模型模块入口；`cargo check -p openrust --bin main --locked --offline` 与追加 `--no-default-features` 两种编译均通过。证据位于新工作流 `generated-api.log`、`class-api-enabled.log`、`class-api-disabled.log`；新快照全 workspace 格式检查通过（`fmt-final.log`）；安装器定向回归和配置加载复验通过（`startup-final.log`、`configuration-final.log`）。新快照 `cargo +1.94.0 clippy --workspace --all-targets --features sanctum --locked --offline -- -D warnings` 通过（`clippy-final.log`）。静态收集机制目前仅在本机 macOS arm64 实测，不将 crate 的跨平台支持声明当作本项目 Windows／Linux 验收。没有运行时加载未编译模型的能力。

最终检查完成后再次触及磁盘停线；确认没有 Cargo／rustc 进程，取得各编译目录排他锁，保留原锁 inode 后清理已核对的编译产物和文档 dist。未删除根 target 或其中的 tmp 源码与日志。本次实测释放 75.13 GiB，剩余 110.65 GB（103.05 GiB）。证据为新工作流 `cleanup-candidates.json`、`cleanup.log`、`cleanup-result.json`。验收结束后未新增构建。

用户已授权将当前改动按主题提交至本地，不推送；并行包发现任务的既有提交独立保留。
