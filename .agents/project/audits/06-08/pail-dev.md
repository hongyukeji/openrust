# Pail 默认开发进程

## 固定对照与复用

承接 `0439a189d`，对应 06-08、固定 Laravel 的 [The Dev Command](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md#the-dev-command) 与 [Pail](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#tailing-log-messages-using-pail)。逐项核对 Framework 13.40.0 的 [DevCommands::registerDefaults／resolvePriority](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/DevCommands.php)、[Application::markAsRegistered／providerIsLoaded](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php)，以及已固定的 Pail v1.2.5。

检查旧版 `openrust-v0.x` `6d1a81924`，没有 DevCommands／Pail 默认进程实现；旧 foundation/provider/registration.rs 在 register 成功后才保存 RegisteredProvider。保留这一生命周期边界，接入当前 Application 与 ProviderRepository，而非移入旧版的整套注册目录。复用现主线应用作用域注册表、静态 Provider 工厂、Artisan 子进程监督、Pail 原生读取及既有 Terminal 验收夹具。固定 Goravel 仅作为既有入口与编译运行参考，本批不改 main、bootstrap、Runner 或 Worker。

## 实现及 Rust 适配

- Application::providerIsLoaded 接受编译 Provider 类名。每应用 loadedProviders 集合由 markAsRegistered 更新，普通和延迟 Provider 均在 register 成功后、boot 之前标记。类身份在锁外读取，不在持有注册集合锁时调用可覆盖的元数据方法。声明、失败或取消的注册不标记，后续 boot 失败不抹去成功注册状态，延迟工厂记录实际 Provider 类型。
- ServiceProvider 的隐藏 providerClass 以 std::any::type_name::<Self>() 提供类身份，对应 PHP get_class；providerIsLoaded(type_name::<MyProvider>()) 对应 PHP providerIsLoaded(MyProvider::class)。诊断用的 name() 不作身份键。
- 注册表以 Weak 指向所属 Application，避免容器和应用形成强引用环。加载 PailServiceProvider 后，默认顺序是 server、queue、logs，再按 package.json 加 Vite。logs 执行 ./artisan pail --timeout=0，默认优先级 0；没有 Provider 时不添加。框架使用该可选包的正式编译类名，避免框架反向依赖 Pail 造成 Cargo 循环依赖。
- 用户／包同名条目按现有优先级覆盖，沿用 only、except、order、withoutDefaultCommands、withoutVendorCommands、dev:list 过滤和配色。查询列表不会运行进程或创建捕获会话。
- Rust 可在装配期间查询列表，因此每次解析列表重新核对 Pail 已加载状态，不把早期的“不存在”冻结进文件默认项快照。这一时机适配已写入三语正文。HTTP 与 Artisan 共用应用级注册信息，不复制 PHP 全局 SAPI。
- PHP 的 pcntl_fork 判据转换为现有原生 Pail 跟随／信号实现；没有新增 tail、Cargo 或 PHP 子进程。Windows 允许注册，Windows Pail 实机仍留在发布矩阵，不将本批 Linux 验收外推到 Windows。
- testing／Vapor 只控制 Pail 的捕获和命令门禁，不改变 Provider 已注册的事实。因此这些环境依然列出默认 logs，即使 pail 命令禁用；与固定 Framework 判据一致。三语文档说明可在应用中 except logs，未把命令可用性偷换成 Provider 加载状态。

## 实际验收

Ubuntu 原生 amd64，Rust 1.94.0 Debug，从 `0439a189d` 导出隔离快照，仅覆盖核定改动。

- `cargo test --locked --features pail --test pail_dev --test dev_commands --test deferred_lifecycle --test console --test feature --test artisan_entry` 的 50 项通过。包括 5 项新专题测试（多组分支）：默认项与双应用隔离、早期查询、普通／编译工厂延迟注册、失败与取消、成功标记和 boot 失败、诊断名、优先级／颜色／过滤、真实二进制发现和 testing 门禁。
- 默认 feature 的 `cargo test --locked --test dev_commands executable_lists_defaults_without_starting_services -- --exact` 通过；实际根二进制无默认 logs。启用与未启用 Pail 两种构建分别验收。
- `cargo clippy --locked --workspace --all-targets --features pail -- -D warnings`、全仓 Rust 1.94.0 格式检查和 Python AST 解析通过。
- 使用官方 `tests/mysql_acceptance.py`，查询确认 MySQL 8.4.13 后，通过真实 `python3 -B tests/artisan_pail_dev.py`。默认 server／queue／logs 在同一编译二进制下运行，真实 HTTP /up 为 200，隔离目录故意缺少欢迎视图触发 500。会话验证 HTTP 来源及底层 View not found 异常链，终端验证原样外层 HttpError 消息，不把底层异常字符串当成默认消息。
- MySQL 下的 JSON、Tabs 和 Stream 三轮通过，验证实际 HTTP 报告→logs 显示、默认命令和顺序、无 tail／Cargo 子进程、SIGTERM／键盘 Ctrl+C、终端属性恢复、会话删除、子进程树退出和端口释放。独立 Artisan inspire 在关停后成功。只验证本批受管队列的正常启动／关停，不冒称重验 Worker 的所有超时及任务驱动。
- 三语 Provider、Artisan、Logging 正文同步；最终站点构建和 `/`、`/docs/` 两种部署路径的 12 项浏览器检查通过。

旧通用 PTY 脚本改为通过 dev:list 查询 Vite 位置，不再固定为第三项；接入官方夹具提供的 MySQL URL，保留原有 SQLite 发布验收回退。本批仅用 MySQL，启用和不启用 Pail 的两种实际二进制分别通过 Tabs、Stream、小终端回退、搜索、筛选、重启、缩放、退出及终端恢复验收。最终锁顺序调整后重跑 50 项、严格 Clippy、MySQL 专用三轮及默认构建单项，9 份最终源码与验收副本 SHA-256 一致。

开发中修正测试 trait 导入及可变引导句柄；延迟重试夹具遵循既有“失败后已消费的服务键不再触发注册，未消费键可重试”的契约，未改框架重试行为。首次 MySQL 脚本把 View 错误当作默认外层消息的断言已更正；最终三轮使用更正后的脚本完整通过。

## 最小提交范围

```text
vendor/openrust-framework/src/
├── foundation/{application,dev_commands,provider_repository}.rs
└── support/service_provider.rs
tests/
├── dev_commands.rs
├── pail_dev.rs
├── artisan_pail_dev.py
├── artisan_dev_terminal.py
└── support/vendor/pail_dev.rs
docs/{en,zh_CN,zh_TW}/
├── architecture-concepts/providers.md
├── digging-deeper/artisan.md
└── the-basics/logging.md
.agents/project/
├── audits/06-08/{README,pail,pail-dev}.md
├── audits/core-closure-ledger.md
├── current.md
└── progress.md
```

## 保留边界与收尾

命令与默认 dev 注册已实现，认证自动解析继续按 [独立边界](pail-auth-boundary.md)处理，Windows 实机和其他数据库的完整矩阵留到发布前。已记录的 [Blade 标量算术缺口](../07-01/storage-consumers.md)继续跟踪；核心账本保持既有 66/66，不宣称整个 Pail 或框架全量完成。

已清理约 5.30 GB 本批源码／构建／依赖缓存及站点产物，另回收此前不存在的匹配摘要临时 MySQL 镜像；Docker 数据卷前后一致，保留共享 Rust 镜像和文档依赖。提交前项目磁盘可用 55.12 GB（51.33 GiB），周额度剩余 14%，第二窗口无读数。沿用用户忽略本轮额度限制的授权，磁盘规则继续生效；未启用 credits、重置额度或推送。

私有证据位于 `/private/tmp/openrust-pail-dev-20261119`。本批提交号以本记录所属 Git 提交为准；仅提交下列核定范围，不将既有未跟踪 WIP、生成物和运行缓存纳入提交。

## Windows 接续（2026-11-20）

后续 [V05 专项验收](pail-windows.md)已通过原生 ARM64 与 x64 仿真进程的真实 ConPTY、文件跟随、管道关闭、根 HTTP／Artisan 及 MySQL 8.4.13 默认 dev 关停／进程树／会话清理；具体源码基线、架构和未过范围见专项，不将本页历史待办误当为当前结果。
