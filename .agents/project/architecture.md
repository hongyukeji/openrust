# 核心架构

| 位置 | 职责 |
|---|---|
| [main.rs](../../main.rs) | 建立 Tokio 运行时，将应用工厂交给 `run_application`。 |
| [bootstrap/app.rs](../../bootstrap/app.rs) | 确定应用根目录，声明配置、Provider、Artisan 命令、路由、中间件及异常策略。 |
| [bootstrap/providers.rs](../../bootstrap/providers.rs) | 登记应用的服务提供者。 |
| [框架入口](../../vendor/openrust-framework/src/foundation/entry.rs) | 解析参数，选定 HTTP 或 Artisan，再创建应用并返回进程退出码。 |
| [Application](../../vendor/openrust-framework/src/foundation/application.rs) | 通过 `start` 或 `handleCommand` 进入对应内核，复用环境、配置及 Provider 引导。 |

应用入口负责声明注册，框架负责执行生命周期。构造时解析根目录和容器，运行时由所选内核完成引导。HTTP 在监听前完成路由装配，Artisan 在命令分派前完成命令登记。启动顺序、根目录优先级及发布布局见[请求生命周期](../../docs/zh-Hans/architecture-concepts/lifecycle.md)。部署命令见[部署指南](../../docs/zh-Hans/getting-started/deployment.md)。

启动范围以实际 Cargo 工作区、根应用装配和相应验收为准。最小应用启动的归档复现、运行时文件与测试结果见 [00-01 验收记录](audits/00-01/README.md)。各批次依赖见[交付计划](plan.md)，上游版本与适配原则见[项目规范](conventions.md)。

## 成熟机制复用与适配边界

2026-11-21 复核核心框架时，先检查现有实现，再联网核对以下上游源码。Laravel 固定基线仍决定公开契约；Rust 项目用于选择内部机制，不直接替换具有不同行为的组件。

| 主题 | 已核对的成熟实现 | 当前决定与原因 |
|---|---|---|
| 路由匹配与缓存 | [Laravel RouteCacheCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/RouteCacheCommand.php)、[AbstractRouteCollection](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/AbstractRouteCollection.php)、[Axum 路由](https://github.com/tokio-rs/axum/blob/8d60d69eb195dad2f983926ab11fc91a79b98e06/axum/src/docs/routing/route.md)、[matchit](https://github.com/ibraheemdev/matchit/blob/4b5a2c356022f544187d6e67325d8e98dd7f11ca/README.md) | 复用现有 HTTP 传输和持久化候选索引，保留注册顺序、域名及参数约束。matchit 重叠路径优先静态段，不能直接替换当前按注册顺序匹配的机制。Rust 处理器随二进制编译，缓存保存匹配索引；不另造处理器序列化或路由 DSL。替换索引须先验证重叠路径、约束、域名与损坏缓存回退的等价行为，再以测量支持收益。 |
| 迁移事务 | [Laravel Migrator::runMigration](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Migrations/Migrator.php) | 沿用 `withinTransaction` 和驱动能力判断，复用现有 Connection／Transaction。当前 PostgreSQL、SQL Server 开启迁移事务；其他驱动不强套统一 DDL 事务。用真实驱动验证失败时 Schema、数据与账本的回滚，不新增一层迁移事务管理器。 |
| 安装互斥与缓存替换 | [Rust 1.94 File](https://doc.rust-lang.org/1.94.0/std/fs/struct.File.html#method.try_lock)、[tempfile 3.27.0](https://docs.rs/tempfile/3.27.0/tempfile/struct.NamedTempFile.html#method.persist) | 安装沿用标准库文件锁和既有 SourceInstallation，独立锁文件避免替换 Cargo.toml 后失锁，扩展共享同一次锁持有。路由缓存沿用同目录临时文件与 persist 替换；无需再引入文件锁库、分布式锁或通用安装事务框架。此处原子替换不等同于断电持久性保证。 |
| API 参考 | [Cargo doc](https://doc.rust-lang.org/cargo/commands/cargo-doc.html) | 复用 Cargo／Rustdoc 生成公开类型、源码与搜索；项目脚本仅负责快照、指纹和站点接入，不维护第二套 API 解析器。 |

本次仅核对并记录设计，没有引入上述项目的新源码或依赖，也没有仅凭项目热度替换已有机制。数据库翻译的 Spatie 结构与 Rust 适配由扩展交付维护，见[数据库翻译记录](audits/localization-database/README.md)。核心复验及依赖剩余项分别见[部署验收](audits/08-04/README.md#核心机制复验与成熟方案核对)和[依赖审计](audits/dependency-security/README.md#核心整合依赖复核)。
