# 运行时配置、数据库队列与动态调度

> 状态：用户授权的设计与排期；本轮仅事实审计和文档，不代表实现或真实验收。
> 依据：[ADR-080](../decisions/adr-080-runtime-configuration-scheduling.md)、[Laravel 固定基线](laravel-rebuild-baseline.md)、[唯一队列](../todo.md)。

## Spec 与 Laravel 体验

1. `.env` 作为启动来源，`config/` 保存领域配置，继续使用同一 `ConfigRepository`、类型化读取与 `ServiceProvider`；普通开发者不操作 revision/snapshot 客户端。
2. V1必须交付DB动态业务设置底层及受权限保护的管理服务/公开API；宿主是否启用该Provider扩展可选，不是本能力可有可无。只覆盖代码显式允许的值，未启用时保持现有启动读取。
3. 数据库是运行时配置、管理员计划和默认队列任务的权威存储；Redis 通知和缓存可丢，不影响持久恢复。
4. 类型/范围/跨字段校验、权限、版本 CAS、审计和完整快照发布均由框架封装；不能以简洁为由省掉安全与一致性。
5. 任务实现由 Rust 代码注册；管理员只配置已注册任务的允许字段，动态计划进入同一 Scheduler/Queue，不启动第二调度服务。
6. 开发者注册typed分组/schema/defaults，管理员经未来后台编辑保存，不写Rust或.env；V1交付权限保护API与Laravel风格CLI，V2表单消费同一服务。具体公共命名、配置格式和默认范围在实现前 Spec 收敛，不在计划中预造 API。
7. 当前 HTTP 功能簇照常闭环；随后按 R2 契约 → R10 实现 → R14 真实验收排期，不立即实现或更改部署值。

此处“管理API”指Rust公开管理服务方法/必要traits，不是固定HTTP endpoint。框架不自动注册admin/settings路由，不预置URL、响应格式或权限标识；宿主在自定义路由/controller显式传入认证/授权上下文，经既有Gate/Policy调用同一服务。schema验证、脱敏返回与审计在服务内封装，不能暴露无鉴权管理入口。modules/example仅在自己的模块routes/controller示范；V2后台和显式注册的CLI复用同一服务，CLI也不得绕过授权，不新增SDK或控制面。

## 当前仓库事实与缺口

| 生产入口 | 已有事实 | 本目标尚缺 |
| --- | --- | --- |
| [`ConfigLoader`](../../../src/config/mod.rs) 与 [`ConfigRepository`](../../../src/config/repository.rs) | Figment/`.env`/领域配置；注册 typed section/schema/validator；锁外校验候选后比较进程内 revision 发布；`snapshot` 独立，clone 共享仓储 | 持久 DB revision/CAS/审计、运行时白名单、跨节点刷新与期限；现有 `version` 不是 DB 全局版本 |
| [`ProviderBuilder::start`](../../../src/core/provider.rs) | 全体 register → validate_base → 顺序 boot/overlay → freeze → finalizers/解析 → Ready；freeze 后拒绝 overlay | 在保留启动冻结语义下新增受控运行时更新路径；不能直接解冻整树或绕过注册屏障 |
| [`App`/生命周期](../../../src/core/lifecycle.rs) | 已有受管后台任务与 cancellation/join/有界关闭；服务和配置对象可在启动时被解析、持有 | 审计白名单消费者，避免长期持有旧 typed 值；刷新归同一资源所有者，不另起独立 daemon |
| [`Queue`/`JobRegistry`/`QueueContributions`](../../../src/queue.rs)、[`QueueConfig`](../../../src/config/queue.rs) | 默认 database，任务名显式注册；同库事务派发；DB lease/token、并发、失败与重试；共享内存贡献目录 | 管理权限、参数 schema 和允许策略目录；运行时 worker 并发调整；完整 Laravel after_commit/outbox 对应不能以当前事务方法冒充 |
| [`Scheduler`](../../../src/schedule.rs) | `entries()` 读取内存贡献；DB 时钟、cron/IANA/DST、有界补跑；slot 与 job 同事务、slot 唯一去重 | DB 可编辑计划/版本/审计/状态；禁用/删除线性化；当前所有计划全局 slot 去重不等于已完成可选 `onOneServer()` 默认语义 |
| [`Encryptor`](../../../src/security.rs)、[`Storage`](../../../src/storage.rs) | 已有带key ID/context的加密封装；storage manager缓存disk，extend不替换现有实例，purge移除缓存引用 | DB设置secret加密/权限/轮换迁移，以及业务渠道版本构建/原子激活/在途资源管理尚未接入；只更新Repository不重建客户端 |
| [队列/调度事实](../core/queue.md)、[调度事实](../core/schedule.md) | 有旧双库/Redis、多进程和故障专项记录 | 本轮未执行任何服务或 Cargo 验证，不能把旧证据计入新目标完成 |

