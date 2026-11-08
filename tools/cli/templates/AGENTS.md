# 应用开发指南

本项目基于 OpenRust，普通 `cargo run` 启动应用，`cargo test` 执行测试。

标准 `openrust new` 预置 MySQL、Redis 缓存以及数据库和 Redis 队列实现；`--db postgres` 可选择 PostgreSQL，`--auth` 或 `--fortify` 选择官方 Web/Fortify、`web` Guard 和受保护 Session/Cookie，自动启用加密与现代因子迁移；只有显式 `--profile jwt-api` 选择不使用 Cookie/Session 的完整 JWT API，认证装配复用默认数据库，`--minimal` 明确省略数据库驱动、迁移包和队列（仍保留 Redis 缓存、view、session）。生成项目显式选择框架依赖 feature；认证应用提供本地 `auth`、`encryption`、`auth-fortify` feature 转发，其他能力由生成时所选依赖启用，不能仅靠 `cargo check --no-default-features` 取消这些已选依赖。

复制 `.env.example` 为 `.env` 并填写实际连接信息；标准默认 `QUEUE_CONNECTION=database`，可改为 `redis`，不需要手改 Cargo 或 Provider。Redis 队列 URL 默认继承同次有效 `cache.stores.redis.dsn`，可用 `QUEUE_REDIS_URL` 独立覆盖，`QUEUE_REDIS_NAMESPACE` 覆盖键隔离名。框架迁移与应用自定义迁移由同一 `Migrator` 贡献，部署须显式执行 `openrust migrate`。任务处理器须已编译并登记；`openrust generate job delivery` 自动接入应用任务 Provider，生成的未实现 handler 会明确报错。启动 worker 使用 `openrust queue work database --queue default` 或 `openrust queue work redis --queue default`；第一个参数选择连接，`--queue` 选择该连接的队列。`serve` 不自动运行迁移或 worker，未选连接不初始化，不静默改用其他后端。

`src/main.rs` 只负责入口；`bootstrap/app.rs` 装配应用与路由前缀、middleware 组，`bootstrap/providers.rs` 登记应用 Provider，`routes/mod.rs` 只声明路由文件的 Rust 模块；`routes/api.rs` 声明无状态 API，`routes/web.rs` 声明 Web 路由与全局 fallback。普通业务可直接写在 `app/` 的控制器、模型和 Provider 中；`modules/<业务>/` 是可选并存的组织方式。配置在 `config/`，迁移与种子在 `database/`，资源在 `resources/`，公开文件在 `public/`，运行数据在 `storage/`。

本地依赖通过应用自身 Cargo.toml 的 workspace 配置解析；保持框架源码目录可达。`config/*.rs` 经 `config/mod.rs` 显式编译，`bootstrap/app.rs` 以 `ApplicationDefinition::config` 接入统一配置仓储；`.env` 在同次启动快照中先读取，配置函数可按环境构造类型化值。`--grpc` 生成 proto/build.rs、Echo 服务与 `serve all` 联合运行命令；普通 `cargo run` 仍只启动 HTTP。在 `config/grpc.rs` 配置 TLS 证书和私钥后用 `cargo run -- serve all` 启动 HTTP 与 gRPC 两个端口；不配置 TLS 时 gRPC 启动会拒绝，只有本机开发或可信 TLS 终止拓扑才能显式开启明文。

`routes/api.rs` 只声明相对路径，由 `bootstrap/app.rs` 从唯一配置仓储读取 `config/api.rs` 的 `api.prefix`（默认 `api`，最终 URL `/api`），统一装配前缀与 `api` middleware 组；需要会话的 Web 路由在 `routes/web.rs` 显式声明 `web` 组。默认 `/api` 路由使用 `app/providers/app.rs` 中公开 Handler 扩展选择统一 JSON 错误包装；错误分类读取同一个 `ApiConfig`，更改前缀不另设第二判定来源；官方 `api/admin` 示例对应最终 `/api/admin`。HTML/Web 路由保持自身异常表现。`APP_DEBUG=false` 默认关闭服务端错误详情，非法布尔值使启动失败。

`bootstrap/app.rs` 的 `configure_providers` 为 `HomePresenter` 提供编译类描述符。默认 `/api` 的控制器按编译类符号从当前请求作用域解析 `dyn HomeResponse`，由 `HomePresenter` 提供同对象投影。

`modules/mod.rs` 显式编译业务代码并登记命名Provider/文件初始化绑定，默认 `MODULES_ACTIVATOR=registered`，`manifest(root, &config.modules)` 返回 `ModuleManifest::new().with_application_root(root)` 空清单，不扫描目录、不读写状态文件；显式选择 file/file-live 等激活器时委托 `ModuleManifest::discover_from_config(root, config)`；`ApplicationDefinition::configure_providers` 引入 `openrust_modules::ModuleProviderBuilderExt`，经 `builder.module_manifest(...)` 使用同一清单；模块类型来自独立 `openrust-modules` 包，框架核心不重导出。包register读取当前配置的module.json，disabled元数据仍会解析，未编译disabled模块可持久启用，实际加载时缺少绑定明确失败。config/modules.rs的paths.modules默认modules、scan.enabled默认false、scan.paths默认vendor/*/*；MODULES_PATH可覆盖主目录。activators.file.statuses-file独立选择持久状态文件，显式文件激活器默认应用根modules_statuses.json，MODULES_STATUSES_FILE部署覆盖；生成器不创建该文件，已有文件保留。相对路径按application_root，不自动搬迁旧文件。metadata.module_directory只覆盖files包含目录；工厂ModuleLoadContext::directory取仓储真实模块目录，普通Provider到boot通过同一ModuleRepository读取资源；不是运行未编译Rust或自动加载crate。files经显式ModuleFile实现，真实Provider注册后、boot前按解析路径一次初始化，共用同Container。class aliases和完整deferred加载仍有明确缺口，不能以Container alias替代。

应用提供 `module:list`、`module:enable 名称…`、`module:disable 名称…`，支持启停 `--all/-a`、`--no-interaction/-n` 与 list 的 `--only` / `--direction`。先验全部名称，异常停止后项、已提交前项保留；TTY 搜索选择仍待实现。命令沿完整 bootstrap，不能绕过坏状态或依赖服务失败。显式file保实例缓存，自身写前修改内存、写失败不伪回滚，外部写盘只在新实例读取；file-live为显式实时扩展。激活器视图、写入结果与当前 `Module::is_loaded()` 快照分别观察：禁用不卸载现有 Provider/路由，启用不热挂当前未加载模块，下一 App 按新状态选择。文件 hardlink 等价和 Windows 原生行为仍待验，模块管理与并发 `shutdown` 也不保证原子提交屏障。

模块专属配置、迁移、资源与测试留在该模块目录；现有根级生成器产物属于应用级职责，模块生成与资源发布等管理命令仍待后续实现。第三方依赖在本应用 Cargo.toml 管理，不在普通模块下放无效 Cargo.toml。
