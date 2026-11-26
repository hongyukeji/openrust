# Webhook 扩展开发计划

记录日期：2026-11-25。状态：**用户已于2026-11-25启用持续开发目标并授权实施；两包配置、签名及扩展基础已实现并验收，完整收发尚未闭合**。

接手前阅读[项目指引](../README.md)、[项目规范](../conventions.md)、[固定 Laravel 基线](laravel-rebuild-baseline.md)、[待办](../todo.md)和最新工作区。本文是实施依据；最初方案提交不构成开发授权的历史边界，已被本轮明确持续开发指令替代。当前实现与待补缺口见[Webhook实施记录](../audits/webhooks/README.md)。仍不擅自改变支付事务、操作正式环境或发布产品。源码、公开签名、默认值及测试须在开发时逐项裁决，计划完整不等于实现／验收完成。

## 1. 需求来源与范围决定

### 用户要求

下列第3项记录最初方案讨论轮次的授权范围；当前已明确授权持续实施，不能据其历史措辞停止开发。

1. Spatie Webhook 对应能力也应作为 OpenRust 独立扩展实现，不能只作为内容审核包内部的参考代码。
2. 联网核对 GitHub 热门项目和 Laravel 生态，优先借鉴或复用经过实践验证的设计，保持 Laravel 使用习惯及项目 Rust 命名，避免重复造轮子。
3. 先将需求与建议记录为开发计划，再按成熟的完整形态补齐计划；本轮不开始实现。
4. 客户端与服务端均需支持单机和集群部署，使用同一套实现／公开契约，通过既有后端配置与进程副本数适配；不是另立未来集群版。

### 本次建议决策

- 拆为两个可选包：`openrust-webhook-client`／`vendor/openrust/webhook-client/`负责接收；`openrust-webhook-server`／`vendor/openrust/webhook-server/`负责发送。client／server沿用Spatie语义，并非反向命名错误。应用可以只安装其中一个。
- 以固定Spatie两个包的**全部公开类型、配置和用法入口**为直接对照；Rust标识符使用snake_case／UpperCamelCase。保留方法机制与可观察行为，语言适配逐项说明，不机械移植PHP继承／对象序列化。
- 完整交付不止“验签后dispatch一次”或“HTTP POST加重试”：必须包含可靠接收／投递记录、去重冲突、配置／秘钥版本、补偿、重放、端点／订阅接缝、并发／限流、监控、清理和独立消费者验收。
- 直接HTTP是内置发送实现，沿用现有HTTP Client、Queue／Bus、Scheduler、Cache锁／RateLimiter、Database、Storage、容器与包发现；不新造队列、数据库、规则引擎、通用插件框架或工作流平台。
- Standard Webhooks签名／验证优先复用维护中的Rust库；若依赖／MSRV不合适先核对其他维护版本和既有加密原语，留决定与测试，不直接复制大量签名代码。Spatie兼容签名与供应商原生签名仍独立支持。
- Svix作为可选独立投递后端跟踪，Convoy作为网关参考；不默认内嵌／部署任一平台，不移植整套平台。该条件性后续不等于必须实现其所有UI、连接器或企业功能。
- 通用Webhook扩展与内容审核为独立交付单元：先接收基础，内容审核成为实际消费者，再完成发送与成熟运行能力。Pay／Cashier现有契约与同一事务边界保留，暂不强制迁移。

### “成熟完整”的明确含义

本计划的完成范围是第4／5节Spatie全入口对照＋第6至10节的原生可靠性与运维能力，以及第11／13／14节必需验收。不是只交付最小演示，也不是承诺复制Svix／Convoy全部产品。平台后端、严格FIFO、管理前端、任意载荷脚本转换、Kafka等连接器列为条件性后续；不能用这些后续项拖住已核定模块，也不能将模块闭合说成平台完全等价。

## 2. 固定调研与复用裁决

2026-11-25核对仓库元数据、固定提交源码、README／配置、测试清单和部分实现。Star是发现候选的快照；维护活跃／存在测试不是本项目生产质量验证。本轮未运行这些服务或其测试，未验证第三方托管账号、费用或部署。

