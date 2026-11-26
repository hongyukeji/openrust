# 短信模块重构验收

2026-11-21。短信骨架、实现和本机行为验收已完成；真实供应商账号、审核模板、运营商投递与手机收信仍未验。原有供应商协议、市场路由、模板、加密载荷与持久化受理记录继续使用，无新增依赖。

## 调研与取舍

按固定 Laravel 基线与旧版契约核对，联网读取以下官方仓库的源码、测试与许可证，固定副本位于本批日志目录的 `upstream/`。网页搜索工具未返回有效结果，实际资料由官方 GitHub API／原始源码获取，不以项目星数替代源码核对。

- [Laravel Manager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Manager.php)：采用具名驱动懒创建和复用、`extend`、`getDefaultDriver` 与 `forgetDrivers`；注册替换工厂不自动替换已解析实例，清缓存也不改变调用者已持有的实例。同期核对 ChannelManager／SendQueuedNotifications。
- [EasySms](https://github.com/overtrue/easy-sms/tree/35783de549aa57048ed939ad4b86ad36e9254921)：MIT，核对消息／网关接口、EasySms、Messenger、OrderStrategy 与测试，采用消息、网关和管理器分工及网关复用。其异常后继续尝试其他网关的策略不适合本项目受理结果未知时停止补发的要求，未引入自动 failover、策略框架或新依赖。
- [Laravel Vonage v3.3.4](https://github.com/laravel/vonage-notification-channel/tree/48c5b3805aff38e289c1bd8a567c243f1a80d2ed)、[Laravel Twilio channel](https://github.com/laravel-notification-channels/twilio/tree/31abf845e315dda8cd039bc433550d34df8df674)：核对渠道、消息、装配和依赖；保留消息交给供应商驱动发送的边界，以及现有 Vonage 独立通知渠道。
- 旧版 `v0.x` 提交 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 的 `vendor/openrust-framework/src/sms.rs`：提取独立短信职责、文本／模板、Twilio 字段、校验与脱敏经验，未搬回旧容器、配置或 feature 体系。
- [历史短信验收](sms-providers.md)已证明需要持久化受理记录解决 Worker 在厂商受理后被终止的重复发送窗口。本次保留 `sms_deliveries` 与原有崩溃恢复，不因 Laravel 通用 Manager 没有这张表而移除。

## 实现

原 `notifications/sms.rs` 拆为 `sms/{mod,facade,message,failure,submission,sms_manager,sms_sender}.rs`，公开导入路径继续为 `notifications::sms::*`。`SmsManager` 按应用、配置名缓存驱动；`SmsSender` 可克隆并持有同一驱动与配置快照。默认名称每次读取，模板与凭据随已解析实例保持一致。`forgetDrivers` 后重新解析才读取新配置／工厂，已持有 Sender 保持原行为。

Rust 适配用每个配置名的独立初始化锁保证并发首次解析只创建一次，避免在全局缓存写锁内调用工厂；工厂可解析其他配置的单向依赖。失败工厂可重试，未配置名称先验证，不累积无效缓存。应用装配默认注册 `sms` 渠道，可继续用 `extend` 与 `SmsChannel::using` 替换配置。三语正文同步说明生命周期与通知用法。

## 验收

隔离源码及日志：`/private/tmp/openrust-sms-refactor-20261121/`。初始基线 `18a44b15771301eae70050bbf589861a7efdb98c`；后续已合入的数据库迁移与注释改动以 `fd39601257b9f115a1e6a5797dc0d29d55f364da` 为集成复验基线（另存 `integration-revision`）。全程显式 Rust 1.94.0，独占 target、增量关闭、jobs=3、dev/test debug=0。

- `cargo +1.94.0 test --locked --offline -p openrust-framework --test sms_providers --test sms_manager`：6项通过，覆盖实际 HTTP 厂商协议、默认渠道、配置／模板快照、动态默认名、显式重载、12线程并发一次创建、工厂失败与替换、跨配置工厂及应用／Facade 隔离。最终日志 `providers-manager-final.log`。
- `cargo +1.94.0 test --locked --offline --test sms_worker_app --no-run` 构建独立 Artisan 入口。`tests/sms_worker.py` 从原 MySQL 专项扩为精确选择隔离 PostgreSQL／MySQL／SQLite，保留正式 wrapper 的 REQUIRE 检查；SQLite 使用独立临时文件，Python sqlite3 仅以只读 URI 查询，实际写入由应用捆绑 SQLite 3.54.1 完成。
- 通过本批 `run-workers.py` 调用正式 SQL wrapper／临时 SQLite 与专属 Redis8，分别在 PostgreSQL180006＋database缓存、MySQL8.4.13＋Redis8缓存、SQLite3.54.1＋file缓存下运行同组场景，每组均覆盖 database 与 Redis 两种队列。`workers-{postgres,mysql,sqlite}-cache-final.log` 记录实际选用缓存；修正原脚本强制 array 覆盖外层配置的问题后重跑全部三组。这里只计短信流程使用这些缓存配置通过，未修改共享 Cache 实现，不代表完整缓存驱动回归或 database 缓存在三库的全部专项已验。
- 每组验证直接发送、四个独立 Worker、派发端与 Worker 凭据隔离、任务无端点或密钥、阿里云／短信宝／云片两市场、具名配置、默认日志脱敏、模板、加密任务与失败记录、HTTP事件脱敏、未知受理停止、监听器失败及明确429重试。分别在受理结果保存前后 SIGKILL，新 Worker 与手动 retry 均零重复厂商提交。供应商为本地真实 HTTP 协议端点，未使用生产账号。
- `cargo +1.94.0 clippy --workspace --all-targets --all-features --locked --offline -- -D warnings` 与 `cargo +1.94.0 fmt --all --check` 通过。SQLite runtime／WAL 指定回归通过。日志 `clippy.log`、`fmt.log`、`sqlite-runtime.log`。
- `node docs/tests/check_content.mjs`、三语 VitePress 构建通过；`check_sms.mjs` 在 `/` 与 `/docs/` 的三语6页验证新增生命周期和渠道内容，通用浏览器12组合通过。日志 `docs-content-final.log`、`docs-build.log`、`docs-sms-browser.log`、`docs-browser.log`。

临时数据库／Redis 容器由所属夹具回收，前后核对保留已有数据卷；源码、上游固定副本与原始日志保留。本批只本地提交，不推送；其他对话的认证与缓存／settings工作不计入本批。

## 主线集成复验

在上述集成基线重新通过6项协议／生命周期测试、根应用配置测试、Worker重新构建，以及三库各自两种队列的完整短信与SIGKILL恢复场景。日志为 `integration-{providers,configuration,worker-build,workers-postgres,workers-mysql,workers-sqlite}.log`。独占SQL／Redis临时容器均正常回收，既有数据卷保留。

集成基线的严格全feature Clippy与fmt再次通过，日志 `integration-clippy.log`／`integration-fmt.log`。确认无进程使用后回收本批独占 target／站点dist，并仅解除指向共享node_modules的符号链接；源码、日志和共享依赖保留。清理前后 Available 为 48,852,072／52,648,208 KiB，净增 3,796,136 KiB；清理后约 53.91 GB（50.21 GiB）。资源读数受共享工作区并行活动影响，以 `cleanup.json` 为原始记录。


## 统一短信入口与 Vonage 驱动

2026-11-21，用户明确允许短信完整重构、无需兼容旧数据，并进一步要求 Vonage 合入短信模块统一管理。本节覆盖上文保留独立 Vonage 渠道和旧通知构造方法的历史设计；不提供旧类型、配置或渠道的兼容转发。

重新联网核对固定 Laravel Manager、EasySms 的消息／Gateway 边界、Laravel Vonage 和 Twilio 通知渠道。参考版本沿用上文固定提交，官方源码与仓库元数据保存在 `/private/tmp/openrust-sms-clean-design-20261121/upstream/`；Laravel、EasySms、Vonage 核对为 MIT，Twilio 元数据没有许可证值，未引入或复制其实现。项目旧版 `v0.x` 的短信职责、脱敏与文本／模板设计继续作为核对依据。采用 Laravel 的具名管理器和一个渠道一个消息构造入口，保留现有 HTTP 客户端与队列，不增加重试策略框架或外部依赖。统一 `sms` 渠道是本项目对多供应商的扩展；不声称 Laravel 原生提供这个通用渠道。

实际清理：

- 删除 `VonageSmsChannel`、`VonageMessage`、`NotificationRoute::Vonage`、`to_vonage` 和 `config/vonage.rs`；取消 `vonage` 渠道默认注册。
- Vonage 位于 `notifications/sms/providers/vonage.rs`，与其他供应商统一由 `SmsManager` 按配置名管理。`sms.drivers.<name>` 独立保存凭据、发件人、端点、跟踪标识与回执地址；使用 `SMS_VONAGE_API_KEY`／`SMS_VONAGE_API_SECRET`／`SMS_VONAGE_FROM` 及对应可选环境变量，无旧变量回退。Unicode 自动选择；多账号／发件人使用多个具名配置。
- `Notification::to_sms(recipient, to)` 返回 `SmsMessage`，覆盖文本、厂商模板和业务模板。删除 `to_sms_template` 与 `to_sms_message` 的优先级链；类型化路由号码作为额外参数传入是 Rust 适配。更新 Prepared 通知转发、Worker 夹具和三语正文，通知章节引用短信正文。
- 删除未配置时自动选择 log、虚构 log／vonage 配置的分支。应用配置文件显式提供默认 log；自定义加载器缺少必要配置明确返回错误。
- 保留 `sms_deliveries` 与未知受理停止补发：其用途是正在发生的发送安全，已有受理前后 SIGKILL 证据支持，属于当前业务机制。旧独立 Vonage 测试有效场景迁入统一短信的 HTTP／Worker 测试；厂商协议、明确限流重试和脱敏继续验证。

验收使用独立源码与独占 target，显式 Rust1.94.0、增量关闭、jobs=3、dev/test debug=0。初始基线 `95e904727`，集成基线 `c44864afe2a5de4c25a5b13c56a0ee6c03735c12`；不纳入其他对话尚未提交的日期迁移改动。

- 短信8项通过：6项管理器生命周期（含缺失配置负例）、既有厂商HTTP协议、Vonage多账号凭据／发件人／回执选项／配置快照与重载；默认 `sms` 渠道存在，独立 `vonage` 渠道未注册。日志 `sms-tests-final.log`；首次新HTTP夹具错误使用未启用的 axum Json feature，已改为现有响应方式后复验。
- 根配置1项、init14项通过，初始化凭据检查同步使用新变量。整个 `mail_notifications` 测试目标编译通过，确认删除专属类型后其他通知继续可构建。日志 `configuration-init.log`／`notification-build.log`。
- PostgreSQL180006＋database缓存、MySQL8.4.13＋Redis8缓存、SQLite3.54.1＋file缓存下，实际独立 Artisan／database与Redis两队列全组通过。新增 Vonage Unicode、跟踪／回执字段、派发端凭据与端点不入队、Worker默认配置反向切换仍使用所选Vonage配置、Worker独立密钥、明确状态1限流只重试一次、重定向拒绝、永久拒绝、空结果／缺失ID／无效状态拒绝、混合受理结果及手动retry均不补发。保留其他厂商及受理结果保存前后SIGKILL回归，零重复厂商提交。日志 `workers-{postgres,mysql,sqlite}.log`。缓存仅计短信流程选用对应驱动通过，不扩称完整缓存专项矩阵。
- 三语内容检查、站点构建、根／子目录12项通用与6项短信浏览器通过。日志 `docs-content-final.log`、`docs-build-final.log`、`docs-browser.log`、`docs-sms-browser.log`。

供应商为本地实际 HTTP 协议服务；真实账号、运营商投递和手机收信仍未验。数据库、Redis实例均由隔离夹具回收，前后核对既有数据卷保留。原始源码、固定上游副本与日志保留，本轮只核范围本地提交，不推送。

在集成基线重新通过短信8项、根配置1＋init14、Worker构建及三库完整双队列复验；全workspace/all-targets/all-features严格Clippy（`-D warnings`）和fmt通过，日志 `integration-{sms,configuration-init,worker-build,workers-postgres,workers-mysql,workers-sqlite,clippy,fmt}.log`。初轮Clippy发现新测试在改用已有HTTP响应方式后残留未使用Value导入，已删除并通过集成严格复验；初始失败日志仍保留。集成后的文档内容检查亦通过。

确认本批构建／Worker均结束后，删除独占target、站点dist及cache，仅解除共享node_modules符号链接；源码、上游副本、日志与全部数据卷保留。清理前后Available为36,738,756／40,959,008 KiB，净增4,220,252 KiB；原始读数见本批cleanup.json，共享并行活动可能影响差值。
