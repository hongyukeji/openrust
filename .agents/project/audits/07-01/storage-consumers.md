# 存储路径消费者与配置上下文

## 固定对照与复用

承接 `24a11f944` 的 [Application 存储路径](storage-path.md)，对应 00-02、07-01，涉及配置、日志、Session、缓存、视图、本地磁盘和维护模式。固定 Laravel 应用骨架 `06d016a364a37430eec9cbc52209adce3d7667ce` 的 filesystems、logging、cache、session 配置，以及固定 Framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 FileBasedMaintenanceMode、LogManager、SessionManager、ViewServiceProvider 和 StorageLinkCommand。应用入口仍按固定 Goravel 装配，本批不改 main、bootstrap 或 Runner。

[固定公共磁盘配置](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/config/filesystems.php) · [维护状态路径](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/FileBasedMaintenanceMode.php) · [启动前维护入口](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/public/index.php)

旧版 `openrust-v0.x` `6d1a81924` 的 config/cache.rs、storage.rs 采用相对存储路径，未找到 Application 存储覆盖与多应用配置上下文；不直接复制其目录约定。当前主线既有文件日志、缓存、Session、视图和维护模式实现与回归复用，仅更换路径来源及必要接线。

## 实现与语言适配

- 配置默认值在加载时通过 `Environment::storage_path(base, subpath)` 解析所属应用的路径。保留现有配置加载函数签名，Environment 仅附带 Weak<Paths>；独立快照按自身 APP_STORAGE_PATH 和默认应用根目录解析，不访问全局 Facade，也不形成 Paths→Environment→Paths 强引用环。
- 日志渠道和应急路径、文件 Session、文件缓存数据与锁、编译视图、local/private 与 public 磁盘默认值使用该上下文。PUBLIC_STORAGE_ROOT、VIEW_COMPILED_PATH 仍分别覆盖对应目录；默认 public 根改为应用存储路径下 app/public，JSON 配置与 storage:link 目标一致。URL 仍从 APP_URL 派生 /storage，挂载顺序不变。
- Provider 解析默认日志及应急、缺少 files 的文件 Session、缺少 compiled 的视图时，从所属容器取得 Paths。独立构造的日志／Session 管理器保留原构造签名和默认行为；显式日志路径仍按原有应用根目录规则解析。
- 文件维护模式每次 activate、active、data 和 deactivate 都读取当前 storagePath，即使服务已解析仍跟随 setter。启动前预渲染入口使用当时可用的路径，显式 setter 和进程变量可用；dotenv 在 boot 前尚未加载，不为这一早期检查额外引导应用。
- 加载后的配置和已解析渠道／视图等服务不会因后续 setter 自动改写。配置缓存保存既有路径值；清除缓存并重新加载才重新计算。配置、路由、事件与包缓存文件位置保持独立的 APP_*_CACHE 契约，不归入本次存储根迁移。
- 建议部署时使用绝对存储根。getter 保留相对路径；各消费者已有的相对配置路径按应用根目录解析规则未改变，与 PHP 直接按进程 cwd 解释相对文件路径存在既有适配差异。

## 实际验收

隔离快照基线为 `24a11f944`，仅覆盖本批 19 个源码／测试文件。Ubuntu 原生 amd64、Rust 1.94.0 Debug。

- 根 configuration、configuration_cache、environment_lifecycle、maintenance_http、storage 和 storage_paths 共 14 项通过。
- 新增 4 项验证独立快照与覆盖、并发双应用隔离、路径服务不被环境强持有、配置加载时机、缺省日志／视图／存储链接，以及带 Cookie 保护的文件 Session 在请求间切换目录。
- 框架 logging、maintenance_prerender、maintenance_state、session_http 和 view_finder 共 41 项标准测试函数通过，覆盖默认目录回归、已解析维护服务的动态路径、预渲染期间 Provider 等待与失败及应用隔离。外部 MySQL／Redis 条件用例未配置连接，未计作实际数据库验收；本批改动仅涉及文件消费者与配置，不改数据库或队列。
- `cargo clippy --locked --workspace --all-targets --features pail -- -D warnings` 通过，退出 0。
- 实际根二进制通过自定义目录中的文件日志、Session 连续请求计数、文件缓存写入与回读、编译视图及 /storage 文件访问；storage:link／unlink 的真实链接指向正确。独立 down／up 改变正在运行的 HTTP 服务，503 与恢复 200 通过。
- config:cache 保存原路径；下一个进程更改 APP_STORAGE_PATH 后仍写入缓存中的原日志路径，config:clear 后重新加载改用新路径。inspire、SIGINT 退出 0 和端口释放通过。
- 最终 19 个源码／测试文件与隔离验收副本 SHA-256 一致；临时修改的两个路由文件恢复并逐字节一致。
- 工作区格式及差异检查、三语配置／日志正文、站点构建和根路径／docs 子路径的 12 项浏览器检查通过。

初始夹具中的 trait 导入、模板算术、Cookie 保护和 URL 配置问题已修正后重跑。模板路径验收使用已支持的变量插值；`{{ 1 + 2 }}` 暴露的 Blade 算术表达式缺口属于视图表达式范围，另行跟踪，不能以本批路径测试声称已支持任意 PHP 表达式。

## 最小文件范围

```text
config/{cache,filesystems,logging,session,view}.rs
tests/storage_paths.rs
vendor/openrust/framework/
├── src/config/environment.rs
├── src/foundation/
│   ├── application.rs
│   ├── bootstrap/load_environment_variables.rs
│   ├── console/storage_link_command.rs
│   ├── http/server.rs
│   └── maintenance.rs
├── src/log/{log_manager,log_service_provider}.rs
├── src/session/{session_manager,session_service_provider}.rs
├── src/view/view_service_provider.rs
└── tests/{maintenance_prerender,maintenance_state}.rs
docs/{en,zh_CN,zh_TW}/
├── getting-started/configuration.md
└── the-basics/logging.md
.agents/project/
├── audits/07-01/storage-consumers.md
├── audits/06-08/{README,pail}.md
├── audits/core-closure-ledger.md
├── current.md
└── progress.md
```

## 后续与资源

下一批按固定 Pail v1.2.5 推进 CliPrinter、实时会话命令、超时／信号清理和 dev 注册。认证自动解析、命令模式边界和 Windows 发布验收仍独立跟踪。核心账本保持 66/66 的既有验收范围，不等同于整个框架或 Pail 完成。原始证据保存于 `/private/tmp/openrust-storage-consumers.yl5ao_ni`。已回收约 6.31 GB 当批远程源码／构建／下载缓存及 60.61 MB 站点产物；数据卷清单一致，保留共享 Rust 镜像和文档依赖。本批未启动临时数据库。提交前项目可用空间 55.23 GB（51.43 GiB），周额度剩余 15%，第二窗口无读数。按用户忽略本轮额度限制的授权继续，磁盘停线仍生效，未使用重置或启用 credits。本批未推送。
