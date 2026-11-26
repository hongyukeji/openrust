# Horizon Redis 队列管理扩展

## 范围与来源

固定 [Laravel Horizon](https://github.com/laravel/horizon/tree/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa) 提交 `b86d5aa87ca474db352ea2668bdd1df3a90bf8aa`，对应 MIT 许可保留在包的 `LICENSE-LARAVEL`。核对其配置、ProvisioningPlan、SupervisorOptions、MasterSupervisor、Supervisor、ProcessPool、AutoScaler、WorkerProcess、分仓储、标签、长等待通知、Console、路由与面板接口。上游官方完整源码保存在项目 `tmp/openrust-ecosystem-20261121/upstream/laravel-horizon`；联网入口读取未返回正文，源码对照以已取得的固定官方 Git checkout 为依据，未核实 GitHub 热门排名或 Star 数。

旧版 `v0.x` 固定 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 没有 Horizon 包。已查旧 `framework/src/queue.rs`、`queue/redis.rs`，不复制旧 SeaORM／租约内核。沿用现行框架的队列执行、失败提供器、进程、通知、发布、源码安装锁、事件、HTTP 授权及 Redis 连接机制；必要核心接口直接在所属模块补齐，不另外建立执行内核。

此前“Master 直接启动 queue:work、只读面板、只存监控投影”的实现是历史子集，不能作为完整上游对齐证据。当前已替换该结构，Horizon本地定向验收已完成，O02的统一状态见[对齐账本](../extension-parity.md)。

## Master 控制命令的主机范围

PHP 8.5.12 在 Linux 隔离容器里使用固定 Horizon 命令、真实子进程与 POSIX 信号对照 `horizon:pause`、`horizon:continue` 和 `horizon:terminate`。三条命令只向本机 Master 名称前缀匹配的 PID 分别发送 USR2、CONT 和 TERM。夹具覆盖空暂停／继续集合，并保留模拟其他主机的进程。快速终止下 `--wait` 写入 `horizon:terminate:wait`。终止还向 Laravel Cache 写入 `illuminate:queue:restart`，供原生 Worker 使用。证据为 `target/tmp/observability-parity/horizon-master-local-control-upstream.log`。 [固定 PauseCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/PauseCommand.php) [固定 ContinueCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/ContinueCommand.php) [固定 TerminateCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/TerminateCommand.php)

Rust 三条 Artisan 命令按心跳中的完整主机名选择 Master。完整主机名可区分名称前缀相同的机器，升级前缺少主机信息的心跳需通过重启更新。Unix 使用原生信号，Windows 通过按 Master 标识隔离的 Redis 状态控制。暂停会传播到选中 Master 的 Supervisor 和 Worker。终止复用原生队列 Worker 的应用缓存重启标记。命令先写入该标记，再通知 Master 退出，以便容器回收命令进程前完成广播。快速终止的等待选项按目标 Master 登记。仓储的全局操作继续供原有调用方和授权面板使用，后者可清除旧版本留下的全局暂停。

Linux 7.0.14 ARM64 与 Redis 8.10.3 上，两个有独立主机名和 PID 命名空间的容器分别以 PID 1 运行真正的 Master，共用 Redis 和应用缓存。本机暂停后另一台仍能执行真实队列任务。本机继续与终止只操作本机 Master，另一台的 Master 身份保持，Worker 收到原生重启通知后更换 PID。证据为 `target/tmp/observability-parity/horizon-master-control-linux-pid1.log`。首轮在容器退出时曾回收 Artisan 执行进程，复验以 Master 自身的容器退出码和存活远端为准，并核对远端 Worker 完成重启。

Windows 11 ARM64 与 Redis 8.10.3 使用最终源码运行原生 Artisan。真实 Master 在空部署计划下经过启动、暂停、继续、终止，以 0 退出且清除心跳。带 Worker 的后续实测见 [Windows 原生进程树](#windows-原生进程树)。macOS 最终源码通过严格 Clippy 和真实进程组合，覆盖 `horizon:work`、普通 `queue:work` 收到重启通知、外部信号、快速终止与 `--wait`。另经真实 HTTP 验证，本机继续不会清除旧全局暂停，未授权的面板请求返回 403，授权请求可清除该状态并使 Master 恢复。三语文档构建及 12 组桌面／移动浏览器检查通过。证据为 `target/tmp/observability-parity/horizon-master-control-{windows,macos,docs}-final.log` 与 `target/tmp/observability-parity/horizon-master-control-upgrade.log`。

### Master 名称与解析器

固定默认名称使用 `Str::slug(gethostname())` 和进程内四字符令牌，支持 `determineNameUsing`。PHP 8.5.12 探针确认同进程重复读名称以及更换解析器时共用四字符令牌，解析器返回的点号和下划线保留。证据为 `target/tmp/observability-parity/horizon-master-name-upstream.log`。 [固定 MasterSupervisor](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/MasterSupervisor.php)

Rust 的 `MasterSupervisor::basename`、`process_name` 和 `determine_name_using` 接通应用作用域内的解析器。保留实例读取入口 `name(&self)`。Master 和 Supervisor 心跳均携带名称前缀，以完整主机名及完整前缀定位控制目标。未配置自定义解析器时，同主机的旧 UUID 心跳仍可控制。Rust 与固定上游的原始前缀匹配相比会区分名称只在末尾不同的主机及组，避免相近前缀误发信号。

上游解析器接受空字符串、`North:東京 1`、`North:東京` 和 124 字节前缀。固定 RedisMasterSupervisorRepository 经 RESP 夹具连接独占 Redis 8.10.3，实际写入、查找及获取心跳列表时均保留原名。固定命令构造器、Symfony 子进程及固定 Artisan 签名共同复验，空前缀生成的交付名称被当作选项，带空格的名称被 shell 拆分，两者均无法通过上游解析。`North:東京` 和 124 字节前缀均已通过上游位置参数解析。证据为 `target/tmp/observability-parity/horizon-master-name-{boundaries,redis,child,cli}-upstream.log`。

Rust 的名称解析、Redis 仓储及子进程接受 Unicode、冒号、空前缀及长名称。子进程使用原生参数和显式父标识，带空格或以连字符开头的完整名称可按一个参数传递。控制字符仍被拒绝，实际启动受操作系统的参数和环境容量约束。Master 聚合按完整父标识选择监督器，旧心跳回退到部署计划里的完整子名称。单独启动的 Supervisor 按最后一个冒号寻找父名称，受管 Supervisor 使用传入的父名称。`horizon:list` 的表格保留固定上游从第一个冒号后显示的规则。状态 JSON、Redis 心跳及面板接口保留完整名称。`horizon:purge` 处理进程表时保留命令中的连续空格。 [固定 ListCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/ListCommand.php)

Windows ARM64 首轮用非空部署计划实测。Master 心跳保留 `North:東京 1`，但旧实现的 Windows 父 PID 检查直接返回 `Unavailable`，受管 Supervisor 反复以退出码 1 退出。前态证据为 `target/tmp/observability-parity/horizon-master-extended-windows-{final,diagnostic}.log`。后续修复与执行证据见 [Windows 原生进程树](#windows-原生进程树)。

macOS 通过名称解析公开入口、空前缀与 124／1025 字节名称的真实接管、同机两组相近 Unicode 名称的 Artisan 隔离。macOS 与 Linux ARM64 均使用 Redis 8.10.3 通过扩展名称的接管，以及连续空格名称下真实孤儿进程的查找和终止。macOS 最终源码通过完整进程组合，覆盖 `horizon:list`、`horizon:supervisor-status`、授权面板 `/api/stats` 和 `/api/masters`、Worker 重建及排空。覆盖范围内的严格 Clippy、隔离快照格式和三语文档正文检查通过。证据为 `target/tmp/observability-parity/horizon-master-extended-{before,macos-checked,purge-before,macos-post-cleanup,linux-post-cleanup,same-host}.log`。

macOS 配置与公开 API 7 项通过，原按条件忽略的双应用 Redis 隔离专项另行通过。完整原生进程组合通过。真实 Artisan 在同机同一 Redis 前缀上启动名称相近的两组 Master 和 Worker，按全名执行监督器状态、暂停、继续及 Master 控制，另一组保持可用。旧 UUID 心跳在默认解析器下仍接收本机信号。Linux ARM64 两个 PID 1 Master 使用相同的自定义短名，共用 Redis 并通过本机控制和远端 Worker 重启复验。Windows ARM64 的实际空部署 Master 用连字符开头的自定义名通过启动、暂停、继续及终止。Horizon 严格 Clippy 和三语文档 12 组浏览器通过。证据为 `target/tmp/observability-parity/horizon-master-name-{macos-artisan,linux-artisan,windows-artisan,same-host,rolling,redis-configuration,docs}.log`。

### 同名 Master 的启动与心跳所有权

固定上游在 `HorizonCommand::handle` 发现同名记录时输出警告并停止启动，`MasterSupervisor::ensureNoOtherMasterSupervisors` 抛出同文异常，固定 PHP 探针已分别复现。证据为 `target/tmp/observability-parity/horizon-master-name-collision-upstream.log`。 [固定 HorizonCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/HorizonCommand.php)

Rust 用单个 Redis 操作检查同名记录、占用名称并写入初始心跳。续期、Master 心跳、直接信号引起的暂停写入及清理均比较独立令牌。令牌与心跳分别保存，列表与面板无需暴露令牌。旧版本的有效心跳可阻止同名启动，仓储仍可维护无主记录。直接 `run` 返回 `MasterAlreadyRunning`，Artisan 显示上游警告且正常退出。正常关停期间继续续期，持有者消失时租约自然失效。沿用独占 Redis 的单实例所有权边界。 [Redis 令牌锁与续期](https://redis.io/docs/latest/develop/clients/patterns/distributed-locks/)

macOS 的真实 Redis 8.10.3 回归覆盖同时竞争时仅一方获得名称、旧记录的 Artisan 警告、公开仓储拒绝覆盖活跃记录及原有无主记录的 Supervisor 清理。模拟租约失效并接管后，旧 Master 退出并保留新 Master 的记录。严格 Clippy 与完整原生进程组合通过。真实 Artisan `horizon:terminate --wait` 等待 12 秒任务完成。隔离 Redis 将退出中的租约缩短为 5 秒后，原令牌续期到约 15 秒，任务完成后正常释放。Linux ARM64 的双 PID 1 进程和 Windows ARM64 的空部署原生 Master 通过启动、控制与退出复验。证据为 `target/tmp/observability-parity/horizon-master-lease-{macos-accepted,macos-process-verified,linux-accepted,windows-accepted,live-drain-verified}.log`。

### Supervisor 与 Worker 接管

修复前，隔离 Redis 模拟接管后，真实旧 Supervisor 退出时删除了接管方的同名心跳。当前 Master 显式把名称及所有权交给受管子进程。Supervisor 的心跳、暂停状态、命令领取与清理在 Redis 内先比较所属 Master 的令牌。Master 启动受管 Supervisor 前，在同一所有权约束下清理该名称的旧元数据。未受 Master 管理的单独 Supervisor 保留原有调用方式。前态证据为 `target/tmp/observability-parity/horizon-master-lease-stale-child.log`。

Horizon 通过框架 `WorkCommand::with_reservation_guard` 在实际领取任务前检查所有权，包含受管 `--once`。失去令牌的 Worker 在下一个检查点停止领取，已经运行的任务沿用原生排空。普通 `queue:work` 无需检查 Horizon 的进程信息。

macOS 与 Linux ARM64 使用真实 Redis 8.10.3 与实际 Master、Supervisor 和 Worker。先确认旧 Worker 能处理任务，再暂停两个父进程模拟接管。旧 Worker 保留新任务并退出，旧 Supervisor 的退出清理保留接管方心跳和未处理命令。持新令牌的 `horizon:work --once` 处理同一任务，普通 `queue:work --once` 即使带有旧进程环境也能处理后续任务。macOS 完整进程及 Worker 回归、Horizon 和框架所涉严格 Clippy、同名 Master 与 102 条心跳回归通过。Linux 双 PID 1 Master 组合通过。Windows ARM64 的原生 `--once` 拒绝陈旧令牌，普通 Worker 正常执行，原生 Master 控制通过。带实际子树的后续排空实测见 [Windows 原生进程树](#windows-原生进程树)。证据为 `target/tmp/observability-parity/horizon-child-ownership-{macos,macos-takeover-verified,linux,linux-takeover-native,windows,rust-targets,preflight}.log`。

固定 Laravel `RedisQueue` 先用 `BLPOP` 等待通知，再单独发送领取请求。修复前，旧 Worker 在 Redis 已阻塞时失去所有权，仍会处理接管后入队的任务。现在通知返回或等待到期后，受管 Worker 会先重新检查所有权及暂停状态，再次确认可领取才向 Redis 发出请求。队列的 `block_for` 与空闲等待上限沿用原有配置。已经发出的领取请求和 Redis 主从切换仍需单独验收，所有权检查与跨连接或实例的领取命令独立执行。 [固定 Laravel RedisQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/RedisQueue.php#L589-L607)

阻塞接管用例确认 Redis 中的 Worker 处于 `BLPOP` 后，在同一条脚本内写入继任令牌、追加真实派发的任务并唤醒旧 Worker。旧 Worker 消费通知后退出，任务留在待处理列表，尝试次数仍为 0、保留集合为空。持新令牌的 `horizon:work --once` 处理同一任务。macOS 与 Linux ARM64 在 Redis 8.10.3 上通过阻塞与非阻塞回归，macOS 的普通 Redis 队列 HTTP 与阻塞领取回归、Horizon Worker 回归、所涉严格 Clippy 和格式检查通过。证据为 `target/tmp/observability-parity/horizon-blocking-ownership-{before,macos,linux,plain-queue,format}.log`。

### Master 命令队列与动态部署

固定上游的本机队列名称为 `master:<Master 全名>`。`commandQueueFor` 在未传名称、传入空串或 `"0"` 时使用本机队列。Redis 按顺序推送 `command` 与 `options`，`pending` 领取后移除，`flush` 清空指定队列。固定 PHP 探针使用真实 Redis 8.10.3 验证名称、空与嵌套选项、FIFO、领取后的空队列和清空，再调用 `AddSupervisor::process` 验证新 Supervisor 的名称与队列。Symfony 启动动作由探针替身承接，Rust 用实际子进程复验。上游证据为 `target/tmp/observability-parity/horizon-master-command-upstream.log`。 [固定 MasterSupervisor](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/MasterSupervisor.php) [固定 RedisHorizonCommandQueue](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/RedisHorizonCommandQueue.php) [固定 AddSupervisor](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/MasterSupervisorCommands/AddSupervisor.php)

Rust 提供 `MasterSupervisor::command_queue`／`command_queue_for`、可由应用容器解析的 `RedisHorizonCommandQueue`，以及 `AddSupervisor::process`。领取与删除在同一条 Redis 脚本内完成。Master 获得名称所有权时清空旧命令，运行中领取须持当前令牌。旧的 Supervisor 文本命令与结构化队列使用不同 Redis 键。动态 Supervisor 校验完整父名称并采用当前 Master 的 PID，活跃同名请求只保留一份。暂停期间先接受部署，继续后启动。显式工作目录用于动态 Supervisor 子进程，目录暂不可用时继续监控并重试。同批无效命令不会阻断后续有效命令。

macOS 和 Linux ARM64 用真实 Redis 验证并发领取、FIFO 与嵌套参数、启动清空、暂停继续、无效命令与去重、工作目录恢复以及新 Worker 的真实任务。旧 Master 退出保留接管方排队的命令，竞争同名启动也不会清空现有持有者的待办。Windows 11 ARM64 通过公开队列把空部署扩展为真实进程树并执行任务，原生排空与意外退出回归通过。证据为 `target/tmp/observability-parity/horizon-master-command-{macos-final,process-regression,competing,linux-final,windows-verified}.log`。

### ProvisioningPlan 与默认部署

固定 PHP 验收确认环境名称按输入顺序列出，精确存在检查和通配部署分开。部署 `testing` 时优先采用排在前面的 `testing*`，递归合并保留未覆盖的队列位置。有效 Supervisor 先通过同一命令队列提交，再派发部署事件。未匹配环境无命令及事件。匹配的空计划或全部停用计划仍派发事件。证据为 `target/tmp/observability-parity/horizon-provisioning-{upstream,cases-upstream}.log`。 [固定 ProvisioningPlan](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/ProvisioningPlan.php)

Rust 的公开 `ProvisioningPlan` 可从应用配置取得，包含有序的环境及 Supervisor 查询入口。环境选项保留停用项供读取，部署只发送启用项。Artisan `horizon` 完成配置校验及获得名称所有权后，在 Redis 比对令牌并一次提交所有 `AddSupervisor` 命令，再发布部署事件。已运行 Master 可由公开 `deploy` 入口追加计划。相对工作目录以应用根为基准，Supervisor 及其 Worker 使用同一目录。部署事件中的监督器列表与已提交的命令一致。

macOS 的真实 Artisan 完整进程回归在部署监听器中检查 Redis 待办数量，覆盖未匹配、匹配空计划、全部停用、正常启动及目录恢复。macOS 与 Linux ARM64 使用公开 `deploy` 与真实 Master 验证首个通配环境、新 Worker 执行任务和相对工作目录。Windows ARM64 复验默认计划启动、排空、快速终止及父进程异常退出。配置测试 8 项、Horizon 全目标严格 Clippy、格式与三语正文检查通过。证据为 `target/tmp/observability-parity/horizon-provisioning-{macos-artisan,macos-process,linux-artisan,windows}.log`。

### 应用容器中的命令处理器

固定 PHP 探针用应用容器替换 `HorizonCommandQueue`，Master 和 Supervisor 构造时都调用替代实现的 `flush`，之后用其 `pending` 领取。容器可解析应用自己的命令类，也可替换内置 `Pause`。两个监控循环把目标实例和原样选项传给命令的 `process`，按领取顺序执行。证据为 `target/tmp/observability-parity/horizon-command-contract-upstream.log`。 [固定命令队列接口](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Contracts/HorizonCommandQueue.php)

Rust 以 `MasterSupervisorCommandHandler` 和 `SupervisorCommandHandler` 表达两个可借用的进程类型。结构化消息领取后，监控进程按命令名优先解析当前应用的绑定，向其传入运行中的实例和原始 JSON 选项。未绑定时运行内置命令。同名异类型绑定按无效配置处理，后续消息仍能执行。Supervisor 处理器可申请以给定状态码排空退出。

macOS 和 Linux ARM64 的真实 Artisan 与 Redis 测试覆盖自定义 Master 部署、同名 `Pause` 覆盖、嵌套选项、处理器退出状态 23，以及另一份应用配置仍执行内置命令。Windows ARM64 的原生进程验证自定义部署和覆盖后真实任务继续运行。原有命令组、Master 重启、窗口内接管及输出按所涉环境复验。Horizon 严格 Clippy、格式和三语文档正文检查通过。证据为 `target/tmp/observability-parity/horizon-command-handlers-{macos,linux,windows,docs}.log`。

### 可替换的命令队列

容器默认解析到 `RedisHorizonCommandQueue`。默认 Master 部署、接管和重启沿用 Redis 原子脚本。应用可绑定 `dyn HorizonCommandQueue`，Master、Supervisor、公开部署与公开重启均采用当前应用的绑定。独立 Supervisor 启动时清空自己的逻辑队列。受管进程以逻辑名称和当前所有权代次区分队列，发布者通过 `Repository::command_target` 解析目标。Master 取得所有权后先清空并发布路由，再发布进程心跳。命令领取后复核所有权，结束时按原代次清理。旧实例的延迟领取或清理只接触自己的队列。

自定义传输须按 `CommandQueueTarget` 的名称和代次区分存储，并保证各目标的 FIFO 领取。Redis 仍负责进程心跳及所有权。进程被强制终止后，可由后端按代次回收旧队列。内置 Redis 适配器的有代次列表设置一天有效期。自定义命令传输和 Redis 元数据之间不存在统一事务，跨队列操作在部分失败时沿用进程排空和重启流程。

固定 PHP 的替换接口与启动清理证据见上节。macOS 与 Linux ARM64 使用替代 Redis 键空间和加锁文件后端，经过公开发布、部署和重启入口启动真实 Worker、发送和领取命令、重启受管进程并保持独立 Supervisor 的退出码。文件后端在旧 Master 领取过程中切换所有权。旧命令的应用处理器没有执行，继任者的命令可用，继任令牌未被旧实例清除。Windows ARM64 两种后端均通过真实进程的动态部署、任务执行、重启排空和结构化命令回归。Horizon 全目标严格 Clippy、格式和三语正文检查通过。证据为 `target/tmp/observability-parity/horizon-custom-command-queue-{macos-final,linux,windows-transport,docs}.log`。

### Supervisor 结构化命令

固定 PHP 探针经真实 Redis 8.10.3 发送并领取 `Pause`、`ContinueWorking`、`Restart`、`Scale`、`Balance` 及 `Terminate`。调用固定命令类和固定 `Supervisor` 方法，使用进程池替身观察调用。暂停和继续切换工作状态，重启恢复工作并更换 Worker，`Scale(5)` 在两个池各申请 2 个 Worker 并将上限提高到 5，`Balance` 忽略不存在的池，终止状态可指定且默认 0。证据为 `target/tmp/observability-parity/horizon-supervisor-commands-upstream.log`。 [固定 SupervisorCommands](https://github.com/laravel/horizon/tree/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/SupervisorCommands) [固定 Supervisor](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Supervisor.php)

Rust 透过 `supervisor_commands` 提供命令名称。独立 Supervisor 启动时清空旧的结构化命令。受管 Supervisor 由 Master 在创建子进程之前清空旧命令，因此可接收启动过程中抵达的命令。受管 Supervisor 的清空、领取和退出清理均比对 Master 所有权。运行中按顺序领取，使用现有进程池扩缩容和排空机制。旧文本命令继续由原 Redis 键处理。无效结构化命令记录错误并继续，同批后续合法命令仍可处理。`run_with_status` 和真实 Artisan 返回终止状态，CLI 状态取值为 0 至 255。

macOS 与 Linux ARM64 用真实 Artisan Supervisor、Worker 与 Redis 验证了六类命令，覆盖启动清空、重启恢复工作、两个池的扩缩容、排空和退出码 23／0。接管复验确认旧进程保留了新持有者的结构化命令。Windows ARM64 原生受管进程树验证了六类命令，以及默认状态码 0 的终止。证据为 `target/tmp/observability-parity/horizon-supervisor-commands-{build,macos,linux,windows}.log`。

### Master 公开重启

固定上游 `MasterSupervisor::restart` 恢复 Master 工作状态，向每个 Supervisor 的结构化队列发送 `Terminate` 和状态码 1。监控循环按原配置重新部署退出的 Supervisor。PHP 8.5.12 与 Redis 8.10.3 上验证两个 Supervisor 的命令及选项，进程由探针替身承接。证据为 `target/tmp/observability-parity/horizon-master-restart-upstream.log`。 [固定 MasterSupervisor](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/MasterSupervisor.php) [固定 SupervisorProcess](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/SupervisorProcess.php)

Rust 的 `MasterSupervisor::restart().await` 通过该实例的命令队列提交请求。独立应用进程也可向目标 Master 队列提交 `RESTART`。Master 在 Redis 比对所有权后一次写入各子进程的暂停状态及终止命令，并清除自己的暂停状态。已开始的任务继续执行。默认模式下，Supervisor 排空后以状态码 1 退出。同一 Master 按原部署选项重新拉起子树，主动产生的状态码 1 不计入异常退出。

macOS 和 Linux ARM64 在真实 Redis 8.10.3 上验证暂停恢复、执行中的任务只完成一次、待处理任务随后执行、两个 Supervisor 及其 Worker 更换 PID，Master 保持原 PID。Windows 11 ARM64 原生进程验证同一流程，并直接检查旧 Supervisor 以状态码 1 退出。macOS 的公开实例方法和所有权竞争也在 20 项 Redis 运行时测试中通过，完整进程回归、Horizon 全目标严格 Clippy、格式和三语正文检查通过。证据为 `target/tmp/observability-parity/horizon-master-restart-{macos-final,macos-regression,linux,windows-final,docs}.log`。快速终止模式的专项见下文，正式部署组合继续验收。

固定 PHP 8.5.12 的重启命令始终传入退出码 1。通过固定 Supervisor 的真实命令入口验证，默认及显式等待配置先停止领取并等待旧 Worker。启用快速终止且无等待标记时，Supervisor 发出终止后即退出。macOS 实测两种配置，Linux 7.0.14 ARM64 实测快速配置。后者经真实 Artisan 和独占 Redis 8.10.3 保证两个待处理结果各写一次，接替 Worker 能在原任务结束前完成新任务。证据为 `target/tmp/observability-parity/horizon-{fast-restart-upstream,default-restart-macos,fast-restart-macos,fast-restart-linux}.log`。

Windows 11 ARM64 复现了输出管道与退出状态耦合。操作系统记录旧 Supervisor 在发送重启后约 0.8 秒退出，旧 Worker 约 11.4 秒后退出，替代 Worker 直到约 14.1 秒后可见。已定位到 Master 以进程输出读取任务完成为退出信号，旧 Worker 仍持有输出管道时阻塞替代进程。证据为 `target/tmp/observability-windows/horizon-fast-restart-arm64-exit-times.log`。现在 Master 使用退出码 1 确认旧 Supervisor 已退出，并继续异步读取旧输出。Windows ARM64 的同一 Rust 1.94.0 原生构建在专用 Redis 8.10.3 中通过八组进程矩阵。快速重启组实际验证同一 Master 更换子树，两项任务各成功一次，新任务在旧任务完成前结束。独立构建成品重跑通过，初轮剩余场景因项目磁盘清理移除了 Cargo 产物而中断。证据为 `target/tmp/observability-windows/horizon-fast-restart-arm64-{repaired,isolated}.log`。Windows x64、Windows Release 和实际服务组合继续验收。

### Master 与 Supervisor 输出回调

固定 PHP 8.5.12、Horizon 与 Symfony Process 8.0.5 探针验证 `MasterSupervisor::handleOutputUsing` 和 `Supervisor::handleOutputUsing` 返回原实例。两者通过直接 `output`、`AddSupervisor` 生成的真实子进程或 `ProcessPool` 启动的真实子进程接收 stdout／stderr，保留换行和末尾无换行字节。探针控制子进程的打印内容，Symfony 禁用输出留存。上游的 `horizon` 和 `horizon:supervisor` 都将收到的两个流写入各自的 Artisan 输出。证据为 `target/tmp/observability-parity/horizon-output-upstream-final.log`。 [固定 AddSupervisor](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/MasterSupervisorCommands/AddSupervisor.php) [固定 ProcessPool](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/ProcessPool.php) [固定 HorizonCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/HorizonCommand.php) [固定 SupervisorCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/SupervisorCommand.php)

Rust 的 `MasterSupervisor` 和 `Supervisor` 提供 `handle_output_using` 与 `output`，按直接子进程的流类型传回原始字节。Rust 使用可在线程间共享的回调，调用方自行处理跨子进程的并发。真实 Artisan 实时写出两个流，`horizon:supervisor` 将 Worker 的两个流写入自己的 stdout。文件监听命令实时转发重启前后的进程输出，保留自身的错误流。框架进程的 `start_with_streamed_output` 在调用回调时跳过历史留存。显式捕获的进程内 Artisan 结果仍按原模型收集输出。

Symfony Console 8.0.6 的实测覆盖 `<info>`、跨输出片段的样式、未知标签和强制 ANSI。Rust 的 Artisan 复用同一个控制台格式器处理有效 UTF-8 输出片段。已知标签按当前装饰选项显示，未知标签保留原文。程序直接注册的回调仍接收原始字节。无效 UTF-8 或格式器报错的片段继续按字节转发，文件监听命令保留 Cargo 的独立诊断。

macOS 和 Linux ARM64 的实际 Artisan／Worker 验证强制 ANSI、跨写入的样式、未知标签及原始回调。Windows ARM64 用真实 Redis 验证普通标签格式、未知标签和原生进程树回归。Horizon 严格 Clippy、格式及三语正文检查通过。证据为 `target/tmp/observability-parity/horizon-output-format-{upstream,macos-final,linux,windows,docs}.log`。

macOS 与 Linux ARM64 用真实 Redis 和 Worker 核对公开回调、实时输出和静默模式。macOS 文件监听在源码变动前后都收到实际任务的输出，框架进程测试 17 项通过，其中真实子进程输出 64 MiB，回调收到完整字节，结果无保留副本。框架和 Horizon 所涉严格 Clippy、格式及三语正文检查通过。

Windows ARM64 首轮使用默认子进程输出管道时，快速终止等待了正在执行的任务。流式模式改用 Tokio 异步命名管道之后，真实 Redis 复验通过双流输出、Master 重启、默认终止、`--wait`、快速终止先退出 Master 以及父进程意外退出后的排空。证据为 `target/tmp/observability-parity/horizon-output-{macos-final,macos-pipes,framework-clippy,linux-final,windows,windows-final,docs}.log`。 [Tokio 异步命名管道](https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/struct.ServerOptions.html)

### WorkerProcess 与 SupervisorProcess 输出回调

固定 WorkerProcess 与 SupervisorProcess 的原生 Symfony 子进程验证 `handleOutputUsing` 返回包装器，并注册 stdout／stderr 回调。已启动子进程保留启动时的回调。Worker 后续受 `monitor` 重启时使用更新后的回调，并在连续重启后保持注册。Supervisor 的受管重部署由 Master 决定。证据为 `target/tmp/observability-parity/horizon-wrapper-output-upstream.log`。 [固定 WorkerProcess](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/WorkerProcess.php) [固定 SupervisorProcess](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/SupervisorProcess.php)

Rust 的两个包装器提供 `handle_output_using`、`start(PendingProcess)`、`process` 与 `take_process`。包装器拥有当前的进程句柄，显式取句柄后可启动下一次。注册或替换回调影响后续启动，输出仍按原始流和字节转发。受管路径从 Master 和 Supervisor 继承回调，并通过相同包装器启动。PHP 的动态方法转发在 Rust 使用显式进程句柄。

macOS、Linux ARM64 和 Windows ARM64 均运行实际子进程验证独立包装器连续启动、回调替换、重复启动保护和两路字节传输。macOS 复验真实 Worker SIGKILL 补位后的双流输出，Linux ARM64 核对 Master 重部署后真实任务继续输出，Windows ARM64 的完整进程树及两种自定义命令后端通过。Horizon 全目标严格 Clippy、格式和三语文档正文检查通过。证据为 `target/tmp/observability-parity/horizon-wrapper-output-{macos,linux,windows,docs}.log`。

### WorkerProcess 与 SupervisorProcess 控制

Linux 的固定 PHP 探针用系统信号编号和真实 shell 子进程核对 Worker 包装器的 `pause`、`continue`、`terminate`，观察到 USR2、CONT、TERM 的顺序及终止状态码，并确认首次 `start` 启动冷却。该探针对应启用 PCNTL 时由该扩展提供的信号常量。证据为 `target/tmp/observability-parity/horizon-wrapper-control-upstream.log`。

Rust 的 Worker 公开 `monitor` 及 `cooling_down`，和受管 Worker 共用退出检查、重启事件及冷却。首次尝试进入一秒冷却，失败退出时等候原截止时间，下一次补位采用一分钟冷却，正常退出可立即补位。两个包装器提供 `pause`、`continue_working`、`terminate` 和 `stop`。Unix 信号只发给子进程本身，`stop` 按宽限期等待排空。Windows 独立包装器的暂停和继续缺少 Unix 信号入口，受管进程继续由 Redis 状态控制，并通过实际关停管道排空。Supervisor 的 `terminate_with_status` 向当前应用绑定的队列发送带退出码的命令。

框架进程驱动根据所持子进程句柄发送信号。直接子进程控制复用同一驱动的模拟信号记录和 Windows 关停管道。真实 Unix 组内进程测试分别验证单进程信号与全组信号。

macOS 和 Linux ARM64 的原生子进程验证暂停、继续、终止、排空和冷却。真实 Redis 验证默认及替代队列的公开终止入口，独立 Supervisor 以状态码 23 退出。Windows ARM64 验证真实关停管道、冷却和完整受管进程树。macOS 框架进程测试 18 项、Horizon 原生进程矩阵、相关严格 Clippy、格式及三语正文检查通过。证据为 `target/tmp/observability-parity/horizon-wrapper-control-{owned-macos,owned-linux,owned-windows,framework-final,docs}.log`。

### SupervisorProcess 独立监控与重部署

固定上游的真实 Symfony 子进程与 Redis 验证退出码 13 立即标为死亡，0／2 满冷却后标为死亡并结束，1 满冷却后删除旧心跳并按原选项发出 `AddSupervisor`。证据为 `target/tmp/observability-parity/horizon-supervisor-wrapper-monitor-upstream.log`。 [固定 SupervisorProcess](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/SupervisorProcess.php)

Rust 的 `SupervisorProcess::monitor` 接收目标 Master 的完整名称，并复用受管监控的退出判定和冷却逻辑。框架的 `InvokedProcess::spawned_id` 保存退出清理需要的启动 PID。独立包装器只删除匹配其已退出子进程的心跳，然后通过当前应用的命令队列提交原选项。同名替代进程的心跳仍归新进程。受管 Master 在现有所有权租约下直接维护其子进程。

macOS 与 Linux ARM64 的真实 Redis 8.10.3 验收覆盖默认及替代 Redis、文件命令队列。0／2／13 退出不触发重部署，异常退出后同一包装器只发起一次恢复，运行中的 Master 用原队列选项部署真实 Worker。验收向恢复的 Worker 派发任务并取得执行结果。同名替代心跳保留。框架进程测试 18 项、对应严格 Clippy、全仓格式、三语站点构建及两种挂载路径的浏览器检查通过。

Windows 11 ARM64 的默认 Redis 及文件命令队列均验证独立包装器的退出分类、同名替代心跳与实际重部署，并通过正常终止、排空、快速终止、父进程丢失、结构化命令和命名处理器回归。证据为 `target/tmp/observability-parity/horizon-supervisor-wrapper-monitor-{macos-final,linux-final,windows-final,framework,fmt,docs-final}.log`。

上游的 `dontRestartOn` 可由调用方覆盖。固定 PHP 实测将 1 加入列表后保留心跳且不重部署，清空列表后退出码 2 可重部署，退出码 13 始终立即标为死亡。Rust 的 `dont_restart_on` 同样是包装器实例上的公开字段。按原选项重新部署时，新实例从默认清单开始。Master 对受管包装器的访问和配置入口另需逐项核对。上游证据为 `target/tmp/observability-parity/horizon-supervisor-restart-codes-upstream.log`。

macOS、Linux ARM64、Windows 11 ARM64 复验自定义退出码和实际重部署。macOS 与 Linux 的默认队列验收派发任务至两组重部署的 Worker 并取得执行结果。三平台均覆盖文件命令队列，macOS 与 Linux 另覆盖独立 Redis 键空间的命令队列。Horizon 全目标严格 Clippy、快照全仓格式、三语文档及两种挂载路径的浏览器检查通过。证据为 `target/tmp/observability-parity/horizon-supervisor-restart-codes-{macos,linux,windows,docs}.log`。

### Windows 原生进程树

框架进程在 Windows 可由 `PendingProcess::with_shutdown_pipe` 保留一条仅用于关停的管道。Master 和 Supervisor 启动受管子进程时各持有对应管道的写端。正常终止、快速分离和父进程丢失都会关闭写端，子进程收到 EOF 后走既有排空路径。Worker 领取前也检查该管道，避免已经获知关停时领取新任务。快速终止可以让 Master 先退出，已开始的任务继续到完成或超时。失去 Redis 所有权仍按独立令牌处理。Unix 继续使用原生信号及父进程检查。

标准 Windows ARM64 MSVC 的首轮调试产物在真实任务前以 `0xC00000FD` 退出。对相同 PE 的隔离副本测得 1 MiB 栈预留，将预留提高到 16 MiB 可继续执行任务。Windows 验收夹具通过实际 SQLite 接口创建 `failed_jobs` 表。根应用可执行文件与 Horizon 验收示例现在由 Cargo 为 Windows MSVC 链接 16 MiB 栈预留。证据为 `target/tmp/observability-parity/horizon-windows-shutdown-{native-schema,stack-trace,stack-schema,linked}.log`。

Windows 11 ARM64 用正常 Cargo 构建及真实 Redis 8.10.3 运行 [进程树验收](../../../../vendor/openrust/horizon/tests/windows_process_acceptance.ps1)。确认实际父子 PID、非空部署、带 Unicode 的 Master 和 Supervisor、暂停期间保留任务、继续后执行。空闲终止、`--wait` 等待 12 秒任务、快速终止先退出 Master、强制结束 Master 后子树仍完成正在执行的任务均通过。父进程丢失后，后续任务留在队列，由普通 `queue:work --once` 成功处理。直接检查 Cargo 生成的根应用与验收程序 PE，均预留 16 MiB。优化构建、Windows x86_64 与正式服务管理器组合仍待验收。证据为 `target/tmp/observability-parity/horizon-windows-shutdown-{linked,retention}.log`。

Windows ARM64 的 Horizon 库与验收示例严格 Clippy 通过。macOS 的框架进程测试 15 项、Horizon 完整进程回归、Horizon 全目标与框架库严格 Clippy 通过。Linux ARM64 复验通过阻塞和非阻塞接管及真实孤儿清理。三语文档正文检查通过。证据为 `target/tmp/observability-parity/horizon-windows-shutdown-{retention,core-regression,macos-final,linux-final}.log`。

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