## 固定源码参照与取舍

读取日期为 2026-10-29；Laravel/Goravel 沿用 R0 固定 commit，不跟浮动分支扩范围。下列为机制参考，无源码复制。

| 来源 | 本轮读取的固定源码与问题 | 采纳、调整及边界 |
| --- | --- | --- |
| Laravel framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181` | [Config Repository](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Config/Repository.php)、[DatabaseQueue](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Queue/DatabaseQueue.php)、[Queue::enqueueUsing](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Queue/Queue.php)、[Schedule](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Console/Scheduling/Schedule.php)、[Event](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Console/Scheduling/Event.php) | 沿用配置读取、job/connection/queue、after_commit 和独立调度策略职责；用 Rust typed/async 与公开 Provider 扩展。DB 跨节点热更新和管理员动态计划是授权增强，不宣称 Laravel 核心原生提供 |
| Goravel framework `a554ced468cada50a269780692a0491e5f9e3a11` | [config/application.go](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/config/application.go)、[queue/driver_database.go](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/queue/driver_database.go)、[schedule/application.go](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/schedule/application.go)、[schedule/event.go](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/schedule/event.go) | 参考领域注册/命名连接和调度职责；不复制 Go callback/全局 Facade，也不照搬其该快照数据库领取对 cache 锁的依赖或替换现有 SQL fencing；框架默认队列仍 DB |
| Spatie laravel-settings `3b152ac9dc7dc6755d09fea3f796e0e790c55778` | [配置默认DB repository](https://github.com/spatie/laravel-settings/blob/3b152ac9dc7dc6755d09fea3f796e0e790c55778/config/settings.php)、[typed Settings/save/refresh/encrypted](https://github.com/spatie/laravel-settings/blob/3b152ac9dc7dc6755d09fea3f796e0e790c55778/src/Settings.php) | 采用typed分组业务设置、DB持久化与字段加密理念，经现Repository/Provider公开扩展；不引入PHP依赖、不另造配置系统。Rust常驻多节点的CAS/期限/原子快照由内部补齐，不假设该包提供本项目全部保证 |
| [Filament官方Settings插件](https://filamentphp.com/plugins/filament-spatie-settings)（页面2026-10-29读取） | 后台表单直接读写Settings，区分canAccess/canEdit | V2只消费V1受权管理服务；能编辑不等于能读取secret，secret默认不回显。仅体验参考，不采用其PHP UI、不改变既定Vue/uni-app路线 |
| RuoYi-Vue `13db1fcef36bee9ce45d2d636a1d4e8f5ed5bbc3` | [SysConfigServiceImpl](https://github.com/yangzongzhuan/RuoYi-Vue/blob/13db1fcef36bee9ce45d2d636a1d4e8f5ed5bbc3/ruoyi-system/src/main/java/com/ruoyi/system/service/impl/SysConfigServiceImpl.java) 的DB写后缓存更新与启动加载 | 仅证明DB加缓存的常见职责划分，不足以证明原子多节点快照/CAS；不照搬其Java/Redis缓存客户端或缺少的保证 |
| Laravel同一固定commit [MailManager](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Mail/MailManager.php)、[Goravel配置说明](https://www.goravel.dev/getting-started/configuration.html)（页面2026-10-29读取） | MailManager缓存已解析mailer并提供purge；Goravel说明.env/config/Get/Add | config set/Add不等于DB持久化，也不自动更换在用mailer。保留Laravel领域Manager职责，业务driver显式构建新实例再切换，不靠改配置对象伪造生效 |
| OpenSpring 本地只读、干净 HEAD `d04eaa02995e95221ae5deef440afe8529495345` | `openspring-core/openspring-core-config/src/main/java/com/openspring/core/config/` 下 `RuntimeConfigWriter.java`、`provider/DatabaseConfigRepository.java`、`binding/DynamicConfigRegistry.java`、`refresh/ConfigRefreshManager.java`；`openspring-starters/openspring-starter-system/src/main/java/com/openspring/starter/system/config/` 下 `service/impl/SysConfigServiceImpl.java`、`support/ConfigDatabaseConfigRepository.java`；`openspring-core/openspring-core-scheduler/src/main/java/com/openspring/core/scheduler/provider/DbSchedulerTaskDispatcher.java` | 参考批量候选校验、DB revision 快照、提交后通知和已注册任务分发；不搬 Java 代理/注解/反射、Spring 事务线程绑定或 db-scheduler 库。其本地源码可用 `git -C /Users/Home/Codes/Java/openspring show <commit>:<path>` 复核；不改该仓库 |

上述源码读取不代替上游测试对应；实施 Spec 须在固定 revision 中补齐精确测试 → Rust 行为用例、必要差异和 modules/example 证据，仍沿总矩阵登记。

## 配置持久化与生效

- 代码经现有 Provider/section 机制登记可变键、类型、范围/长度/集合边界、跨字段 validator、读写权限与敏感标记；默认不可动态修改。不得用 `Any`、未知 map 或管理端输入绕过白名单。模块禁用或 schema 不兼容时明确拒绝，不抹掉数据。
- 基础设施DB/Redis driver、连接/凭据、APP_KEY/加密keyring部署密钥、编译feature、路由、任务实现代码和监听地址/端口属于启动配置，经.env/领域配置与部署重启生效。SMS/mail/payment/storage业务渠道凭据不能一刀切排除，按下节显式driver能力受控激活；本目标不做任意资源全热换。
- 启动配置和白名单元数据仍由代码决定。可选扩展启用后，DB 中已提交的允许值覆盖对应启动默认值；管理员恢复默认也须显式受权事务操作与审计，不因 DB 故障回退 `.env`。首次安装的默认值初始化与 schema 升级须有明确 CLI/迁移流程；缺表或损坏不能隐式新建空权威视图。
- 以应用隔离命名空间保存一致配置集合、单调 revision、schema 版本和审计。写操作先鉴权，再在同一 DB 事务中核对 expected revision、读取一致当前集合、形成完整候选并执行全部校验，原子写值/递增版本/审计；冲突不覆盖、不自动重放管理员修改。审计失败则事务失败；空修改不制造假版本。
- DB 提交成功表示持久化成功；本节点发布及各节点生效分别可观测，通知发送失败不能谎报 DB 回滚。无效/越权/未知键/CAS 冲突返回稳定安全错误，日志和审计详情按既有脱敏/白名单机制，不记录秘密原值。
- 每节点从 DB 一致快照读取 revision 与全套允许值，完整验证后原子替换运行视图；不得逐键修改在用对象。过期、乱序或重复通知不能使版本倒退。普通代码继续仓储/类型化读取，框架内部为一次请求/任务操作固定读取代次，避免同一操作混用不同版本；已开始操作不追溯改值。
- 沿用启动 ConfigRepository 的 schema 与唯一发布机制，区分冻结的启动元数据/值和受限的运行时值，不能另建独立配置客户端或放开现有 boot overlay。白名单消费者须改为实际读取已发布运行视图，单改仓储而 singleton 永远持旧值不算完成。
- 提交后发 revision 失效提示；Redis 只加速，消息不携带可信配置值。各 API/worker/scheduler 用低频 DB 版本轮询补偿，通知丢失/断连/重连都从 DB 追平，cache 清空可重建；无额外消息总线。采用部署级有限默认轮询/读取超时/最大陈旧期；5秒轮询仅候选，经Spec性能与双库实测再定，不宣称硬实时SLA，普通用户不必逐项调参。
- 启动加载失败则不 Ready；写时 DB 不可用直接失败。运行读失败标记 unhealthy、记录错误和当前版本/最后确认时间；仅在事先定义的有限陈旧期内允许既有完整快照服务，到期后受影响读取/新任务触发明确失败，不能永久用本地旧值或静默退内存。刷新在途有超时、退避和资源上限，关闭归既有 cancellation/drain/join。
- 正常生效上界包含轮询周期、DB 读取与本地验证发布的有界耗时；不是同步所有节点的分布式事务。若某节点没在期限内追平必须可诊断，不把“通知已发”报告为全节点生效。权限收紧等不能容忍陈旧的操作须在执行授权处重新确认，不能只依赖旧任务/快照的权限信息。

## 管理员业务设置与驱动激活

- 最短体验是开发者在现Provider中登记typed分组/schema/defaults，管理员在后台修改邮件from、SMS模板/通道、支付sandbox账号、storage bucket/endpoint等允许字段后保存；V1先提供供后台调用的受权管理服务/API、校验错误和CLI，V2才绘制表单。管理员不修改.env、不编写任务/驱动代码；支付本轮只规划配置底座与现有/公开driver对接，不新增支付产品业务。
- 区分配置读取、编辑、secret替换/清除等权限，默认secret不返回明文，脱敏占位不能被保存成新secret。DB持久值使用现Encryptor及密钥轮换机制加密，绑定应用/分组/字段上下文；APP_KEY/实际加密keyring部署密钥留在DB之外，迁移须保留必要旧解密key。现Encryptor已有keyring接口，不据此虚称APP_KEY已是其现有映射。日志、错误、审计不含secret；审计只记录安全字段名/版本/操作者等。密钥丢失显错，不静默覆盖或返回默认。
- 字段声明区分可直接发布值、需要重建driver的业务设置、仅重启生效项。代码只开放已实现安全刷新能力的driver/连接，限制endpoint协议/主机/出站目标、超时及参数范围，复用现安全校验防SSRF；管理员可填URL不等于任意内网访问或任意插件代码。具体secret/热字段白名单逐driver Spec确定。
- 保存事务首先产生DB期望版本；响应必须区分“DB已保存”和各节点“已应用/应用失败/待重启”。支持刷新时，节点按完整候选配置在锁外构建并校验新driver实例，成功后对该分组配置与资源代次一起原子激活，后续新操作使用新有效版本；不得先发布配置再让旧客户端继续冒充生效。跨组需要一起生效的值须同一验证/激活单元，不保证所有节点瞬时同步切换。
- 不支持安全刷新则明确 `pending_restart`，不宣称立即生效；构造/验证失败保留上一有效代次、报告失败和期限，超出允许陈旧期按既定边界拒绝新操作。DB已提交不因某节点应用失败而伪报回滚；回退版本是另一次受权CAS操作，不静默覆盖已提交历史。
- 在途请求持有原不可变配置/资源引用至操作结束，切换只影响后续操作；旧实例最后引用释放后走既有有界资源清理，不在切换时强关使用中的连接。供应商撤销旧凭据仍可能使在途失败，不能承诺外部系统无缝。
- 已排队任务保留dispatch时已确定的业务payload/内容/任务版本，不序列化渠道secret；每次attempt开始选择当时有效渠道代次并持有至结束，重试可能使用新凭据，行为须公开。需固定目标/幂等的任务显式保存非secret目标标识，不能改配置后把旧对象/支付重试错误路由到新bucket/账户；与现通知“内容入队即固定”契约一致。
- 应用+DB恢复可保留设置记录，但对象/本地文件不因DB迁移自动搬走，须另迁文件/对象及外部账号资源；更换storage bucket/endpoint也不自动搬历史对象，历史引用解析/迁移责任须在driver Spec与示例验证。

## 默认 DB 队列与事务

- 沿用 [ADR-003](../decisions/adr-003-database-queue-default.md)、[ADR-037](../decisions/adr-037-queue-worker-lifecycle.md)、[ADR-046](../decisions/adr-046-redis-queue-driver.md)，`database` 为默认持久队列；显式 Redis connection 仍可选，不能热切 driver/凭据或失败后换本地队列。
- 普通派发、同库同事务派发、Laravel `after_commit` 必须分别描述并测试：普通派发不自动与业务回滚绑定；同事务插入在提交后可见且回滚消失；after_commit 在外层提交后再派发，回滚不派发，提交到回调间崩溃可能丢失，不能称可靠 outbox。需可靠跨存储投递时在业务事务写 outbox，再由同一队列体系补偿投递，不能把 Redis 与 SQL 假称原子提交。
- Job 仍由代码注册稳定名称和 payload schema/版本，旧未完成任务的 handler/参数兼容属于滚动升级契约。管理员只能向授权的 connection/queue 和已注册 schema 派发/重试；不允许 Rust/PHP/Java 字符串反射执行、任意命令或 shell。未注册版本、无权限、无效参数均显错，不吞任务。
- 复用当前并发领取、lease/heartbeat/owner token、退避、timeout、failed/retry 关联和 SIGTERM/drain/join；至少一次与 handler 幂等边界保持，外部副作用不保证 exactly-once。运行状态、尝试和必要失败关联保留 DB，完成后有界清理与审计保留策略在 Spec 定义。
- 动态 worker 并发属于明确允许的运行参数：实施 Spec 先定作用域（connection/queue/worker组）、范围和合理默认。增并发只改变后续领取，降并发不强杀在途，暂停停止新领取、在途按原 deadline 排空；不得隐式热改现有 lease/timeout/存储。多节点实际并发总量与单进程上限不能混称。

## 管理员动态计划

- Rust 注册目录声明稳定任务名、payload schema/版本、允许目标 connection/queue、可选频率/IANA 时区/策略及约束；同一 JobRegistry/QueueContributions 与 Provider 提供。管理员不能注册实现代码。静态计划与动态计划的 ID/名称冲突显错，不互相覆盖。
- DB 保存计划标识、注册任务版本、启用状态、频率/时区、校验参数、目标队列、允许策略、CAS 版本/审计，以及去重水位/运行关联等必要状态；具体表名与公共字段在 R10 Spec 定稿。参数大小、计划数量、扫描分页/补跑数量、日志保留须有界。删除保留 tombstone/审计直到旧触发与引用可安全回收，不能因重建同名计划丢失防重历史。
- 新建/编辑/暂停/删除均鉴权、CAS、整计划校验后事务提交；沿同一通知+DB轮询路径刷新，暴露提交版本和生效状态。数据库计划以共享 DB 当前版本/状态为准；实际触发事务重新核对并锁定计划版本/启用状态，再提交 slot+DB job，不能只信缓存通知。
- 禁用/删除提交后不允许新触发事务使用旧版本入队；与变更竞争已先提交的 job 保留入队时参数/计划版本，默认继续执行，不撤销已提交副作用。停止/取消既有任务是独立授权动作和合作取消，不把删计划称作终止运行任务。已入队 job 在 worker 执行时仍确认任务注册及当前权限边界。
- 改频率/时区/参数以明确版本生效点隔离水位，默认不回填首次安装或改版前历史；版本过渡同一逻辑触发是否可重复、补跑上下限及 DST gap/fold 必须在 Spec 和真实竞争测试锁定，不能把简单增加版本号视作解决重入。
- 先补齐 Laravel 每任务 `onOneServer()` 与 `withoutOverlapping` 两种独立策略：前者控制同次触发资格、后者控制前次业务未结束的后续运行。与未启用时上游默认分别验收；去重身份含计划/任务/参数与调度版本的明确规则。当前全局 slot 唯一模型须适配策略，而非给所有任务强制同一语义。队列入队完成不等于业务完成，不在该时点释放执行互斥。
- 使用同一 Scheduler、DB 时钟与 slot/job 事务、公开共享锁驱动；缺少所选策略必需的共享存储就报错。租约过期只能允许接管，旧节点的外部副作用仍须幂等/fencing；不宣称永久无重叠。多 scheduler 同槽仅按已配置策略派发，所有权/重试/崩溃边界不由通知可靠性决定。

## 迁移与恢复边界

| 状态 | 权威与迁移责任 |
| --- | --- |
| 管理员配置/审计、动态计划/版本/水位、DB pending/running/failed jobs | 应用构建、已注册代码/schema、必要部署凭据及数据库备份/迁移即可恢复；Redis 缓存可不迁。使用一致备份和应用迁移，数据库外部副作用仍需核对 |
| Redis 通知/短缓存 | 丢失后用 DB 全快照和轮询补偿；迁移后从空 Redis 重建，不把消息历史作为恢复前提 |
| 显式 Redis queue | Redis 自身保存 jobs/lease/failed 等；必须单列持久化/备份/复制/搬迁和恢复目标，仅应用+DB不能恢复这些任务 |
| 进程在途与外部系统 | 新节点恢复 DB lease/slot，不恢复旧进程内存/未提交事务；旧节点须先排空或隔离，数据库迁移不能撤销外部邮件/HTTP等副作用 |

滚动升级先提供向前/后兼容的 schema 和任务版本，再迁移/混跑/退出旧节点；不兼容构建不得悄悄忽略新配置或执行未知 payload。恢复必须验证目标权威命名空间，不能因为版本数字下降就接受旧备份覆盖现役集群；完整数据库备份恢复在受控停旧/切换流程中重建节点快照。Redis 锁与缓存 epoch 不得让 DB 动态计划依赖已丢失的内存标志。

## 依赖排期与交付

| 功能簇 | 依赖与最小交付 | 状态 |
| --- | --- | --- |
| R2 配置运行时契约 | 先做 Laravel 简约 API/配置格式与安全白名单 Spec/ADR、消费者读取及生命周期；持久实现依赖 R4 DB/事务、R7权限审计；不阻塞当前 HTTP | 仅规划 |
| R2 配置持久实现（V1必交） | typed分组DB业务设置/secret加密、权限保护管理API/CLI、候选/CAS/审计、完整发布与driver受控激活、通知+轮询、期限/故障；复用R4/R7、业务driver及适用R8，DB单独可运行 | 未实现 |
| R10 队列/调度 | R2契约与持久基础、R4–R9适用依赖后，先事务与每任务策略，再动态目录/计划/并发/状态；同一内核 | 未实现 |
| R13 开发体验 | CLI/生成器/README/config/测试随各功能簇交付，R13补齐，不把管理能力全推给V2 | 未实现 |
| R14 故障/迁移门禁 | 双库真实多API/worker/scheduler、Redis通知中断、滚动升级/恢复和安全边界 | 未验收 |
| R15 V2 | R14底座完成后管理表单消费同一V1管理API，管理员编辑邮件/SMS/支付/存储设置并区分保存/应用状态；PC仍只规划 | 未启动 |

每簇同批更新唯一 `modules/example` 业务调用、模块私有迁移/种子、根通用装配、生成应用、README/config 及公开测试；不把业务表放框架核心。不添加平行 ConfigManager、第二总线、控制面、DSL、自动发现集群或独立后台服务。最少必要配置、明确默认、少量公开操作；内部一致性元数据不强迫普通开发者手工传递。

## 必交验收矩阵

以下都是未来门禁，本轮未执行，不能凭文档关闭 todo。

| 场景 | 证据要求 |
| --- | --- |
| Laravel 体验与真实调用 | 普通应用 `.env`/config/类型读取、注册 Job、connection/queue/schedule 最短流程；公开消费者、modules/example 与真实生成应用 check/test；逐项固定上游对应 |
| 配置原子与安全 | 一批多键含跨字段约束，失败不发布/不递增；无权/未知/无效/CAS竞争/敏感审计；操作内不混版本；长期消费者实际读到新值 |
| 版本传播与故障 | PostgreSQL18、MySQL8.4分别多独立API/worker/scheduler，在规定时限内一致；真实Redis通知断开/丢弃/重启/乱序，DB轮询补偿；DB中断/超时/陈旧期限显错，不本地降级；区分DB保存与各节点激活，构建失败/pending_restart不假报 |
| 管理员secret/资源 | 不读.env/Rust的管理API；secret写/读权限、加密/轮换/丢key、脱敏不回写、SSRF拒绝；真实支持的邮件/SMS/存储driver新操作激活、在途旧实例、已排队重试目标稳定；支付仅现有能力/driver适用验收，无driver保留缺口 |
| 事务与任务 | DB业务提交/回滚/嵌套after_commit/回调失败和崩溃窗口；需要可靠场景验证outbox；多worker领取/降并发/暂停/重试/强杀/旧token拒绝 |
| 动态计划竞争 | 未注册/参数schema/权限/允许策略拒绝；多scheduler同槽与未启用策略的上游默认；编辑/禁用/删除与触发事务竞争、在途不误杀、版本/DST/补跑与运行关联 |
| 重启与迁移 | 管理变更/计划/未完成DBjobs在API、worker、scheduler重启及滚动迁移后保留；只迁app+DB、空Redis缓存可恢复；显式Redis queue另验自身备份恢复、不假报DB可代替 |
| 资源与退出 | bounded扫描/快照/通知/重试/日志，取消/drain/join、节点隔离、schema不兼容和未知版本失败；独立审查及适用真实服务门禁 |

## 简约性自查

保留现有仓储、Provider、队列、调度、权限审计和后台资源所有权；不引入新平台或新依赖。采用Laravel config加Spatie型typed分组Settings理念，V1必交DB管理底层，宿主按需启用，V2表单复用同一API。本轮只决定行为和边界，候选API/表字段/轮询默认数值须在相应实现Spec中以真实用例收敛。内部 revision/CAS/快照是满足用户持久性与并发要求的必要机制，公开日常配置体验保持 Laravel 风格，不让管理员或业务开发者拼装协调协议。