| 项目／固定提交 | 证据及维护／许可 | 采用决定与成本 |
|---|---|---|
| [Spatie Webhook Client `ff886d1bae33d35ad98b6c91403f280df158129d`](https://github.com/spatie/laravel-webhook-client/tree/ff886d1bae33d35ad98b6c91403f280df158129d) | 约1.2k Star、MIT、未归档，最近提交2026-11-05；公开Processor／Validator／Profile／Response／Model／Job及测试 | **接收基线**。借鉴结构与测试反例，不引入PHP运行时。Processor的保存后派发不自动保证原子队列交付或业务恰好一次 |
| [Spatie Webhook Server `6da6be3a721190bcdedcdef32d31d5980b77eb0e`](https://github.com/spatie/laravel-webhook-server/tree/6da6be3a721190bcdedcdef32d31d5980b77eb0e) | 约1.1k、MIT、未归档，最近提交2026-11-05；完整WebhookCall、Signer、BackoffStrategy、Job／事件和重试测试 | **发送基线**。核对全部公开入口和默认值，不只实现url／payload／dispatch。安全增强与原默认差异明确公开，不能默改或假称等价 |
| [Standard Webhooks `4a8ee40c5bcb6157f856905cc8871040d41a78ce`](https://github.com/standard-webhooks/standard-webhooks/tree/4a8ee40c5bcb6157f856905cc8871040d41a78ce) | 约1.8k、未归档，最近提交2026-11-23；根规范Apache-2.0，libraries/LICENSE及Rust清单MIT；Rust库快照1.3.0，有签名／验签、时间戳及多签名反例 | **优先复用小型签名库**。逐文件许可与实际Cargo依赖独立核对，不能把根许可证当全仓统一。库不提供可靠收发／存储／业务去重，不把验签成功当业务已处理 |
| [Svix `931467b95a1bbcc520c47bcb96f4e436782abcd2`](https://github.com/svix/svix-webhooks/tree/931467b95a1bbcc520c47bcb96f4e436782abcd2) | 约3.5k、MIT、未归档，最近提交2026-11-24；Rust服务／SDK、投递与尝试、端点／事件类型及接收验证 | 参考消息／尝试分离、重试／重放与平台边界；可选外部后端优先使用官方SDK／API。该服务端清单MSRV为1.95，高于当前项目1.94，不能直接塞进workspace；SDK另验，不推定同MSRV |
| [Convoy `8ae7f34fae6de320d22e023e1a99a95b934bd2cb`](https://github.com/frain-dev/convoy/tree/8ae7f34fae6de320d22e023e1a99a95b934bd2cb) | 约2.9k、未归档，最近提交2026-11-24；当前LICENSE为Elastic License 2.0，独立Go网关及商业功能 | 参考端点生命周期、投递隔离、滚动秘钥与限流，不复制代码或默加额外网关。自托管限制／许可／运维与托管使用分别评估，不将source available泛称MIT式可复用 |
| [固定Laravel v13.40.0](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) | 本项目固定行为／队列基线 | 复用Provider、Config、路由、Queue／Bus、after_commit、批次／限流等职责；不以框架存在Queue便推定所有Webhook恢复语义已经具备 |

重要官方来源：[Spatie接收README](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/README.md)、[Spatie发送README](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/README.md)、[Standard规范](https://github.com/standard-webhooks/standard-webhooks/blob/4a8ee40c5bcb6157f856905cc8871040d41a78ce/spec/standard-webhooks.md)、[Rust库源码](https://github.com/standard-webhooks/standard-webhooks/blob/4a8ee40c5bcb6157f856905cc8871040d41a78ce/libraries/rust/src/lib.rs)、[Svix重试与手动重放说明](https://docs.svix.com/retries)。实现时刷新安全／维护状态并锁定实际接入版本；HTTP文档是调研日期快照，不能替代协议实测。

## 3. 本地基础与包结构

本轮静态检查未确认已有通用收发包；现有专用代码不因名称出现webhook就视为本计划已实现。实施前按规范补查`openrust-v0.x`和最新源码，复用符合当前契约的代码／测试。

| 本地机制 | 复用入口与约束 |
|---|---|
| Pay | [DatabaseNotifications](../../../vendor/openrust/pay/src/notifications.rs)保持验签后的身份校验、去重／行锁、业务SQL回调和完成标记在同一事务；通用异步接收不得拆散 |
| Cashier | [VerifiedEvent／webhook](../../../vendor/openrust/cashier/src/webhook.rs)已有供应商签名及受控可信事件；通用Validator不能伪造该领域可信类型 |
| HTTP Client | [PendingRequest](../../../vendor/openrust/framework/src/http/client/pending_request.rs)和既有传输；有重定向开关不代表已有完整SSRF防护，须跟到连接实际地址 |
| Queue／Bus | [middleware](../../../vendor/openrust/framework/src/queue/middleware.rs)、[限流](../../../vendor/openrust/framework/src/queue/middleware/rate_limited.rs)、[Batch](../../../vendor/openrust/framework/src/bus/batch.rs)，复用调度、限流、防重叠，领域状态不能只用batch finished判断 |
| 配置／驱动 | 容器与包发现，参考[SmsManager](../../../vendor/openrust/framework/src/notifications/sms/sms_manager.rs)；配置保存名／版本／凭据引用，工厂和业务handler按进程注册 |
| 分层 | 各包models只描述持久数据与既有模型入口，storage负责SQL／事务，services／Processor协调，不将协议／业务流程塞进Model |

建议接收包模块包括config／contracts、signature_validator、webhook_profile、webhook_response、processor、jobs、models、storage、services、provider与tests；发送包包括webhook_call、signer、backoff_strategy、jobs、delivery／attempt、storage、services、provider与tests。遵循已有独立包布局，不为形式强建空目录，也不预先增加第三个共享crate。两包出现真实且稳定的共同代码时才评估复用位置；不得循环依赖。

配置模板分别使用`webhook-client`／`webhook-server`中性名称，并按项目配置文件解析规则核定最终`config/*.rs`路径；不因命名PHP文件便假定Rust配置键规则相同。文档导入明确两个包中同名`WebhookCall`的完整路径，不能靠用户猜测。

## 4. 接收端完整对照清单

以下为核定交付要求，当前实施状态以[逐类型账本](../audits/webhooks/contracts.json)和[验收记录](../audits/webhooks/README.md)为准，不把局部实现当整批闭合。以下是固定源码直接声明与教程入口清单，不包含PHP框架自动继承的全部方法；实施前逐类型补齐实际消费者／trait／宏／继承语义裁决，不能把本表或同名扫描当完整兼容证明。

| 编号／公开面 | 必交付内容及Rust表达 | 验收重点 |
|---|---|---|
| WC-01 安装／Provider | 独立安装、发现、配置默认与合并、迁移发布、正常引导；保持默认应用可不装包 | 全新应用、安装前后／重复安装、不覆盖自定义文件、HTTP／Artisan／Worker |
| WC-02 WebhookConfig | 具名多接收配置；指定Validator、Profile、Response、Model／存储、Job的Rust注册替代 | 缺名／重名／错误类型、未知实现、秘钥缺失和不同应用隔离；不能队列序列化类名后任意反射 |
| WC-03 WebhookConfigRepository | `add_config`／`get_config`及默认／未知选择语义，配置版本不可变 | 多业务／多账号、注册替换与旧任务、并发解析 |
| WC-04 路由／控制器 | WebhookController对应处理、显式路由注册、多个应用／端点、路由方法与唯一名称令牌配置 | 允许的方法、404／405、同名路由与前缀、配置映射；令牌不是验签替代，不能关闭全局CSRF |
| WC-05 无控制器入口 | `WebhookProcessor::process`可以从自定义HTTP入口调用 | 与控制器路径共享验证／存储／响应，保留原始字节 |
| WC-06 签名 | SignatureValidator／DefaultSignatureValidator与`is_valid`，Spatie默认HMAC兼容、配置签名Header、自定义验证器 | 缺签名、重复Header、错误／轮换秘钥、原始body、时效协议差异与恒定时间比较 |
| WC-07 Profile | WebhookProfile／ProcessEverythingWebhookProfile与`should_process`，自定义筛选 | 拒绝／忽略事件的响应／是否存储沿基线核对，不能漏验就算可信 |
| WC-08 Response | RespondsToWebhook／DefaultRespondsTo与`respond_to_valid_webhook`，状态／body／Header可替换 | 供应商特定确认、持久失败不假ACK、业务失败不混淆收件结果 |
| WC-09 WebhookCall模型 | `store_webhook`、`headers_to_store`、`header_bag`、`headers`、`save_exception`、`clear_exception`、`prunable`、`get_attachments`的公开语义 | 模型定制保留；实际查询／异常存储／清理委托storage，不能都写进模型文件。[可信旧版附件恢复](../audits/webhooks/legacy-attachments.md)独立于在线 JSON 接收 |
| WC-10 载荷／Header／附件 | 正文类型、选定Header／全部Header选项、大小限制、附件提取开关及Storage引用 | JSON／form／raw／multipart分别裁决；不在验签前重编码body，附件不暴露上传临时路径；权限／关联清理／恶意类型 |
| WC-11 ProcessWebhookJob | 定制业务处理入口、连接／队列／尝试／超时，保存及清除异常 | 工厂／handler可由独立crate注册；Worker独立恢复，失败可追踪／重试不重复业务效果。[显式终态失败](../audits/webhooks/processing-terminal-control.md)与接收租约同事务结束，[终态回调](../audits/webhooks/processing-failed-hook.md)由原生队列结算后触发 |
| WC-12 清理 | `delete_after_days`、prunable和定时执行；实际固定配置默认30天，null不自动清理的契约单独核对 | 成功／失败／处理中保留策略、附件和墓碑、分页清理、防止清理仍在用的载荷 |
| WC-13 异常／事件 | InvalidWebhookSignatureEvent、InvalidConfig各工厂、InvalidMethod／InvalidWebhookSignature及原事件载荷／触发时机 | 面向人提示中文、机器分类稳定、source保留／脱敏；不把异常名称数量当兼容 |

安全增强与基线冲突须显式裁决：上游允许保存所有Header／附件和无限保留，不把这些选项默默删除，也不默认在生产记录Authorization、Cookie或无界文件。提供受控显式配置、有限资源预算和文档说明；调整默认值时登记差异，不假称完全相同。

## 5. 发送端完整对照清单

### 5.1 WebhookCall公开方法分组

固定[src/WebhookCall.php](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/WebhookCall.php)直接声明以下入口，Rust使用对应snake_case；构造／所有权表达可适配，但每组都要有实际调用与反例。

| 编号 | 上游直接入口 | 处理要求 |
|---|---|---|
| WS-01 构造／目的地 | create、__construct、url、getUrl | 对应Builder／构造，目的地校验、快照与错误行为 |
| WS-02 载荷 | payload、sendRawBody | JSON／原始字节、Content-Type、固定发送字节与签名相同；禁止重试时重新序列化改变内容 |
| WS-03 消息身份 | uuid、getUuid、withWebhookIdHeader | 稳定消息ID、可配置Header、投递／尝试ID分开，不能只提供随机重试ID |
| WS-04 队列 | onQueue、onConnection | 复用既有连接／队列选择，延迟／after_commit按现有机制接线 |
| WS-05 请求方式／Header | useHttpVerb、withHeaders、useTimestamp | 明确各HTTP方法载荷语义；保留Header合并与覆盖规则；timestamp Header不自动代表它已纳入签名 |
| WS-06 签名 | useSecret、signUsing、doNotSign | 内置与外部Signer，显式不签名入口保留；秘钥不入普通payload／日志，凭据安全适配及原值语义登记 |
| WS-07 重试 | maximumTries、retryOn、respectRetryAfter、withJitter、useBackoffStrategy、timeoutInSeconds | 有限尝试／时间预算，HTTP响应与连接失败分别裁决，秒／日期形式Retry-After与异常值测试 |
| WS-08 TLS／网络 | mutualTls、verifySsl、doNotVerifySsl、useProxy | 默认证书校验，mTLS／证书路径及代理实际接线；显式不校验入口需清楚标注危险适用范围，不自动关闭或降级TLS。具名协议映射、主机排除与 IPv6 字面目标见[验收](../audits/webhooks/proxy-map.md) |
| WS-09 元信息／Job | meta、withTags、useJob | 任务元数据与线上请求体区分，标签有界脱敏；自定义Job不绕开可靠记录／安全策略 |
| WS-10 异常策略 | throwExceptionOnFailure | 同步返回错误与后台最终失败分别验证，不能只改状态文字 |
| WS-11 异步／条件派发 | dispatch、dispatchIf、dispatchUnless | 条件为假不创建实际投递／请求，闭包语义按Rust表达；创建记录与事务边界明确 |
| WS-12 同步／条件派发 | dispatchSync、dispatchSyncIf、dispatchSyncUnless | 同步走同一序列化／签名／网络／状态逻辑，按WD-04统一领取，不跳过安全／尝试记录或与扫描旁路竞发 |

### 5.2 其他公开类型与配置

- `Signer`／`DefaultSigner`：signatureHeaderName、calculateSignature及构造语义；自定义Signer获取哪些上下文按固定源码核对，不能只硬编码一套HMAC。
- `BackoffStrategy`／`ExponentialBackoffStrategy`：waitInSecondsAfterAttempt；源码默认等待数值与实际配置有记录，不把某平台重试时间表误当所有服务约定。
- `CallWebhookJob`：handle、tags、getResponse、getBody、getWebhookUrl、failed；自定义Job的公开契约与失败行为从实际Worker测试。
- `SerializesWebhookData`、WebhookCallEvent的__serialize／__unserialize：保留数据往返／恢复语义，用有版本的Rust DTO／注册名表达，不引入PHP对象或反射序列化。无法表达部分单列裁决，不能无声丢掉meta／tags／原始body。
- 事件：DispatchingWebhookCallEvent、WebhookCallSucceededEvent、WebhookCallFailedEvent、FinalWebhookCallFailedEvent及WebhookCallEvent基础载荷。核对发送前、单次失败、最终失败和恢复重放的触发次数／时机；事件中敏感请求／响应受控，不直接Debug整包。结果事件和 Job 可读取的传输诊断见[三库验收](../audits/webhooks/transport-events.md)。
- 异常：CouldNotCallWebhook的urlNotSet／secretNotSet、InvalidBackoffStrategy、InvalidSigner、InvalidWebhookJob；按项目错误分层保留可诊断分类。
- 配置覆盖queue、connection、http_verb、proxy、signer、signature_header_name、timestamp_header_name、webhook_id_header_name、headers、timeout_in_seconds、tries、backoff_strategy、retryable_status_codes、respect_retry_after、retry_with_jitter、webhook_job、verify_ssl、throw_exception_on_failure、tags。
- 调研快照默认值：超时3秒、tries=3、POST、Signature／Timestamp／Webhook-Id Header、证书校验true、retryable_status_codes=null代表重试所有非成功状态，respect_retry_after和retry_with_jitter默认false。实现前分别验证继承／覆盖；原生推荐生产策略可显式选取408／429／5xx、有限退避和Retry-After，不能暗改兼容模式默认或把所有4xx无限重试。
- 测试方式：复用项目既有HTTP／Queue／Bus fake与真实消费者；定制错误、事件／Tags、同步和原始body均实际调用，不另造一套测试运行器。

## 6. 成熟运行形态：记录、幂等、事务与恢复

### 6.1 最小数据职责

| 记录 | 必须表达 |
|---|---|
| 接收记录WebhookCall | 应用／租户、配置／验证器及版本、源事件ID／去重依据、原始字节摘要、验证状态、受控载荷／附件引用、接收／处理时间、处理状态／异常／版本 |
| 出站消息 | 稳定event_id／message_id、事件类型／schema版本、不可变正文／摘要、来源业务修订、创建／截止时间、提交幂等键 |
| 投递Delivery | 消息＋端点及版本、目标URL／协议／签名配置快照、凭据引用／修订、状态、尝试／下一次时间、取消／禁用与租约、累计预算 |
| 尝试Attempt | attempt_id、delivery_id、开始／结束、请求摘要／时间戳／签名修订、状态码／网络分类、受限响应、是否可重试；不得记录秘钥。[持久化传输诊断](../audits/webhooks/transport-diagnostics.md)提供耗时与脱敏分类 |
| 端点／订阅 | 业务／租户、端点身份、状态与版本、订阅事件类型、网络／签名策略、并发／速率；配置定义与可选持久定义明确优先关系 |

这是领域职责，不强制五张表、全局event bus或第三个基础包。使用已有Database／Migration；接收、发送可各自持有迁移，独立包不互相要求安装。模型公开入口可以委托storage，业务服务决定实际效果。

### 6.2 接收可靠性

- 验签前保留原始bytes，限制字节、Header／附件、方法与解析时间。验签失败不进入可信处理集合；需要审计时只留受限失败摘要，不把攻击载荷永久入库。
- 先按端点选验证器／凭据再验证，不能按未验证payload任意选跨租户配置。可信事件ID在应用／源账号／配置范围去重，同ID不同内容必须报冲突；供应商无稳定ID时明确摘要去重窗口，不能把不同合法事件误合并。
- 保存可信接收记录后返回协议确认，业务处理异步重试；忽略的有效事件按Profile及Response约定返回。数据库保存失败不能假装接收成功。
- 通过记录状态与有限Scheduler补偿解决“记录已提交、派发前崩溃”；after_commit避免事务未提交的读取，但不宣称队列派发与数据库原子。唯一锁减少并发，最终去重／状态条件更新承担一致性。
- 处理器应在明确的业务事务中实现幂等；外部副作用走提交后／已有领域outbox，不在数据库锁内等待网络。不承诺分布式恰好一次。
- 提前回调、尚无领域任务关联的通知按WD-02持久Deferred并由消费者关联；不能误标Completed或一律抛异常耗尽重试。未验证提示必须符合WD-01显式策略，只允许受限查单，错误签名直接拒绝，不能作为可信结果或扩大无限后台任务。
- 收件响应确认不保证业务完成；若消费者确需同步业务成功后才确认，显式提供策略并验其超时／事务／重试边界，不能让通用快速ACK覆盖支付原契约。

### 6.3 发送可靠性

- 按WD-05有界单事务保存消息和完整端点投递集合，每次尝试独立；扇出时同消息ID、不同delivery／attempt，失败端点不能重复触发成功端点的业务效果。
- 业务提交前建立发送意图／消息记录，提交后Queue派发；Scheduler有限扫描恢复未派发／过期租约。与业务不同数据库时不宣称跨库原子，应用按现有事务／outbox能力接线。
- 序列化一次，不可变bytes用于所有重试。每次尝试按协议更新时间戳并重新签名，稳定消息ID用于对方去重；配置、目标、端点和凭据版本归属固定，当前端点许可／撤销按WD-03优先检查；重放／换地址不能悄悄改变原记录。
- 提交幂等键按应用／租户／来源作用域唯一：同键同消息返回原消息，同键不同正文或不兼容投递集合报冲突。不得依赖Queue唯一锁或每次随机UUID代替提交幂等。
- HTTP 2xx表示对端协议接收，网络超时可能对端已经处理；标记Unknown／不确定尝试，允许按稳定ID至少一次重试，不声称远端未处理。永久失败／可重试／可暂停按策略区分。
- Retry-After受总截止与最大等待限制；Queue尝试和HTTP Client重试不得无意叠乘。原执行代次持久计数不能因Worker重启、release而清零；人工重放必须按WD-06新建有审计且有限的代次，不修改原预算／历史。
- Pending／Running／Succeeded／Failed／Cancelled／Expired状态变化用短事务与条件更新；取消是停止后续本地尝试，不保证已发HTTP或远端业务撤回。晚到响应只能更新对应尝试审计，不能复活过期或已取消投递。
- 默认无全局顺序保证。并发发送可能乱序，消费者用事件版本处理；严格FIFO作为单独后续，不用“Queue有序”冒充端到端业务顺序。

## 7. 签名、凭据与网络安全

- 内置Spatie兼容Signer／Validator：按固定协议计算签名及Header。兼容模式不能凭空宣称有时间窗／抗重放；useTimestamp添加Header不等于默认Signer覆盖该字段。
- 内置Standard Webhooks实现：优先复用已核对的Rust库，检查最低工具链、依赖锁定、重复Header／时间窗／多签名／错误消息和跨语言向量。Standard消息ID与时间戳校验不替代持久幂等。
- 供应商原生协议经SignatureValidator或Signer实现，不能要求第三方更改签名；原始字节、form编码、重复字段、时间单位和Header语义按真实协议。无签名接口显式配置与应用授权，不能以“支持do_not_sign”自动放行所有接收。
- 秘钥引用、有限修订及安全解析由小型契约／既有注入解决，不新建Vault；配置缓存／config:show与Debug／日志不泄漏。上游useSecret接收明文的调用体验需要安全DTO／私有字段／现有加密队列封装适配，不将秘钥直接加入普通可观察payload；差异需测试／三语说明。
- 支持轮换期间多秘钥验证／按版本发送，不跨账号兜底；保留旧任务所需修订到有限窗口，紧急撤销优先，缺凭据明确失败。人工重放重新通过权限与版本规则，不绕过撤销。
- 默认HTTPS及证书校验；显式不校验／不签名入口按上游契约提供并明确风险，测试限定受控本地，不作为生产自动恢复。mTLS真实验证客户证书／私钥，代理配置不得使SSRF防护失效。
- 发送默认禁止自动重定向；目标URL注册、更新、每次尝试及实际连接均校验scheme／port／DNS／IP，防止DNS rebinding／TOCTOU、回环／私网／云元数据地址。确需内部服务用显式受信端点策略，不能一刀删除合法内部场景，也不能用全局放行取代。
- SSRF保护须连接到已验证的地址并保留正确Host／SNI／TLS校验，检查代理／重定向路径；仅url字符串过滤或先DNS后由客户端重新解析不算通过。HTTP Client不足时直接补必要接缝并验，不新增平行HTTP引擎。
- 回调路由只调整专用路径的CSRF／会话策略，默认全局保护不关闭。管理与重放入口有独立身份／Gate授权、租户隔离、请求配额和审计；路由名令牌不能代替鉴权。
- 附件与原始payload按需私有存储／加密／限期，Header默认allowlist且敏感值保护；清理不删除在途任务引用的文件，删除后保留有限幂等墓碑防重放。

## 8. 自定义扩展的完整交付

外部crate可实现SignatureValidator、Signer、WebhookProfile、Response、业务处理Job／handler、BackoffStrategy及已核定存储适配，不需fork／改内部enum。所有必要输入／结果／错误构造可公开使用，同时不把未经验证的通知直接变成Pay VerifiedNotification／Cashier VerifiedEvent。

注册通过应用Provider、容器和明确工厂完成，HTTP／Artisan／Worker分别启动。配置只保存注册名和可序列化数据，不动态执行用户输入类名、不将闭包放进队列。多配置／多应用注册隔离、失败可重试、实例复用／替换／旧句柄行为均说明。

如果增加Svix等发送后端，则提供小型Sender契约与Manager／extend，直接HTTP与外部后端使用同一发送入口／结果语义；不把“Signer可替换”说成“已支持任意投递后端”。该后端能力属WH-08条件性后续，不能谎报已实现。

独立消费crate必须实际实现各契约并通过正式公开入口，包含自定义响应、业务Job、持久恢复、配置缓存和三语完整例子；包内mock不能代替外部扩展验收。自定义同步状态不得跨请求混入其他应用。

## 9. 成熟运维与管理能力

以下作为原生完整形态必交付，而非只在README建议用户自己搭建；具体Artisan名／服务方法在WH-00按项目命名冻结。

| 能力 | 最小完整范围／限制 |
|---|---|
| 查询与诊断 | 通过公开服务及Artisan按接收ID／消息／投递／尝试查询状态、时间和脱敏错误；分页和过滤有界，不默认全量导出载荷。[接收摘要及尝试分页](../audits/webhooks/receipt-diagnostics.md)和[出站诊断](../audits/webhooks/outbound-diagnostics.md)已接通 |
| 重试／重放 | 按授权重试失败收件处理或失败投递；记录新处理／重放代次、原因和关联，稳定消息身份／去重策略清楚；可信历史收件不按过期外部时间戳重新验签，也不跳过原验证记录／当前撤销／业务幂等；出站重放用原正文与稳定消息ID重新生成单次签名。复制为新消息必须显式调用并记录来源，不能静默改ID绕过去重或把“只读详情”授权当重放授权 |
| 端点管理 | 具名配置入口保留；提供可选持久端点服务接口，注册、修改、暂停／恢复、删除及配置修订，按应用Gate／租户控制；不给默认应用自动开放匿名CRUD路由 |
| 订阅／事件类型 | 小型显式事件类型／schema版本与端点订阅过滤、受控扇出；不执行任意表达式／JavaScript，不新造规则引擎；旧消息固定投递集合，新增订阅不追溯重复发送 |
| 速率与并发 | 接收／处理／发送分别限制，按应用／端点／账号配置；复用共享RateLimiter、Queue和锁／存储租约，在途限制不等于请求速率。[已验签接收入站额度](../audits/webhooks/verified-rate-limit.md)已接入原子共享限流 |
| 故障隔离 | 一个失败／慢端点不饿死其他端点；有限重试、期限、暂停／恢复与隔离队列；自动熔断若实现须状态／探测／恢复可观察，不无期限静默丢弃 |
| 指标／事件 | 接收拒绝／重复／冲突、处理等待／失败、投递成功／失败／Unknown、延迟、尝试／重放、补偿滞留；标签有界，不把原文／URL／秘钥／无限ID作为指标 |
| 清理／留存 | 接收、附件、发送正文、响应、尝试和幂等墓碑分别配置期限；分页清理与依赖保护、法定／业务保留由应用提供；重放后不得引用已清理正文假称可恢复 |
| 运行与恢复 | 多Worker租约、进程崩溃、部署重启、迁移／回滚边界、配置／秘钥修订、备份恢复和截止时间、WD-04领取令牌与代次；跨服务至少一次而非恰好一次 |

不默认提供独立管理前端／用户门户或公网管理API，服务＋Artisan已承担管理接缝；若业务需要Web UI可在应用构建。严格FIFO、跨地域复制、任意转换脚本、额外消息系统连接器、Svix／Convoy托管平台适配等单列后续，不包含在原生模块完全体等价声明。

## 10. 与内容审核及既有领域包的依赖边界

- **新的复用决定替代内容审核计划中“不要新增通用Webhook模块”的旧局部限制。**用户现已要求独立扩展，内容审核优先依赖可选webhook-client，而不是再做一套签名验证器注册、接收记录、入队补偿和处理重放。
- 接收包持有传输接收／处理记录、验证来源、有限重试与原始消息引用；内容审核持有父子审核任务、内容修订、供应商账号／任务关联、模态证据、策略和发布状态。两个层的记录职责不同，不把通用收件记录复制成另一个内容审核inbox。
- 供应商Validator／Response以及将可信接收记录关联审核子任务的handler归内容审核包，可从公开契约外部实现；通用包不依赖内容审核或四家云服务，保持单向依赖。
- 通用无签名通知仍只可作为受限查单提示，不进入可信结果；内容审核负责查单与任务归属／覆盖判定。通用签名成功不能直接批准内容。
- 先完成WH-01／WH-02及WH-03可被复用的可靠接收门禁，再进行内容审核CM-03；CM-01／CM-02的非接收部分可独立推进，不要求发送包先完成才做审核。
- Pay／Cashier不作为首批强制迁移消费者；未来若迁移独立比较签名、去重、行锁／业务事务、源事件顺序、返回响应和完成标记，逐项实测。不允许通用快速ACK或独立收件事务破坏既有公开契约。Slack等专用通知也不自动迁移。
- 避免双重去重误吞合法事件：通用层限制重放／重复处理，领域层仍在其事务中保证业务效果；两个去重键、状态与失败重试责任明确，通用“已接收”不能让失败业务被当“已完成”。

## 11. 实施批次与验收矩阵

### 11.1 批次

| 批次 | 交付 | 退出条件／状态 |
|---|---|---|
| WH-00 | 固定上游全部类型／方法／配置／默认值与Rust映射，复用／许可／MSRV、公开契约、迁移及状态机 | 逐项裁决，不留仅名称命中或悄悄删入口；计划已记录，未实施 |
| WH-01 | 两可选包骨架、Provider／配置、接收契约及独立Validator／Profile／Response／handler消费者 | 安装／重复安装、HTTP／Artisan／Worker基础装配和配置缓存；不计完整持久恢复完成 |
| WH-02 | 接收全入口、raw／form／附件、可信保存、去重冲突、处理Job、失败／清理和路由 | WC-01至WC-13公开消费者及关键反例通过；可靠接收补偿由WH-03收尾 |
| WH-03 | 接收持久恢复、WD-01信任分类、WD-02 Deferred、WD-04领取、提前通知／补偿／版本及内容审核集成消费者 | 实际数据库／并发／崩溃点、外部审核类handler验证；构成CM-03接收前置，四家完整联合消费者随CM-03／WH-07收尾，不反向要求CM-03先完成 |
| WH-04 | 发送WebhookCall完整Builder、Signer／Backoff／自定义Job、同步／异步／条件、事件／HTTP行为 | WS-01至WS-12与其他公开类型覆盖；WD-07基础TLS／SSRF／资源限制、最小持久记录及统一领取前置通过，再实测HTTP；未过仅计受控原型 |
| WH-05 | 发送持久记录、扇出／尝试、重试／截止／Unknown、秘钥与端点版本、限流／网络安全 | WD-03当前端点许可、WD-05原子扇出、WD-06预算／重放代次、崩溃恢复／多Worker、mTLS／代理、逐次事件与快照一致性；WH-04基础安全不得延期到此 |
| WH-06 | 查询／重放／清理、持久端点及显式订阅接缝、事件类型、隔离与运维指标 | 第9节全部有服务／Artisan实际消费者和权限／反例，不以文档建议代替实现 |
| WH-07 | 原生完整范围跨批回归、三语公开指南／支持矩阵、生成应用与恢复演练 | WH-V01至WH-V27全矩阵通过／真实外部边界单列，用户验收；未实施 |
| WH-08 | 可选Svix投递后端／其他平台、严格FIFO、管理UI等 | 另行核定实际需求、费用／环境／许可后实施，不计原生必需范围，未实施 |

批次可按真实文件冲突和依赖调整，不为展示并行而新增分支或代理。共享工作区保留他人改动；实现负责人可直接补必要核心接缝并验收。当前只保存计划，用户安排开发后再进入WH-00。

### 11.2 必需验收

| 编号 | 内容 |
|---|---|
| WH-V01 | WC-01至WC-13、WS-01至WS-12及其他公开类型／配置逐入口映射；独立导入／实际消费者，继承／trait／默认值和安全适配明确 |
| WH-V02 | 独立crate实现Validator／Signer／Profile／Response／Job／Backoff／存储定制，正式装配与错误；不能只测包内部，保持多应用隔离 |
| WH-V03 | 全新生成应用、两个包各自安装／联合安装／重复安装、自定义配置／模型／stub保留；真实HTTP和独立Artisan／Worker、配置缓存前后 |
| WH-V04 | Spatie兼容与Standard签名跨语言向量，raw／JSON／form／multipart、空body、非UTF8与不支持输入，原始字节／重复Header／错误时间窗／轮换秘钥／格式畸形 |
| WH-V05 | 路由方法／组／前缀／同名／多配置，控制器／手动Processor一致，专用CSRF边界和供应商响应，签名失败／忽略事件／存储失败反例 |
| WH-V06 | 接收去重竞争、同ID不同body冲突、忽略与可信状态、提前回调／未知关联、落库后派发崩溃、处理失败、业务事务回滚与重放 |
| WH-V07 | JSON／raw正文稳定，UUID／消息／delivery／attempt身份、提交幂等同键同正文／异正文冲突、Header合并、HTTP方法、同步／异步／条件为假、元数据／tags不进入错误请求体 |
| WH-V08 | HTTP状态分类、连接／TLS／超时、2xx接收与Unknown、Retry-After日期／秒／非法值、退避／抖动、最大尝试／截止、重启预算不清零，防止两层重试相乘 |
| WH-V09 | mTLS成功／错误证书／过期、默认TLS验证／显式测试关闭、代理与不自动重定向、DNS重绑／解析-连接竞态、内网／元数据地址、受信内部策略 |
| WH-V10 | 正式三库迁移、唯一性／并发／短锁／行锁／回滚、恢复扫描和租约；存储适配边界与业务事务，不用内存mock代替 |
| WH-V11 | 多端点订阅／事件schema／扇出，单端点失败隔离，端点暂停／更新／删除、版本及秘钥轮换、取消／截止与迟到响应，不默认宣称FIFO |
| WH-V12 | 事件载荷／触发时机／次数、异常source／脱敏，配置命令／缓存／payload／附件／失败记录／日志的哨兵秘钥泄漏测试 |
| WH-V13 | 公开查询／Artisan、重试／重放权限和审计、分页清理、附件／正文／幂等墓碑保留，已清理载荷禁止假重放 |
| WH-V14 | 内容审核真实handler接入：四家验证器／不同回调形态、提前关联／查单、一个接收记录体系；Pay／Cashier原契约回归且未强制迁移 |
| WH-V15 | Bus／Queue／HTTP测试替身与真实HTTP服务／独立Worker双覆盖；生成应用内两端互通、进程终止／重启恢复、正常关停与实际资源清理 |
| WH-V16 | Rust最低支持工具链1.94.0，fmt、严格workspace Clippy及相关回归；标准签名库／SDK依赖版本／许可与跨平台实际检查，未验平台如实列出 |
| WH-V17 | 三语完整可编译用法／自定义扩展／安装／配置／Artisan／错误／重放说明；记录真实供应商、外部接收端、TLS及平台的实际测试条件，不以模拟效果计全部通过 |

实际HTTP须包括合法收发、签名／原始字节、2xx／4xx／5xx／断连／延迟、并发重复、缓存前后与重启。协议模拟可验证代码和恢复，但第三方托管平台、供应商账号及正式TLS等不能据此算真实通过；无条件时列实现已验／外部未验，不能删任务或宣称恰好一次。

## 12. 交付管理与接续

- 计划中的包／命令名是实施基准，WH-00按实际上游类型与项目命名落实，必要调整记录理由；不把上游没有的扩展说成基线已有，不用Rust适配掩盖缺方法。
- 三语文档放对应packages正文，内部逐项账本／运行记录建议放`.agents/project/audits/webhooks/`，只有真实实施后创建有效证据，不先堆空验收文件。最终映射包含来源、公开入口、裁决、测试路径／结果、剩余限制。
- 代码注释及未国际化提示按项目中文规则，协议Header／状态／机器字段保留。模板、配置、迁移、安装／发布和独立应用消费者一起验收。
- 临时应用／日志／构建统一`target/tmp/`，设置项目内TMPDIR／独占CARGO_TARGET_DIR，按规范复用缓存与磁盘停线清理。每批完成后只本地提交本任务已验范围、不推送，保留其他协作者WIP。
- 本次落盘只检查文档链接／覆盖／差异，不运行无关Rust构建，不安装服务、不修改支付／计费／审核业务、不发真实Webhook、不推送。后续开发需由用户安排；实际服务接入及付费操作按后续授权执行。

## 13. 七项审计补充决策与成熟实践依据

2026-11-25用户要求将补充调研后的建议和决策写入计划。本节补齐上轮审计发现的七项接缝，并覆盖前文含糊部分；关闭的是**计划欠缺**，并非实现／运行验收。本轮仍只改文档，后续由用户安排实施。

### 13.1 补充调研与采用理由

| 固定依据 | 已核对行为 | 本计划采用／不采用 |
|---|---|---|
| [Spatie SignatureValidator](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/SignatureValidator/SignatureValidator.php)及[Processor](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/WebhookProcessor.php) | 验签契约返回bool，流程区分验证、Profile、存储、处理及Response | 保留兼容契约和职责；可信／非可信提示的分类是下面明确的Rust扩展，不假称Spatie原生提供三态或待关联状态 |
| [固定Laravel队列文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md)及[本地Worker](../../../vendor/openrust/framework/src/queue/worker.rs) | release／限流／防重叠重新执行仍计Queue attempts；timeout须短于retry_after，避免旧Worker尚未停止就重新领取 | 复用release／延迟派发与既有调度；领域Deferred预算与Queue尝试分离，不改全框架attempts契约，不靠无限tries掩盖等待关联 |
| [Svix重试说明](https://docs.svix.com/retries)及[固定Worker](https://github.com/svix/svix-webhooks/blob/931467b95a1bbcc520c47bcb96f4e436782abcd2/server/svix-server/src/worker.rs) | 文档明确禁用／删除端点停止投递；源码区分Manual／Scheduled，人工重试失败路径与普通计划重试不同 | 借鉴当前端点控制优先、人工／自动执行分离。具体租约／预算／重放代次是本计划决定，不声称可直接复制整套实现；源码仍有重发可能产生重复队列任务的相关TODO，成熟参考不替代本项目竞争测试 |
| [Svix幂等说明](https://docs.svix.com/idempotency)及[固定消息入口](https://github.com/svix/svix-webhooks/blob/931467b95a1bbcc520c47bcb96f4e436782abcd2/server/svix-server/src/v1/endpoints/message.rs) | 区分API提交幂等、event ID与接收端去重；源码可见消息／正文事务写入 | 采用身份与事务职责分离。单事务生成完整投递集合是本项目有界扇出的决策，不将该局部源码证据外推为Svix与本项目扇出原子性等价 |

未执行第三方测试／服务或生产流量。无需为此新增Temporal、第二套Queue、分布式锁服务或通用状态机引擎；复用当前Database条件更新／唯一约束、Queue、Scheduler及锁接缝，只增加必要的Webhook领域记录与状态。

### 13.2 WD-01：验证分类与查单提示独立

- 接收上下文明确保存`Verified`、`UnverifiedHint`、`Rejected`分类，包含采用的验证器／策略／凭据版本及有限验证依据。分类是接收可信度，不是业务处理状态或审核结论。
- 保留Spatie的bool式`is_valid`兼容适配：配置要求验签时，true映射为Verified，false／缺签名／格式错误均拒绝。不得让验证器用返回true表达“暂且查单”。
- 另设小型显式接收策略接缝（名称可按WH-00落实）：仅在该profile明确允许无签名提示时，缺签名请求才可标为UnverifiedHint；错误签名不得降级绕过验证。Hint单独存储／标记、限流和计数，不能进入只接受可信事件的handler。
- Hint处理器只能针对本地已登记且归属一致的业务任务执行受限查单，未知任务直接拒绝／忽略。查询后的供应商认证响应由领域服务独立验证，不能反向把原通知改成已验签。拒绝通知不进入可信业务队列。
- 独立crate能实际实现兼容Validator、接收策略和Hint消费者；公开DTO不能意外隐藏构造接缝，也不能把Hint构造成Pay／Cashier的可信领域类型。

### 13.3 WD-02：待关联与处理结果

- 定义小型领域处理结果：`Completed`、`Deferred`、`RetryableFailure`、`PermanentFailure`，正式Rust签名在WH-00落实；保留上游Job／handler使用习惯，通过适配接入，不修改通用Queue返回契约。
- Deferred携带有界原因／下次时间，接收记录进入待关联／待处理状态，保存`next_attempt_at`、总截止、累计暂缓次数和处理代次；不能将Job返回Ok直接标为业务Completed，也不把Deferred一律计作业务异常。
- Queue attempts、领域失败次数、领域暂缓次数分别计数。release仍遵循原Queue规则；若本地通知Job已结束或耗尽尝试，持久Deferred记录由有限Scheduler重新唤醒，必要时生成新的调度Job，但不重置领域截止／暂缓预算。
- 领域任务关联就绪可主动唤醒，扫描作为恢复补偿；两者使用同一领取与代次校验避免重复。超过截止／次数则形成明确终态／待人工诊断，不永久重排。
- 收件ACK只表示记录已可靠接收，Deferred仍未业务完成。覆盖回调先到、提交响应延迟、关联永不出现、Worker重启、重复唤醒和预算耗尽。

### 13.4 WD-03：端点当前授权优先于历史快照

- 历史快照保存目的地、协议和配置版本；**当前端点暂停、删除、租户归属变更或紧急凭据撤销**决定是否允许获得新发送许可，不能靠旧快照绕过。
- 每次自动、同步、人工重放／恢复尝试均检查权威端点状态。暂停表示保留投递并在有限期限内等待显式恢复；删除／归属失效禁止后续发送并形成可诊断取消／终止状态；秘钥撤销按凭据策略失败，不使用其他账号兜底。
- 领取尝试与端点禁用通过短事务／一致控制版本确定先后；发送前再检查取消／控制版本。已获许可且进入执行的请求存在竞态，不能承诺撤回已发HTTP；明确暂停／删除只保证阻止其后取得许可的尝试，不宣称撤销后世界上绝无在途数据包。
- 恢复端点不自动复活已终态投递；只有仍未到期的暂停项按原预算恢复。改URL不悄悄迁移旧快照，显式新投递／新代次必须有来源、授权及网络校验；出站重放也遵守当前控制状态。

### 13.5 WD-04：同步、Worker与恢复扫描统一领取

- 所有执行路径进入同一storage领取方法，以应用／租户、delivery／处理记录ID、执行代次和条件状态为边界。短事务原子设置owner／claim token、递增执行代次、租约和对应尝试记录；使用现有数据库／锁能力，不新建通用租约服务。
- 完成、失败、延期、预算扣减与租约续期必须携带同一token／代次并条件更新。旧Worker晚到不能覆盖新执行或端点撤销／取消／过期结果；只可追加属于自己尝试的受控诊断。
- 同步dispatch也先领取，再走同一发送实现；Scheduler不另走旁路。同步调用退出／崩溃后，由租约及恢复路径处理，不同时启动第二次正常尝试。
- 网络超时、Job timeout、Queue retry_after／SQS可见性与领域租约分别核定，留出正常清理／写回余量，避免旧请求未结束就重新领取；长操作按已存在能力续期或在有限HTTP超时内结束，不能用无限租约保活。
- 租约不能保证跨网络恰好一次；进程失联／远端已处理但响应丢失仍可能导致至少一次重试，稳定消息ID、attempt记录和对方幂等继续必要。

### 13.6 WD-05：有界扇出的原子生成

- 首版选择简单确定的实现：限制单条消息可投递的端点数量，在同一短数据库事务内持久消息／正文引用、固定端点版本集合和全部Delivery。不在事务中联网或调用用户远程handler。
- 同一消息＋端点版本集合项具有唯一约束；提交幂等相同输入返回同一已完整生成结果，不因部分记录存在就假称完成。并发订阅修改只影响其后按规则取得的新消息投递集合。
- 如果正文位于Storage，先写入不可变私有对象，再以引用完成数据库事务；回滚后的孤立对象由有期限清理处理。不能假称Storage写入与数据库天然原子，也不能将缺正文的引用标为可投递。
- 超出扇出上限明确拒绝／报错，不静默截断。只有实际负载证明需要大规模展开时，再单列可恢复分批manifest方案，首版不建设分发引擎。
- 崩溃点测试覆盖保存消息前、保存部分Delivery后事务回滚、事务提交后派发前，以及与订阅更新／同幂等键并发；恢复不漏端点、不为同集合重复建投递。

### 13.7 WD-06：自动重试、人工重放与新消息

| 操作 | 身份／状态 | 预算／并发规则 |
|---|---|---|
| 自动重试 | 原消息、原delivery及原执行代次下的新attempt | 共享原始截止／尝试预算，不改成功或已取消／过期的终态 |
| 人工重放 | 原消息正文／稳定消息ID；建立关联原delivery的独立重放代次，原历史不改回Pending | 独立且有限的重放次数／期限，并计入端点／账号总额度和人工操作限流；先获得当前端点许可。存在活动自动重试／重放时拒绝冲突请求，需显式暂停／取消后再做，不并行竞发 |
| 复制为新消息 | 显式创建新消息ID、记录原消息／操作原因，按当前订阅生成新集合 | 走新消息幂等、权限与正常预算；不将“重放”API静默变成绕过去重的新消息 |

- 原生建议默认只允许重放最终失败的投递／失败业务处理；成功项重放或恢复整段历史必须显式选择并拥有更高权限、提供理由，不能由日常重试默认执行。取消／过期项保持终态，若需发送建立明确关联的新重放代次并重新检查授权。
- 收件重放只能来自原始可信记录，保留验证证据和原始业务幂等；不因原外部时间戳已过期而重新将合法历史判伪，也不能跳过当前撤销和载荷留存检查。Deferred应走唤醒，不当失败重放。
- 出站重放保持原正文／消息ID，按当前获准使用的凭据版本生成本次签名／时间戳；记录凭据迁移决定，不使用被撤销修订。接收方可能按稳定消息ID去重，因此重放只承诺重新投递，不承诺强制对端再次产生业务效果。
- Svix是人工／自动路径分离的参考，不将其具体重试表或人工处理行为直接当成本项目全部语义。

### 13.8 WD-07：首次真实发送门禁前移

- WH-04首次产生真实HTTP请求前，须具备默认TLS、默认禁止重定向、目标地址与实际连接防护、Header／正文／响应大小及超时上限、不可变正文、最小尝试记录和WD-04统一领取。WS-01至WS-12不能用无安全旁路先交付。
- WH-05完善跨进程补偿、完整扇出／预算、端点控制和运行观测，不能被解释为WH-04可以暂时忽略基础安全／领取。若门禁未通过，WH-04只可保留不可对外交付的受控原型，不能计公开发送能力完成。
- WH-03、WH-04和WH-05依赖共享的最小持久状态时，可在WH-01后先落公共存储接缝；不以原批次编号为由跳过必要依赖，调整记录在实际验收中。

### 13.9 审计问题到决定与新增验收的映射

| 问题 | 本次决定 | 必须实际执行的验收 |
|---|---|---|
| WA-01 信任分类不明确 | WD-01 | WH-V18：兼容bool Validator＋显式Hint策略的独立消费者，缺签名／错签名／禁Hint／未知归属分别处理，Hint不能进入可信handler |
| WA-02 待关联处理缺语义 | WD-02 | WH-V19：Deferred唤醒、Queue attempts与领域预算分离、关联出现／永久缺失、重启／重复唤醒／到期，不误标Completed |
| WA-03 撤销与快照冲突 | WD-03 | WH-V20：暂停／删除／恢复／归属改变与快照、凭据撤销、领取前后竞态，自动／同步／重放均不能绕过当前许可 |
| WA-04 同步与恢复竞争 | WD-04 | WH-V21：同步／多Worker／扫描竞争同记录、租约过期／续期、旧token写回、取消／终态竞争及HTTP超时；不宣称网络恰好一次 |
| WA-05 扇出部分生成 | WD-05 | WH-V22：消息与全部投递事务原子性、唯一约束、订阅并发／幂等重复、Storage孤立对象、超上限拒绝及提交后派发崩溃 |
| WA-06 重放身份／预算含糊 | WD-06 | WH-V23：自动／人工／新消息身份及预算、活动重放冲突、成功项权限、保留历史、原消息ID对端去重与载荷已清理反例 |
| WA-07 首次发送门禁倒置 | WD-07 | WH-V24：WH-04基础TLS／SSRF／大小／超时／领取门禁，以及WH-05恢复能力分别有实际证据，不把受控原型计可交付 |

WH-V18至WH-V24与原WH-V01至WH-V17及第14节WH-V25至WH-V27一起构成必需验收；目前全部仍是待执行项。上游源码／文档验证只支撑设计决定，不能替代本项目HTTP、独立Artisan／Worker、数据库并发与崩溃点实测。

## 14. 单机与集群使用同一套实现（用户明确要求）

2026-11-25用户确认计划必须同时包含客户端和服务端，并要求**同一套方案可用于单机与集群部署**。两包已分别覆盖接收与发送；本节将部署能力列为原生必交付范围，不放到可选平台后端或未来“集群版”。当前同一制品已运行单机和同主机双节点验收，具体覆盖及仍开放的故障场景见[拓扑记录](../audits/webhooks/topology.md)。

### 14.1 同一代码、公开API与数据契约

- `openrust-webhook-client`负责接收、验证／筛选、可靠保存、入队及协议响应；`openrust-webhook-server`负责构建、签名、发送、重试和投递记录。两者都必须适用于下面两种部署，并可独立或一起安装。
- 使用相同crate、Builder／Processor／扩展契约、迁移、状态机、幂等键、消息／任务DTO和错误语义；区别仅在部署配置与进程副本数，不复制单机／集群业务代码，不增加平行队列／存储引擎或自动集群探测器。
- 通过既有Database／Queue／Cache／Storage配置选择后端并启动多个HTTP／Worker／Scheduler。进程内仅缓存不可变解析结果；权威状态、去重、授权、领取、预算及恢复必须使用选定的持久／共享后端，单机也走同一正确性路径。
- “同一方案”不等于任意后端组合都能跨主机共享，也不等于承诺所有规模的容量／底层基础设施高可用。明确下表受支持组合、实际容量和未验边界，不能用本机跑几个Worker替代所有集群验证。

### 14.2 建议部署组合

| 资源 | 单机部署 | 集群部署 |
|---|---|---|
| HTTP | 一个应用实例，可带多个进程 | 负载均衡后多个实例，任意节点可接收同一个profile回调；Webhook不依赖粘性会话 |
| 权威数据库 | 本机或外部PostgreSQL／MySQL；SQLite作为单机受测组合 | 所有节点访问同一受支持PostgreSQL／MySQL主写服务／逻辑数据库；去重、领取、端点授权及完成判定不读可能滞后的只读副本 |
| Queue | 既有database队列，或已支持的Redis等具名连接；异步恢复不以sync／null／进程内队列冒充 | 同一共享database／Redis等已验后端与相同队列命名；HTTP、Worker可不同节点。SQS等其他后端按项目支持矩阵另验，不因原包可配置就宣称本批全通过 |
| 锁／限流 | 复用database或受测本机file／Redis策略；数据库条件更新仍是领域权威 | database／Redis等真实跨节点共享后端；所有节点共同计数和领取，不能各用自己的memory／file缓存来实现全局限流 |
| 正文／附件 | 私有local磁盘或S3，HTTP／Worker实际可读取 | 共享S3／对象存储，或经过实际验证的共享文件系统；节点本地临时文件不得成为跨节点任务的持久正文引用 |
| 凭据／配置 | 受控配置引用与版本、必要APP_KEY、默认注入路径完整 | 各节点可取得相同已授权修订；加密payload使用一致受控密钥／版本，不能每个节点随机生成密钥。缺修订明确失败，不改用当前默认账号 |
| Scheduler | 一个定时进程可完成补偿／清理 | 一个或多个实例均可安全运行，任务按共享锁／条件领取协调；单实例调度是可用部署选择，不能作为避免重复的唯一正确性保证 |

集群不强制新增Redis：共享PostgreSQL／MySQL＋既有database队列／缓存锁是必须验证的简化组合；Redis是另一受测选择。SQLite文件共享、每节点独立数据库、节点私有file锁／附件都不计为支持的跨主机组合。单机file缓存的实际并发能力按项目测试覆盖，不能外推为网络文件系统锁保证。

### 14.3 跨节点不变量与配置检查

1. **统一作用域。**相同应用／租户在所有节点使用相同的稳定作用域、表／队列／缓存前缀和限流键；节点ID仅用于领取owner／诊断，不得进入全局去重／额度键导致各算各的。不同应用／租户必须保持隔离，不能因为共用Redis或数据库发生串单。
2. **共同领取。**WD-04的token／代次／租约跨节点有效，跨节点完成、暂停／撤销、重放、预算、扇出与清理都遵守同一条件更新；不存在“单机只用Mutex，集群另走一套算法”的旁路。
3. **调度安全。**复用Laravel风格的共享锁／单服务器调度机制减少扫描，但领域领取允许多个扫描器竞争；锁过期／扫描器死亡后可恢复。扫表分页、抢占与索引必须有界，不能持有大事务或导致一个节点独占所有端点。
4. **文件与秘钥可访问。**ACK前完成所需持久正文／附件记录与可靠接收条件；HTTP节点退出后另一Worker仍能处理。配置修订、插件／handler注册名、加密秘钥与凭据引用在对应节点可用，部署诊断不输出秘钥。
5. **不静默降级。**启动／配置诊断检查所选后端、持久恢复需求、处理器注册、共享命名及资源引用等可判定前提；明确错误组合，不回退本地memory／临时文件／其他账号。诊断不能自动证明两个数据库实际是同一逻辑库或网络永不分区，跨节点实际验收仍必需。
6. **协调时基。**租约／截止等持久比较使用统一时基，优先权威数据库时间；时间单位沿项目规范，客户端时钟偏差不能让旧owner越权完成。外部签名时间窗继续按供应商协议，节点需正常校时，不引入新的时钟服务。

复用依据：[固定Laravel调度文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md)、[缓存原子锁](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md)、[Queue文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md)。采用共享协调与既有Worker机制，不据此推定Webhook全部集群语义已自动提供；实际后端能力及行为按本地源码与测试裁决。

### 14.4 故障、滚动升级与扩缩容

- 杀掉接收节点、处理Worker或扫描器，其他节点从共享记录接续；覆盖接收落库后／派发前、投递领取后／发送前、远端已处理／本地完成前等窗口，仍是至少一次交付。
- 数据库／共享锁／队列／对象存储不可用或发生网络分区时，不假ACK、不跳过授权／领取、不退回本地缓存冒充共享成功。已持久接收但派发失败的恢复与仍在途HTTP的不确定性分别记录，故障恢复不重置预算。
- 扩缩容与滚动升级复用正常关停／Worker重启：停止领取新任务，允许有界在途任务结束或由租约恢复；保持消息、配置和凭据修订可读。旧版本不认识的新payload不能错处理或丢弃，支持范围／升级次序明确。
- 迁移由受控发布步骤执行，避免每个节点并发自动迁移；两版共存时的数据／DTO兼容与回滚限制需要实际测试。单机转集群不改变业务API；若同时更换数据库驱动／存储位置，按备份、数据迁移和切换流程独立验收，不承诺SQLite热切换到PostgreSQL自动无损。
- 基础设施自身主从／容灾不是本包另建一套实现；对数据库／Redis／对象服务恢复后的行为必须验证，未部署真实HA服务时不能宣称已经证明跨地域／多主容灾。

### 14.5 双拓扑验收与批次门禁

| 编号 | 必须执行的证据 |
|---|---|
| WH-V25 单机 | 同一提交／制品使用database队列完成接收与发送；HTTP、独立Artisan／Worker／Scheduler，缓存前后、同步／异步、多Worker、退出／重启恢复。按项目三库与受影响缓存驱动要求验证，SQLite仅标单机覆盖 |
| WH-V26 集群 | 至少两个具有独立进程与本地文件空间的应用节点，经负载均衡交替接收；各有Worker，至少两个Scheduler竞争。分别验证共享PostgreSQL／MySQL＋database队列／锁组合及共享Redis队列／锁组合，正文／附件可跨节点读取；同事件并发、同投递竞争、总额度不能按副本倍增，租户隔离通过 |
| WH-V27 故障与升级 | 集群中逐个终止HTTP／Worker／Scheduler，模拟共享服务断连／恢复、旧owner回归、端点撤销、文件／凭据修订缺失、节点时差；同制品扩缩容、兼容版本滚动升级与正式迁移，稳定身份／预算／历史不丢失 |

WH-00／WH-01落实同一API与配置能力；WH-03接收恢复、WH-04首次安全发送、WH-05持久投递须按其已完成范围覆盖双拓扑，WH-06管理／限流／清理也必须跨节点有效；WH-07以WH-V01至WH-V27作为最终门禁。不能单机完成就关闭模块、把集群放WH-08，或只拿一台机器的并发单元测试计所有集群项通过。

本机可用隔离容器／网络命名空间验证逻辑多节点，但必须标明模拟拓扑、独立文件空间与共享服务配置；真实跨主机、目标操作系统及正式基础设施测试尚未执行时单列未验范围，不能将容器模拟写成真实实机HA通过。最终记录部署组合、版本、副本数、外部依赖、实际HTTP／Artisan结果与未验边界；本次只是计划要求。
