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
| Telescope `ClientRequestWatcher::input` | 已捕获构建器提供的 multipart 普通字段及 `attach_body` 附件的文件名、长度、头部。普通字段先于附件发送，同名取最后一项，复用请求字段／头部脱敏。原生 `attach` 保留发送契约。 | 原生 reqwest `Part` 元数据和中间件替换流的边界见[包审计](telescope/README.md#出站-multipart)。 |
| Telescope 非 multipart 出站载荷 | 诊断从原始 GET 地址或结构化正文选择载荷，并与线上 URL 分别记录。已核对 JSON／表单 POST、GET 查询覆盖、HEAD、原始正文、自定义媒体类型与表单标量快照。成功重定向保留原始输入，连接失败从实际缓冲正文解码。 | multipart 重定向、重试回调改写与其他中间件组合仍需对账。 |
| Telescope `FetchesStackTrace`、`QueryWatcher`、`GateWatcher` | 固定上游按路径子串跳过被忽略的帧，再选第一处合格来源。找不到合格帧时 Query 不入库，Gate 仍入库且行号和文件为空。Gate 已提供授权入口位置，Query 已从核心 SQL、分页、模型写入、直接关系读取及多对多中间表入口收集候选。 | Gate 外层候选、关联模型写入和读取时创建的变体、schema、懒加载入口，以及发布构建路径重映射仍须逐项核对。 |
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
| Horizon | 先核对面板名称与配置默认值、等待时间与通知，再核对管理命令、监督器与 Worker、进程恢复、标签、仓储、指标和面板操作。Redis Cluster 沿用固定上游支持边界。 |
| 发布验收 | 跟踪 Fortify 三个测试目标编译深度溢出，复验最终交付快照的全仓门禁。锁定提交、依赖、目标平台和实际数据库／Redis／Worker 组合，验收迁移、正常关停、故障恢复、业务端到端及容量。平台与供应商所需真实条件按[发布待办](../todo.md#平台与发布条件)记录。 |

### Horizon 面板名称与配置默认值

固定上游[配置](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/config/horizon.php)与[面板模板](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/resources/views/layout.blade.php)均已核对。补齐 `HORIZON_NAME`，将可选名称作为文本显示于页面标题与主标题，同时发布 `waits.redis:default=60`、空静默清单以及 Supervisor 的 `nice=0`。现有等待时间监控、静默行为及进程参数的完整行为对账继续保留。

隔离快照沿用 `8041ae15a` 与前批文件，叠加本批。配置消费者 5 项及隔离 Redis 8.10.3 的双应用配置隔离 1 项通过。真实 Redis 8.10.3 进程／HTTP／Artisan 回归、全仓格式、Horizon 全目标严格 Clippy、三语文档构建及 12 组浏览器检查通过。发布组合与其他目标平台仍待专项复验。

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

### Telescope 出站 multipart 构建器快照

真实回环 HTTP 与 SQLite 应用仓储覆盖上传成功、连接被拒、脱敏、原生 `Part` 兼容及两种重复字段覆盖。服务端检查附件字节、文件名和上传顺序。框架回归验证未知长度流到发送阶段才被读取，正文流只执行一次，快照长度为空。验证使用 `8041ae15a` 加前批和本批文件的隔离快照。Telescope 32 项、框架 HTTP 客户端 20 项、快照全仓格式、框架库及 Telescope 全目标严格 Clippy、三语站点及 12 组浏览器检查通过。Telescope 独立 Redis 用例本批未执行；出站 multipart 未遍历部署数据库或目标平台。保留原生 `Part`、预格式化输入与中间件替换流的差异，见[包审计](telescope/README.md#出站-multipart)。

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

`ignore_paths` 使用路径子串，匹配时统一路径分隔符。`ignore_packages` 识别 vendor 与 Cargo 默认依赖路径，框架包源码始终排除。当前采集一处可确定的调用位置。被过滤或缺失时保留 Gate 结果，文件和行号为空。通过任意异步包装器寻找可用的外层调用帧、非标准依赖路径、Query 的来源传播继续核对。

真实应用路由验证 Facade、直接调用、trait 对象和批量方法写入精确调用行号。三种 Watcher 配置分别验证来源可见、路径过滤、依赖过滤及 `ignore_abilities`。SQLite、PostgreSQL 18.7、MySQL 8.4.13 各通过。框架授权 5 项、Telescope 33 项、快照全仓格式、框架库及 Telescope 全目标严格 Clippy、三语文档构建和 12 组浏览器检查通过。快照沿用 `8041ae15a` 加前批及本批文件。独立 Redis 用例本批未跑。发布构建的路径重映射与目标平台仍需验收。

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

### Telescope Query 多对多中间表调用来源

固定 [InteractsWithPivotTable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Concerns/InteractsWithPivotTable.php) 的附加、修订、同步、切换、移除均通过原生查询事件。PHP 8.5.12 和 SQLite 的参考探针实际注册固定 QueryWatcher，使用 `attach`、`updateExistingPivotOrFail`、`sync`、`toggleOrFail`、`detachOrFail`，九条 SQL 分别归到五处调用行。参考夹具使用 Laravel 声明的 Doctrine Inflector 2.0.5；旧版 SeaORM 链接查询无对应诊断来源。

Rust 在 BelongsToMany 的中间表入口传播调用位置，覆盖带属性及事务变体，MorphToMany 沿用现有解引用入口。同步和切换中的读取、插入、删除经同一查询来源作用域传播。内部事务、查询锁和自定义中间表模型装配保持现有行为。关联模型自身的保存、创建、touch 与先读后写变体仍须核对。

基于 `42bca1a84` 加本批文件的隔离快照，在 SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 的真实应用路由验证五个入口的九条 SQL 与精确行号，同时检查更新行数、同步和切换的附加／移除集合及最终移除行数。Telescope 默认 38 项、框架关系 6 项、MySQL 两项显式连接原生回归、快照全仓格式、严格 Clippy、三语文档构建及 12 组浏览器检查通过。Release 与其他目标平台留在发布组合验收。证据位于 `target/tmp/observability-parity/query-pivot-*.log`。

### Pulse SlowQueries 数据库调用来源

固定 [SlowQueries](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/SlowQueries.php) 从查询调用栈选首个可用文件与行号，省略应用根路径并跳过框架、Pulse 和启动脚本。无可用位置写空字符串，关掉 `location` 写 `null`。在 PHP 8.5.12 中实际注册固定 Recorder，通过 SQLite 查询、Pulse 延迟队列和 Ingest 契约核对这两种配置的条目键。

Rust 使用原生 `QueryExecuted.callers` 已捕获的位置，跳过框架、Pulse 及启动入口，无需解析本地调试符号。默认保留第三方包的候选，符合 Pulse 对内部来源较窄的排除范围。来源不可用时写空字符串，禁用时写 `null`。移除 Pulse 唯一的 `backtrace` 依赖及其孤立依赖。未接入调用来源的数据库入口和自定义路径重映射仍继续核对。

基于 `d987d7fa9` 加本批文件的隔离快照，SQLite 真实路由验证有来源与关闭来源的条目键。Pulse 默认 21 项、独立 Redis 8.10.3 Stream 用例、Pulse 与 Telescope 的运行时组合、快照全仓格式、Pulse 全目标严格 Clippy、三语文档构建及 12 组浏览器检查通过。同一 macOS Release 二进制分别验证 SQLite、PostgreSQL 18.7、MySQL 8.4.13 的慢查询来源和关闭开关。Linux／Windows 与部署组合仍需验收。证据位于 `target/tmp/observability-parity/pulse-query-source-*.log`。

### Pulse Servers 默认主机名与配置缓存

固定 [Pulse 配置](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/config/pulse.php) 在载入时求值默认主机名，显式 `PULSE_SERVER_NAME` 覆盖默认。PHP 8.5.12 探针直接载入固定配置，确认两种名称和先前已载入值的稳定性。Rust 包默认配置与发布配置共用现有原生主机名检测。缺少名称的历史快照和手写 Recorder 配置沿用采样时的回退，系统查询失败保留 `unknown`。

基于 `38285b7c1` 加本批文件的隔离快照，使用真实 Artisan `config:cache`、`config:show`、`config:clear` 验证包默认配置与发布配置。两种配置均可缓存本机主机名或显式覆盖，缓存期间修改环境变量仍读取原值。Pulse 默认 21 项、快照格式、全目标严格 Clippy、三语构建和 12 组浏览器检查通过。证据位于 `target/tmp/observability-parity/pulse-server-*.log`。

### Horizon Linux ARM64 Release 进程矩阵

固定 Horizon 的进程树采用 Unix 信号，包依赖 `ext-pcntl` 和 `ext-posix`。PHP 官方明确 PCNTL 不在原生 Windows 上工作。Linux 的进程验收对应固定上游支持范围；OpenRust 在 Windows 上的元数据命令及应用装配仍须按项目发布范围单独核对。 [固定 Horizon 依赖](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/composer.json) [PHP PCNTL 安装文档](https://www.php.net/manual/en/pcntl.installation.php)

使用 `8745bc5fa` 的 Git 隔离快照与 Debian Bookworm 原生 ARM64 容器构建 `horizon-acceptance`。进程夹具可在独占容器网络内使用 Redis CLI，无需挂载 Docker 套接字。末段关闭该批 Redis，验证元数据连接中断后的进程回收。首轮优化构建的框架编译进程收到 SIGKILL。Docker 虚机内存约 8 GB，重试将 Cargo 并发限制为 1，保留正常 Release 优化。

Rust 1.94.0 单作业构建通过，实际运行于 Linux 7.0.14-linuxkit、Debian glibc 2.36 与隔离 Redis 8.10.3。Worker、孤儿清理和完整进程矩阵均通过，覆盖 Master／Supervisor／Worker 进程树、扩缩容、异常补位、暂停恢复、SIGTERM 排空、维护模式、内存保护、文件热重载、Redis 中断回收和 HTTP 授权／脱敏。首轮热重载暴露夹具在一次判断中重复读取状态的竞态。改为一次读取一份状态后，同一 Linux 二进制完整复跑通过。证据为 `target/tmp/horizon-linux/build-retry.log`、`acceptance.log` 与 `acceptance-retry.log`。

本批验证 Linux ARM64 容器中的 Artisan 和 HTTP 接口。Linux x86_64、部署主机资源与信号配置、正式 Redis 拓扑、浏览器及完整发布组合仍需按目标环境验收。

### Horizon 待补齐的公开进程与队列事件

固定 `MasterSupervisorReviving` 和 `SupervisorProcessRestarting` 保留公开类型。当前固定源码只找到前者的定义和后者的定义与事件映射，移植前须保留该区别。已接入的 `WorkerProcessRestarting`、`MasterSupervisorDeployed` 和 `UnableToLaunchProcess` 见下文。 [固定进程代码](https://github.com/laravel/horizon/tree/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src) [固定 EventMap](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/EventMap.php)

Rust 的 Provider 当前经核心队列事件更新仓储。下列固定上游 Redis 运输事件均待补齐公开契约，现有仓储更新仅证明对应消费者的一部分行为。共同字段为连接、队列和载荷，`JobsMigrated` 提供载荷序列。 [固定 RedisQueue](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/RedisQueue.php) [固定失败事件桥接](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/src/Listeners/MarshalFailedEvent.php)

| 固定事件 | 运输边界与上游消费者 |
|---|---|
| `JobPending` | 即时与延迟写入 Redis 前派发，接入 `StoreJob`。需验证写入失败时仍可观察到此前事件。 |
| `JobPushed` | 即时与延迟写入完成后派发，存储被监控标签。需分别核对普通、延迟及事务后提交。 |
| `JobReserved` | Redis 取出任务并成功保留后，以保留态载荷派发，早于 Worker 执行业务；登记保留状态并开始计时。 |
| `JobReleased` | Redis 删除保留记录并重新入队后派发，携带延迟秒数，更新释放状态。 |
| `JobDeleted` | Redis 删除保留记录后派发，携带 Job 与保留态载荷，触发完成状态和指标监听器。需核对正常删除、手动删除和失败路径。 |
| `JobsMigrated` | 从延迟或保留集合迁回 Ready 后派发，固定实现将 Redis 返回的 `false` 映射为空数组。需验证实际迁移、空批和任务详情更新。 |
| `JobFailed` | 固定监听器仅将底层的 Redis Job 失败事件转换为 Horizon 事件，携带异常、Job 和保留态载荷；分包监听器负责失败状态与标签。 |

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

Rust 将进程创建错误保留在原席位的冷却周期，记日志后继续监督。缺少已启动子进程时，`UnableToLaunchProcess` 的 PID 和退出码为空。进程实际退出后继续保留原字段和事件次序。固定上游创建异常会中止当次监督循环的剩余操作，多席位同时创建失败时的处理顺序仍须独立核对。

在 macOS 隔离快照中叠加两批改动。实际 Artisan 与 Redis 8.10.3 验证初次启动遇到不存在的工作目录时监督器仍正常运行，监听器收到空 PID 与空退出码，补回目录后按分钟冷却期限恢复，关闭后无残留 Worker。包含前批两种正常／异常退出分支的进程矩阵通过，包默认 24 项与独立 Redis 12 项通过，全仓格式、包全目标严格 Clippy、三语站点构建和 12 组文档浏览器检查通过。证据位于 `target/tmp/observability-parity/horizon-worker-spawn-macos.log` 和 `horizon-worker-spawn-docs.log`。

在新的独占 Redis 8.10.3 容器上，使用同一 macOS 二进制开启浏览器选项重跑真实进程矩阵。Chrome 桌面和移动视口验证授权、任务详情、监控标签写入、指标、CSP 刷新与布局，进程矩阵也通过。证据为 `target/tmp/observability-parity/horizon-worker-spawn-browser.log`。

Linux ARM64 以 `3515bafe8` 的进程范围源码重新构建 Release。首轮因触及 32 GB 停线中断，清理后确认原构建容器退出，再用 Rust 1.94.0、单作业完成优化构建。Linux 7.0.14、Debian glibc 2.36、独占 Redis 8.10.3 上，原生 Worker、孤儿清理、完整进程及 HTTP 矩阵均通过。两条一分钟路径均实际等待到期，分别验证异常退出补位及缺失工作目录恢复，最后关闭 Redis 并验证进程回收。证据为 `target/tmp/horizon-linux/worker-spawn-release.log`。Windows ARM64 结果见下节。

### Horizon Windows ARM64 应用装配与条件编译

Windows 11 ARM64 虚机的 Parallels 共享目录在 Rust 写归档时返回错误 87。构建切换到镜像位于项目临时目录的独立 NTFS 盘，并在本次命令的 PATH 中接入虚机已有的 ARM64 Clang 19.1.5 和 CMake。Rust 1.94.0 的原生目标为 `aarch64-pc-windows-msvc`。

修复 Windows 编译发现的两个 Unix 专用导入和一处不可达表达式。同一虚机重跑 Horizon 默认 24 项、Horizon 实际 Artisan 配置入口一项与默认骨架两项，全部通过，Horizon 编译无告警。macOS 的实际孤儿清理、完整进程矩阵、全仓格式和包全目标严格 Clippy 通过。Windows 测试未运行 Redis 专项或 Unix 信号矩阵。证据为 `target/tmp/observability-windows/horizon-*-ntfs-clean.log` 和 `target/tmp/observability-parity/horizon-windows-cfg-macos.log`。

验收结束后虚机已恢复挂起。Parallels 需关机才能拆除新增磁盘，本批保留原虚机挂起会话和项目内的验收盘。

### 三个扩展同时启用时的 Artisan 配置验收

三个根应用隔离验收原先从已经迁入框架的根 `lang/` 目录复制迁移注释，导致测试在运行 Artisan 前失败。按当前框架翻译资源位置修正三份夹具。macOS 同时启用 `horizon,pulse,telescope` 后，默认骨架两项及三个扩展的配置入口各一项通过，实际覆盖包发现、配置发布、有缓存时的读取及清缓存。Pulse 另在 SQLite、独占 PostgreSQL 18.7 和 MySQL 8.4.13 下执行真实迁移和命令。证据为 `target/tmp/observability-parity/observability-combined-artisan-*.log`。
