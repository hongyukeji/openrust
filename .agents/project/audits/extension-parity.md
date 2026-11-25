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
| Redis与平台 | Redis8.10.3独立夹具通过；拒绝上游同样不支持的Redis Cluster。Unix/macOS信号实测，不扩大为Linux／Windows或真实收件人通知已验 |

上述关闭原来“Master直管Worker／命令与分仓储未实现／只读面板”等过时缺口。不宣称执行完整PHP PHPUnit，也不隐藏Rust显式标签、轮询listen与内存衡量方式等已记录适配。

## O03 Telescope：原生支持范围验收通过

[原生链路及补齐记录](telescope/README.md)涵盖18类Entry／公开Watcher、独立注册／选项、框架事件开关、作用域与defer、仓储、队列更新、缓存、SQL、日志、邮件／通知、事件／模型／视图／Gate／调度／Dump／Redis／出站HTTP。新增已缓冲响应脱敏、字符限制及实际路由字段通过真实HTTP三库；文件与流不读取。原26项＋新增2项默认通过，Redis单独实跑。

三库／缓存／Worker、实际服务器Chrome桌面手机、安装、发布保留定制、根配置缓存／骨架及全部扩展组合见包审计与[最终组合](extension-final.md)。先前公开Watcher、ignoreFrameworkEvents、缓冲响应／路由字段缺口已关闭。

与PHP仍有明确差异：原生安全UI替代Vue／avatar；无任意结构或异步调用栈反射，Query／Gate未提供来源调用者过滤；未知Body／流及HTTP客户端multipart元数据不自动读取；CLI普通表格／时间戳替代PHP装饰与相对时间。这些限制留在三语正文和包审计，不能称作Laravel所有API或前端像素级完整移植。正式服务／其他OS也不由本地结果覆盖。

## O03–O06 与完成条件

O01 Activitylog、O02 Horizon、O03 Telescope原生范围、O04 Pennant、O05 Pulse及O06 Scout首个引擎范围已有实现与定向验收；当前统一组合见[最终记录](extension-final.md)。Scout不内置所有外部引擎，Telescope的PHP反射／UI等边界如上，不能从路线完成推断上游所有插件功能全部等价。必要核心改动由本对话完成；不联系其他对话，保留共享WIP，未执行暂存、提交或推送。

每项关闭前要求：上游源码／适用测试到 Rust 的对照、真实入口和失败路径、相关三库／缓存驱动、默认骨架／配置缓存边界、三语开发者文档及浏览器验收。与最终核心源码的全功能组合统一复验。许可证、命名、默认值及任何必要差异同时核对；仅“自己的测试全绿”不构成完整对齐证据。

早期Horizon纠偏的224组固定PHP参考只证明对应算法／配置／Worker选项；后续进程树、命令、仓储、标签及面板已另行实现验收，当前状态以O02段和最终组合记录为准。完整PHP PHPUnit套件未执行。

## Telescope、Pulse、Horizon 上游复核（2026-11-25）

本轮沿用上表固定提交。旧版没有对应独立扩展；复用当前包的配置加载、事件与消费者测试。三个包继续保留逐项对齐未完成状态。

| 所属类型或配置 | 本轮裁决 | 后续核对 |
|---|---|---|
| Telescope `ClientRequestWatcher::duration` | 固定上游在有正计时时以毫秒向下取整，缺少计时时输出空值。Rust 从现有传输统计输出整数毫秒或 JSON 空值。测试由真实出站 HTTP 与 fake 消费者覆盖。 | 命令与面板展示随完整输出审计核对。 |
| Telescope `ClientRequestWatcher::input` | 固定上游使用结构化 multipart 字段，保留普通值，附件按来源产生名称、大小、头部。Rust 的 `attach` 接收 reqwest `Part`，该类型未公开读取这些属性的接口。 | 在请求构建阶段保留结构化元数据，核对原附件入口兼容、单次流消费、重复字段、脱敏和成功／连接失败事件。Query／Gate 来源过滤见[包审计](telescope/README.md)。 |
| Pulse 默认 Recorder 清单 | 固定配置的十个 Recorder 均可映射到本地配置。核对了采样率 1、慢操作阈值 1000、异常和慢查询位置开关、默认存储及采集驱动；这一轮只读配置。 | 逐 Recorder 核对触发事件、筛选、采样、聚合、配置覆盖和扩展入口，再核对命令和面板。Servers 主机名上游在配置加载时求值，本地在记录时回退，配置缓存边界待裁决。 |
| Horizon `horizon:listen` 的 `watch` | 上游发布配置提供默认监听列表。Rust 配置补充应用、启动、配置、数据库、路由、视图、公开目录及资源目录中的 Rust 文件和 Cargo 构建入口的对应监听路径。真实 Artisan 使用默认配置监控 `routes/web.rs`，文件更新后重建 Master 和 Worker，SIGTERM 后子树退出。 | 补 Linux／Windows 目标平台验收。 |
| Horizon 其余配置 | 固定上游含 `name`、`waits`、`silenced`、`silenced_tags` 及 Supervisor `nice`。本地已读静默配置并具备 `nice` 运行时默认；配置形状、面板名称、等待时间入口仍须逐项核对。 | 对照发布配置及实际命令、Redis 和面板消费者，分别核对默认、覆盖及非法值。 |

源文件对照：[Telescope ClientRequestWatcher](https://github.com/laravel/telescope/blob/39a8ea8ad79915df6c3ac9b4e72659ade808eba3/src/Watchers/ClientRequestWatcher.php) · [Pulse 默认配置](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/config/pulse.php) · [Horizon 默认配置](https://github.com/laravel/horizon/blob/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa/config/horizon.php)。

全仓严格 Clippy 卡在隔离快照中 Fortify 的 `two_factor_model`、`installed_actions`、`starter_ui` 测试编译深度溢出。

验证使用提交 `8041ae15a` 加本批文件的隔离快照，保留共享工作区的在途锁文件和核心改动。Telescope Provider／Watcher 共 19 项通过，独立 Redis 用例未执行。Provider 的数据库 Worker 用例在默认测试线程栈上溢出，设置 `RUST_MIN_STACK=33554432` 后重跑通过。Horizon 配置 4 项通过，独立 Redis 配置用例未执行；真实进程、HTTP 和 Artisan 验收使用隔离 Redis 8.10.3。三语站点构建和 12 组浏览器检查通过，构建有现有包大小提示。快照全仓格式、Telescope／Horizon 全目标严格 Clippy 及差异空白检查通过。

## 持续对齐与生产验收

2026-11-25 用户授权持续实现、逐项验收，直至三个扩展完成对齐。沿用本账本固定提交，每批核对所属类型、公开入口、配置和真实消费者，记录可观察差异、未实现和未验证项。验收后按范围本地提交，保留并行改动。

| 范围 | 接续工作与关闭条件 |
|---|---|
| Telescope | 优先补齐出站 multipart 元数据、Query／Gate 来源过滤。逐类型核对 Watcher、Entry／过滤与脱敏、关联和仓储、生命周期、配置发布、命令输出及授权面板。原生界面和来源传递须验证同等使用结果。 |
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
