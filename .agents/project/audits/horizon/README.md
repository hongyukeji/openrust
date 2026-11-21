# Horizon Redis 队列管理扩展

## 范围与来源

固定 [Laravel Horizon](https://github.com/laravel/horizon/tree/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa) 提交 `b86d5aa87ca474db352ea2668bdd1df3a90bf8aa`，对应 MIT 许可保留在包的 `LICENSE-LARAVEL`。核对其配置、ProvisioningPlan、SupervisorOptions、MasterSupervisor、Supervisor、ProcessPool、AutoScaler、WorkerProcess、分仓储、标签、长等待通知、Console、路由与面板接口。上游官方完整源码保存在项目 `tmp/openrust-ecosystem-20261121/upstream/laravel-horizon`；联网入口读取未返回正文，源码对照以已取得的固定官方 Git checkout 为依据，未核实 GitHub 热门排名或 Star 数。

旧版 `v0.x` 固定 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 没有 Horizon 包。已查旧 `framework/src/queue.rs`、`queue/redis.rs`，不复制旧 SeaORM／租约内核。沿用现行框架的队列执行、失败提供器、进程、通知、发布、源码安装锁、事件、HTTP 授权及 Redis 连接机制；必要核心接口直接在所属模块补齐，不另外建立执行内核。

此前“Master 直接启动 queue:work、只读面板、只存监控投影”的实现是历史子集，不能作为完整上游对齐证据。当前已替换该结构，Horizon本地定向验收已完成，O02的统一状态见[对齐账本](../extension-parity.md)。

## 当前实现

- 可选 `HorizonServiceProvider` 通过包发现装配，懒绑定应用内服务、默认配置及发布模板。默认应用不预装扩展 Provider、配置或业务示例。面板必须显式注册路由、显式授权，默认拒绝；每次读写重新授权，写操作校验同源请求标识。
- 真实 `MasterSupervisor → SupervisorProcess → Supervisor → ProcessPool → horizon:work` 进程树。Supervisor 独立心跳、控制与重建，Worker 复用原生 WorkCommand，保留原生超时、失败、重试、退避、维护模式和信号语义。父进程丢失后 Worker 排空退出；Unix 调整 Supervisor 优先级，Worker 继承，避免重复增加 nice。
- 配置环境匹配、递归合并、零预算禁用、队列共享、浮点目标、逐池升序限幅与终止中进程计数按固定上游对齐。`time`／`size`／`log` 分配器由固定 PHP 源码生成的 224 组数据验证，参考 fixture 不代表完整 PHPUnit 套件。
- `JobRepository`、`TagRepository`、`MetricsRepository`、`MasterSupervisorRepository`、`SupervisorRepository`、`ProcessRepository`、`WorkloadRepository` 分离职责。任务存储包括 pending／reserved／completed／failed／silenced、授权详情、显式标签、监控标签、重试引用、清理与到期；指标包括成功吞吐、运行时间、快照、快照锁、长等待锁与告警去重锁。保留原子 Lua 的重复／过时事件保护及兼容滚动统计。
- 所有 Horizon Console 入口已实现，包括独立 supervisor、控制／状态／列表、purge、clear、snapshot、clear-metrics、install、publish、listen、forget；retry／failed 是额外保留的显式入口。安装器复用原生发布与 SourceInstallation，幂等登记编译型配置和应用 Provider，保留应用定制。
- 面板提供任务分页／详情／重试、指标与快照、队列工作量、标签监控、失败任务、批次列表／详情／取消／重试。批次读取复用原生 DatabaseBatchRepository，按有序 UUID v7 分页；共享 SQL 行为按三库验收。
- 标签及通知服务均作用域绑定到应用，未引入新的进程全局状态。原生 JobQueued 事件供推入时记录 pending，保留原生 worker 事件的职责。按固定 StoreJob／RedisQueue，观测所有 Redis 队列事件，不按当前部署计划过滤，避免 HTTP／调度进程与显式部署环境不同导致记录丢失。`horizon.env` 优先选择部署计划，Master 不改写子进程的 APP_ENV；选项已经通过命令参数传递。队列统计合并配置计划和活跃 Supervisor 队列并去重。

## 必要 Rust 适配

Rust 无 PHP 的任务对象运行时反射，模型标签通过 `Horizon::tag`／`Tags::forModel` 显式声明，不能称作 PHP 自动反射。队列使用 `Vec<String>`，配置兼容字符串／数组，序列化保留 Rust 类型表示。内存上限以原生进程的 resident memory（MiB）衡量，而非 PHP 堆分配量。

编译型应用的 `horizon:listen` 默认每 500ms 轮询配置路径，忽略符号链接；有 Cargo.toml 时重新运行 `cargo run -- artisan horizon`，继承外层工具链选择。部署后二进制监听使用当前可执行文件。`--poll` 保留显式选择入口，Rust 当前默认即轮询。

PHP Horizon 的 JobPayload::id 优先 UUID，重试时 preparePayload 改写 UUID 并保存 retry_of。Rust 原生队列的 payload UUID 同时参与失败与批次语义，因此 Horizon 使用每次 Redis push 的 transport ID 作为监控身份，并以原 payload UUID 桥接原生失败提供器；重试生成独立监控记录及引用，不覆盖父记录。历史失败删除会核对当前失败索引，避免误删同一 payload UUID 的新一次失败。这属于明确的语言／原生契约适配，不能写成 PHP 本身直接按 transport ID 管理。该修复及真实 Redis+SQLite 回归已执行通过。

信号控制、进程检查与父进程检测依赖 Unix；未将 macOS 实测扩展为 Linux／Windows 实机通过。Windows job 的 detach 明确返回不支持。Redis Cluster 队列运输拒绝，与上游支持边界一致。面板详情授权后可读取载荷及异常，旧监控投影仍只返回固定元数据；两者不可混同。

## 本轮验收环境与已执行结果

隔离源码、专属 Cargo target 和日志统一放在项目 `tmp/openrust-horizon-20261121/`。该目录由原外部路径同盘 rename 迁入，未复制 target 或保留外部写入软链。后续命令显式使用项目内 TMPDIR。Rust 1.94.0，Cargo 原生增量及默认并发，当前命令 dev／test debug=0；不恢复旧全局禁增量或固定低并发策略。

2026-11-21 最新隔离源码实际结果（日志均在上述工作目录，不能用先前子集结果替代）：

| 范围 | 实际入口与结果 | 证据 |
| --- | --- | --- |
| 包默认测试 | `cargo +1.94.0 test --offline --locked -p openrust-horizon`：安装模板1项、选项8项、HTTP2项、标签／通知4项通过；Redis用例默认 ignored，单独执行如下 | `latest-tests.log` |
| 真实 Redis | `redis_acceptance.py -- cargo +1.94.0 test --offline --locked -p openrust-horizon --test runtime --test tags_notifications -- --ignored`：Redis8.10.3，runtime6项、标签／锁2项通过，无静默跳过 | `latest-redis-tests.log` |
| 原生 Process／SQLite 批次 | `cargo +1.94.0 test --offline --locked -p openrust-framework --test processes --test batch_listing`：Process14项和SQLite批次1项通过，另两库显式ignored后分别实跑 | `latest-core-tests.log` |
| PostgreSQL 批次 | `tests/postgres_acceptance.py` 包装 `batch_listing_paginates_on_postgres -- --ignored --exact`，PostgreSQL18.7，1项通过 | `latest-batches-postgres.log` |
| MySQL 批次 | `tests/mysql_acceptance.py` 包装 `batch_listing_paginates_on_mysql -- --ignored --exact`，MySQL8.4.13，1项通过 | `latest-batches-mysql.log` |
| 根配置／骨架 | `cargo +1.94.0 test --offline --locked --features horizon --test horizon_configuration --test application_skeleton`：3项通过；关闭可选feature时默认骨架另2项通过 | `latest-root-horizon-tests.log`、`latest-root-default-tests.log` |
| Unix orphan purge | 专属Redis包装`purge_acceptance.py`，受控fixture保留真实可执行文件与Horizon参数，launcher退出后通过ps确认PPID1；真实purge发现／TERM退出，活跃Master保留，通过 | `latest-purge-tests.log` |
| 原生 work | 专属 Redis 包装 `work_acceptance.py`，隐藏入口、直接执行、无限尝试、PHP max(delay,backoff)三组、暂停隔离和SIGTERM通过 | `latest-work-tests.log` |
| 显式安装 | 专属 Redis 包装 `install_acceptance.py`，配置／Provider发布登记、重复幂等、保留应用定制与publish提示通过 | `latest-install-tests.log` |
| 进程／HTTP／面板 | 专属 Redis 包装 `process_acceptance.py`，master→supervisor→worker真实父子树、独立控制、SIGKILL重建／孤儿排空、nice、维护force、失败重试／forget、TERM排空、fast termination／--wait、两层内存退出12、listen重启、Redis丢失回收通过；Chromium1243桌面／手机覆盖授权、详情、标签写入、指标、CSP刷新及布局 | `latest-process-tests.log` |
| 质量检查 | Horizon所有targets及framework库的Clippy `-D warnings`、包Rustdoc `-D warnings`、所属包及受影响核心文件rustfmt检查、Python脚本AST和`git diff --check`通过；第三方vendored依赖既有warning未伪称清零 | `latest-clippy.log`、`latest-core-clippy.log`、`latest-rustdoc.log` |
| 三语站点 | 隔离source执行`npm --prefix docs run build`通过；随后`check_browser.mjs`与`check_horizon.mjs`分别覆盖`/`与`/docs/`、三语、桌面／手机，各12组合通过。临时node_modules链接仅复用项目依赖，dist／cache都在项目tmp隔离source | `latest-docs-build.log`、`latest-docs-site-browser.log`、`latest-docs-browser.log` |

本轮真实验收发现并修复：TERM整组提前中断job、无条件Input.shutdown绕过原生WorkerSignals、监控身份混淆transport ID与payload UUID、历史失败forget与重试的await竞态、部署队列过滤丢失原生Redis事件、Supervisor nice入口遗漏、重建时丢失历史意外退出数，以及forget／purge后legacy近期元数据未同步删除。删除任务详情及其legacy投影现在复用同一Lua原子操作。回归包含Notify控制的SQL失败查找窗口：保留同UUID的新delivery，旧快照条件删除不会误删新失败；CLI／HTTP重试拒绝已切换的delivery。未部署队列同时覆盖pending、原生worker完成、面板活跃队列合并／去重与CLI重试。

验收脚本修正了两项夹具假设：maxJobs Worker可在心跳与ps之间退出，Supervisor重建使用先前实核的稳定Supervisor PID；维护模式下HTTP正确返回503，force执行先从原生完成日志确认，再退出维护检查Horizon。没有通过放宽产品行为绕过失败。

父任务曾按用户要求统一清理全部本项目编译产物，实际释放48.617GiB、可用107.104GiB，保留锁文件原inode与源码／日志／依赖下载／数据库。清理窗口结束后明确恢复持续开发与验收；本轮开始可用118,310,052KiB，阶段边界均高于32GB停线，当前缓存沿用Cargo原生并发与增量，不按每批冷清理。

## 收尾与外部边界

本地完整进程矩阵及剩余管理命令实际入口（timeout／snapshot锁／forget --all／clear-metrics／clear／空purge及非法信号）均已通过，真实Unix孤儿purge另有独立入口验收。Horizon本地定向交付与验收完成，O02由父任务同步统一账本。未执行原PHP完整PHPUnit套件，224组PHP分配器对照不代表该套件。最终workspace全包扩展组合／全局Clippy由父任务统一验收，本包定向结果不扩展为全组合通过。

验收结束检查未残留Horizon Cargo／fixture／watcher，专属Redis／数据库夹具均已退出；无数据库数据卷变动。最终`df`可用79,819,424KiB，target约11GiB继续保留复用，未触及停线。

本轮未提交、推送、打 tag 或部署；保留其他协作者的工作区改动。真实收件人通知、Linux／Windows 平台实机和最终全部扩展组合不在本地模拟结果内。

## 扩展审计修复（2026-11-21）

Horizon 环境前缀／APP_NAME 默认、配置驱动路由、显式优先与去重、域名／路径／中间件／关闭入口已修正。默认拒绝访问；配置缓存、同 Redis 总前缀的独立隔离、真实进程与自定义路径桌面／手机通过。 统一命令、环境、结果和边界见[全量扩展修复记录](../extension-design-review.md#修复验收与交付边界)。
