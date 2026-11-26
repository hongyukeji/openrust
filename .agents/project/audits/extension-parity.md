# 扩展与固定上游的逐项对齐

2026-11-21 用户要求严格按 Laravel 生态开发、联网调研成熟方案，并持续完成扩展。本表区分已交付范围、上游等价性与组合验收。O01–O06约定范围已通过当前源码的本地组合验收；未支持能力及正式发布／外部服务边界继续明确保留，见[最终组合](extension-final.md)。

## 来源和取舍

联网取得以下提交的完整 GitHub 源码归档（含包内文档、测试与许可证，以归档实际内容为准），保存于 `tmp/openrust-ecosystem-20261121/upstream`。搜索工具未返回可用内容，因此依据直接取得的 GitHub 主来源核对，不宣称已核实实时热门榜或 Star 排名。

| 对照项目 | 固定提交 | 采用理由 |
|---|---|---|
| [spatie/laravel-activitylog](https://github.com/spatie/laravel-activitylog/tree/fdcc5ee063cbf91114679336c4fbf92837196419) | fdcc5ee063cbf91114679336c4fbf92837196419 | 对应日志模型、流式记录、模型事件、属性选择、操作者和清理；核对该提交实际结构，不套用旧版接口 |
| [laravel/horizon](https://github.com/laravel/horizon/tree/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa) | b86d5aa87ca474db352ea2668bdd1df3a90bf8aa | 官方 Redis 队列监督、扩缩容、管理命令、标签、指标与面板 |
| [laravel/telescope](https://github.com/laravel/telescope/tree/39a8ea8ad79915df6c3ac9b4e72659ade808eba3) | 39a8ea8ad79915df6c3ac9b4e72659ade808eba3 | 官方诊断条目、Watcher、过滤、关联、SQL 存储及清理 |
| [laravel/pennant](https://github.com/laravel/pennant/tree/9046466feb0146b6223db8b8e9c589aeba23f7f0) | 9046466feb0146b6223db8b8e9c589aeba23f7f0 | 官方特性作用域、解析、缓存、驱动、批量查询及中间件 |
| [laravel/pulse](https://github.com/laravel/pulse/tree/33cc6fc68cd3052aa06227e88e8d933dd3d70a51) | 33cc6fc68cd3052aa06227e88e8d933dd3d70a51 | 官方 Recorder、采样、聚合、存储和授权面板 |
| [laravel/scout](https://github.com/laravel/scout/tree/6f6560dbcda3ec99d9ba90bd6eebed5e0f16e363) | 6f6560dbcda3ec99d9ba90bd6eebed5e0f16e363 | 官方 Builder、Searchable、模型观察器、引擎及索引命令 |

这些包直接对应用户要求的生态能力，优先复用其结构与契约。Rust 内部复用已有数据库、事件、队列、Redis、进程、授权和包发现，不另造通用观测内核或任务执行内核。所有默认值、错误行为、Artisan 参数及公开拼写以固定源码核对；语言差异逐项说明，不能用“Rust 适配”掩盖可直接保持的行为差异。

旧版参考 `v0.x` 为 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf`；该提交没有上述六个独立扩展包。已有 audit／queue 内部实现的复用边界见各包审计；未找到的旧包不写成已复用。

## O01 Activitylog：固定上游对齐验收通过

2026-11-21 依据[包审计](activitylog/README.md)及 `activitylog-final-*` 日志核对，关闭先前重新打开的实现缺口；当前核心与扩展组合另见最终组合记录，不从本包结果推断整个生态完成。

| 契约 | 当前状态与边界 |
|---|---|
| 独立发现、Provider、配置、发布、迁移与缓存 | 原生应用 SQLite／PostgreSQL18／MySQL8.4 验收通过；默认骨架两项通过 |
| 显式记录、主体／操作者、查询、事务及清理 | 三库各10项数据行为通过；默认迁移为数值nullable morph、nullable JSON、原生timestamps／自增ID，字符串身份须定制迁移 |
| `activity()`、`ActivityLogger`、`PendingActivityLog`及流式入口 | 新鲜记录器、成功后重置、tap／Action／占位符及匿名优先级已验 |
| `ActivityLogStatus`、`CauserResolver`及认证 | 嵌套恢复、按guard已认证快照、真实morph、原始ID异步Provider查询与跨请求隔离已验；同步回调不主动发起认证I/O |
| `LogOptions`、ChangeDetector及模型事件 | dirty／排除／raw／JSON路径、逐实例开关、事务内懒加载关系、自定义软删除列、定制模型／Action及原生模型Creating事件已验 |
| `ActivityBuffer`及生命周期 | HTTP／Artisan／Worker使用原生defer；事务提交后入队、回滚／保存点回滚丢弃及嵌套记录顺序已验；standalone须显式flush，取消／SIGKILL／硬超时不保证刷新 |
| 固定上游对照 | 8组LogOptions、4组status、900组ChangeDetector固定PHP结果回归通过；本轮没有重新执行PHP参考生成 |
| 严格检查、三语文档与浏览器 | 包23项、严格Clippy／Rustdoc、快照fmt、三语构建及专项／通用各12组通过；31所属文件与已验源码一致 |

Rust动态关系须显式登记、LoggingState显式提供、异步原始ID解析等语言边界及证据路径见包审计。管理HTTP接口、租户授权、防篡改存储与外部副作用恰好一次不由本包自动提供。上述状态替代先前“by原始ID待齐／自动缓冲未完成”等旧描述。

## O02 Horizon：本地验收通过

Horizon包与其已验快照逐文件一致；当前核心的Worker／事件／Artisan已有后续改动，因此另以当前快照重新构建真实程序并执行Redis／进程／浏览器组合，不直接沿用旧核心结果。详见[包审计](horizon/README.md)及[最终组合](extension-final.md)。

| 契约 | 当前状态与边界 |
|---|---|
| AutoScaler／ProvisioningPlan／SupervisorOptions | 固定PHP224组分配／环境／选项对照；浮点比例、升序限幅、退出计数、time／size／log、未知runtime、递归合并及零预算停用已齐 |
| Master→SupervisorProcess→Supervisor→ProcessPool→horizon:work | 实际父子进程树、独立心跳与控制、崩溃重建、孤儿排空、TERM／fast termination、内存退出与listen重启已验 |
| Artisan管理与安装 | work／supervisor／控制／状态／list／supervisors／purge／timeout／snapshot／clear-metrics／clear／forget／install／publish／listen已有真实入口；retry／failed明确为额外入口 |
| 分仓储、任务、标签、指标与通知 | 七类仓储职责、任务终态与详情／重试引用、监控／静默标签、trim、指标快照／锁、长等待与去重已实现；原生database批次三库已验 |
| 原生队列身份 | transport ID与payload UUID显式桥接，重复／过时事件保护、历史forget竞态、未部署队列观测均已实际验证，属明确原生队列适配 |
| 授权面板 | 实际任务分页／详情／重试、指标／快照、标签写入、失败和批次操作；当前源码Chrome桌面手机、同源写保护、XSS／CSP及布局通过 |
| Redis与平台 | Redis 8.10.3 隔离夹具通过；拒绝上游同样不支持的 Redis Cluster。macOS 和 Linux ARM64 容器均有真实进程与信号证据。Windows ARM64 的 Horizon 包和 Artisan 应用装配已验。Linux x86_64、实际收件与部署组合仍待验收。 |

上述关闭原来“Master直管Worker／命令与分仓储未实现／只读面板”等过时缺口。不宣称执行完整PHP PHPUnit，也不隐藏Rust显式标签、轮询listen与内存衡量方式等已记录适配。

## O03 Telescope：原生支持范围验收通过

[原生链路及补齐记录](telescope/README.md)涵盖18类Entry／公开Watcher、独立注册／选项、框架事件开关、作用域与defer、仓储、队列更新、缓存、SQL、日志、邮件／通知、事件／模型／视图／Gate／调度／Dump／Redis／出站HTTP。新增已缓冲响应脱敏、字符限制及实际路由字段通过真实HTTP三库；文件与流不读取。原26项＋新增2项默认通过，Redis单独实跑。

三库／缓存／Worker、实际服务器Chrome桌面手机、安装、发布保留定制、根配置缓存／骨架及全部扩展组合见包审计与[最终组合](extension-final.md)。先前公开Watcher、ignoreFrameworkEvents、缓冲响应／路由字段缺口已关闭。

与PHP仍有明确差异：原生安全UI替代Vue／avatar，Query 部分入口和 Gate 外层调用帧选择仍待核对，不透明 Body／流及原生 `Part` 仍有元数据边界，CLI 使用普通表格与时间戳。HTTP 客户端已支持构建器提供的 multipart 元数据，见[包审计](telescope/README.md#出站-multipart)。正式服务／其他OS需要独立验证。

## O03–O06 与完成条件

O01 Activitylog、O02 Horizon、O03 Telescope原生范围、O04 Pennant、O05 Pulse及O06 Scout首个引擎范围已有实现与定向验收；当前统一组合见[最终记录](extension-final.md)。Scout不内置所有外部引擎，Telescope的PHP反射／UI等边界如上，不能从路线完成推断上游所有插件功能全部等价。必要核心改动由本对话完成；不联系其他对话，保留共享WIP，未执行暂存、提交或推送。

每项关闭前要求：上游源码／适用测试到 Rust 的对照、真实入口和失败路径、相关三库／缓存驱动、默认骨架／配置缓存边界、三语开发者文档及浏览器验收。与最终核心源码的全功能组合统一复验。许可证、命名、默认值及任何必要差异同时核对；仅“自己的测试全绿”不构成完整对齐证据。

早期Horizon纠偏的224组固定PHP参考只证明对应算法／配置／Worker选项；后续进程树、命令、仓储、标签及面板已另行实现验收，当前状态以O02段和最终组合记录为准。完整PHP PHPUnit套件未执行。

## Telescope、Pulse、Horizon 上游复核（2026-11-25）

本轮沿用上表固定提交。旧版没有对应独立扩展；复用当前包的配置加载、事件与消费者测试。三个包继续保留逐项对齐未完成状态。

| 所属类型或配置 | 本轮裁决 | 后续核对 |
|---|---|---|
| Telescope `ClientRequestWatcher::duration` | 固定上游在有正计时时以毫秒向下取整，缺少计时时输出空值。Rust 从现有传输统计输出整数毫秒或 JSON 空值。测试由真实出站 HTTP 与 fake 消费者覆盖。 | 命令与面板展示随完整输出审计核对。 |
| Telescope `ClientRequestWatcher::input` | 已捕获普通字段、直接预格式化列表及混合对象、原生 `Part` 字段和附件元数据。提交的数据先于登记附件发送，同名取最后一项并保留首次位置。顺序连续的数字字段以 JSON 数组记录。浮点发送使用固定 PHP 默认精度，普通及描述部件使用编码长度填充默认部件头。请求中间件改写媒体类型时接入原部件列表、空列表回退和连接失败路径。 | 诊断形状、脱敏及平台验收见[包审计](telescope/README.md#出站-multipart)。 |
| Telescope 非 multipart 出站载荷 | 诊断从原始 GET 地址或结构化正文选择载荷，并与线上 URL 分别记录。已核对 JSON／表单 POST、GET 查询覆盖、HEAD、原始正文、自定义媒体类型与表单标量快照。成功重定向保留原始输入，连接失败从实际缓冲正文解码。 | multipart 重定向、重试回调改写与其他中间件组合仍需对账。 |
| Telescope `FetchesStackTrace`、`QueryWatcher`、`GateWatcher` | 固定上游按路径子串跳过被忽略的帧，再选第一处合格来源。找不到合格帧时 Query 不入库，Gate 仍入库且行号和文件为空。Gate 已覆盖授权入口及同应用嵌套授权来源。Query 已覆盖核心 SQL、分页、模型写入、直接关系读取、关系加载、多对多中间表、关联写入和常规 Schema 入口。 | 普通异步包装器的外层候选、关系变体逐类型验收、驱动特有 Schema 操作和发布构建路径重映射仍待核对。 |
| Pulse 默认 Recorder 清单 | 固定配置的十个 Recorder 均可映射到本地配置。已核对采样率 1、慢操作阈值 1000、异常和慢查询位置开关、默认存储及采集驱动。SlowQueries 现使用数据库事件的调用位置。Servers 默认主机名随配置加载求值。 | 逐 Recorder 核对触发事件、筛选、采样、聚合、配置覆盖和扩展入口，再核对命令和面板。 |
| Horizon `horizon:listen` 的 `watch` | 上游发布配置提供默认监听列表。Rust 配置补充应用、启动、配置、数据库、路由、视图、公开目录及资源目录中的 Rust 文件和 Cargo 构建入口的对应监听路径。macOS 和 Linux ARM64 的真实 Artisan 使用默认配置监控 `routes/web.rs`，文件更新后重建 Master 和 Worker，SIGTERM 后子树退出。 | 发布组合与其余适用的 Unix 目标。 |
| Horizon 其余配置 | 固定上游含 `name`、`waits`、`silenced`、`silenced_tags` 及 Supervisor `nice`。本地已读静默配置并具备 `nice` 运行时默认；配置形状、面板名称、等待时间入口仍须逐项核对。 | 对照发布配置及实际命令、Redis 和面板消费者，分别核对默认、覆盖及非法值。 |

源文件对照：[Telescope ClientRequestWatcher](https://github.com/laravel/telescope/blob/39a8ea8ad79915df6c3ac9b4e72659ade808eba3/src/Watchers/ClientRequestWatcher.php) · [Pulse 默认配置](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/config/pulse.php) · [Horizon 默认配置](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/config/horizon.php)。

全仓严格 Clippy 卡在隔离快照中 Fortify 的 `two_factor_model`、`installed_actions`、`starter_ui` 测试编译深度溢出。

验证使用提交 `8041ae15a` 加本批文件的隔离快照，保留共享工作区的在途锁文件和核心改动。Telescope Provider／Watcher 共 19 项通过，独立 Redis 用例未执行。Provider 的数据库 Worker 用例在默认测试线程栈上溢出，设置 `RUST_MIN_STACK=33554432` 后重跑通过。Horizon 配置 4 项通过，独立 Redis 配置用例未执行；真实进程、HTTP 和 Artisan 验收使用隔离 Redis 8.10.3。三语站点构建和 12 组浏览器检查通过，构建有现有包大小提示。快照全仓格式、Telescope／Horizon 全目标严格 Clippy 及差异空白检查通过。

## 持续对齐与生产验收

2026-11-25 用户授权持续实现、逐项验收，直至三个扩展完成对齐。沿用本账本固定提交，每批核对所属类型、公开入口、配置和真实消费者，记录可观察差异、未实现和未验证项。验收后按范围本地提交，保留并行改动。

| 范围 | 接续工作与关闭条件 |
|---|---|
| Telescope | 继续 Query／Gate 来源过滤与出站 multipart 剩余边界。逐类型核对 Watcher、Entry／过滤与脱敏、关联和仓储、生命周期、配置发布、命令输出及授权面板。原生界面和来源传递须验证同等使用结果。 |
| Pulse | 按固定配置的十个 Recorder 逐一核对订阅事件、默认参数、过滤、采样和聚合。继续核对自定义 Recorder、存储和 ingest 契约、Worker 与命令、面板授权及卡片数据。 |
| Horizon | 继续核对 `Tags`、其余管理命令、监督器与 Worker、进程恢复、仓储、指标和面板操作。Redis Cluster 沿用固定上游支持边界。 |
| 发布验收 | 跟踪 Fortify 三个测试目标编译深度溢出，复验最终交付快照的全仓门禁。锁定提交、依赖、目标平台和实际数据库／Redis／Worker 组合，验收迁移、正常关停、故障恢复、业务端到端及容量。平台与供应商所需真实条件按[发布待办](../todo.md#平台与发布条件)记录。 |

Pulse 命令复核发现独立的调度差异。固定 `CheckCommand::handle` 以本轮采样时刻加一秒为下一次采样时间，`WorkCommand::handle` 则在完成 digest、清理和 Telescope 收集后固定等待一秒。修复前，Rust `commands.rs` 两条路径都从等待时间扣除本轮处理耗时，慢 digest 可能结束后立即再读。 [固定 CheckCommand](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Commands/CheckCommand.php) [固定 WorkCommand](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Commands/WorkCommand.php)

Rust 修复在检查命令内以采样时的单调时钟为准，在工作命令内把一秒等待放到本轮全部处理之后。新增的真实 Artisan 回归以 Tokio 可控时钟模拟耗时两秒的 Ingest 和耗时 400 毫秒的检查监听器，并通过共享缓存重启信号自然结束两条命令。修正用例先在应用启动前登记自定义 Ingest，再通过命令自身作用域解析缓存。macOS 的 cache 2 项、Telescope 命令联动 1 项、Pulse 所涉严格 Clippy 和三语内容检查通过。证据为 `target/tmp/observability-parity/pulse-work-cadence-macos-rebound.log`。Linux ARM64 的同一 Artisan 间隔回归和 Telescope 联动也通过，证据见文件重放组合。Windows、Redis 实例上长期运行及完整发布组合继续验收。

2026-11-26 阶段组合包含 Telescope 关联写入、关系加载和 Gate 回退，Pulse 异常报告入口，以及 Horizon 显式监督器事件。三个扩展的同一隔离源码快照默认 94 项通过、23 项按环境条件跳过，仓库格式通过。证据为 `target/tmp/observability-parity/observability-combined-verified.log`。Release、各扩展剩余公开契约和实际部署组合继续验收。

同日补齐 Pulse [Recorder 模式修饰符与分隔符](pulse/README.md#recorder-模式修饰符与分隔符)，复跑同组源码的默认测试，96 项通过、23 项按环境条件跳过。后续补齐[零宽匹配与替换转义](pulse/README.md#分组的零宽匹配与替换转义)，组合默认测试增至 97 项通过、23 项按环境条件跳过。经固定上游 Recorder 和 SQLite 进一步确认的非 UTF-8 分组键差异保留在专项审计中。

### Horizon 面板名称与配置默认值

固定上游[配置](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/config/horizon.php)与[面板模板](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/resources/views/layout.blade.php)均已核对。补齐 `HORIZON_NAME`，将可选名称作为文本显示于页面标题与主标题，同时发布 `waits.redis:default=60`、空静默清单以及 Supervisor 的 `nice=0`。现有等待时间监控、静默行为及进程参数的完整行为对账继续保留。

隔离快照沿用 `8041ae15a` 与前批文件，叠加本批。配置消费者 5 项及隔离 Redis 8.10.3 的双应用配置隔离 1 项通过。真实 Redis 8.10.3 进程／HTTP／Artisan 回归、全仓格式、Horizon 全目标严格 Clippy、三语文档构建及 12 组浏览器检查通过。发布组合与其他目标平台仍待专项复验。

### Horizon Artisan 本机监督器选择与状态类别

PHP 8.5.12 用固定 Horizon、Laravel 和 Symfony Console 8.0.6 实跑 `StatusCommand::handle` 与 `SupervisorStatusCommand::handle`。无 Master 返回 2 并输出错误类别；只要有一个暂停的 Master 就返回 1 并输出警告，其余返回 0 和信息类别。监督器状态先选当前机器前缀，再匹配名称后缀。固定暂停和继续命令源码也采用这一选择规则，后续向选中 PID 发送本机信号。证据为 `target/tmp/observability-parity/horizon-supervisor-local-upstream.log`。 [固定 StatusCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/StatusCommand.php) [固定 SupervisorStatusCommand](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Console/SupervisorStatusCommand.php)

Rust 利用锁定的 hostname 0.4.2 读取系统主机名，Master 和 Supervisor 心跳登记主机。具名状态、暂停和继续命令先核对完整主机名，再按后缀匹配。共享 Redis 上名称相似的其他主机及升级前缺少主机字段的心跳不会成为本机信号目标。Master 现有 UUID 和 Redis 键格式继续使用。默认名称前缀、可替换名称解析器及 Master 级全局控制与上游本机控制的差异另待对账。`horizon:status` 的退出码和文本类别使用同一份 Master 心跳。真实 Redis 回归覆盖多主机与缺主机字段选择、状态类别和无跨主机写入，现有 Unix 进程矩阵验证本机信号。

固定 `RedisMasterSupervisorRepository::names` 读取 14 秒窗口内全部 Master，并以最近心跳为先。`horizon:status` 与 `horizon:list` 均使用该仓储契约，Rust 修复也改走已有的无条数上限的 Master 仓储。新增用例登记 102 条心跳，确认唯一暂停节点仍影响退出码，JSON 和文本列表均可见。 [固定 Master 仓储](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Repositories/RedisMasterSupervisorRepository.php)

macOS 初轮独占 Redis 8.10.3 的主机选择、状态类别和无跨主机写入用例、Horizon 严格 Clippy 以及原生进程矩阵均通过。矩阵覆盖监督器恢复、重试及忘记、暂停、继续、正常终止、面板授权与脱敏。三语文档构建及 12 组浏览器检查通过。初轮证据为 `target/tmp/observability-parity/horizon-local-supervisor-{macos,docs}.log`。包含 102 条心跳的 Mac Redis 回归和相同源码的 Horizon 库及运行时目标严格 Clippy 通过，证据为 `target/tmp/observability-parity/horizon-pulse-lifecycle-macos.log`。Linux ARM64 的初轮同样通过主机选择、Redis 原生模型标签与失败重试，以及 6 项标签／通知测试，4 项按条件忽略。精确快照重新编译 Horizon 后，Linux 7.0.14 ARM64 与 Redis 8.10.3 的 102 条心跳回归通过。Linux 证据为 `target/tmp/observability-parity/{horizon-local-supervisor-linux,horizon-master-heartbeats-linux}.log`。其他目标平台和最终部署组合仍待验证。

固定上游 [Master 控制命令](horizon/README.md#master-控制命令的主机范围) 和 [Pulse 慢任务计时](pulse/README.md#slowjobs-任务尝试计时) 已保留真实消费者对照证据。Horizon 的本机控制通过 macOS 原生进程、Linux ARM64 双容器 PID 1 和 Windows ARM64 空部署计划验收。默认命名及自定义前缀已接通同机多组与双容器的 Artisan 控制，剩余名称输入边界见[命名审计](horizon/README.md#master-名称与解析器)。同名启动、接管与 Master 排空续期通过真实 Redis 验收。受管 Supervisor 元数据及 Worker 领取入口已加入所有权校验，并通过接管回归。 [所有权审计](horizon/README.md#supervisor-与-worker-接管) Pulse 已在原生 SQLite Worker 上修复并验收。两包正式部署组合继续核对。

Redis 中已阻塞的 Horizon Worker 会在被唤醒后、再次请求领取前复核所有权。真实 Worker 接管回归确认任务留在待处理列表，尝试次数保持为零，并由新持有者通过 Artisan 执行。Redis 主从切换和可能已经发出的领取命令仍待验收。 [阻塞接管证据](horizon/README.md#supervisor-与-worker-接管)

Horizon 的 Master 名称验证已接通空前缀、Unicode、冒号及超过原限制的长名称，使用完整父名称聚合子进程心跳。Windows ARM64 的受管子树现已通过真实 Redis 的启动、暂停续跑、排空、快速终止和父进程丢失回归。Windows x86_64、优化构建及正式服务管理器组合仍待验。固定 PHP 包依赖 `ext-pcntl` 和 `ext-posix`。 [Windows 进程树证据](horizon/README.md#windows-原生进程树) [固定 Horizon Composer 依赖](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/composer.json)

Horizon 增加 Master 命令队列和动态 `AddSupervisor` 消费。真实 Redis 和运行中的 Master 验证启动时清除旧命令、暂停后恢复部署、启动失败后的恢复，以及新 Worker 处理实际任务。 [Master 命令队列](horizon/README.md#master-命令队列与动态部署)

默认 Artisan 部署与公开 `ProvisioningPlan::deploy` 共用结构化队列及 `AddSupervisor` 消费。保留首个通配环境的选择、递归默认值、停用项过滤和空部署事件契约。 [ProvisioningPlan 审计](horizon/README.md#provisioningplan-与默认部署)

Supervisor 的结构化命令已接入同一 Redis 队列服务，覆盖暂停、继续、重启、整组扩缩容、分池调整与带状态码的排空终止。 [Supervisor 命令审计](horizon/README.md#supervisor-结构化命令)

Horizon 增加 Master 公开重启。恢复暂停后，原 Master 按之前的部署选项替换 Supervisor 与 Worker，已开始的任务继续执行。 [Master 公开重启审计](horizon/README.md#master-公开重启)

Horizon 的 Master 和 Supervisor 增加公开输出回调，Artisan 与文件监听命令接通子进程的实时输出和控制台标签格式。使用真实任务核对 stdout、stderr、静默模式和文件重启前后的输出。WorkerProcess 和 SupervisorProcess 接通独立输出注册及后续启动。 [输出回调审计](horizon/README.md#master-与-supervisor-输出回调) [进程包装器回调](horizon/README.md#workerprocess-与-supervisorprocess-输出回调)

Horizon 的结构化命令可按名称解析应用容器中的 Master 或 Supervisor 处理器。真实 Artisan 进程验证自定义部署、同名命令覆盖、选项传递、排空退出码和应用隔离。 [命令处理器审计](horizon/README.md#应用容器中的命令处理器)

Master 和 Supervisor 已接通容器中可替换的命令队列。验收覆盖独立的 Redis 键空间、文件后端、启动清理、真实任务、部署、重启和领取期间的同名接管。 [可替换队列审计](horizon/README.md#可替换的命令队列)

WorkerProcess 与 SupervisorProcess 的公开控制接通原生信号、排空和终止状态。独立 Worker 的监控与受管路径共用冷却及补位逻辑。 [进程包装器控制审计](horizon/README.md#workerprocess-与-supervisorprocess-控制)

独立 `SupervisorProcess::monitor` 已接通默认及自定义退出码判定、冷却、旧心跳清理及基于原选项的重部署。退出码 13 始终立即结束监控。 [独立监控审计](horizon/README.md#supervisorprocess-独立监控与重部署)

### Horizon 分组等待时间与 HTTP 排序

固定上游 [RedisWorkloadRepository::get](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Repositories/RedisWorkloadRepository.php) 与 [WaitTimeCalculator::calculateTimeToClear](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/WaitTimeCalculator.php) 分别确定各分队列先取整再累加、整组以原始总量独立取整的行为。[WorkloadController::index](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Http/Controllers/WorkloadController.php) 对 HTTP 结果按名称排序，仓储按等待时间降序供监控消费。Rust 已补齐两处规则。

隔离 Redis 8.10.3 上通过授权 HTTP 写入真实待处理与延迟任务、队列运行时指标以及两个 Supervisor 的进程数。两个分队列各 0.6 秒时，分项得到 1、2 秒，整组为 1 秒。零进程单队列仍按原始工作量得到 2 秒，延迟任务不计入 Ready；HTTP 中该单队列按名称排在分组之后。全量 Horizon `runtime` 9 项、快照全仓格式及 Horizon 全目标严格 Clippy 均通过。快照沿用 `8041ae15a` 加前批与本批文件。

### Horizon 分组队列的去重与 JSON 结构

固定 `RedisWorkloadRepository::get` 的 `mapWithKeys` 保留首次出现顺序。重复队列在 Ready 数和分项只计一次，整组估算仍按 `WaitTimeCalculator` 逐次累加。以 PHP 8.5.12 实测键规则：`0,1` 产生数组，`1,0` 和普通名称产生对象，`01` 保留字符串，`-1` 成为整数行名。Rust 的负载仓储与授权 HTTP 已同步这些行为，`split_queues` 由固定数组改为按键决定对象或数组。单队列仍返回 `null`。三语文档说明调用方可观察的变化。

沿用隔离快照及 Redis 8.10.3。真实 HTTP 测试覆盖数字与文本键、有序分项、重复后仅一项的分组、独立单队列、Ready／延迟边界、两 Supervisor 的进程数及整组等待时间。Horizon `runtime` 9 项、全仓格式、Horizon 全目标严格 Clippy 和三语文档构建及 12 组浏览器检查均通过。

原生面板仍需补上分组负载展示。另需核对独立数字队列名的 HTTP 排序、同等等待时间的仓储次序，以及等待通知的进程内节流与跨进程锁。三个扩展完整对齐与发布验收继续开放。

### Horizon 等待监控的本地冷却与 Redis 锁

固定 [MonitorWaitTimes](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/MonitorWaitTimes.php) 先检查上一次成功抢锁的时间，满一分钟才重新尝试。未取得锁不启动本地冷却，成功抢锁后在收集负载或派发事件前更新时间。Rust 在每个 Master 实例内使用单调时钟记录成功时间，保留跨 Master 共享的原子 Redis 锁和 60 秒有效期。这样 Redis 锁提前消失时，同一 Master 仍遵守本地间隔。

构建守卫在前置检查持锁清理后正常恢复构建。可用空间从 32.108 GB（29.90 GiB）增至 150.403 GB（140.07 GiB），净释放 118.295 GB（110.17 GiB）。源码快照、验收记录与数据库数据保留。

隔离 Redis 8.10.3 和真实 Artisan Master 进程验收通过。先让另一个持有者占锁，移除该锁后 Master 在 8 秒内获取。随后提前移除 Master 自己取得的锁，跨过两个循环仍无重新加锁。完整进程验收、快照全仓格式、Horizon 全目标严格 Clippy 均通过。60 秒届满后的真正重新取锁未单独等待实测，本批覆盖冷却期间与抢锁失败两条回归路径。

下一项通知对账已定位到 [LongWaitDetected 事件](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Events/LongWaitDetected.php) 与 [通知实现](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Notifications/LongWaitDetected.php)。上游经容器创建可替换的通知；邮件主题、短信正文和 Slack 附件会读取 `horizon.name`，Slack 有 webhook 与频道 ID 两种路径。本地事件和通知共用类型。本批改动前文案未读取配置名称，Slack 使用简化 Webhook 路径。

### Horizon 长等待告警文案与 Slack 渠道

补齐邮件主题、短信正文、Webhook 附件及 Bot API Block Kit 对部署名称的读取。上游缺省名称在邮件主题产生空前缀，短信和 Slack 详情保留空方括号，Rust 同样处理。现有 `Horizon::route_slack_notifications_to` 接受 Webhook URL 或频道 ID，复用原生 Slack 渠道。品牌采用 OpenRust Horizon，未引用 Laravel 的托管图标。

隔离 Redis 8.10.3 上 Horizon 标签与通知测试 7 项、全仓格式及 Horizon 全目标严格 Clippy 通过。真实应用请求作用域内检查邮件与短信消息，通过原生通知渠道投递到 HTTP fake，断言 Webhook 附件、Bot API 授权头、频道 ID 和 Block Kit。三语文档构建和 12 组浏览器检查通过。第三方收件与实际服务投递未实测。可替换通知工厂、Webhook 可选频道覆盖及通知监听器的公开契约仍待处理。

### Horizon Slack Webhook 可选频道

固定 [Horizon::routeSlackNotificationsTo](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Horizon.php) 的第二个参数会进入旧式 Webhook 请求的 `channel` 字段。Rust 保留单参数入口并增加双参数 `route_slack_webhook_notifications_to(url, channel)`。框架 `SlackRoute::for_webhook` 将目标地址和可选频道一并交给原生渠道，原生渠道在构建请求时应用覆盖。提供频道仍走 Webhook 路径，Bot API 的地址、令牌与频道优先级沿用现有契约。空频道按旧式 Slack 消息的空值规则省略。

本批隔离 Redis 8.10.3 标签与通知回归 8 项通过。HTTP fake 捕获普通 Webhook、覆盖频道的 Webhook、空频道与 Bot API 出站请求，核对地址、授权头、频道及附件。真实 Master 循环读取 Redis 队列和运行时指标，通过公开 Horizon 注册入口配置的 Webhook 发出带覆盖频道的请求。第三方收件未实测。全仓格式、框架库和 Horizon 全目标严格 Clippy、三语文档构建及 12 组浏览器检查通过。[Slack 应用的传入 Webhook 固定使用安装时选定的频道](https://api.slack.com/messaging/webhooks)。可替换通知工厂及通知监听器的公开契约继续待处理。

### Horizon 长等待事件的通知工厂与监听器

固定 [LongWaitDetected::toNotification](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Events/LongWaitDetected.php) 经容器构造通知，[SendNotification](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/SendNotification.php) 在投递前按通知签名加 300 秒锁。Rust 的 `LongWaitDetected::to_notification` 解析应用容器的 `dyn LongWaitNotificationFactory`，并返回实现 `LongWaitDetectedNotification` 的通知。Provider 注册同步 `SendNotification` 监听器。等待监控和其他公开派发入口走同一监听器。

默认通知签名保留现有 Rust 的 `connection:queue`，保障已有通知锁和 `Repository::notification_lock` 调用方兼容。固定 PHP 实现对连接名与队列名直接拼接再取 MD5。Rust 自定义通知可以提供自己的签名。已发现固定 [EventMap](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/EventMap.php) 将等待监控调度接在 Supervisor 循环。本地监控触发仍在 Master 循环，生命周期入口与故障切换须再对账。

隔离 Redis 8.10.3 上标签与通知 9 项通过。应用 Provider 在默认 Horizon Provider 之前绑定自定义工厂，真实请求作用域中调用 `to_notification` 并派发两个不同队列的事件，原生通知 fake 只记录共享签名的第一条。默认工厂同时通过 Master 主循环到 Slack HTTP fake 的回归。快照全仓格式、Horizon 全目标严格 Clippy、三语构建及 12 组浏览器检查均通过。第三方实际收件仍未实测。

### Horizon 等待监控的 Supervisor 生命周期

固定 [Supervisor::loop](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Supervisor.php) 在持久化状态后派发 `SupervisorLooped`，由 `MonitorWaitTimes` 处理；暂停 Supervisor 仍走持久化与派发。本地将等待监控、成功取锁的进程内时钟以及监控失败后的日志移到 Supervisor 状态持久化之后。Master 不再尝试监控锁，Redis 锁继续协调不同 Supervisor。Rust 当前在 Supervisor 循环直接调用监控，公开 `SupervisorLooped` 事件及可注册的内置监听器仍待与其他进程生命周期事件一起实现。

构建守卫内沿用隔离快照与 Redis 8.10.3。标签与通知 9 项、快照全仓格式、Horizon 全目标严格 Clippy 和独立 Artisan 进程验收通过。同一组超阈值数据下，先运行无子进程的 Master 并确认状态已持久化，HTTP fake 没有告警；随后运行暂停 Supervisor，实际取得锁并发出配置的 Slack Webhook 请求。独立进程回归覆盖锁竞争失败后的重试与抢锁成功后的冷却。第三方收件、其他平台和公开生命周期事件仍未验收。

### Horizon SupervisorLooped 公开事件

固定 [SupervisorLooped](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Events/SupervisorLooped.php) 和 [EventMap](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/EventMap.php) 的派发点及等待监控注册已接通。Rust 事件公开 Supervisor 名称及暂停状态，应用可经仓储读取该循环已经持久化的记录。原生异步事件派发传递拥有所有权的数据，运行中的 Supervisor 进程池仍由 Supervisor 持有。内置 `MonitorWaitTimes` 在 Provider 同步注册，沿用每个 Supervisor 的冷却状态及跨进程 Redis 锁。取得锁并标记之后才查询负载并派发通知，应用监听器报错经日志记录并保留后续循环。

隔离 Redis 8.10.3 上标签与通知 9 项、独立 Artisan 进程验收、快照全仓格式、Horizon 全目标严格 Clippy、三语文档构建和 12 组浏览器检查通过。应用监听器读取同一循环已持久化的暂停心跳，首次返回错误后收到下一循环事件；内置监听器只发出一条 Slack HTTP fake 请求。其他进程生命周期事件、各自的内置监听器与生产平台矩阵继续待逐项对账。

### Horizon MasterSupervisorLooped 与过期进程索引

固定 [MasterSupervisor::loop](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/MasterSupervisor.php) 在持久化之后派发 `MasterSupervisorLooped`，暂停状态同样派发。Rust 公开事件名称、Master 名称与暂停状态，运行句柄归 Master 持有。同步监听器报错会记录日志并继续循环。

Provider 按固定 [ExpireSupervisors](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/ExpireSupervisors.php) 注册内置监听器，复用两个原生仓储的 `flush_expired`，以 Redis 时间清理 14 秒以上未更新的 Master 与 Supervisor 排序索引。当前仍需逐项对齐 Master 的三类周期清理监听器，以及内存事件和终止时机。

隔离 Redis 8.10.3 运行时和通知 18 项通过。真实 Master 循环回归验证应用监听器能读取该轮已持久化的暂停状态，首次返回错误后仍收到下一循环事件，两个 Redis 索引的过期成员均被清除。快照全仓格式、Horizon 全目标严格 Clippy、独立 Artisan 进程、三语文档构建及 12 组浏览器检查通过。

### Horizon 周期任务索引清理

固定上游 [Master 事件映射](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/EventMap.php) 注册三类任务清理。Rust 公开并同步注册 `TrimRecentJobs`、`TrimFailedJobs` 和 `TrimMonitoredJobs`，复用 `JobRepository` 现有的 Redis 时钟与保留期计算。每类在首次循环检查，执行成功后才开始自己的间隔。最近索引间隔 60 秒，失败及监控索引各自以保留分钟数除以 12 向下取整，最少一分钟。间隔在该 Master 第一次调用时确定，运行时状态不跨 Master 共用。

隔离 Redis 8.10.3 上，真实 Master 首轮清除七个索引中过期项目并保留新项目，测试监听器在首轮结束时写入的过期项目在紧接的循环中未被提前清除。本批未等待所有实际配置的完整周期。运行时与通知 18 项、独立 Artisan 进程回归、快照全仓格式、Horizon 全目标严格 Clippy、三语文档构建及 12 组浏览器检查通过。

### Horizon Master 与 Supervisor 内存超限事件

固定 [MonitorMasterSupervisorMemory](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/MonitorMasterSupervisorMemory.php) 与 [MonitorSupervisorMemory](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/MonitorSupervisorMemory.php) 均在进程持久化后的循环阶段检查，并在退出码 12 之前派发各自事件。Rust 将检查移到心跳持久化和循环事件之后，补齐 `MasterSupervisorOutOfMemory` 与 `SupervisorOutOfMemory`。事件保存进程名称、实测常驻内存及上限，Supervisor 保留读取与覆写记录用量的接口。

本批内存保护直接由原生循环调用，公开内存监控监听器和对应装配仍待补齐。监听器报告错误时仍执行 native 终止与清理，以维持内存保护边界。与 PHP 分配器统计的差异沿用项目的常驻内存定义，其他平台的内存测量和终止仍待实测。

构建守卫内隔离 Redis 8.10.3 与独立 Artisan 进程通过。Master 和 Supervisor 的上限各设为 1 MiB，应用监听器分别记录同名事件、进程名称及超限的实测用量，随后故意返回错误，两条命令仍以 12 退出且无残留活动进程登记。快照全仓格式、Horizon 全目标严格 Clippy、三语文档构建和 12 组浏览器检查通过。

### Horizon 公开内存监控监听器

固定 `MonitorMasterSupervisorMemory` 与 `MonitorSupervisorMemory` 在对应循环事件上的同步注册已补齐。Supervisor 先判断是否超限，再执行等待监控。每个循环事件与进程共享一次检测结果；内置监听器触发超限事件后停止后续循环监听器，进程读取结果并清理退出。如果更早的应用监听器中断派发，原生循环仍使用同一个检查入口完成保护。检测与事件派发之间不持有同步锁。此处保留 Rust 的强制内存保护，避免应用监听器错误绕过退出码 12。

隔离 Redis 8.10.3 上通过运行时与通知共 18 项和独立 Artisan 进程验收。Master 与 Supervisor 分别在正常注册顺序、应用监听器在前且主动报错两种情形下运行；四次超限各收到一个事件并以 12 退出，提前报错的两类循环事件有单独记录。快照全仓格式、Horizon 全目标严格 Clippy、三语文档构建及 12 组浏览器检查通过。生产平台矩阵与其他生命周期事件仍需验收。

### Pulse CacheInteractions 缓存事件

通过固定上游的缓存仓储和 Rust 的应用 HTTP 路由，核对缓存命中和未命中的事件时间、值、第一条匹配的分组、忽略及延迟采样。macOS 经 file、database、Redis 缓存和 SQLite、PostgreSQL、MySQL 存储的九种组合核对实存字段与计数。Linux ARM64 和 Windows 11 ARM64 在 SQLite 上复验三种缓存。 [对照与证据](pulse/README.md#cacheinteractions-缓存事件对照)

### Pulse 命名更新请求的页面归因

固定 [LivewireRoutes](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/Concerns/LivewireRoutes.php)、[SlowRequests](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/SlowRequests.php) 和 [UserRequests](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/UserRequests.php) 在采样后检查 `*livewire.update`。两个原生记录器现使用已解码输入的 `components[0].snapshot.memo.path` 归因并应用过滤，慢请求记录在第三项保留原始路由的 `via`。未提供快照路径时继续使用常规路由。已认证用户键 `0` 计入普通用户请求，按上游规则不产生慢用户计数。

隔离快照从真实应用 HTTP、认证和 SQLite 原生存储验证命名更新、路径规范化、忽略路径、坏快照、缺少路径、未提供已解码快照、普通路由及用户键 `0`。首次用普通 JSON 请求模拟未解码输入时，框架已提前解码；改用不透明媒体类型后验证了正确的回退。Provider 10 项、快照全仓格式、Pulse 全目标严格 Clippy、三语文档构建及 12 组浏览器检查通过。

当前使用原生 `RequestHandled` 已有快照，观察阶段不主动解码不透明的请求体。未运行 Laravel Livewire 客户端，PHP 请求终止回调与 Rust HTTP 事件的计时边界、异常请求与其他 Recorder 仍需独立对账。

### Pulse 请求终止后的完整生命周期计时

固定 [Laravel HTTP Kernel::terminate](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Http/Kernel.php) 在终止中间件和应用终止回调结束后，才使用同一次结束时间调用请求生命周期处理器。Pulse 的两个请求 Recorder 都使用该入口。Rust HTTP 内核新增 `after_termination`，以同一个单调时钟耗时通知已注册回调，覆盖所有响应状态，防止同一请求的重复终止提前触发观察。原有 `RequestHandled` 的派发点保留；Pulse 在该事件中捕获认证身份和已解码的路由数据，实际记录与归档移到终止之后。手工调用记录器且无活跃 HTTP 生命周期时，仍使用事件本身的耗时。

通过真实应用内核回归：422 响应前认证用户，响应后由终止中间件等待 25ms、始终执行的延后任务及其嵌套任务各等待 45ms，慢请求达到至少 115ms 并持久化普通及慢用户计数。Pulse Provider 11 项、框架 HTTP 中间件生命周期 5 项（含响应流与关停）、快照全仓格式、Pulse 全目标与框架库严格 Clippy、三语构建及 12 组浏览器检查通过。请求级回归使用 SQLite；最终生产组合、被中断的传输与外部 Livewire 客户端仍需验收。

### Pulse 响应传输中断验收

从独立 Artisan 启动真实 HTTP 服务，以 SQLite 存储和原生请求认证发送请求。夹具返回持续打开的分块流，客户端读到首段后继续连接至少 150ms；在流仍打开时没有请求指标。客户端提前关闭 TCP 后，数据库收到一条不少于 150ms 的慢请求、一个普通用户计数和一个慢用户计数，用户 ID 均为 42。夹具首次发现未显式启用普通用户 Recorder，补齐测试配置后通过；生产配置与采集实现无需改动。使用内核分配的端口，测试结束关闭子进程。相同快照的 Provider 11 项、全仓格式和 Pulse 全目标严格 Clippy 通过。仍需最终生产组合、其他平台及外部 Livewire 客户端验收。

### Pulse 慢出站请求的公开记录入口

固定 [SlowOutgoingRequests](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/SlowOutgoingRequests.php) 在完成和异常两条路径记请求，用完成时刻作为条目时间戳，并在采样、忽略、阈值及分组前去除 URL 用户凭据。Rust 的原生 HTTP 中间件和公开 `SlowOutgoingRequests::record_outgoing` 共用记录与脱敏入口，已有 `Recorders::outgoing` 也经同一规范化函数。公开 `normalize_url` 接受有效绝对 URL，无遮蔽需求时保留原拼写；校验错误不回显地址。Native 传输使用单调时钟计时，保留已有完成时刻时间戳。

真实回环 HTTP 覆盖先返回 500 后重试成功、已接收请求后断开以及带用户凭据的直接记录。SQLite 应用数据库中两次重试分别按脱敏地址分组，断开单列一条，直接记录保留查询字符串和明确的 25ms 耗时。Provider 12 项、快照全仓格式、Pulse 全目标严格 Clippy、三语文档构建及 12 组浏览器检查通过。PHP Guzzle 重定向统计、跨协议 URI 细节及生产组合仍需对账。

### Pulse 队列任务显示名

固定 Pulse 的 `Queues` 和 `UserJobs` 在入队时读取任务对象提供的 `displayName()`，`Queues` 的 Worker 事件及 `SlowJobs` 读取 `Job::resolveName()`。PHP 8.5.12 用同一处理器与两种显示名派发入队、开始和结束事件，三类 Recorder 均排除了被显示名规则忽略的任务。保留任务写入 `queued`、`processing`、`processed`、`user_job`，慢任务条目按显示名记录 7 毫秒。证据为 `target/tmp/observability-parity/pulse-job-display-name-upstream.log`。 [固定 Queues](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/Queues.php) [固定 UserJobs](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/UserJobs.php) [固定 SlowJobs](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/SlowJobs.php)

Rust 公开复用 `Payload::display_name()`，未设置显示名的载荷使用处理器注册名。`Queues`、`UserJobs` 与 `SlowJobs` 按显示名匹配忽略规则，慢任务阈值和聚合键也沿用显示名。真实 HTTP 夹具发现数据库队列虽然成功落库却未派发 `JobQueued`，造成入队及用户计数缺失。补齐每次成功写入后的事件，携带连接、队列、运输 ID 和载荷，立即或延迟投递复用同一入口。 [固定 Queue::enqueueUsing](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Queue.php)

带认证的 HTTP 入口通过 `Artisan::queue` 写入两个共享处理器的不同命令名。真实数据库 Worker 消费两个任务，Pulse 只保留一个入队、处理中、完成和用户计数，慢任务按保留命令名分组。macOS 的 SQLite、PostgreSQL 和 MySQL 专项均通过。SQL Server 发布组合待验。外部队列见下节。

macOS 同源码通过三包默认 88 项、Pulse 全目标严格 Clippy、全仓格式及三语页面 12 组浏览器检查。框架原生 HTTP 数据库队列回归分别在 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 通过，覆盖延迟、事务内派发和标准 Worker 入口。证据为 `target/tmp/observability-parity/pulse-job-display-name-{baseline,focused,verified,docs,core}.log`。

Linux 7.0.14 ARM64 与 Windows 11 ARM64 用 `ee28c3a27` 的变更源码和 Rust 1.94.0，通过已认证 HTTP、SQLite 真库、原生 Artisan 入队及数据库 Worker 的显示名专项。证据为 `target/tmp/observability-parity/pulse-job-display-name-linux.log` 与 `target/tmp/observability-windows/pulse-job-display-name-arm64.log`。

### Pulse 外部队列成功入队事件

固定 Laravel `BeanstalkdQueue` 与 `SqsQueue` 在成功发送后派发 `JobQueued`。SQS 批次逐项关联请求 ID 与响应消息 ID，先派发本块已成功的消息，再返回拒绝错误并停止后续分块。当前 Rust 基线的批次写入使用 SQL 事务，部分发送失败时回滚批次行，已被队列接收的消息仍留在运输端。此前的发布账本和自动恢复记录属于不同基线，本批沿用当前事务与队列行为。 [固定 SqsQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/SqsQueue.php) [固定 BeanstalkdQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/BeanstalkdQueue.php)

Rust 在 Beanstalkd 取得已接收的任务 ID，以及 SQS 取得单条或批次的消息 ID 后，公开连接、队列、运输 ID 和原始载荷。链式后继复用相同确认路径，SQS 保留发送时配置的 FIFO 去重。Rust 沿用现有确认后观察机制，把监听器失败记入日志并保留运输端结果。SQS HTTP 200 部分拒绝的回环协议测试验证了首项监听器报错时，第三项仍被观测，实际返回第二项的拒绝错误。Beanstalkd 1.13 和 ElasticMQ 1.6.15 专用回环实例分别通过真实入队、Pulse 显示名筛选、认证用户计数、Artisan 消费及链式后继计数。托管 AWS 不在本轮实测范围。

macOS 同源码通过核心 Cloud 16 项、三包默认 88 项、全仓格式与修改范围的严格 Clippy。链式验收由 HTTP 请求派发首项并在请求结束后落库，两种运输均在 Worker 发送后继后增加一次成功计数。证据为 `target/tmp/observability-parity/pulse-external-queue-{verified,brokers-verified,sqs,regression}.log`。

Linux 7.0.14 ARM64 与 Windows 11 ARM64 用 `f14c6a340` 的变更源码通过 SQS 回环部分确认专项。Linux 首次因项目磁盘停线中止，在确认容器退出且空间恢复后冷构建通过。Windows 既有 Unix 测试夹具辅助方法产生未使用警告。证据为 `target/tmp/observability-parity/pulse-external-queue-linux-cold.log` 与 `target/tmp/observability-windows/pulse-external-queue-arm64.log`。真实 Broker 验收的平台为 macOS。

### Telescope 出站 multipart 构建器快照

真实回环 HTTP 与 SQLite 应用仓储覆盖上传成功、连接被拒、脱敏、原生 `Part` 兼容及两种重复字段覆盖。服务端检查附件字节、文件名和上传顺序。框架回归验证未知长度流到发送阶段才被读取，正文流只执行一次，快照长度为空。验证使用 `8041ae15a` 加前批和本批文件的隔离快照。Telescope 32 项、框架 HTTP 客户端 20 项、快照全仓格式、框架库及 Telescope 全目标严格 Clippy、三语站点及 12 组浏览器检查通过。Telescope 独立 Redis 用例本批未执行；出站 multipart 未遍历部署数据库或目标平台。保留原生 `Part`、预格式化输入与中间件替换流的差异，见[包审计](telescope/README.md#出站-multipart)。

PHP 8.5.12 运行固定 Laravel HTTP Client 和 Telescope `ClientRequestWatcher::input`，以 Guzzle MockHandler 检查线上请求。跨源 302 第二跳以空正文 GET 发送，保留原 multipart 媒体类型，响应事件仍从原始参数给出文件名、大小、部件头部和表单字段。可回卷的 PHP 文件流在 307 和 422 重试时重新发送；重试回调更新请求头，当前请求继续使用首次解析的附件列表。发送回调替换正文时诊断仍显示原始参数。证据为 `target/tmp/observability-parity/multipart-combinations-upstream.log`。Rust 原先从末跳线上正文判断是否提供 multipart 快照，导致 302 成功条目缺失原始参数。发送和成功响应事件沿用原始 multipart 参数，线上发送快照仍按实际请求生成。清空正文后的媒体类型和一次性流边界继续核对。

固定 reqwest 0.13.5 将所有非空 multipart 编码成不可复制的正文流，即使每个部件都已缓冲。Rust 复用现有本地 reqwest 副本及请求复制路径，保存可复制的部件、原 boundary 和编码方式，每次传输独立生成流，缓冲附件共享字节。一次性部件沿用原行为。旧版直接使用 reqwest 表单，未提供这类重放。真实回环用例覆盖 307、308、422 重试与头部改写，并逐字节比较两次正文；仍需验收可重开的文件和自定义流。初次验收在 32 GB 停线终止；第二次编译发现测试引用了框架未直接声明的 bytes crate，改用 Axum 已导出的同一类型后通过。 [固定 reqwest 请求复制契约](https://docs.rs/reqwest/0.13.5/reqwest/struct.Request.html#method.try_clone)

固定 PHP 组合的实际回环服务器确认，302 后目标收到 `GET` 及原 multipart 媒体类型，正文长度为零，未发送原 Content-Length 或 Transfer-Encoding；307、308 后收到 `POST` 和完整附件。原 Rust 响应事件随空正文丢弃媒体类型，修复和验收见下段。另用不可回卷的 Guzzle 流确认，422 后上游仍尝试发送，但第二次请求的附件内容已丢失。Rust 对一次性流保留明确的不可重放错误；可回卷文件对应的 Rust 入口继续处理。证据为 `target/tmp/observability-parity/multipart-{redirect-wire-upstream,combinations-extended-upstream}.log`。

媒体类型专项在固定 PHP 组合的 63 组方法与状态中确认 `text/plain` 均保留。Rust 清空正文时移除旧的 Content-Length 与 Transfer-Encoding，保留 Content-Type。锚定测试扩充 63 组真实回环和 Telescope 在 302 后的表单、multipart 和脱敏条目。macOS 隔离快照的框架客户端 23 项、Telescope Provider 15 项、SQLite／PostgreSQL 18.7／MySQL 8.4.13 定向回归与严格 Clippy 均通过。独立 Redis 用例按环境条件跳过。证据为 `target/tmp/observability-parity/{redirect-media-upstream,multipart-redirect-media-macos}.log`。

在基于 `177b24933` 的 macOS 隔离快照上，框架 HTTP 客户端 23 项、Telescope Provider 15 项和所涉严格 Clippy 均通过。一个需要独占 Redis 的 Provider 用例未执行。真实回环与 Telescope 仓储验证跨源 302 原始表单参数与完全脱敏、307 重放、422 后重试头和两次条目，在 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各通过。核心另验证 308、原生 Part 与一次性流拒绝重试。三语内容检查通过。证据为 `target/tmp/observability-parity/multipart-replay-macos-retry.log`。目标平台复验及生产部署组合仍待补齐。

常规文件补充专项。固定 PHP 与真实回环服务器使用已打开的 84,000 字节常规文件，302 清空正文，307 和 308 仍传输完整文件，见 `target/tmp/observability-parity/multipart-file-redirect-wire-upstream.log`。Rust 新增 `PendingRequest::attach_file`，采用同一已打开文件句柄及每个尝试独立的逻辑偏移，分块读至登记长度。原生 reqwest `Part::file` 复用该路径。框架回归按字节核对混合缓冲和文件部件，测试 422 后替换原路径时仍使用原句柄，并检查缺失路径与目录失败。Telescope HTTP 回归核对 307 两跳文件内容以及仓储中的文件名、长度和部件媒体类型。macOS 在隔离快照上通过框架 HTTP 客户端 23 项、Telescope Provider 15 项、三个数据库上的定向入库与脱敏回归，以及所涉严格 Clippy。Provider 的真实 Redis 用例另行验收。三语内容检查通过。 [Tokio AsyncSeekExt](https://docs.rs/tokio/1.54.1/tokio/io/trait.AsyncSeekExt.html)

首次 Rust 编译触及 32 GB 停线，守卫退出并持锁释放 82,869,366,784 字节。在同一隔离源码重新编译并验收通过，证据分别为 `target/tmp/observability-parity/multipart-file-macos.log` 和 `target/tmp/observability-parity/multipart-file-macos-retry.log`。Linux ARM64 与 Windows 11 ARM64 的定向复验已通过。

Linux 7.0.14 ARM64 在 Rust 1.94.0 的隔离容器中通过原生组合 82 项，5 项按环境条件忽略。实跑包含框架 HTTP 客户端、文件重放与 SQLite 入库、Telescope 分块及逐次消费来源、Pulse Artisan 调度及原生显式报告、Horizon 标量模型标签。首次离线解析缺少翻译包的锁定依赖，取回对应目标依赖后以离线方式完成编译与测试。证据为 `target/tmp/observability-parity/observability-file-linux-retry.log`。

Windows 11 ARM64 用 MSVC 工具链实测文件重放和真实 Telescope SQLite 入库均通过。入库夹具用两个明确的回环端口构成跨源跳转，并等待应用和线上服务同时完成。原五秒限时不足以容纳三次真实关闭端口连接，Windows 独立探针测得 2,064、2,025、2,025 毫秒，均返回 ConnectionRefused。修正总限时为 15 秒，定向用例实际 6.64 秒通过，保留所有失败与脱敏断言。证据为 `target/tmp/observability-windows/{refused-socket-timing,multipart-file-refused-timing-arm64}.log`。

### Telescope 非 multipart 出站诊断载荷

在 PHP 8.5.12 容器里运行固定 [Laravel PendingRequest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/PendingRequest.php) 和 [Request](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/Request.php)，用 Guzzle MockHandler 观察真实 `beforeSending` 回调。9 组用例确认 JSON／表单正文不会混入 URL 查询，普通 GET 使用传入地址的 query，`withQueryParameters` 修改线上地址，HEAD 及不透明原始正文的载荷为空，原始 JSON 可以解码，JSON 构建器覆盖媒体类型仍保留结构化正文。参考日志为 `target/tmp/observability-parity/diagnostic-payload-upstream.log`。

Telescope 改用 `RecordedRequest::diagnostic_data()`，失败事件复用同一载荷选择，完全脱敏时清除诊断查询与正文。PHP 另行验证表单源数据 `{"flag":false,"count":2}` 原样保留，Rust 目前从线上缓冲表单解码得到字符串。表单原始类型、重定向和中间件修改行为仍需对账。公开结构体新增 `diagnostic_source`，完整字面量需补齐字段。

隔离快照沿用 `8041ae15a` 加前批和本批文件。真实回环 HTTP 和 SQLite 仓储对照线上 URL、JSON／表单、HEAD、原始正文、媒体类型覆盖、连接被拒和完全脱敏。Telescope 32 项、框架 HTTP 客户端 20 项、快照全仓格式、框架库及 Telescope 全目标严格 Clippy、三语站点及 12 组浏览器检查通过。Telescope 独立 Redis 用例本批未执行。

### Telescope 表单标量与原始字段名

固定 Laravel 参考以 `beforeSending` 观察到 `{"flag":false,"count":2}`。核心在诊断或测试记录启用时用 [serde-save 0.1.1](https://docs.rs/serde-save/0.1.1/serde_save/) 捕获一次应用提供的 Serde 数据。该库保留标量类型、结构和重复键顺序，复用 reqwest 的表单编码产生网络正文。Rust 序列对可以发送同名字段，诊断映射保留最后一项，与 PHP 关联数组覆盖规则对应。发送前替换正文时，记录的实际请求从新正文解码，字段类型无法从 URL 编码恢复。

固定 Telescope 的 `hideParameters` 在原始点号字段和嵌套路径同名时会保留原始字段值，PHP 复现实测见 `target/tmp/observability-parity/typed-form-upstream.log`。OpenRust 同时遮盖已登记的原始字段和嵌套路径，防止新快照写入未脱敏值。此处列为有意行为差异。

旧版 HTTP 客户端直接调用 reqwest 表单编码，未提供类型快照。对比以 JSON 或排序映射作中间值的方案时，发现会丢失重复键、顺序或原始标量编码，故依赖锁只增加捕获库及框架引用。仍须逐项核对重定向、修改请求的中间件和诊断覆盖。

真实回环 HTTP 回归在 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各通过，验证线上表单顺序、实际入库的布尔值和数字、原样字段名和嵌套字段遮盖。框架 HTTP 客户端 20 项验证单次序列化、重复字段和替换正文后的快照，Telescope 全包 32 项及独立 Redis 8.10.3 用例通过。快照全仓格式、框架库及 Telescope 全目标严格 Clippy、三语文档构建及 12 组浏览器检查通过。快照沿用 `8041ae15a` 加前批和本批文件，根工作区另用锁定依赖树验证新增依赖。直接匹配预览 API 的 `RecordedRequestDataSource::Form` 时需使用新的带快照变体。

### Telescope 出站请求重定向与连接失败

固定 [Laravel PendingRequest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/PendingRequest.php) 在每一跳执行首个 `beforeSending` 回调保存请求。成功响应复用最后一跳的回调前请求和原始 GET／JSON／表单参数。连接失败通过 `marshalTransportException` 根据异常的线上请求重新构造 `Request`，未传递原始参数。PHP 8.5.12 和 Guzzle 7.10.0 MockHandler 实测 9 组，覆盖 GET／JSON／表单／原始正文 302、表单 307、发送回调改写以及 GET／表单／multipart 失败。记录位于 `target/tmp/observability-parity/redirect-upstream.log`。

Rust 将原始参数传给 `RequestSending` 与 `ResponseReceived`，连接失败仅按线上媒体类型解析可用缓冲正文。真实请求记录器仍使用实际发送的快照，原生 URL 编码器及其布尔值拼写沿用现有接口，所以失败表单记录实际线上值。额外 PHP 探针确认重定向超限派发六次发送事件，响应与失败事件均为空。发现 302 的 OPTIONS 行为差异，修复见下节。流式附件的 multipart 重定向、退避重试改写和初始参数为空时的正文回退仍需核对。

隔离快照沿用 `8041ae15a` 加前批和本批文件。真实回环 HTTP 核对跨源目标、GET 初始查询、表单 302／307、原始正文 302、回调前后头部与重定向超限。Telescope 仓储用例在 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 分别实际通过。Telescope 全包 32 项、框架 HTTP 客户端 21 项、独立 Redis 8.10.3 用例、快照全仓格式、框架库及 Telescope 全目标严格 Clippy、三语文档构建和 12 组浏览器检查通过。

### 出站重定向的方法、正文与 Referer

固定 Laravel HTTP Client 通过 [Guzzle 7.10.0 RedirectMiddleware](https://github.com/guzzle/guzzle/blob/7.10.0/src/RedirectMiddleware.php) 默认跟随带 `Location` 的 3xx。300–303 清正文并保留 GET、HEAD、OPTIONS，其他方法变为 GET。其余 3xx 沿用原方法与正文。默认关闭 Referer 追踪，因此清除上一跳的 Referer。PHP 8.5.12 中通过 Laravel `PendingRequest::send` 和 Guzzle MockHandler 对 9 种状态与 7 种方法实际验证 63 组，日志 `target/tmp/observability-parity/redirect-method-upstream.log`。

Rust 复用 `Redirect::follow` 执行上述默认规则，仍要求 HTTP(S) 目的地和可重放正文。清正文时删除旧正文相关头，避免在空请求携带旧 framing。旧版只提供禁用 reqwest 内建跳转的入口。不同源剥离凭据、原生 `max_redirects(0)` 的超限行为和 `without_redirecting` 入口沿用现有契约。Guzzle 严格模式、手工 Host 覆盖和位置错误处理的边界仍待对账。

同一 63 组在 Rust 通过真实回环 HTTP 验证到达目标的请求方法、正文与 Referer，并核对响应事件中的目标地址和方法。Telescope 真实 OPTIONS 302 的入库结果在 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 通过。隔离快照沿用 `8041ae15a` 加前批及本批文件。框架 HTTP 客户端 22 项、Telescope 32 项、独立 Redis 8.10.3 用例、Pulse 慢出站 Recorder 定向回归、快照全仓格式、框架库及 Telescope 全目标严格 Clippy、三语站点构建与 12 组浏览器检查通过。

### Telescope Gate 授权入口的调用来源

固定 [GateWatcher](https://github.com/laravel/telescope/blob/39a8ea8ad79915df6c3ac9b4e72659ade808eba3/src/Watchers/GateWatcher.php) 与 [FetchesStackTrace](https://github.com/laravel/telescope/blob/39a8ea8ad79915df6c3ac9b4e72659ade808eba3/src/Watchers/FetchesStackTrace.php) 按调用来源筛选，来源无效时仍写入授权结果。Rust 在公开授权入口的 Future 创建时通过 `Location::caller()` 捕获位置，直接 Gate、Facade、对象安全的 Gate trait 和批量授权共用事件字段。原生事件新增可选 `caller`，手工构造的事件可以传入 `None`。旧版的二次验证 `factor_gate` 不提供原生授权事件来源。

`ignore_paths` 使用路径子串，匹配时统一路径分隔符。`ignore_packages` 识别 vendor 与 Cargo 默认依赖路径，框架包源码始终排除。初批采集授权入口的一处调用位置，过滤后无可用来源时保留 Gate 结果，文件和行号为空。同应用嵌套授权的后续处理见下文。

真实应用路由验证 Facade、直接调用、trait 对象和批量方法写入精确调用行号。三种 Watcher 配置分别验证来源可见、路径过滤、依赖过滤及 `ignore_abilities`。SQLite、PostgreSQL 18.7、MySQL 8.4.13 各通过。框架授权 5 项、Telescope 33 项、快照全仓格式、框架库及 Telescope 全目标严格 Clippy、三语文档构建和 12 组浏览器检查通过。快照沿用 `8041ae15a` 加前批及本批文件。独立 Redis 用例本批未跑。发布构建的路径重映射与目标平台仍需验收。

### Telescope Gate 嵌套授权来源

PHP 8.5.12 使用固定 Gate 与固定 Telescope 复现四条授权记录。直接调用使用应用行，忽略路径中的普通包装函数与嵌套授权会向外查找。嵌套例中里外两条授权记录均指向应用的外层授权行。证据为 `target/tmp/observability-parity/gate-outer-caller-upstream.log`。 [固定 GateWatcher](https://github.com/laravel/telescope/blob/39a8ea8ad79915df6c3ac9b4e72659ade808eba3/src/Watchers/GateWatcher.php)

Rust 在授权 Future 中传播同一应用的外层授权位置，通过当前同步事件派发上下文提供给 Watcher，保留 `GateEvaluated.caller`。Watcher 先选最近的合格位置，内层被过滤时使用合格的外层位置。真实 HTTP 回归同时验证同一任务中交错轮询的兄弟授权无法取得无关外层位置，且来源为空时仍记录授权结果。普通异步包装器的外层应用位置、非标准依赖路径以及 Release 和实际部署组合仍待验收。

SQLite、PostgreSQL 18.7、MySQL 8.4.13 的 Telescope Gate 专项及框架授权实测通过，框架事件默认 8 项、三个扩展默认 91 项、快照全仓格式、严格 Clippy、三语站点构建和 12 组浏览器检查通过。Linux 7.0.14 ARM64 和 Windows 11 ARM64 使用 Rust 1.94.0 通过同一源码快照的嵌套授权 HTTP 与 SQLite 专项。证据为 `target/tmp/observability-parity/gate-outer-caller-complete{,-linux,-docs}.log` 与 `target/tmp/observability-windows/gate-outer-caller-complete-arm64.log`。

### 发布构建的源码路径重映射

固定 Telescope 的来源过滤以可观察路径为输入。Rust 使用 `cargo rustc --test` 给实际调用点所在测试目标传入 `--remap-path-prefix`。验证可执行文件内包含重映射文件名，再从应用路由执行真实授权与 SQL，通过仓储核对文件、行号、忽略规则和外层回退。Pulse 同批验证慢查询位置及关闭位置选项。

将测试调用点映射到自定义 Cargo 注册表布局，原 Telescope 的包过滤漏掉了没有 `.cargo` 段的依赖路径。修复依据实际 Cargo registry 和 Git checkout 的缓存布局识别包，并以有效包目录判断框架来源，避免把父目录中的 `openrust-` 前缀误认为框架包。Windows 首轮回归暴露测试中的 Unix 路径分隔符假设，修复只归一化该断言的文件名比较。原始证据为 `target/tmp/observability-parity/source-remap-{custom-cargo-baseline,windows-test}.log`。

重映射保留包目录结构时可继续使用 `ignore_packages`。重映射若抹平应用与依赖的包目录，Rust 无法从文件位置恢复归属，部署应为依赖保留可区分前缀，再用 `ignore_paths` 配置。

修复版在 macOS、Linux ARM64 和 Windows 11 ARM64 的测试构建中各实跑 8 项 SQLite 路由回归。标准映射验证 Gate、嵌套 Gate、Query 和 Pulse SlowQueries。自定义 registry 与 Git checkout 两种布局各验证 Gate 和 Query，并在 Cargo 根的父目录加入 `openrust-` 前缀检查误判。相关严格 Clippy、快照全仓格式、三语文档构建和浏览器检查通过。编译后的实际路径及测试证据为 `target/tmp/observability-parity/source-remap-{macos-test,linux-verified,windows-verified,clippy-fmt,docs}.log`。

macOS 默认 Release 的标准路径重映射以自定义 Cargo 路径修复前的快照构建。Telescope 的 Gate、嵌套 Gate 与 Query 加上 Pulse 的 SlowQueries，在 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各通过四项真实路由测试，共 12 项。证据为 `target/tmp/observability-parity/source-remap-macos-release.log`。自定义 Cargo 路径修复版的优化构建与 Linux／Windows Release 继续验收。

### Telescope Query 数据库调用来源

固定 [QueryWatcher](https://github.com/laravel/telescope/blob/39a8ea8ad79915df6c3ac9b4e72659ade808eba3/src/Watchers/QueryWatcher.php) 选择第一处未过滤来源。Rust 用 `Location::caller()` 捕获连接与 Facade SQL、事务 SQL、游标、常用 Query Builder 终结操作和 Eloquent Builder 读取入口的编译位置。位置只在 Future 或 Stream 的当前 `poll` 期间加入候选，复用原有成功 SQL 事件和观测派发。旧版数据库连接示例使用 SeaORM，缺少对应的 OpenRust 查询事件来源。

Telescope 从最近的已捕获候选开始应用 `ignore_packages` 与 `ignore_paths`。有应用候选且全部被过滤时省略该 Query 条目。因调用入口尚未覆盖而没有应用候选时，暂保留诊断条目并将文件与行号留空。这项兼容边界须在分页、关系、模型写入、schema、懒加载、任意异步包装器及发布构建入口复核后重新评估。查询失败处理、SQL 日志与 Gate 结果保留规则仍各自使用所属入口。

真实回归首轮暴露仓储写入的内部 INSERT 被另一个容器执行作用域的 Telescope 实例采集。固定 `Telescope::store` 在 `withoutRecording` 内持久化。Rust 扩展同一异步任务的抑制范围，覆盖仓储写入、更新和存储回调，并在 Watcher 解析实例前跳过持久化期间的事件。

基于 `c2ee09bec` 加本批文件的隔离快照验收。SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 均通过真实应用路由的并发查询行号、内层过滤后回退到外层来源、依赖过滤、仓储 SQL 抑制和 Eloquent 查询来源回归。Telescope 默认 34 项通过，独立 Redis 8.10.3 的用例另行通过。框架 SQL 观测回归覆盖成功、失败、作用域与取消，三库实际回调验收覆盖一次性消费、结果转换和事务。快照全仓格式、框架库及 Telescope 全目标严格 Clippy、三语文档构建及 12 组浏览器检查通过。发布构建与其他目标平台未在本批验收。证据位于 `target/tmp/observability-parity/query-caller-*.log`。

### Telescope Query 分页调用来源

固定 [Query Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php)、[Eloquent Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Builder.php) 与关系分页调用沿 PHP 同步栈保留应用位置。PHP 8.5.12 的 SQLite 参考探针实际注册固定 QueryWatcher，验证 Query Builder 长度分页、Eloquent 游标分页及预加载、HasMany 长度分页、HasOne 简单分页的 7 条 SQL 均归到 4 处应用调用行。

Rust 复用查询来源作用域，接入 Query Builder、Eloquent、SearchBuilder 的分页方法，以及 HasMany、HasOne、BelongsTo、MorphOne、MorphMany、MorphTo、HasManyThrough、HasOneThrough、BelongsToMany 关系入口。MorphToMany 经现有解引用转发到 BelongsToMany。普通页、简单页、游标页及带选项变体均保留异步执行；只转发参数的同类入口直接传递编译器调用位置。旧版对应 SeaORM 分页无此观测入口。

基于 `4f19d7c29` 加本批文件的隔离快照，通过 SQLite、PostgreSQL 18.7、MySQL 8.4.13 的真实应用路由验证与 PHP 探针对应的 7 条 SQL 和精确行号。Telescope 默认 35 项、框架分页 14 项及 SQL 观测回归通过。另显式启用 PostgreSQL 的分页解析器、MySQL 的分页解析器和完整 HTTP 分页用例，确认均连接相应数据库。快照全仓格式、框架库及 Telescope 全目标严格 Clippy、三语文档构建和 12 组浏览器检查通过。模型写入、关系直接读取、schema、懒加载和任意异步包装器继续核对。原始输出位于 `target/tmp/observability-parity/query-pagination-*.log`。

同一快照的 macOS 默认 Release 构建中，SQLite Provider 15 项及 Watcher 7 项通过。用相同 Release 二进制分别验证 PostgreSQL 18.7 与 MySQL 8.4.13 的分页来源，并在隔离 Redis 8.10.3 上验证原生命令采集。自定义源码路径重映射、Linux／Windows 及生产组合仍需实测。

### Telescope Query 模型写入调用来源

固定 [Model](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) 与 [Eloquent Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Builder.php) 让模型持久化经过同一个数据库事件。PHP 8.5.12 的 SQLite 参考探针注册固定 QueryWatcher，确认模型创建、静默更新、事务更新、构建器创建和删除各写入一条带实际调用行的记录。旧版 SeaORM ActiveModel 写入缺少该事件契约。

Rust 在 `Model` 的保存、更新、删除、恢复、计数器、已加载关系图保存及其静默、错误和显式事务变体传播调用位置。Eloquent Builder 补齐创建、填充插入、upsert、计数器、删除与恢复。默认方法继续返回惰性 Future，并复用原有事务及模型事件逻辑。直接关系类型的读写入口和单独的 schema／惰性读取入口继续核对。

基于 `19b34ff61` 加本批文件的隔离快照，在 SQLite、PostgreSQL 18.7、MySQL 8.4.13 的真实应用路由验证上述五条写入的精确源码位置。三库均验证写入变体的事务、取消、模型事件与冲突处理，MySQL 另显式运行模型事件和持久化测试。Telescope 默认 36 项、框架模型／事件／写入变体测试、快照全仓格式、框架库及 Telescope 全目标严格 Clippy、三语文档构建和 12 组浏览器检查通过。模型写入的 Release 与目标平台验收留在发布组合。原始输出位于 `target/tmp/observability-parity/query-model-write-*.log`。

### Telescope Query 直接关系读取调用来源

固定 [Eloquent Relation](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Relation.php) 委派 Builder 查询，固定 QueryWatcher 能沿调用栈找到应用行。PHP 8.5.12 的 SQLite 参考使用 HasMany 查找和读取、BelongsTo 读取、事务内关系加载以及游标实际消费，加上作为对照的模型主键读取，共六条查询均归到实际调用行。旧版 SeaORM 的关系查询无原生 QueryExecuted 来源。

Rust 为公开 `Relation::get_results`、批量匹配与事务匹配接入位置传播，覆盖现有关系实现及 BelongsTo、MorphTo 的单结果覆盖。关系查找宏同时覆盖单条、批量、唯一结果、缺失报错和回退入口。游标在创建时捕获位置，后续轮询在原作用域内传播。尚待核对的入口包括关系写入与先读后写变体、Model／Collection 的惰性关系加载、分块读取、schema 及任意异步包装器。

基于 `949ed9c65` 加本批文件的隔离快照，SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 的真实应用路由均验证六条查询的精确行号。Telescope 默认 37 项、框架关系 6 项及分页 14 项通过，MySQL 关系主用例和 upsert 另显式连接夹具通过。全仓格式、框架库与 Telescope 全目标严格 Clippy、三语构建与 12 组浏览器检查通过。证据位于 `target/tmp/observability-parity/query-relation-*.log`。

同一快照 macOS Release 的 SQLite Watcher 9 项通过，PostgreSQL 18.7 和 MySQL 8.4.13 分别用相同 Release 二进制复测分页、模型写入和关系读取来源，三项均通过。其他目标平台及部署组合仍需验收。

### Telescope Query 分块与惰性读取来源

PHP 8.5.12 使用固定 Telescope QueryWatcher 实测 Query Builder 的 `chunk`、Eloquent 的 `eachById`、HasMany 的 `chunkById` 和 `lazy`。停止回调在第二批返回 `false`，准确产生两条 SQL。其余三个入口各产生三条 SQL，含最后一次空查询，全部来源为各自应用调用行。证据为 `target/tmp/observability-parity/query-chunks-upstream.log`。 [固定分块与惰性读取](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Concerns/BuildsQueries.php)

PHP 将回调内主动执行的查询归到回调行，将惰性集合的实际 SQL 分别归到 `rewind` 和各次 `next`，创建集合和取得迭代器时无查询。两次惰性读取之间的独立查询仍归到自己的应用行。证据为 `target/tmp/observability-parity/query-chunks-poll-upstream.log`。Rust 标准 `Stream` 消费入口使用公开流构造位置，并在轮询时恢复。

Rust Query Builder、Eloquent Builder 和直接、反向、多对多、Through、多态关系的公开分块／遍历／惰性方法已接入现有查询调用位置作用域。偏移与按键推进方法在创建 Future 或流时捕获位置，轮询返回后恢复作用域。SQLite、PostgreSQL 18.7、MySQL 8.4.13 的实际 Telescope HTTP 路由核对两批提前停止、回调查询各自的来源、Eloquent 和 HasMany 的末次空查询，以及流两次轮询之间的独立查询来源。证据为 `target/tmp/observability-parity/query-chunks-macos.log`。其他关系变体的来源逐项实测仍待完成。

macOS 隔离快照通过框架分页／关系／惰性集合回归、Telescope Watcher 15 项、严格 Clippy 和全仓格式。三扩展默认组合 100 项通过，23 项按环境条件跳过。框架分页 14 项通过，另在真实 MySQL 8.4.13 上验收完整分页 HTTP 流程，含分块、反向关系、按键遍历和惰性读取。Windows 11 ARM64 在 SQLite 上通过本批真实 Telescope HTTP 来源专项。Linux ARM64 复用同一构建目录，排在 Pulse 原生报告单作业重跑之后。证据为 `target/tmp/observability-parity/query-chunks-{macos,pagination-macos,pagination-mysql-retry,windows}.log`。

逐次消费由 `DatabaseCursorExt::try_next_with_source` 捕获调用位置。它在本次轮询作用域内登记消费位置，并跳过该消费位置内部的流创建帧。已经捕获的独立数据库操作、外层作用域和 Telescope 的路径过滤继续生效。普通 `TryStreamExt` 产物仍使用公开创建位置。

在隔离快照上，macOS 的 SQLite、PostgreSQL 18.7、MySQL 8.4.13，以及 Windows 11 ARM64 的 SQLite 均通过 Telescope 真实 HTTP 路由验证两次读取、末次空查询及中间独立查询的精确源码行。普通消费者的流创建位置也随同复验。macOS 的框架惰性集合、关系和多态聚合回归通过，三个扩展的默认组合 100 项通过、23 项按环境条件跳过。快照全仓格式、所涉框架与扩展目标严格 Clippy、三语站点构建和 12 组浏览器检查通过。证据为 `target/tmp/observability-parity/query-consumer-{macos-verified,windows,docs}.log`。Linux ARM64 首次触及 32 GB 停线，后在文件重放组合通过同一来源专项。证据为 `target/tmp/observability-parity/{query-consumer-linux,observability-file-linux-retry}.log`。

### Telescope Query 多对多中间表调用来源

固定 [InteractsWithPivotTable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Concerns/InteractsWithPivotTable.php) 的附加、修订、同步、切换、移除均通过原生查询事件。PHP 8.5.12 和 SQLite 的参考探针实际注册固定 QueryWatcher，使用 `attach`、`updateExistingPivotOrFail`、`sync`、`toggleOrFail`、`detachOrFail`，九条 SQL 分别归到五处调用行。参考夹具使用 Laravel 声明的 Doctrine Inflector 2.0.5；旧版 SeaORM 链接查询无对应诊断来源。

Rust 在 BelongsToMany 的中间表入口传播调用位置，覆盖带属性及事务变体，MorphToMany 沿用现有解引用入口。同步和切换中的读取、插入、删除经同一查询来源作用域传播。内部事务、查询锁和自定义中间表模型装配保持现有行为。关联模型自身的保存、创建、touch 与先读后写变体仍须核对。

基于 `42bca1a84` 加本批文件的隔离快照，在 SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 的真实应用路由验证五个入口的九条 SQL 与精确行号，同时检查更新行数、同步和切换的附加／移除集合及最终移除行数。Telescope 默认 38 项、框架关系 6 项、MySQL 两项显式连接原生回归、快照全仓格式、严格 Clippy、三语文档构建及 12 组浏览器检查通过。Release 与其他目标平台留在发布组合验收。证据位于 `target/tmp/observability-parity/query-pivot-*.log`。

### Telescope Query 关联模型写入调用来源

固定 Laravel 的 `HasMany` 与固定 Telescope 在真实 SQLite 上核对 `create`、未命中的 `firstOrCreate`、命中后的 `updateOrCreate`、`saveQuietly` 和 `createMany`。共八条 SQL 归到五个应用调用位置，读写复合操作的 SQL 归到同一行，同时验证了写入的外键。证据为 `target/tmp/observability-parity/query-relationship-write-upstream.log`。 [固定 HasOneOrMany](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrMany.php)

Rust 在关联的公开创建、保存、读后写、upsert 及 touch 入口捕获应用调用位置，沿用 Schema 与 SQL 查询的轮询作用域。实现覆盖直接、多态、多对多、远程关系现有的写入入口及反向关系的 touch。通过真实 HTTP 路由调用五个上游对照入口，并验证 `HasOne`、`MorphMany`、`MorphOne` 创建及 `BelongsToMany` 的模型与中间表复合写入。仓储按精确文件及行号核对 SQL 数量，同时核对写入外键、多态类型和更新后的同一条记录。

macOS 的 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 均通过上述仓储验收。三包默认 90 项、框架关系默认 6 项及原生 MySQL 2 项、快照全仓格式和严格 Clippy 通过。Windows 11 ARM64 及 Linux 7.0.14 ARM64 以 Rust 1.94.0 编译同一快照并通过 HTTP 与 SQLite 专项，三语站点构建及 12 组浏览器检查通过。证据为 `target/tmp/observability-parity/query-relationship-write-{final,final-linux,core-mysql,docs}.log` 和 `target/tmp/observability-windows/query-relationship-write-final-arm64.log`。

补验固定 PHP 8.5.12 的真实 SQLite 与 QueryWatcher。直接 HasMany 的 `upsert` 记录一条写入，冲突后的 `createOrFirst` 仅记录恢复查询。HasManyThrough 的命中与关联范围外的未命中各记录一条恢复查询，未命中继续返回原唯一键冲突。四条均归到调用行。UUID 生成由 Laravel 自带的可替换工厂提供。证据为 `target/tmp/observability-parity/query-relationship-recovery-upstream.log`。

Rust 在现有 HTTP 路由和 Telescope 仓储补验对应入口。三库均确认直接 upsert 的实际更新、按原关系归属恢复同一记录、通过中间表恢复、跨父记录时保留唯一键冲突、空批次无 SQL，以及各自文件与行号。macOS 的 SQLite、PostgreSQL 18.7、MySQL 8.4.13 专项、Watcher 15 项、快照全仓格式和相关严格 Clippy 通过。证据为 `target/tmp/observability-parity/query-relationship-recovery-macos.log`。

其余远程关系、保存阶段的递归触碰、其他写入变体及 Release 与生产组合继续验收。

#### Telescope Query 反向关联直接刷新时间戳

固定 PHP 的 `BelongsTo::touch` 在真实 SQLite 上只执行一条更新时间戳的 SQL。更新跳过包括软删除在内的全局作用域。空外键及 `withoutTouching` 跳过查询，非空但不存在的所有者仍尝试更新，Telescope 记录实际文件与行号。证据为 `target/tmp/observability-parity/query-reverse-touch-upstream.log`。 [固定 BelongsTo](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsTo.php)

补齐 Rust 公开的 `BelongsTo::touch(&child)`，复用关联键、Eloquent 更新时间戳和现有查询来源栈。macOS 的 SQLite、PostgreSQL 18.7、MySQL 8.4.13 各经真实 HTTP 验证软删除行的更新时间、禁用和空外键分支、找不到所有者时的尝试及两次准确调用位置。除验证数据的读取外，触碰不加载模型。Watcher 16 项、快照全仓格式、框架及扩展相关严格 Clippy、三语构建与关系章节桌面／移动检查通过。证据为 `target/tmp/observability-parity/query-reverse-touch-{macos,core-clippy,docs,docs-chapter}.log`。其他反向类型及 Release 继续验收。

### Telescope Query 模型与集合的关系加载来源

PHP 8.5.12 在固定 Laravel、固定 Telescope 与真实 SQLite 上核对首次访问关联属性、模型 `load`、模型与集合 `loadMissing`、集合 `load` 和模型 `loadCount`。六处应用入口各产生一条查询并记录精确文件与行号。重复读取已经加载的关系和重复 `loadMissing` 不产生查询。证据为 `target/tmp/observability-parity/query-relation-loading-upstream.log`。 [固定 Eloquent Model](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) · [固定 Eloquent Collection](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php)

Rust 在 Model、Collection、CollectionModel、BorrowedRelatedModel 和 RelatedBatch 的现有延迟加载、显式加载与聚合入口捕获调用位置。实际 HTTP 路由核对首次关系加载、缓存命中、Model 的 `load`／`load_missing_paths`、Collection 的 `load_missing`／`load_paths`、通过 CollectionModel 的批量自动加载及 `load_count` 的源码位置和读取结果。动态关系、多态加载及聚合变体、本地显式事务、脱离集合后跨模型访问与更深的关联遍历入口仍需逐项来源验收。

真实 SQLite、PostgreSQL 18.7、MySQL 8.4.13 专项通过。快照的框架关系 6 项、多态聚合 1 项、三个扩展默认 92 项、全仓格式、严格 Clippy、三语站点构建及 12 组浏览器检查通过。Linux 7.0.14 ARM64 和 Windows 11 ARM64 使用 Rust 1.94.0 通过同源码的 HTTP 与 SQLite 专项。Windows 首次构建触及宿主机的 32 GB 停线，守卫持锁清理后重跑通过。证据为 `target/tmp/observability-parity/query-relation-loading-{final,linux,docs}.log` 和 `target/tmp/observability-windows/query-relation-loading-verified-arm64.log`。Release 与部署组合待验。

### Telescope Query Schema 调用来源

PHP 8.5.12、固定 Laravel 和固定 Telescope、真实 SQLite 连接。`Schema` Builder 的 `create`、`hasTable`、`table`、`hasColumn`、`dropIfExists` 以及删除后再查表，共七条 SQL 落到对应六行，其中 `hasColumn` 发出两条目录查询。证据为 `target/tmp/observability-parity/query-schema-source-upstream.log`。 [固定 Schema Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php)

公开 Facade 探针另确认 `whenTableHasColumn` 的目录读取及实际 DDL 共三条都归到条件入口，`getForeignKeys` 归到实际目录入口，`withoutForeignKeyConstraints` 的两条外键设置归到包装调用，回调内的业务查询单独归到回调实际行。证据为 `target/tmp/observability-parity/query-schema-facade-upstream.log`。

Rust 的 `Schema` Facade、Builder、目录检查和 `SchemaSession` 复用数据库调用位置栈。应用调用时捕获位置，SQL Future 在轮询时携带该位置。条件改表的目录查询和 DDL 归到同一应用入口，显式外键状态变化及恢复各产生查询事件，业务回调的 SQL 优先使用自身的调用行。SQLite 改列重建的独立任务继承查询位置和仍有效的请求作用域，请求取消后继续完成有保护的重建和会话恢复。

直接以物理会话执行的 MySQL 批量清理、自动取消后的后台回收、PostgreSQL 外键约束包装和发布路径重映射仍须分别对账。PostgreSQL 的建表、目录检查和条件 DDL 路径独立验收。

macOS 用真实 HTTP、SQLite、PostgreSQL 18.7、MySQL 8.4.13 核对 DDL、目录检查、执行及跳过条件改表、SQLite 重建、支持驱动上的外键往返和回调行。框架默认 `database_schema` 目标 80 项通过，含 SQLite 取消后仍完成重建并恢复外键状态。该目标中依赖外部参数的用例沿用原有环境判断，另用 MySQL 8.4.13 实跑会话恢复专项。Telescope Watchers 11 项、三包默认 89 项、全仓格式、修改范围严格 Clippy、三语构建和 12 组浏览器检查通过。证据为 `target/tmp/observability-parity/query-schema-source-{runtime,matrix,docs}.log`。

Linux 7.0.14 ARM64 与 Windows 11 ARM64 用 `274644de4` 的变更源码及 Rust 1.94.0，通过真实 HTTP 和 SQLite 的 Schema 归因专项，包含改列重建、外键开关与应用回调。证据为 `target/tmp/observability-parity/query-schema-source-linux.log` 和 `target/tmp/observability-windows/query-schema-source-arm64.log`。

### Pulse SlowQueries 数据库调用来源

固定 [SlowQueries](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/SlowQueries.php) 从查询调用栈选首个可用文件与行号，省略应用根路径并跳过框架、Pulse 和启动脚本。无可用位置写空字符串，关掉 `location` 写 `null`。在 PHP 8.5.12 中实际注册固定 Recorder，通过 SQLite 查询、Pulse 延迟队列和 Ingest 契约核对这两种配置的条目键。

Rust 使用原生 `QueryExecuted.callers` 已捕获的位置，跳过框架、Pulse 及启动入口，无需解析本地调试符号。默认保留第三方包的候选，符合 Pulse 对内部来源较窄的排除范围。来源不可用时写空字符串，禁用时写 `null`。移除 Pulse 唯一的 `backtrace` 依赖及其孤立依赖。未接入调用来源的数据库入口和自定义路径重映射仍继续核对。

基于 `d987d7fa9` 加本批文件的隔离快照，SQLite 真实路由验证有来源与关闭来源的条目键。Pulse 默认 21 项、独立 Redis 8.10.3 Stream 用例、Pulse 与 Telescope 的运行时组合、快照全仓格式、Pulse 全目标严格 Clippy、三语文档构建及 12 组浏览器检查通过。同一 macOS Release 二进制分别验证 SQLite、PostgreSQL 18.7、MySQL 8.4.13 的慢查询来源和关闭开关。Linux／Windows 与部署组合仍需验收。证据位于 `target/tmp/observability-parity/pulse-query-source-*.log`。


固定 PHP 探针另从 Pulse 包内执行同一 SQLite 查询，确保调用栈只剩应排除的位置。启用 `location` 时实际条目键为 `["select 42 as pulse_source_probe",""]`，关闭后为 `["select 42 as pulse_source_probe",null]`。证据为 `target/tmp/observability-parity/pulse-query-no-source-upstream.log`。

修复前的实际 HTTP 与 SQLite 用例复现两处差异。Pulse 保留了自定义 Cargo Git 缓存中的框架调用位置，又因 Cargo 根目录包含 `openrust-framework-` 前缀而丢弃了第三方注册表包的位置。Pulse 和 Telescope 现在共用 Cargo 包目录识别，并分别保留各自的过滤范围。Pulse 排除框架与 Pulse 包，按完整的注册表包名或 Git 仓库名识别，名字相近的第三方包（如 `openrust-pulse-exporter`）仍以实际文件和行号写入。保留 Cargo 目录结构的 Rust 路径重映射用于回归，自定义映射抹去包身份时仍受上节所述限制。修复前证据为 `target/tmp/observability-parity/pulse-cargo-source-macos-baseline.log`。注册表包名及语义版本以 Cargo 规则解析，缓存路径结构按 Rust 1.94.0 的实际布局验证。 [Cargo 包名与版本](https://doc.rust-lang.org/cargo/reference/manifest.html#the-name-field) [Cargo 缓存结构](https://doc.rust-lang.org/cargo/guide/cargo-home.html)

macOS 使用本批完整源码，以实际编译器重映射验证五种路径。SQLite 通过 Git 框架包、普通第三方注册表包、同名前缀第三方注册表包、Pulse 注册表包和标准发布路径的 HTTP 测试。标准路径的 Pulse Provider 17 项及 PostgreSQL 18.7、MySQL 8.4.13 慢查询专项通过。Pulse 和 Telescope 的 Provider／Watcher 共 48 项，独占 Redis 8.10.3 的 Telescope 原生命令专项、共享识别规则单项、快照全仓格式和相关严格 Clippy 均通过。证据为 `target/tmp/observability-parity/pulse-cargo-source-exact-{macos,redis}.log`。

同一修复源码在 Linux ARM64 与 Windows 11 ARM64 使用 Rust 1.94.0 通过全部五种 Pulse 路径的真实 HTTP 和 SQLite 专项。两个平台各通过自定义注册表与 Git 缓存布局下的 Telescope Gate／Query 四项，Windows 另通过标准重映射的三项 Telescope 授权与查询测试。证据为 `target/tmp/observability-parity/pulse-cargo-source-linux.log` 和 `target/tmp/observability-windows/pulse-cargo-source-exact-arm64.log`。本次 Linux／Windows 结果使用测试构建。修复版在 macOS 默认 Release 通过上述五种 SQLite 路径、Pulse Provider 17 项，以及标准重映射下的 PostgreSQL 18.7、MySQL 8.4.13 慢查询专项。证据为 `target/tmp/observability-parity/pulse-cargo-source-exact-macos-release.log`。macOS 的 Telescope 自定义 Cargo registry 与 Git checkout 也在修复版 Release 各通过 Gate 与 Query 两项真实 HTTP 和 SQLite 测试。证据为 `target/tmp/observability-parity/telescope-cargo-source-exact-macos-release.log`。Windows 11 ARM64 默认 Release 以同一修复源码通过 Pulse 的五种 SQLite 真实路由测试。证据为 `target/tmp/observability-windows/pulse-cargo-source-exact-release-arm64.log`。Linux 7.0.14 ARM64 同样以默认 Release 通过 Pulse 的五种 SQLite 路由回归，编译器实际重映射路径均已核对。证据为 `target/tmp/observability-parity/pulse-cargo-source-exact-linux-release.log`。Windows 11 ARM64 的 Telescope 修复版默认 Release 通过标准布局下的 Gate、嵌套 Gate、Query，以及自定义 registry 和 Git checkout 各两项，共七项真实 HTTP／SQLite 测试。证据为 `target/tmp/observability-windows/telescope-cargo-source-exact-release-arm64.log`。Linux Telescope Release 继续验收。

### Pulse Servers 默认主机名与配置缓存

固定 [Pulse 配置](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/config/pulse.php) 在载入时求值默认主机名，显式 `PULSE_SERVER_NAME` 覆盖默认。PHP 8.5.12 探针直接载入固定配置，确认两种名称和先前已载入值的稳定性。Rust 包默认配置与发布配置共用现有原生主机名检测。缺少名称的历史快照和手写 Recorder 配置沿用采样时的回退，系统查询失败保留 `unknown`。

基于 `38285b7c1` 加本批文件的隔离快照，使用真实 Artisan `config:cache`、`config:show`、`config:clear` 验证包默认配置与发布配置。两种配置均可缓存本机主机名或显式覆盖，缓存期间修改环境变量仍读取原值。Pulse 默认 21 项、快照格式、全目标严格 Clippy、三语构建和 12 组浏览器检查通过。证据位于 `target/tmp/observability-parity/pulse-server-*.log`。

### Pulse Exceptions 报告入口

PHP 8.5.12 使用固定 Laravel 和固定 Pulse，通过实际异常处理器、Logger 和 Pulse Ingest 核对记录来源。框架 `Handler::report` 与显式 `Pulse::report` 各产生一条异常。前者在日志传输失败时仍然采集。直接向 Logger 写入附带 Throwable 的日志、被较早报告回调停止的异常均无 Pulse 条目。两个条目保留准确类型与源码位置。证据为 `target/tmp/observability-parity/pulse-exception-sources-upstream.log`。 [固定 Exceptions Recorder](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/Exceptions.php)

Rust 将默认采集注册到框架异常处理器的 `reportable_all`，保留显式 Pulse 事件。真实 HTTP 核对普通带类型日志、日志级别过滤、较早报告回调停止传播、关闭来源与持久化键。自定义处理器缺少运行期报告配置时可正常启动，并使用显式 Pulse 报告。macOS 的 SQLite、PostgreSQL 18.7、MySQL 8.4.13，以及 Linux 7.0.14 ARM64 与 Windows 11 ARM64 的 SQLite 均通过两个专项。框架异常上下文 14 项、三个扩展默认 93 项、严格 Clippy、全仓格式、三语文档与 12 组浏览器检查通过。证据为 `target/tmp/observability-parity/pulse-exception-sources-{accepted,complete-linux,custom-linux,complete-docs}.log` 和 `target/tmp/observability-windows/pulse-exception-sources-accepted-arm64.log`。Release 与部署组合仍待验。

固定 Pulse 对 `ViewException` 以直接来源的类型与应用相对视图路径分组，禁用位置时保留类型并写入空值。PHP 8.5.12 的实际 Blade 编译及渲染探针确认，嵌套视图失败时类型是直接来源的 `ViewException`，位置仍取最内层失败视图。证据为 `target/tmp/observability-parity/pulse-view-exception-{upstream,render-oracle}.log`。普通异常启用位置时的源码文件及行号仍须独立核对。

OpenRust 的 Blade 渲染保留原有 `ViewError::RenderFailed` 分类，并在来源中附带 `ViewException`。Pulse 识别直接视图错误或经框架透明包装传来的错误，沿视图来源读取失败模板路径，按原生类型名记录并去掉应用根路径。保留 Rust `.blade.html` 扩展名。macOS 真实 HTTP 请求通过公开 View Facade 渲染应用对象，分别在 SQLite、PostgreSQL 18.7、MySQL 8.4.13 核对单层错误、嵌套错误与关闭位置时的数据库条目键。证据为 `target/tmp/observability-parity/pulse-view-exception-macos-final-retry.log`。未携带类型名的通用 Rust 动态错误仍使用既有分类回退。 [固定 Blade 包装入口](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Engines/CompilerEngine.php#L98) [固定 Pulse 分组实现](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/Exceptions.php#L70)

同一 macOS 快照通过 29 项框架视图回归、Pulse Provider 16 项、Telescope Provider 15 项及选定目标严格 Clippy，另有 1 项 Telescope Provider 按环境条件跳过。快照全仓格式通过。触及 32 GB 停线后持锁清理，重新构建的三扩展默认组合通过 98 项，23 项按环境条件跳过。最终源码在 Linux 7.0.14 ARM64 和 Windows 11 ARM64 各通过 4 项 Pulse 分组测试和 16 项 Provider 测试，包含 SQLite 上的真实视图失败用例。Linux 同批通过 Telescope Provider 15 项，1 项按环境条件跳过。证据为 `target/tmp/observability-parity/pulse-view-exception-{macos-combined,linux-final,windows-final}.log`。

### Pulse 普通异常的类型与发生位置

PHP 8.5.12 通过固定 Pulse 的公开报告入口、实际 Recorder 和 Ingest 验证普通异常的分组。应用异常写入创建文件与行号，来自应用 `vendor` 目录的异常优先写入首个应用调用帧，禁用位置时写入 `null`。证据为 `target/tmp/observability-parity/pulse-exception-origin-oracle.log`。 [固定 Pulse 位置选择](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/Exceptions.php#L105)

Rust 的 `Error` trait 不提供创建位置与调用栈。应用可在实际构造或首次捕获处使用 `SharedException::located`，传播时可用 `with_caller` 保留调用帧。Pulse 从同一错误实例读取具体类型、原始位置和显式传播的调用帧，框架普通报告及 HTTP 内部错误均传递此元数据。缺少元数据的错误沿用既有位置回退。独立 Rust HTTP 消费者从 `vendor/origin.rs` 创建异常，在应用 `main.rs` 登记调用帧，实际 SQLite 记录保存原类型和 `main.rs:39`。证据为 `target/tmp/observability-parity/pulse-exception-origin-consumer.log`。

macOS 隔离快照经真实 HTTP 覆盖公开报告、HTTP 直接返回、经 HTTP 包装后显式报告、缺少位置与关闭位置。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 均读取到预期的具体错误类型与创建行号。框架异常回归 43 项、Pulse Provider 17 项和 Telescope Provider 15 项通过，另有 1 项 Telescope Provider 按环境条件跳过。快照全仓格式、选定目标严格 Clippy 和三扩展默认组合通过，组合 99 项通过、23 项按环境条件跳过。证据为 `target/tmp/observability-parity/pulse-exception-origin-macos-accepted.log`。

Windows 11 ARM64 和 Linux 7.0.14 ARM64 使用同源码各通过 4 项模式测试和 17 项 Pulse Provider 测试，含 SQLite 上的普通异常与 Blade 异常 HTTP 专项。Linux 同批通过 Telescope Provider 15 项，1 项按环境条件跳过。测试结果完整写入后构建守卫结束本批，停线清理已持锁完成。证据为 `target/tmp/observability-parity/pulse-exception-origin-{windows-accepted,linux}.log`。

无调用帧的部署行为继续按来源核对。

### Pulse 包级原生异常报告

PHP 8.5.12 以固定 Pulse 运行真实监听器，确认 `Pulse::report(Throwable)` 返回原实例、事件携带同一个异常对象，停止记录时仍派发事件且不增加 Ingest 条目。证据为 `target/tmp/observability-parity/pulse-native-report-oracle.log`。 [固定 Pulse 报告入口](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Pulse.php#L200)

Rust Manager 与 Facade 提供 `report_exception(error)`。事件 `NativeExceptionReported` 持有原始共享错误，Recorder 与框架报告共用类型和位置解析并延迟写入。现有双参数 `report(class, location)` 和 `ExceptionReported` 事件仍可由既有调用者使用。用不同的 Rust 方法和事件保留原有显式类型／位置契约。

macOS 真实 HTTP 通过 Manager 与 Facade 报告，验证事件可读取同一错误、停止记录时继续派发且数据库不增加条目、仅通过框架报告的自定义处理器与 Pulse 显式入口各按自己的行为执行。SQLite、PostgreSQL 18.7、MySQL 8.4.13 均通过。实际存储核对类型、捕获位置和关闭位置。严格 Clippy、快照全仓格式及三扩展默认组合通过，组合 99 项通过、23 项按环境条件跳过。证据为 `target/tmp/observability-parity/pulse-native-report-macos-verified.log`。

三语指南与浏览器验收检查原生报告方法、对应事件及位置捕获入口。站点构建和根路径、子目录下的桌面／手机共 12 组验证通过。Windows 11 ARM64 最终源码通过 4 项模式测试和 17 项 Pulse Provider 测试，含 SQLite 上的原生报告事件和关闭记录专项。Linux ARM64 最终源码复验已启动。证据为 `target/tmp/observability-parity/pulse-native-report-{docs,windows}.log`。

Linux ARM64 首次构建在 Provider 链接阶段收到信号 9，未取得该次运行的测试结果。已用单作业重跑。证据为 `target/tmp/observability-parity/pulse-native-report-linux{,-serial}.log`。

单作业重跑在编译阶段触及 32 GB 停线，独占容器已退出，尚未进入测试。本机项目产物随后按守卫协议持锁清理。证据为 `target/tmp/observability-parity/query-consumer-cleanup.log`。Pulse 原生报告及 Telescope 分块随后通过 Linux ARM64 文件重放组合。

### Horizon Linux ARM64 Release 进程矩阵

固定 Horizon 的进程树采用 Unix 信号，包依赖 `ext-pcntl` 和 `ext-posix`。PHP 官方明确 PCNTL 不在原生 Windows 上工作。Linux 的进程验收对应固定上游支持范围；OpenRust 在 Windows 上的元数据命令及应用装配仍须按项目发布范围单独核对。 [固定 Horizon 依赖](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/composer.json) [PHP PCNTL 安装文档](https://www.php.net/manual/en/pcntl.installation.php)

使用 `8745bc5fa` 的 Git 隔离快照与 Debian Bookworm 原生 ARM64 容器构建 `horizon-acceptance`。进程夹具可在独占容器网络内使用 Redis CLI，无需挂载 Docker 套接字。末段关闭该批 Redis，验证元数据连接中断后的进程回收。首轮优化构建的框架编译进程收到 SIGKILL。Docker 虚机内存约 8 GB，重试将 Cargo 并发限制为 1，保留正常 Release 优化。

Rust 1.94.0 单作业构建通过，实际运行于 Linux 7.0.14-linuxkit、Debian glibc 2.36 与隔离 Redis 8.10.3。Worker、孤儿清理和完整进程矩阵均通过，覆盖 Master／Supervisor／Worker 进程树、扩缩容、异常补位、暂停恢复、SIGTERM 排空、维护模式、内存保护、文件热重载、Redis 中断回收和 HTTP 授权／脱敏。首轮热重载暴露夹具在一次判断中重复读取状态的竞态。改为一次读取一份状态后，同一 Linux 二进制完整复跑通过。证据为 `target/tmp/horizon-linux/build-retry.log`、`acceptance.log` 与 `acceptance-retry.log`。

本批验证 Linux ARM64 容器中的 Artisan 和 HTTP 接口。Linux x86_64、部署主机资源与信号配置、正式 Redis 拓扑、浏览器及完整发布组合仍需按目标环境验收。

### Horizon 公开进程与队列事件

固定 `MasterSupervisorReviving` 和 `SupervisorProcessRestarting` 可由应用显式派发。固定 EventMap 仅为后者登记空监听器列表。Rust 公开构造及真实进程消费已验收，见[显式派发的监督器事件](horizon/README.md#显式派发的监督器事件)。自动派发的 `WorkerProcessRestarting`、`MasterSupervisorDeployed` 和 `UnableToLaunchProcess` 见下文。 [固定进程代码](https://github.com/laravel/horizon/tree/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src) [固定 EventMap](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/EventMap.php)

固定 Redis 运输事件的共同字段为连接、队列和载荷，`JobsMigrated` 提供载荷序列。公开的入队、保留、释放、删除、迁移与失败事件，以及原子任务链后继投递的通知时机见下节。 [固定 RedisQueue](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/RedisQueue.php) [固定失败事件桥接](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/MarshalFailedEvent.php)

| 固定事件 | 运输边界与上游消费者 |
|---|---|
| `JobPending` | 即时与延迟写入 Redis 前派发并保存待处理详情，真实 Redis 写失败仍保留该事件和已写详情。 |
| `JobPushed` | Redis 写入成功后派发，保存被监控标签，已验即时、延迟与事务提交。 |
| `JobReserved` | Redis 成功保留后派发，原生快照携带尝试次数，实际入队／取出夹具验证保留态仓储。Worker 计时仍走原生尝试事件。 |
| `JobReleased` | Redis 确认重新入队后派发，原生延迟为 `Duration`，任务详情保留毫秒值。 |
| `JobDeleted` | Redis 确认删除保留记录后派发，带保留快照与失败标记。成功删除更新完成状态和指标，失败删除等待失败处理，避免重复记为成功。 |
| `JobsMigrated` | 延迟集合和过期保留集合分别迁入 Ready 后派发，空批次也派发。同步更新待处理详情及延迟，保留新尝试与终态。 |
| `JobFailed` | 按实际 Redis 保留来源派发，携带异常链与保留快照。Worker 终止失败、直接失败和已持久化失败的恢复均已验，记录失败状态与标签。 |

### Horizon Redis 入队与保留事件

复用核心队列传输和应用 Dispatcher。Horizon 从原生 Redis 写入前、成功写入及原子保留三个入口桥接公开的 `JobPending`、`JobPushed`、`JobReserved`。应用通过事件的 `NAME` 订阅，并直接读取原生载荷或保留快照。继续使用已有投递 ID 访问仓储，载荷 UUID 可关联重复投递。待处理详情随 `JobPending` 保存。成功写入后更新监控标签和保留期限，避免重新写入状态覆盖已经保留的任务。已保留详情在原生 Redis `pop` 完成时更新。 [固定入队监听器](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/StoreJob.php) [固定监控标签监听器](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/StoreMonitoredTags.php)

基于 `0621a67a3` 加本批文件的 macOS 隔离快照，在独占 Redis 8.10.3 上经公开事件监听器逐次读取 Ready、延迟、保留队列、任务详情和监控标签。已验即时与延迟入队、Redis 写入错误、SQLite 事务提交和回滚、直接保留与尝试次数。Horizon 默认 24 项和 Redis 13 项、Pulse 默认 21 项和 Redis 1 项、Telescope 默认 38 项和 Redis 1 项通过。实际 Artisan 进程矩阵与核心不启用 Horizon 的 Redis HTTP／阻塞取任务通过。快照全仓格式、Horizon 全目标严格 Clippy、三语站点和 12 组浏览器检查通过。证据为 `target/tmp/observability-parity/horizon-redis-transport-*.log`。本批 Linux、Windows 和全仓发布组合待验。

固定 `JobPayload::prepare` 在 Redis 写入前一次生成 `type`、`tags`、`silenced` 与 `pushedAt`，后续监听器读取事件载荷中的结果。 [固定 JobPayload](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/JobPayload.php)

PHP 8.5.12 探针执行固定 `JobPayload::prepare`、`JobPushed` 与 `RetryFailedJob` 的重投递载荷构造。普通投递调用对象标签方法一次，原载荷标签与对象标签按顺序合并。公开 ID 优先使用 UUID，旧载荷用 `id` 回退。静默标签与 `Silenced` 标记均生效，`pushedAt` 为数字字符串。无任务对象的原始重投递沿用标签，重置尝试次数和 ID，写入 `retry_of` 并按原期限的相对时长重算截止时间，重新准备时静默为 `false`。证据为 `target/tmp/observability-parity/horizon-job-payload-upstream.log`。 [固定重投递](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Jobs/RetryFailedJob.php)

Rust Horizon 在应用自己的 Redis 写入入口生成观测元数据。即时、延迟与同一连接上的原子任务链均在写入前计算一次标签与静默标志，保存十进制投递时间，后续公开事件和仓储读取同一份载荷。旧任务无该字段时保留原有读取路径。Artisan 与 HTTP 的 Horizon 重试在新投递中保留标签、清除静默、携带原监控投递 ID，并按截止时间相对于原投递时刻的间隔重算。没有投递时刻的旧记录继续使用已保存的绝对期限。Rust 保留各次 Redis 投递 ID 和核心任务 UUID 的既有职责，详见[专项审计](horizon/README.md#必要-rust-适配)。

固定 Horizon 的 `later` 在交给事务延后逻辑之前生成载荷。PHP 8.5.12 探针执行公开 `push`、`later`，经固定核心 `enqueueUsing` 登记到受控事务回调，确认等待提交时即时投递标签调用为零，延迟投递已调用一次，延迟回调捕获写入前的标签和 `pushedAt`。旧失败载荷缺少保存的标签元数据时，Rust 在重投递时使用应用登记的标签回调生成一次。对照证据为 `target/tmp/observability-parity/horizon-delayed-commit-upstream.log`。 [固定延迟入队路径](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/RedisQueue.php)

Rust 原生 Redis 在事务中登记延迟投递时先保存观测元数据，提交回调消费这一份数据。即时投递在提交回调中求值。两种方式都在该回调内写入 Redis 和派发公开入队事件。SQLite 回滚测试确认延迟标签回调已执行，但 Redis 和任务仓储均无新增投递；提交测试改变静默配置，延迟任务仍使用先前的判定，即时任务使用提交时的判定。延迟提交路径已在 macOS 通过真实 Redis 测试，Horizon Redis 20 项、三包默认 84 项、严格 Clippy、全仓格式、三语内容构建和 12 组文档浏览器检查通过。证据为 `target/tmp/observability-parity/horizon-delayed-commit-{verified,docs}.log`。其他事务驱动的同类组合仍待核对。

Linux ARM64 与 Windows 11 ARM64 用同一事务时机代码和 Rust 1.94.0，分别在独占 Redis 8.10.3 上通过该真实 SQLite 提交／回滚专项。证据为 `target/tmp/observability-parity/horizon-delayed-commit-linux.log` 与 `target/tmp/observability-windows/horizon-delayed-commit-arm64.log`。

PHP 8.5.12 探针使用固定上游的四个实际包装类型并调用 `JobPayload::determineType`，验证广播 `broadcast`、监听器 `event`、邮件 `mail`、通知 `notification` 和普通对象 `job`，另通过公开的 `prepare(null)` 验证原始重试回到 `job`。Rust 类型由原生包装入口在创建任务时写入。应用自定义的监听器名称、通知中间件和加密正文均沿用对应入口的类型。Horizon 原始重试的观测类型为 `job`，原生处理器和运输类型随核心载荷保存。旧数据缺少类型时反序列化为 `job`。上游对照证据为 `target/tmp/observability-parity/horizon-job-types-upstream.log`。

实际 HTTP 生产者与 Redis 8.10.3 验证五类载荷，含加密和自定义中间件命名的通知、与内置处理器同名的普通任务、旧版序列化及事件失败后的原生 `horizon:retry`。macOS 隔离快照沿用 `eb189cfb1` 及已提交的 Redis 事件、载荷补齐代码，再叠加本批分类代码。三包默认 84 项、Horizon Redis 19 项、严格 Clippy、快照全仓格式、三语内容构建及 12 组文档浏览器检查通过。Windows 11 ARM64 以相同源码快照在专用 NTFS 卷编译，14 项 Redis Runtime 通过，包含本批类型与上一批重试元数据。证据为 `target/tmp/observability-parity/horizon-job-types-{verified,docs-verified}.log` 和 `target/tmp/observability-windows/horizon-job-types-arm64.log`。完整发布组合待验。

Linux 7.0.14 ARM64 用与 `52848e62b` 可观测性代码一致的快照，Rust 1.94.0 和独占 Redis 8.10.3 冷构建后通过全部 14 项 Horizon Redis Runtime，含类型与上一批载荷重试元数据。日志为 `target/tmp/observability-parity/horizon-job-types-linux.log`。

PHP 8.5.12 探针按固定源码给四类包装设置对应底层对象。广播取事件对象，监听器取监听器类名，邮件取 Mailable，通知取 Notification。实现 `Silenced`、配置命中底层类型或标签任一条件成立时静默；只配置包装类名不命中。无任务对象的原始重试即使保留静默标签也会清除静默状态。Rust 在真实包装生产者记录底层类型，在 `horizon.silenced` 匹配完整类型名，普通任务兼容其注册名称。旧载荷缺少底层类型时沿用任务名称。Rust 在对象擦除后无法自动检查标记 trait，由应用显式调用 `Horizon::silence::<T>()` 注册实现 `Silenced` 的类型。注册与已保存的静默状态均按应用隔离。对照证据为 `target/tmp/observability-parity/horizon-wrapper-silence-upstream.log`。

固定上游 `JobPayload` 的 PHP 8.5.12 探针已确认，ID 优先读取 UUID，缺失或为 `null` 时回退到旧 ID，空字符串仍作为值返回。命令名来自 `data.commandName`，显示名来自顶层 `displayName`。`tags` 缺失时返回空数组，显式 `null` 原样返回。重试字段把缺失和 `null` 视为无值，空字符串视为存在。下标写入和删除只影响解码值，`set` 会合并字段并重新序列化全部解码值。探针也验证了预置标签与任务标签的合并顺序及重复项保留。证据为 `target/tmp/observability-parity/horizon-job-payload-accessors-upstream.log`。

Rust 公开 `JobPayload::new`、`value`、`decoded`、ID 与名称访问器、`tags`、重试与静默检查、`offset_*`、`set` 及 `prepare`。类型化适配将无效 JSON 对象返回为 `Error::InvalidRecord`，标签输入用字符串数组，准备任务时显式接收原生 `Payload`。原生 Redis 记录映射任务注册名、显示名及已持久化的 Horizon 元数据。原有 `JobPayload::from_payload` 函数保留，返回 `JobPayloadMetadata`，保留原快照的方法及序列化形状。显式声明原返回类型的调用方须更新类型名。

原生 `JobPayload::with_tags` 把预置标签交给现有 Redis 载荷生成器，标签回调结果按顺序追加。预置输入独立于每次投递生成的结果。Redis 仓储保留重复项，静默配置也匹配预置标签，Artisan 重试继续使用已保存结果。

macOS 快照通过两个公开 `JobPayload` 测试，实际应用作用域验证 `prepare` 合并标签及静默，独占 Redis 8.10.3 和 SQLite 验证待处理载荷、仓储、原生 Worker 失败及 Artisan 重试。Horizon Redis 套件 21 项、三包默认 86 项、全仓格式、Horizon 全目标严格 Clippy、三语站点构建及 Horizon 页面 12 组浏览器检查通过。浏览器显式使用项目已有 Chromium 1243。证据为 `target/tmp/observability-parity/horizon-job-payload-public-{verified,docs-verified}.log`。Windows 11 ARM64 同源码通过两个公开测试和真实 Redis／SQLite 重试专项，证据为 `target/tmp/observability-windows/horizon-job-payload-public-arm64.log`。

Linux 7.0.14 ARM64 用 `09ef75a91` 的变更源码和 Rust 1.94.0 通过两个公开测试及独占 Redis 8.10.3／SQLite 重试专项。Linux 夹具首次缺少 Redis 容器身份变量，补传后以同一源码复验通过。证据为 `target/tmp/observability-parity/horizon-job-payload-public-linux-verified.log`。

固定 PHP 的 `value` 与 `decoded` 支持独立直接写入，`set` 会重新编号十进制规范形式的 64 位整数键，保留前导零及越界的字符串键。连续且按顺序排列的整数键编码为列表，包含嵌套对象及空数组。Rust 补齐 `value_mut`、`decoded_mut`，接受对象和数组原文并按对应规则合并。现有无效 JSON 和标量输入仍返回类型化错误。验收按 JSON 内容、形状与字段顺序核对，Rust 使用 serde_json 的字符转义。证据为 `target/tmp/observability-parity/horizon-job-payload-array-shapes-upstream.log`。

macOS 已通过公开载荷 3 项、真实 Horizon Redis 21 项、三包默认 87 项、全仓格式、Horizon 全目标严格 Clippy 和三语页面 12 组浏览器检查。Linux ARM64 与 Windows 11 ARM64 用相同变更源码分别通过公开载荷 3 项，以及独占 Redis 8.10.3／SQLite 的真实 Artisan 重试回归 1 项。证据为 `target/tmp/observability-parity/horizon-job-payload-array-shapes-{verified,docs,linux}.log` 与 `target/tmp/observability-windows/horizon-job-payload-array-shapes-arm64.log`。

紧接着核对固定 `Tags`。PHP 8.5.12 实跑确认 `extractExplicitTags` 有标签时优先于 `modelsFor`，空结果回退到模型属性和 Eloquent 集合并去重。`targetsFor` 读取四种包装的底层对象。排队监听器先求监听器标签再求事件标签并去重，两者均收到实际事件，后续普通任务读不到该事件。证据为 `target/tmp/observability-parity/horizon-tags-contract-upstream.log`。 [固定 Tags](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Tags.php)

Rust 复用原生载荷的底层类型和应用作用域。排队监听器额外记录事件类型，`Tags::target_for` 公开底层标签目标，`extract_explicit_tags` 对监听器合并两组标签，普通目标仅返回其显式标签。`Tags::for_payload` 在目标显式标签为空时回退到该目标注册的模型标签，监听器与事件各自回退。`Horizon::tag_for` 和 `tag_models_for` 负责应用内类型注册，既有 `Horizon::tag` 仍提供全局附加标签。`Tags::model_tags_for` 读取目标注册的模型标签，`for_model` 与 `for_models` 从实际原生模型或集合生成有序标签。应用回调在解锁后执行。

Rust 的类型检查可确认具体类型，模型字段仍由应用回调显式声明。回调接收原生队列载荷，监听器与事件能读取同一份序列化事件。邮件原对象中未进入发送载荷的字段、旧监听器载荷缺少的事件类型，以及加密通知应用回调的解密方式仍作为载荷边界。验收夹具用现有 Crypt 读取已加密通知的发送内容并仅保存返回的标签。 [Rust Any](https://doc.rust-lang.org/std/any/trait.Any.html) [Laravel Horizon 标签](https://laravel.com/framework/docs/13.x/horizon#tags)

固定 Horizon `Tags::for` 拼接实际模型类型和主键。PHP 8.5.12 用非自增模型执行固定方法，验证缺失、null、false 得到空后缀，true 得到 `1`，浮点值及普通字符串保留相应标量文本。Rust 补齐 `Tags::for_model` 的标量转换，浮点数沿用框架的 PHP 精度格式化；结构化值和无效 UTF-8 字节仍返回错误。有序标签集合维持首次出现去重。回归通过实际 HTTP 派发和 Redis 任务详情检查空后缀、布尔与浮点标签。PHP 证据为 `target/tmp/observability-parity/horizon-tags-model-keys-upstream.log`。

集合去重另有差异。PHP 8.5.12 用实际 Eloquent Collection 验证，相同实例或全部状态相同的实例在 `modelsFor` 去重。同一主键但当前属性、原始属性快照或自定义属性不同的模型各自保留。false 与 null 主键也可产生两个相同标签。固定 `JobPayload::prepare` 将重复标签原样附在预置标签后。Rust 原按标签文本去重，本批改为先比较模型。科学计数法 `1.23E+30` 同一探针亦已确认。证据为 `target/tmp/observability-parity/horizon-tags-model-collection-upstream.log`。

Rust 将模型去重移到生成标签前，默认比较原生模型的表和连接，以及当前属性、原始快照、变更、生命周期和可确认的关联状态，并保留不同模型生成的同名标签。应用可通过 `Tags::for_models_by` 为自定义字段或独立装载的关联提供完整比较规则。普通任务的模型标签回调保持原序和重复项，排队监听器仍按上游规则合并去重。补充 PHP 探针确认，其他状态相同的模型在整数 `4`／字符串 `"4"` 或 false／null 时会去重。默认属性比较因此复用核心的 PHP 松比较。末次 Rust 回归同时检查应合并的两组标量和不同模型在 Redis 载荷内保留的同名标签。PHP 证据为 `target/tmp/observability-parity/horizon-tags-model-scalar-equality-upstream.log`。

macOS 隔离快照通过标签及通知 6 项，4 项按环境条件忽略。独占 Redis 8.10.3 上的真实 HTTP、任务仓储和 Artisan 重试专项通过。所涉严格 Clippy 与框架关系回归通过，另在 MySQL 8.4.13 实跑关系用例。三语站点构建及包含 `Tags::for_models_by` 的 12 组浏览器检查通过。证据为 `target/tmp/observability-parity/horizon-model-collection-{macos-loose,mysql,docs}.log`。默认状态比较对不透明的模型自定义字段或独立装载关联使用显式比较入口，其他目标平台的 Redis 端到端及完整发布组合继续验收。

macOS 隔离快照的标签及通知套件 5 项通过、4 项按环境条件忽略。真实 Redis 8.10.3 上的 HTTP 派发、任务仓储和 Artisan 重试专项通过，覆盖本批空后缀、布尔和浮点主键。三语站点构建通过，Telescope 与 Horizon 各通过 12 组浏览器检查，生成页面包含文件附件入口和新主键规则。证据为 `target/tmp/observability-parity/{horizon-model-keys-macos,file-and-model-tags-docs}.log`。Horizon 所涉严格 Clippy 通过，Linux ARM64 通过标量主键测试。真实 Redis 的其他平台复验继续执行。

真实 HTTP 生产、Redis 8.10.3、SQLite 与 Artisan 验证普通任务、四个包装入口、应用间隔离和原始重试保留标签。macOS 通过 Horizon Redis 21 项、全仓格式、Horizon 全目标严格 Clippy 和三语页面 12 组浏览器检查。三包组合遇 32 GB 停线后由构建守卫持锁清理，同源码冷构建通过三包默认 87 项。Windows 11 ARM64 通过同源码真实 Redis 专项。证据为 `target/tmp/observability-parity/horizon-tags-target-{focused,verified,packages,docs}.log` 和 `target/tmp/observability-windows/horizon-tags-target-arm64.log`。

Linux 7.0.14 ARM64 用 `5e92024f0` 的变更源码、Rust 1.94.0 和独占 Redis 8.10.3 冷构建，通过真实 HTTP 生产、SQLite／Worker 失败与 Artisan 重试的标签专项。证据为 `target/tmp/observability-parity/horizon-tags-target-linux.log`。

macOS 冷构建在真实 Redis 8.10.3 验证广播、带自定义名称的监听器、邮件、加密通知及普通任务。覆盖配置底层类、配置只有包装名称、应用间显式注册隔离以及真实 `horizon:retry` 重置静默结果。Horizon Redis 20 项、三包默认 84 项、严格 Clippy、全仓格式、三语文档和 12 组浏览器检查通过。Windows 11 ARM64 用同一源码、专用 NTFS 构建卷和独占 Redis 8.10.3，通过 15 项 Redis Runtime。证据为 `target/tmp/observability-parity/horizon-silenced-{accepted,docs}.log` 及 `target/tmp/observability-windows/horizon-silenced-arm64.log`。完整发布组合待验。

Linux 7.0.14 ARM64 用同一源码快照、Rust 1.94.0 和独占 Redis 8.10.3 冷构建后通过 15 项 Horizon Redis Runtime。证据为 `target/tmp/observability-parity/horizon-silenced-linux.log`。

固定 Laravel `FailoverQueue::later` 在调用目标、登记事务回调时处理 failover。PHP 8.5.12 探针使用两个固定 Horizon RedisQueue 子类和受控事务回调验证，首个准备成功的连接在提交前确定并只生成一次标签。只有登记前的同步失败才会选择下一连接，已登记回调在提交后写入失败时异常交回事务，第二连接不会被调用。对照日志为 `target/tmp/observability-parity/horizon-failover-commit-upstream.log`。 [固定 FailoverQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/FailoverQueue.php)

Rust 的活跃事务派发由 failover 逐个委派给实际候选连接。首个接收任务的连接使用现有事务机制，同时保留派发级和包装连接的提交设置。连接解析及提交回调登记期间发生的错误沿用现有 failover 事件和后续尝试。已选连接的写入在提交回调中执行，错误返回 `DatabaseError::AfterCommit`。以独占 Redis 8.10.3 的两个逻辑库与 SQLite 验证回滚、选定后失败、首连接解析失败并使用备用连接的 `after_commit` 默认值、标签计数和实际连接名。

macOS 快照通过 Horizon Redis 21 项、三包默认 84 项、严格 Clippy、全仓格式、三语文档与 12 组浏览器检查。Windows 11 ARM64 用同一源码快照和独占 Redis 8.10.3 通过 failover 专项。证据为 `target/tmp/observability-parity/horizon-failover-commit-{verified,docs}.log` 与 `target/tmp/observability-windows/horizon-failover-commit-arm64.log`。

macOS 用与 `0493e6e2d` 变更源码一致的快照冷构建，通过独占 Redis 8.10.3 上的真实 Artisan 进程矩阵。验证 Worker 选项、退避、维护模式、分钟级冷却、进程恢复、暂停、重试、正常关停及 HTTP 授权和脱敏。证据为 `target/tmp/observability-parity/horizon-failover-commit-process-verified.log`。

Linux 7.0.14 ARM64 用同一变更源码和 Rust 1.94.0 冷构建，在独占 Redis 8.10.3 与 SQLite 上通过 failover 专项。证据为 `target/tmp/observability-parity/horizon-failover-commit-linux-verified.log`。Linux x86_64、其他事务驱动和完整发布组合仍待分别验收。

基于 `eb189cfb1` 加已提交 Redis 事件与本批文件的 macOS 隔离快照，独占 Redis 8.10.3 和 SQLite 验证 `JobPending`、`JobPushed`、保留和结算读取相同的标签及静默结果。原生 Artisan 验证标签回调仅在入队执行一次、成功完成后保留原静默结果、Horizon 重试保留标签及原投递引用并重算截止时间。授权 HTTP 验证旧失败任务的重投递和标签回退。Horizon Redis 18 项与三包默认 84 项、无 Horizon 的框架 Redis Worker、真实 Artisan 进程矩阵、全仓格式、Horizon 全目标严格 Clippy、三语站点和 12 组浏览器检查通过。新载荷行为的 Linux、Windows 和完整发布组合待验。验证日志位于 `target/tmp/observability-parity/horizon-payload-{verified,core,process,docs}.log`。

### Horizon Redis 原子任务链入队事件

固定 Laravel 完成任务时先派发后继任务，再删除前一任务。固定 Horizon 分别在即时和延迟入队的 Redis 写入前后派发 `JobPending`、`JobPushed`。Rust 后继任务在同一 Redis 连接上推进时，两个事件复用普通入队的消息和同一个新投递 ID。`JobPending` 在 Redis 原子操作前派发。Lua 同时完成入队和删除前一保留，成功后先派发后继任务的 `JobPushed`，再派发前一任务的 `JobDeleted`。此时监听器读取 Redis 会看到原子变更已完成。 [固定任务链完成顺序](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/CallQueuedHandler.php) [固定 Redis 派发入口](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/RedisQueue.php)

原子脚本继续核对旧保留和目标 Redis 键，保留失效时可能已有 `JobPending`，且不会派发 `JobPushed` 或 `JobDeleted`。入队前监听器报错时可以重试同一保留，入队后监听器报错时记录日志并完成旧任务的通知，避免重复推进。显式队列名复用 Redis 入队校验。

基于 `eb189cfb1` 加失败事件与本批文件的 macOS 隔离快照，独占 Redis 8.10.3 经 HTTP `Bus::chain`、实际 `horizon:work` 命令和公开事件监听器验证延迟跨队列推进。队列名保留前置空格、Unicode 和冒号。直接调用 `Job::advance_chain_to` 验证即时入队、清除后失效的保留、无效队列名和前后监听器报错，后继任务载荷保留大整数且只能推进一次。三包默认 84 项与 Horizon Redis 18 项、快照全仓格式、Horizon 全目标严格 Clippy、三语站点和 12 组浏览器检查通过。证据为 `target/tmp/observability-parity/horizon-redis-chain-{focused,accepted,docs}.log`。

同一快照在 Linux 7.0.14 ARM64、Rust 1.94.0 下编译原生测试程序，独占 Redis 8.10.3 复验本节 `JobPending`／`JobPushed` 与下节 `JobFailed` 两个实际入口，均通过。使用编译后的同一程序执行测试，显式向夹具传入专用 Redis 容器标识。证据为 `target/tmp/observability-parity/horizon-redis-events-linux-{serial,verified}.log`。本次 Linux 证据覆盖这两个行为的定向测试，完整 Release 组合仍待验。

同一快照使用 Windows 11 ARM64 虚机、Rust 1.94.0 `aarch64-pc-windows-msvc` 与专用 NTFS 构建卷，在独占 Redis 8.10.3 上通过两个原生行为测试。覆盖公开失败事件的 Worker 回调与持久确认顺序，以及任务链的即时、延迟和无效保留路径。证据为 `target/tmp/observability-windows/horizon-redis-events-arm64.log`。完整 Windows 发布组合仍待验。

### Horizon Redis 释放与删除事件

固定 `RedisQueue` 在释放或删除保留任务后派发事件。释放监听器写入待处理状态和延迟，成功删除监听器完成任务并记录从保留开始的运行时间；失败删除不计为成功。Rust 在实际 Redis 变更确认后桥接公开事件，任务链原子推进时也观察旧保留记录的删除。事件给应用传递拥有所有权的保留快照，`JobReleased::NAME` 使用 `Horizon.JobReleased` 区分核心 Worker 同名事件。任务详情提供可选的 `release_delay_ms`，旧记录无需迁移。原生尝试结算保留失败、重试和显式释放的区别，成功删除与 Worker 最终事件共用幂等记账。固定 PHP 同步监听器可将异常传回调用者。Rust 对已确认的队列变更记日志，保留确认结果及任务句柄的终态，防止重复操作。 [固定释放监听器](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/MarkJobAsReleased.php) [固定完成监听器](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/MarkJobAsComplete.php) [固定指标监听器](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/UpdateJobMetrics.php)

基于 `86b602de9` 加本批文件的 macOS 隔离快照，真实 Redis 8.10.3 验证同步 Listener、授权 HTTP 任务详情、直接释放和删除、失效和重复确认、任务链成功推进、Worker 永久失败及退避重试。额外验证观察者错误时 Redis 结果和句柄标记一致，成功和失败各自只记账一次。三包默认测试共 83 项、Horizon Redis 专项 14 项、真实 Artisan 进程矩阵和无 Horizon 的框架 Redis HTTP／阻塞取任务均通过。快照全仓格式、Horizon 全目标严格 Clippy、三语站点和 12 组文档浏览器检查通过。证据为 `target/tmp/observability-parity/horizon-redis-settlement-*.log`。本批 Linux、Windows 和发布组合待验。

队列名称与已确认保留后的观察错误见下节。

### Horizon 队列名称与保留确认

固定 Supervisor 按逗号分组，固定负载仓储只用首个冒号拆分连接和队列组。Rust 监督器选项、任务仓储和队列指标现在复用原生 Redis 队列名校验。队列名保留空格、Unicode 字符和冒号，单个名称上限为 255 字节。直接传输允许字面逗号，监督器列表的逗号用于分组。控制字符和 Redis 内部集合后缀均被拒绝；连接名仍沿用原有规则。 [固定 Supervisor](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Supervisor.php) [固定负载仓储](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Repositories/RedisWorkloadRepository.php)

原生 Redis 已确认保留后，观察器错误经日志报告，任务句柄仍交给 `pop` 调用方。派发仍是同步的，应用监听器可按顺序读取仓储中的保留状态。PHP Redis 对假值队列名（含字符串 `0`）使用默认队列的规则须单独核对；本批沿用 Rust 对显式名称的字面解析。

基于 `178a87b58` 加本批文件的 macOS 隔离快照，独占 Redis 8.10.3 验证 Unicode、空格、冒号、字面逗号和 255 字节名称，经公开监听器读取保留状态，并在观察器返回错误后仍完成任务。真实 Artisan Master、Supervisor、Worker 验证包含前置空格的分组名称传递和任务执行，授权 HTTP 返回原队列名、分组及对应负载。Horizon 默认 25 项、Redis 专项 15 项、真实进程矩阵、无 Horizon 的框架 Redis HTTP／阻塞取任务回归、全仓格式、Horizon 全目标严格 Clippy、三语站点及 12 组浏览器检查通过。证据为 `target/tmp/observability-parity/horizon-queue-names-*.log`。本批 Linux、Windows 和发布组合待验。

### Horizon Redis JobsMigrated

固定 Redis Queue 按延迟、过期保留顺序逐批迁移，分别完成派发后才继续保留。Horizon 的 `JobsMigrated` 提供连接、队列和包含投递 ID、尝试次数、原生载荷的 `payloads`，空批次保留事件。Rust 在注册原生迁移监听器时逐批执行 Redis 脚本，语义与固定上游的监听位置一致。未订阅时仍走原有的单次原子迁移与保留。现有阻塞等待会重新检查到期项，各次检查可派发对应批次。 [固定迁移事件](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Events/JobsMigrated.php) [固定迁移监听器](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/MarkJobsAsMigrated.php)

迁回 Ready 后把原投递标为待处理并把延迟清零，保留已有详情和有效期。仓储比较同一投递的尝试次数及当前终态，通过 Redis 原子比较旧记录防止迁移事件覆盖新保留或已完成任务。变更确认后的观察器错误沿用日志隔离。

基于 `289e12714` 加本批文件的 macOS 隔离快照，独占 Redis 8.10.3 实测延迟、过期保留和空批次，监听器在保留前读取各阶段 Redis 数量、尝试次数及任务详情。授权 HTTP 和近期投递投影均返回待处理，延迟清零且大整数载荷保持原值。旧保留句柄不能删除已迁移任务，回放旧事件不能覆盖保留或终态。Horizon 默认 25 项、Redis 专项 16 项、真实 Artisan 进程矩阵、无 Horizon 的框架 Redis HTTP／阻塞取任务、全仓格式、Horizon 全目标严格 Clippy、三语站点及 12 组浏览器检查通过。证据为 `target/tmp/observability-parity/horizon-redis-migration-*.log`。本批 Linux、Windows 和发布组合待验。

### Horizon Redis JobFailed

固定 `MarshalFailedEvent` 只桥接实际 Redis Job。Rust 同样以实际 Redis 保留来源决定是否派发 Horizon `JobFailed`，Failover 包装连接保留底层 Redis 连接。公开事件提供连接、拥有所有权的保留快照和完整异常链文本，`JobFailed::NAME` 使用 `Horizon.JobFailed` 区分核心 Worker 同名事件。Horizon 把确认后的失败投影为任务详情和失败标签。Worker 沿用先持久化失败记录再确认删除的顺序，失败回调结束后派发。恢复已持久化失败时补发并沿用较早持久化的异常。直接 `Job::fail` 确认后派发 Horizon 事件，通用失败提供器的持久化和失败回调仍由 Worker 生命周期负责。重复调用已确认的句柄不会重复派发。固定 PHP `finally` 可在删除抛错时仍派发，Rust 要求实际确认成功再派发运输层事件。固定 PHP 失败监听器类型的独立注册接口仍待核对。 [固定失败事件](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Events/JobFailed.php) [固定失败状态监听器](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/MarkJobAsFailed.php) [固定失败标签监听器](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/StoreTagsForFailedJob.php)

实时失败与重投递恢复共用文本异常契约，历史失败记录保存的也是文本。固定 PHP 监听器可检查异常对象类型，实时事件是否保留等价的原生类型信息仍须核对。

基于 `eb189cfb1` 加本批文件的 macOS 隔离快照，独占 Redis 8.10.3 与 SQLite 验证实际 `horizon:work` 的终态失败回调、数据库记录、Redis 保留删除、失败详情与标签的可见顺序。同步队列仍派发核心失败事件，实际落到 Redis 的 failover 队列派发 Horizon 事件。验证手动失败、重复确认、保留失效以及历史失败记录恢复。Horizon、Pulse、Telescope 默认测试共 84 项，Horizon Redis 专项 17 项，无 Horizon 的核心 SQLite 和 Redis Worker、真实 Artisan 进程矩阵、快照全仓格式、Horizon 全目标严格 Clippy、三语站点与 12 组浏览器检查通过。证据为 `target/tmp/observability-parity/horizon-redis-failure-{accepted,core,docs}.log`。Linux ARM64 与 Windows ARM64 专项随上一节复验通过，完整发布组合待验。

### Horizon WorkerProcessRestarting 公开事件

固定 `WorkerProcess::restart` 仅在原进程已经启动过时派发 `WorkerProcessRestarting`，然后启动该 Worker。固定 `ProcessPool::restart` 则缩容到零再扩容。Rust 在已退出 Worker 实际补位前派发公开事件，保留排队期间的退出信息。事件提供监督器、队列、原 PID（可用时）及退出码。Rust 事件传递拥有所有权的快照，进程句柄继续由监督器拥有。初次启动、扩容新增和主动停止不派发。监听器失败记录日志并继续补位。旧版 OpenRust 提供原生队列 Worker，未提供 Horizon 监督器事件。 [固定 WorkerProcess](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/WorkerProcess.php) [固定 ProcessPool](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/ProcessPool.php)

基于 `ebeb57315` 加本批文件的 macOS 隔离快照。真实 Artisan 进程和 Redis 8.10.3 验证首次启动无事件；向实际 Worker 发送 SIGKILL 后，监听器恰收到一条包含旧 PID、退出码 137、正确监督器和队列的事件。监听器返回错误后，新 Worker 正常补位。Horizon 默认 24 项、隔离 Redis 专项 12 项、全目标严格 Clippy、全仓格式和三语文档及 12 组浏览器检查通过。Linux ARM64 使用 `8745bc5fa` 加本批 Worker 事件改动优化构建；隔离 Redis 8.10.3 上 Worker、孤儿回收及包含新事件断言的完整进程矩阵通过。正常退出的冷却规则及连续启动失败事件仍需对账。证据位于 `target/tmp/observability-parity/horizon-worker-restart*.log` 与 `target/tmp/horizon-linux/acceptance-worker-restart.log`。

### Horizon MasterSupervisorDeployed 公开事件

固定 `ProvisioningPlan::deploy` 先为每个启用的监督器入队，再派发 `MasterSupervisorDeployed`。PHP 8.5.12 参考探针实际执行固定计划与事件派发，确认未匹配环境无事件、匹配的空计划和全停用计划均派发事件，普通计划先登记监督器再派发。Rust 在原生监督器描述准备后、主循环启动前派发事件，供应用读取 Master 名称、环境和待启动监督器。Artisan 读取环境配置时保留匹配状态，避免在过滤掉停用监督器后丢失事件；手工构造 Master 时根据传入的监督器计划派发。默认启动和恢复行为沿用现有进程流程。 [固定 ProvisioningPlan](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/ProvisioningPlan.php) [固定部署事件](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Events/MasterSupervisorDeployed.php)

基于 `936c84133` 加本批文件的 macOS 隔离快照，真实 Artisan 与 Redis 8.10.3 验证四种计划及公开事件字段。应用监听器返回错误后，匹配计划仍进入主循环并可正常关停；有监督器的计划正常启动 Worker。Horizon 默认 24 项、Redis 专项 12 项、全目标严格 Clippy、全仓格式、三语文档及 12 组浏览器检查通过。本批未运行该事件的 Linux／Windows 或发布组合。证据位于 `target/tmp/observability-parity/horizon-master-deployed*.log`。

### Horizon Worker 冷却与 UnableToLaunchProcess

固定 `WorkerProcess` 的 PHP 8.5.12 探针使用可控时钟和进程适配器，经真实事件派发确认两条分支：进程以 0 退出时，在首次启动后 500ms 监控可立即重启；以 1 退出时，500ms 不重启，满 1 秒后依次派发 `WorkerProcessRestarting`、`UnableToLaunchProcess` 并再次启动。固定进程池按原顺序缩容，先取消最旧的席位。原始参考为 `target/tmp/observability-parity/horizon-worker-cooldown-upstream.log`。 [固定 WorkerProcess](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/WorkerProcess.php) [固定 ProcessPool](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/ProcessPool.php)

分钟冷却探针继续运行同一席位。正常退出仍可补位；随后再次异常退出，在原冷却期限前 1ms 仍未补位，恰好到期时依次派发两个事件并启动。证据为 `target/tmp/observability-parity/horizon-worker-cooldown-minute-upstream.log`。

Rust 监督器按席位保存退出快照和冷却期限。冷却席位占用扩缩容预算，心跳的 Worker 列表仅发布存活的 PID。冷却到期后，公开的 `UnableToLaunchProcess` 在 `WorkerProcessRestarting` 之后、启动替代进程之前派发，携带监督器、队列、原 PID 与退出码。正常退出可在冷却期间补位并沿用原期限。缩容可直接取消等待补位的席位，仍存活的缩容进程走原有排空。监听器错误保留日志和补位。原生进程创建调用直接返回错误的重试见下节。冷却期间的多队列缩容专项及目标平台组合仍待核对。

基于 `53718e11c` 加本批文件的 macOS 隔离快照，真实 Artisan 和 Redis 8.10.3 验证 SIGKILL 后事件顺序与原 PID、`maxJobs` 正常退出在冷却内只派发重启事件、随后再被杀死时停止补位直至原冷却窗口满一分钟，到期后按序派发事件并补位。Horizon 默认 24 项、隔离 Redis 专项 12 项、快照全仓格式和包全目标严格 Clippy 通过。三语文档构建及 12 组浏览器检查通过。本次冷却改动的 Linux 优化构建与发布组合仍待验收。证据位于 `target/tmp/observability-parity/horizon-worker-cooldown-macos-final.log` 和 `horizon-worker-cooldown-docs.log`。

### Horizon Worker 进程创建错误与恢复

PHP 8.5.12 使用固定 Horizon `WorkerProcess`、Symfony Process 8.0.5 和真实不存在的工作目录。首次创建调用抛出异常，999ms 时跳过。满一秒后派发一条 `UnableToLaunchProcess`，再次创建失败。补回目录后，在同一席位的分钟期限前仍然跳过，到期时再派发一次并启动真实子进程。两次事件均无先前已启动进程，所以没有 `WorkerProcessRestarting`，退出码为空。固定 `Supervisor::loop` 报告此类异常并继续后续轮询。证据为 `target/tmp/observability-parity/horizon-worker-spawn-upstream.log`。 [固定 Supervisor](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Supervisor.php) [Symfony Process 8.0.5](https://github.com/symfony/process/blob/v8.0.5/Process.php)

Rust 将进程创建错误保留在原席位的冷却周期，记日志后继续监督。缺少已启动子进程时，`UnableToLaunchProcess` 的 PID 和退出码为空。进程实际退出后继续保留原字段和事件次序。固定上游创建异常会中止当次监督循环的剩余操作，多席位同时创建失败的处理顺序见下节。

在 macOS 隔离快照中叠加两批改动。实际 Artisan 与 Redis 8.10.3 验证初次启动遇到不存在的工作目录时监督器仍正常运行，监听器收到空 PID 与空退出码，补回目录后按分钟冷却期限恢复，关闭后无残留 Worker。包含前批两种正常／异常退出分支的进程矩阵通过，包默认 24 项与独立 Redis 12 项通过，全仓格式、包全目标严格 Clippy、三语站点构建和 12 组文档浏览器检查通过。证据位于 `target/tmp/observability-parity/horizon-worker-spawn-macos.log` 和 `horizon-worker-spawn-docs.log`。

在新的独占 Redis 8.10.3 容器上，使用同一 macOS 二进制开启浏览器选项重跑真实进程矩阵。Chrome 桌面和移动视口验证授权、任务详情、监控标签写入、指标、CSP 刷新与布局，进程矩阵也通过。证据为 `target/tmp/observability-parity/horizon-worker-spawn-browser.log`。

Linux ARM64 以 `3515bafe8` 的进程范围源码重新构建 Release。首轮因触及 32 GB 停线中断，清理后确认原构建容器退出，再用 Rust 1.94.0、单作业完成优化构建。Linux 7.0.14、Debian glibc 2.36、独占 Redis 8.10.3 上，原生 Worker、孤儿清理、完整进程及 HTTP 矩阵均通过。两条一分钟路径均实际等待到期，分别验证异常退出补位及缺失工作目录恢复，最后关闭 Redis 并验证进程回收。证据为 `target/tmp/horizon-linux/worker-spawn-release.log`。Windows ARM64 结果见下节。


### Horizon 监督循环的多席位创建失败顺序

固定 PHP 8.5.12 通过实际 `Supervisor::loop`、`ProcessPool::monitor`、`WorkerProcess` 与 Symfony Process 8.0.5 验证两席位。不存在的工作目录触发真实创建错误。首个席位失败后本轮只报告该错误。下轮会跳过仍在冷却的席位并检查第二个席位。首个席位再次失败时，即使第二个目录已恢复也推迟到下一轮启动。创建失败的轮次不会持久化本轮状态或派发 `SupervisorLooped`。证据为 `target/tmp/observability-parity/horizon-worker-multiple-upstream.log`。

Rust 原先继续检查后续席位，并在创建进程报错的轮次派发 `SupervisorLooped`。使用真实 `horizon:supervisor` 和隔离 Redis 8.10.3 复现，第一次循环事件前的 `UnableToLaunchProcess` 清单为空。现在在监控开始时先发布初始状态，各席位保留原冷却。进程创建错误交给应用异常处理器并结束当轮检查，下一轮继续。公开命令验收核对 Redis 初始状态、两队列事件顺序、原生进程 I/O 错误进入报告回调及正常退出。macOS 的此项和原有六条结构化命令均通过。证据为 `target/tmp/observability-parity/horizon-worker-multiple-rust-baseline.log` 与 `target/tmp/observability-parity/horizon-worker-multiple-macos.log`。

同一 macOS 快照的原生进程矩阵通过，包括异常退出补位、一分钟冷却、工作目录恢复、命令与信号排空、实际 HTTP 和授权。包默认 32 项及隔离 Redis 8.10.3 的 23 项通过，快照全仓格式和包全目标严格 Clippy 通过。三语站点、12 组通用浏览器检查及 Horizon 章节的三语桌面与移动 6 项通过。证据为 `target/tmp/observability-parity/horizon-worker-multiple-{macos,macos-redis,docs,docs-browser,docs-chapter}.log`。最终部署组合继续验收。


Linux 首轮测试假定两次重试事件均发生在第一次成功循环之前。固定上游允许两个席位都在冷却的轮次正常完成。验收改为确认首次正常循环前已报告初始创建错误，再等待两次有序的重试事件和四次报告。macOS 与 Linux ARM64 的双席位及结构化 Artisan 专项重跑通过。Linux 7.0.14 ARM64、Rust 1.94.0 与隔离 Redis 8.10.3 同时通过实际异常退出补位、一分钟冷却、工作目录恢复、信号排空及 HTTP 和授权。证据为 `target/tmp/observability-parity/horizon-worker-multiple-{macos-timing,linux-initial,linux}.log`。

Windows 11 ARM64 对 `f21ba5bcb` 的工作进程实现复跑原生进程矩阵，覆盖空计划、排空等待、快速终止、父端退出、应用处理器、Redis 与文件命令队列。双席位单独在干净配置中调用实际 Artisan，验证了初始监督器状态、两条重试事件、四次原生 `NotADirectory` 异常报告和正常终止。初次探针受前一组测试环境变量和 PowerShell 多行状态匹配影响，隔离环境并聚合输出后重跑通过。证据为 `target/tmp/observability-windows/horizon-worker-multiple-arm64{,-targeted}.log`。

### Horizon 监督循环等待间隔

固定 PHP 的 Master 与 Supervisor 监控入口在初次持久化后等待一秒，再在每轮处理完成后等待下一秒。实际运行固定 `monitor`，以耗时约 1200ms 的循环夹具验证两个等待各约 1000ms。Rust 原先使用 Tokio 固定间隔。实际 Artisan 在慢监听器结束后约 8ms（Master）和 6ms（Supervisor）即开始下一轮。本次改为在首轮前和每轮结束后设置新的单调时钟期限，等待中仍处理关停及控制信号。参考探针输出位于 `target/tmp/observability-parity/horizon-monitor-cadence-upstream.log`。 [固定 Master](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/MasterSupervisor.php) [固定 Supervisor](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Supervisor.php)

macOS 上以真实 Artisan、隔离 Redis 8.10.3 和 1200ms 同步监听器对照。修复后首轮约 1076ms／1028ms，慢监听器结束后的等待约 1007ms／1006ms。两条命令均在等待期间接到 SIGTERM 并正常退出，实际退出早于下一次轮询。证据为 `target/tmp/observability-parity/horizon-monitor-cadence-macos{,-baseline}.log`。

同一 Rust 快照的原生进程与结构化命令矩阵、包默认 32 项、隔离 Redis 23 项、快照全仓格式和包全目标严格 Clippy 通过。三语文档构建、12 项通用浏览器检查及 Horizon 章节的三语桌面和移动 6 项通过。证据为 `target/tmp/observability-parity/horizon-monitor-cadence-{macos,docs,docs-chapter}.log`。相同源码在 Windows 11 ARM64 实际 Artisan 与隔离 Redis 8.10.3 上验证了两种监控入口。Master 首轮约 1070ms，工作后约 1028ms。Supervisor 分别约 1064ms、1022ms。通过原生命令请求两个进程，各自正常退出。证据为 `target/tmp/observability-windows/horizon-monitor-cadence-arm64.log`。Linux 7.0.14 ARM64 使用相同源码、Rust 1.94.0 和隔离 Redis 8.10.3。Master 首轮约 1043ms，慢监听器结束后等待约 1013ms，Supervisor 对应为 1033ms 和 1007ms。两条真实命令均在等待中响应 SIGTERM 并正常退出。结构化命令、双席位失败、完整运行进程、HTTP 与授权矩阵均通过。证据为 `target/tmp/observability-parity/horizon-monitor-cadence-linux.log`。本批优化构建继续验收。

### Horizon Windows ARM64 应用装配与条件编译

Windows 11 ARM64 虚机的 Parallels 共享目录在 Rust 写归档时返回错误 87。构建切换到镜像位于项目临时目录的独立 NTFS 盘，并在本次命令的 PATH 中接入虚机已有的 ARM64 Clang 19.1.5 和 CMake。Rust 1.94.0 的原生目标为 `aarch64-pc-windows-msvc`。

修复 Windows 编译发现的两个 Unix 专用导入和一处不可达表达式。同一虚机重跑 Horizon 默认 24 项、Horizon 实际 Artisan 配置入口一项与默认骨架两项，全部通过，Horizon 编译无告警。macOS 的实际孤儿清理、完整进程矩阵、全仓格式和包全目标严格 Clippy 通过。Windows 测试未运行 Redis 专项或 Unix 信号矩阵。证据为 `target/tmp/observability-windows/horizon-*-ntfs-clean.log` 和 `target/tmp/observability-parity/horizon-windows-cfg-macos.log`。

验收结束后虚机已恢复挂起。Parallels 需关机才能拆除新增磁盘，本批保留原虚机挂起会话和项目内的验收盘。

### 三个扩展同时启用时的 Artisan 配置验收

三个根应用隔离验收原先从已经迁入框架的根 `lang/` 目录复制迁移注释，导致测试在运行 Artisan 前失败。按当前框架翻译资源位置修正三份夹具。macOS 同时启用 `horizon,pulse,telescope` 后，默认骨架两项及三个扩展的配置入口各一项通过，实际覆盖包发现、配置发布、有缓存时的读取及清缓存。Pulse 另在 SQLite、独占 PostgreSQL 18.7 和 MySQL 8.4.13 下执行真实迁移和命令。证据为 `target/tmp/observability-parity/observability-combined-artisan-*.log`。
