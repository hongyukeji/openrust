# Application 存储路径与 Pail 目录

## 固定对照与复用

承接 `4a32874d4`，对应 00-02 应用路径、07-01 Helpers 的 `storage_path` 与 06-08 Pail 依赖。固定 Laravel Framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [Application](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php)、[join_paths](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/functions.php)、应用契约和 helpers；Pail v1.2.5 `fdb73f5eacf03db576c710d5a00101ba185f2254` 的 Files 工厂。旧版 `openrust-v0.x` `6d1a81924` 未找到 Application 的 storagePath／useStoragePath；旧 `filesystem.rs` 的 StoragePath 及测试限制的是磁盘内对象键，不能将其绝对路径／遍历拒绝规则移用于应用路径拼接。本批复用现有容器、环境加载、应用作用域和 Pail 真实 Worker 夹具。入口装配仍参考固定 Goravel，本批不改 main、bootstrap 或 Runner。

## 实现与 Rust 适配

- Application 及契约提供 `storagePath(path)`；`useStoragePath(path)` 更新共享状态、重绑 `path.storage`，返回 Result。重绑回调失败时已更新状态，沿用 PHP 的操作顺序。空或字符串 0 的显式覆盖按 PHP 假值回退；环境中存在的空值和 0 原样保留。
- 进程环境在构造时取值，启动时切换为选定 Environment 快照；在配置加载回调前就绪。非空非 0 显式覆盖优先，其次环境值，否则 basePath/storage。按用户无品牌变量原则，以 `APP_STORAGE_PATH` 对应 Laravel 的 `LARAVEL_STORAGE_PATH`，不增加兼容别名。
- `path.storage` 构造时绑定；后加载 dotenv 影响 getter，不自动改写早期绑定。显式 setter 才重绑，保持 Laravel 生命周期差异。
- `foundation::Paths` 是应用持有的共享路径服务，不强持有 Application。Pail 工厂直接向所属容器解析它；`support::storage_path` 通过现有当前应用作用域解析。无进程可变全局路径，也没有强引用环。
- Rust 的 PathBuf／OsString 保留本机路径字节；拼接只追加本机分隔符、去掉子路径前导本机分隔符，保留基础路径尾分隔符、相对路径、重复分隔符和 ..，不使用会替换绝对子路径的 PathBuf::join。没有默认参数，根目录传入空字符串。同步读写锁只保护路径状态，文件 I/O 不在锁内。
- Pail 在首次 Files 解析时冻结目录，应在首次捕获前设定路径。配置缓存继续使用既有环境快照规则。

## 实际验收

隔离快照从 `4a32874d4` 导出，仅覆盖本批 9 个源码／测试文件。Ubuntu 原生 amd64，Rust 1.94.0 Debug，两轮退出 0。

- `cargo test --locked -p openrust-pail`：Pail 文件四进程追加、6 项筛选、真实双应用 HTTP、独立 Artisan、22 组 Provider 环境矩阵及自定义路径通过。覆盖默认路径、本机原始分隔符、相对路径、..、0、空值、进程优先、显式覆盖、配置回调读取、boot 后首解析前覆盖、重绑失败和双容器直接解析隔离。
- 通过官方 `tests/mysql_acceptance.py` 启动 MySQL 8.4.13。默认和自定义存储目录分别运行真实 queue:work，失败回调／失败记录、后续消费、硬超时隔离子进程及 deferred 回调来源通过；自定义目录捕获有效且默认会话保持为空。
- 根 `environment_lifecycle` 3 项、`console` 7 项和 `feature` 14 项通过；全工作区全目标、启用 pail 的严格 Clippy 两轮通过。`cargo +1.94.0 fmt --all --check` 和 `git diff --check` 通过。
- 根实际二进制启用 pail，设置 APP_STORAGE_PATH 后，package:discover、testing 捕获开关、HTTP /up 和日志探针、独立 Artisan、inspire、SIGINT 退出 0 和端口释放通过。临时路由已恢复，9 个候选与两份恢复路由 SHA-256 一致。
- 三语配置与日志正文同步，站点构建和根路径／docs 子路径的 12 项浏览器检查通过。

## 最小文件范围

```text
vendor/openrust-framework/src/
├── contracts/foundation.rs
├── foundation/{application,mod,paths}.rs
└── support/{helpers,mod}.rs
vendor/openrust-pail/
├── src/lib.rs
└── tests/{capture,provider}.rs
docs/{en,zh_CN,zh_TW}/
├── getting-started/configuration.md
└── the-basics/logging.md
.agents/project/
├── audits/07-01/storage-path.md
├── audits/06-08/{README,pail}.md
├── audits/core-closure-ledger.md
├── current.md
└── progress.md
```

## 后续边界与资源

本批仅闭合路径 API 和 Pail 依赖。下一批迁移既有配置消费者：config/logging.rs、session.rs、cache.rs、view.rs 和 filesystems.rs 当前仍存在 basePath/storage 默认值，不能宣称已跟随自定义路径。维护模式等框架内存储消费者也须核对；配置缓存文件位置按各自 Laravel 契约处理。之后接 CliPrinter、实时会话命令、超时／信号清理和 dev 注册。[认证自动解析](../06-08/pail-auth-boundary.md)、命令模式边界和 Windows 发布验收仍未闭合。核心账本维持既有 66/66，不等同于整个框架或 Pail 完成。

原始日志、SHA 和根入口探针保存于 `/private/tmp/openrust-storage-path.0zo4b4zo`。约 5.47 GB 当批远程源码与编译缓存、此前不存在且无人使用的匹配摘要 MySQL 镜像，以及 60.61 MB 站点产物已回收；数据卷前后相同。提交前磁盘可用 55.34 GB（51.54 GiB）；周额度剩余 15%，第二窗口无读数。按用户忽略本轮额度停线的授权继续，磁盘规则生效，未使用额度重置或启用 credits。不推送。
