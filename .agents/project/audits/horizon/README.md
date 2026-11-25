# Horizon Redis 队列管理扩展

## 范围与来源

固定 [Laravel Horizon](https://github.com/laravel/horizon/tree/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa) 提交 `b86d5aa87ca474db352ea2668bdd1df3a90bf8aa`，对应 MIT 许可保留在包的 `LICENSE-LARAVEL`。核对其配置、ProvisioningPlan、SupervisorOptions、MasterSupervisor、Supervisor、ProcessPool、AutoScaler、WorkerProcess、分仓储、标签、长等待通知、Console、路由与面板接口。上游官方完整源码保存在项目 `tmp/openrust-ecosystem-20261121/upstream/laravel-horizon`；联网入口读取未返回正文，源码对照以已取得的固定官方 Git checkout 为依据，未核实 GitHub 热门排名或 Star 数。

旧版 `v0.x` 固定 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 没有 Horizon 包。已查旧 `framework/src/queue.rs`、`queue/redis.rs`，不复制旧 SeaORM／租约内核。沿用现行框架的队列执行、失败提供器、进程、通知、发布、源码安装锁、事件、HTTP 授权及 Redis 连接机制；必要核心接口直接在所属模块补齐，不另外建立执行内核。

此前“Master 直接启动 queue:work、只读面板、只存监控投影”的实现是历史子集，不能作为完整上游对齐证据。当前已替换该结构，Horizon本地定向验收已完成，O02的统一状态见[对齐账本](../extension-parity.md)。

## Master 控制命令的主机范围

PHP 8.5.12 在 Linux 隔离容器里使用固定 Horizon 命令、真实子进程与 POSIX 信号对照 `horizon:pause`、`horizon:continue` 和 `horizon:terminate`。三条命令只向本机 Master 名称前缀匹配的 PID 分别发送 USR2、CONT 和 TERM。夹具覆盖空暂停／继续集合，并保留模拟其他主机的进程。快速终止下 `--wait` 写入 `horizon:terminate:wait`。终止还向 Laravel Cache 写入 `illuminate:queue:restart`，供原生 Worker 使用。证据为 `target/tmp/observability-parity/horizon-master-local-control-upstream.log`。 [固定 PauseCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/PauseCommand.php) [固定 ContinueCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/ContinueCommand.php) [固定 TerminateCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/TerminateCommand.php)

Rust 三条 Artisan 命令按心跳中的完整主机名选择 Master。完整主机名可区分名称前缀相同的机器，升级前缺少主机信息的心跳需通过重启更新。Unix 使用原生信号，Windows 通过按 Master 标识隔离的 Redis 状态控制。暂停会传播到选中 Master 的 Supervisor 和 Worker。终止复用原生队列 Worker 的应用缓存重启标记。命令先写入该标记，再通知 Master 退出，以便容器回收命令进程前完成广播。快速终止的等待选项按目标 Master 登记。仓储的全局操作继续供原有调用方和授权面板使用，后者可清除旧版本留下的全局暂停。

Linux 7.0.14 ARM64 与 Redis 8.10.3 上，两个有独立主机名和 PID 命名空间的容器分别以 PID 1 运行真正的 Master，共用 Redis 和应用缓存。本机暂停后另一台仍能执行真实队列任务。本机继续与终止只操作本机 Master，另一台的 Master 身份保持，Worker 收到原生重启通知后更换 PID。证据为 `target/tmp/observability-parity/horizon-master-control-linux-pid1.log`。首轮在容器退出时曾回收 Artisan 执行进程，复验以 Master 自身的容器退出码和存活远端为准，并核对远端 Worker 完成重启。

Windows 11 ARM64 与 Redis 8.10.3 使用最终源码运行原生 Artisan。真实 Master 在空部署计划下经过启动、暂停、继续、终止，以 0 退出且清除心跳。Windows 上带 Worker 的排空和快速终止仍待验证。macOS 最终源码通过严格 Clippy 和真实进程组合，覆盖 `horizon:work`、普通 `queue:work` 收到重启通知、外部信号、快速终止与 `--wait`。另经真实 HTTP 验证，本机继续不会清除旧全局暂停，未授权的面板请求返回 403，授权请求可清除该状态并使 Master 恢复。三语文档构建及 12 组桌面／移动浏览器检查通过。证据为 `target/tmp/observability-parity/horizon-master-control-{windows,macos,docs}-final.log` 与 `target/tmp/observability-parity/horizon-master-control-upgrade.log`。

### Master 名称与解析器

固定默认名称使用 `Str::slug(gethostname())` 和进程内四字符令牌，支持 `determineNameUsing`。PHP 8.5.12 探针确认同进程重复读名称以及更换解析器时共用四字符令牌，解析器返回的点号和下划线保留。证据为 `target/tmp/observability-parity/horizon-master-name-upstream.log`。 [固定 MasterSupervisor](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/MasterSupervisor.php)

Rust 的 `MasterSupervisor::basename`、`process_name` 和 `determine_name_using` 接通应用作用域内的解析器。保留实例读取入口 `name(&self)`。Master 和 Supervisor 心跳均携带名称前缀，以完整主机名及完整前缀定位控制目标。未配置自定义解析器时，同主机的旧 UUID 心跳仍可控制。Rust 与固定上游的原始前缀匹配相比会区分名称只在末尾不同的主机及组，避免相近前缀误发信号。

上游解析器接受空字符串、`North:東京 1`、`North:東京` 和 124 字节前缀。固定 RedisMasterSupervisorRepository 经 RESP 夹具连接独占 Redis 8.10.3，实际写入、查找及获取心跳列表时均保留原名。固定命令构造器、Symfony 子进程及固定 Artisan 签名共同复验，空前缀生成的交付名称被当作选项，带空格的名称被 shell 拆分，两者均无法通过上游解析。`North:東京` 和 124 字节前缀均已通过上游位置参数解析。证据为 `target/tmp/observability-parity/horizon-master-name-{boundaries,redis,child,cli}-upstream.log`。

Rust 的名称解析、Redis 仓储及子进程接受 Unicode、冒号、空前缀及长名称。子进程使用原生参数和显式父标识，带空格或以连字符开头的完整名称可按一个参数传递。控制字符仍被拒绝，实际启动受操作系统的参数和环境容量约束。Master 聚合按完整父标识选择监督器，旧心跳回退到部署计划里的完整子名称。单独启动的 Supervisor 按最后一个冒号寻找父名称，受管 Supervisor 使用传入的父名称。`horizon:list` 的表格保留固定上游从第一个冒号后显示的规则。状态 JSON、Redis 心跳及面板接口保留完整名称。`horizon:purge` 处理进程表时保留命令中的连续空格。 [固定 ListCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/ListCommand.php)

Windows ARM64 用非空部署计划实测。Master 心跳保留 `North:東京 1`，但受管 Supervisor 反复以退出码 1 退出，始终没有 Worker。当前 `Supervisor::tick` 会检查传入的父 PID，Windows 的 `parent_is_running` 分支直接返回 `Unavailable`。仍需实现原生进程树 liveness 与关停，并复验实时工作、排空与快速终止。证据为 `target/tmp/observability-parity/horizon-master-extended-windows-{final,diagnostic}.log`。

macOS 通过名称解析公开入口、空前缀与 124／1025 字节名称的真实接管、同机两组相近 Unicode 名称的 Artisan 隔离。macOS 与 Linux ARM64 均使用 Redis 8.10.3 通过扩展名称的接管，以及连续空格名称下真实孤儿进程的查找和终止。macOS 最终源码通过完整进程组合，覆盖 `horizon:list`、`horizon:supervisor-status`、授权面板 `/api/stats` 和 `/api/masters`、Worker 重建及排空。覆盖范围内的严格 Clippy、隔离快照格式和三语文档正文检查通过。证据为 `target/tmp/observability-parity/horizon-master-extended-{before,macos-checked,purge-before,macos-post-cleanup,linux-post-cleanup,same-host}.log`。

macOS 配置与公开 API 7 项通过，原按条件忽略的双应用 Redis 隔离专项另行通过。完整原生进程组合通过。真实 Artisan 在同机同一 Redis 前缀上启动名称相近的两组 Master 和 Worker，按全名执行监督器状态、暂停、继续及 Master 控制，另一组保持可用。旧 UUID 心跳在默认解析器下仍接收本机信号。Linux ARM64 两个 PID 1 Master 使用相同的自定义短名，共用 Redis 并通过本机控制和远端 Worker 重启复验。Windows ARM64 的实际空部署 Master 用连字符开头的自定义名通过启动、暂停、继续及终止。Horizon 严格 Clippy 和三语文档 12 组浏览器通过。证据为 `target/tmp/observability-parity/horizon-master-name-{macos-artisan,linux-artisan,windows-artisan,same-host,rolling,redis-configuration,docs}.log`。

### 同名 Master 的启动与心跳所有权

固定上游在 `HorizonCommand::handle` 发现同名记录时输出警告并停止启动，`MasterSupervisor::ensureNoOtherMasterSupervisors` 抛出同文异常，固定 PHP 探针已分别复现。证据为 `target/tmp/observability-parity/horizon-master-name-collision-upstream.log`。 [固定 HorizonCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/HorizonCommand.php)

Rust 用单个 Redis 操作检查同名记录、占用名称并写入初始心跳。续期、Master 心跳、直接信号引起的暂停写入及清理均比较独立令牌。令牌与心跳分别保存，列表与面板无需暴露令牌。旧版本的有效心跳可阻止同名启动，仓储仍可维护无主记录。直接 `run` 返回 `MasterAlreadyRunning`，Artisan 显示上游警告且正常退出。正常关停期间继续续期，持有者消失时租约自然失效。沿用独占 Redis 的单实例所有权边界。 [Redis 令牌锁与续期](https://redis.io/docs/latest/develop/clients/patterns/distributed-locks/)

macOS 的真实 Redis 8.10.3 回归覆盖同时竞争时仅一方获得名称、旧记录的 Artisan 警告、公开仓储拒绝覆盖活跃记录及原有无主记录的 Supervisor 清理。模拟租约失效并接管后，旧 Master 退出并保留新 Master 的记录。严格 Clippy 与完整原生进程组合通过。真实 Artisan `horizon:terminate --wait` 等待 12 秒任务完成。隔离 Redis 将退出中的租约缩短为 5 秒后，原令牌续期到约 15 秒，任务完成后正常释放。Linux ARM64 的双 PID 1 进程和 Windows ARM64 的空部署原生 Master 通过启动、控制与退出复验。证据为 `target/tmp/observability-parity/horizon-master-lease-{macos-accepted,macos-process-verified,linux-accepted,windows-accepted,live-drain-verified}.log`。

### Supervisor 与 Worker 接管

修复前，隔离 Redis 模拟接管后，真实旧 Supervisor 退出时删除了接管方的同名心跳。当前 Master 显式把名称及所有权交给受管子进程。Supervisor 的心跳、暂停状态、命令领取与清理在 Redis 内先比较所属 Master 的令牌。Master 启动受管 Supervisor 前，在同一所有权约束下清理该名称的旧元数据。未受 Master 管理的单独 Supervisor 保留原有调用方式。前态证据为 `target/tmp/observability-parity/horizon-master-lease-stale-child.log`。

Horizon 通过框架 `WorkCommand::with_reservation_guard` 在实际领取任务前检查所有权，包含受管 `--once`。失去令牌的 Worker 在下一个检查点停止领取，已经运行的任务沿用原生排空。普通 `queue:work` 无需检查 Horizon 的进程信息。

macOS 与 Linux ARM64 使用真实 Redis 8.10.3 与实际 Master、Supervisor 和 Worker。先确认旧 Worker 能处理任务，再暂停两个父进程模拟接管。旧 Worker 保留新任务并退出，旧 Supervisor 的退出清理保留接管方心跳和未处理命令。持新令牌的 `horizon:work --once` 处理同一任务，普通 `queue:work --once` 即使带有旧进程环境也能处理后续任务。macOS 完整进程及 Worker 回归、Horizon 和框架所涉严格 Clippy、同名 Master 与 102 条心跳回归通过。Linux 双 PID 1 Master 组合通过。Windows ARM64 的原生 `--once` 拒绝陈旧令牌，普通 Worker 正常执行，原生 Master 控制通过。Windows 带实际子树的排空矩阵仍待验。证据为 `target/tmp/observability-parity/horizon-child-ownership-{macos,macos-takeover-verified,linux,linux-takeover-native,windows,rust-targets,preflight}.log`。

固定 Laravel `RedisQueue` 先用 `BLPOP` 等待通知，再单独发送领取请求。修复前，旧 Worker 在 Redis 已阻塞时失去所有权，仍会处理接管后入队的任务。现在通知返回或等待到期后，受管 Worker 会先重新检查所有权及暂停状态，再次确认可领取才向 Redis 发出请求。队列的 `block_for` 与空闲等待上限沿用原有配置。已经发出的领取请求和 Redis 主从切换仍需单独验收，所有权检查与跨连接或实例的领取命令独立执行。 [固定 Laravel RedisQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/RedisQueue.php#L589-L607)

阻塞接管用例确认 Redis 中的 Worker 处于 `BLPOP` 后，在同一条脚本内写入继任令牌、追加真实派发的任务并唤醒旧 Worker。旧 Worker 消费通知后退出，任务留在待处理列表，尝试次数仍为 0、保留集合为空。持新令牌的 `horizon:work --once` 处理同一任务。macOS 与 Linux ARM64 在 Redis 8.10.3 上通过阻塞与非阻塞回归，macOS 的普通 Redis 队列 HTTP 与阻塞领取回归、Horizon Worker 回归、所涉严格 Clippy 和格式检查通过。证据为 `target/tmp/observability-parity/horizon-blocking-ownership-{before,macos,linux,plain-queue,format}.log`。

Master 的 `commandQueue`、`commandQueueFor`、公开重启与输出回调也有独立契约待核对。

## 当前实现

- 可选 `HorizonServiceProvider` 通过包发现装配，懒绑定应用内服务、默认配置及发布模板。默认应用不预装扩展 Provider、配置或业务示例。面板必须显式注册路由、显式授权，默认拒绝；每次读写重新授权，写操作校验同源请求标识。
- 真实 `MasterSupervisor → SupervisorProcess → Supervisor → ProcessPool → horizon:work` 进程树。Supervisor 独立心跳、控制与重建，Worker 复用原生 WorkCommand，保留原生超时、失败、重试、退避、维护模式和信号语义。父进程丢失后 Worker 排空退出；Unix 调整 Supervisor 优先级，Worker 继承，避免重复增加 nice。
- 配置环境匹配、递归合并、零预算禁用、队列共享、浮点目标、逐池升序限幅与终止中进程计数按固定上游对齐。`time`／`size`／`log` 分配器由固定 PHP 源码生成的 224 组数据验证，参考 fixture 不代表完整 PHPUnit 套件。
- `JobRepository`、`TagRepository`、`MetricsRepository`、`MasterSupervisorRepository`、`SupervisorRepository`、`ProcessRepository`、`WorkloadRepository` 分离职责。任务存储包括 pending／reserved／completed／failed／silenced、授权详情、显式标签、监控标签、重试引用、清理与到期；指标包括成功吞吐、运行时间、快照、快照锁、长等待锁与告警去重锁。保留原子 Lua 的重复／过时事件保护及兼容滚动统计。
- 所有 Horizon Console 入口已实现，包括独立 supervisor、控制／状态／列表、purge、clear、snapshot、clear-metrics、install、publish、listen、forget；retry／failed 是额外保留的显式入口。安装器复用原生发布与 SourceInstallation，幂等登记编译型配置和应用 Provider，保留应用定制。
- 面板提供任务分页／详情／重试、指标与快照、队列工作量、标签监控、失败任务、批次列表／详情／取消／重试。批次读取复用原生 DatabaseBatchRepository，按有序 UUID v7 分页；共享 SQL 行为按三库验收。
- 标签及通知服务作用域绑定到应用。原生 Redis 入队、保留、迁移及结算桥接 Horizon 公开事件。`JobPending` 保存待处理详情，`JobPushed` 更新监控标签。同一连接上的任务链原子推进也派发后继任务的两个入队事件。Redis 失败确认后派发 `JobFailed`，供监听器读取失败详情和标签。按固定 StoreJob／RedisQueue，观测所有 Redis 队列事件，不按当前部署计划过滤，避免 HTTP／调度进程与显式部署环境不同导致记录丢失。`horizon.env` 优先选择部署计划，Master 不改写子进程的 APP_ENV；选项已经通过命令参数传递。队列统计合并配置计划和活跃 Supervisor 队列并去重。 [对齐账本](../extension-parity.md#horizon-redis-原子任务链入队事件)

## 必要 Rust 适配

Rust 无 PHP 的任务对象运行时反射，模型标签通过 `Horizon::tag`／`Tags::for_model` 显式声明，不能称作 PHP 自动反射。队列使用 `Vec<String>`，配置兼容字符串／数组，序列化保留 Rust 类型表示。内存上限以原生进程的 resident memory（MiB）衡量，而非 PHP 堆分配量。

编译型应用的 `horizon:listen` 默认每 500ms 轮询配置路径，忽略符号链接；有 Cargo.toml 时重新运行 `cargo run -- artisan horizon`，继承外层工具链选择。部署后二进制监听使用当前可执行文件。`--poll` 保留显式选择入口，Rust 当前默认即轮询。

PHP Horizon 的 JobPayload::id 优先 UUID，重试时 preparePayload 改写 UUID 并保存 retry_of。Rust 原生队列的 payload UUID 同时参与失败与批次语义，因此 Horizon 使用每次 Redis push 的 transport ID 作为监控身份，并以原 payload UUID 桥接原生失败提供器；重试生成独立监控记录及引用，不覆盖父记录。历史失败删除会核对当前失败索引，避免误删同一 payload UUID 的新一次失败。这属于明确的语言／原生契约适配，不能写成 PHP 本身直接按 transport ID 管理。该修复及真实 Redis+SQLite 回归已执行通过。

信号控制、进程检查与父进程检测依赖 Unix。macOS 历史实测之外，Linux ARM64 Release 已独立运行进程矩阵，见[对齐账本](../extension-parity.md#horizon-linux-arm64-release-进程矩阵)。Windows job 的 detach 明确返回不支持。Redis Cluster 队列运输拒绝，与上游支持边界一致。面板详情授权后可读取载荷及异常，旧监控投影仍只返回固定元数据；两者不可混同。

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

## 显式派发的监督器事件

PHP 8.5.12 在固定 Horizon、固定 Laravel 和 Symfony Process 8.0.5 上启动真实子进程，核对 `MasterSupervisorReviving` 与 `SupervisorProcessRestarting` 的公开构造与 Dispatcher 消费。应用显式派发时，监听器分别收到 Master 名称和原监督器进程对象。正常启动、退出及标记 dead 的过程中没有这两种事件。固定 EventMap 对后者登记空监听器列表。证据为 `target/tmp/observability-parity/horizon-public-supervisor-events-upstream.log`。 [固定事件定义](https://github.com/laravel/horizon/tree/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Events)

Rust 提供两个公开事件，监听器通过共享的 `SupervisorProcess` 查看同一原生进程、监督器属性和队列选项。真实进程及同步 Dispatcher 消费在 macOS、Linux 7.0.14 ARM64 和 Windows 11 ARM64 通过。macOS 配置 7 项、选项 10 项、三包默认 93 项、隔离 Redis 8.10.3 的完整 Artisan／HTTP 进程回归、全仓格式、严格 Clippy、三语站点构建与 12 组浏览器检查通过。Linux 测试已输出通过，构建守卫在收尾因宿主磁盘停线和 Cargo 锁暂占用返回失败。证据为 `target/tmp/observability-parity/horizon-public-supervisor-events-{final,linux,docs}.log` 与 `target/tmp/observability-windows/horizon-public-supervisor-events-arm64.log`。Release 和生产进程组合继续按部署环境验收。
