# 短信渠道与多服务商交付计划

> 受众：AI Agent；状态：方案待实施，不代表已支持或已真实投递。属于既有 Notification/SMS 范围的细化，不抢占当前功能簇。

## 定位与依赖

按 Laravel Notification 的渠道、收件人路由、按需通知、同步/排队和公开自定义渠道机制实现；短信传输是通知渠道可复用的底层服务，不另建通知平台。固定 Laravel 来源沿[官方覆盖计划](laravel-official-coverage.md)，官方 Vonage 渠道须对应，阿里云/腾讯云/Twilio/AWS 为按需启用的通用增强，不冒称 Laravel 核心内置。

依赖 R7 身份安全与权限、R8 缓存/驱动、R9 Notification/模板/I18n、R10 队列/事务；数据库动态设置集成依赖 [ADR-080 对应计划](runtime-configuration-scheduling.md)。SMS 基础直发不等待整个动态设置系统；提供静态配置后，按前置成熟程度补受控激活。唯一领取队列仍为 [todo](../todo.md)，本文件不创建第二队列。

## 现状与复用

- `src/sms.rs` 已有 `SmsDriver`、`SmsSender`、文本/供应商模板消息、Twilio HTTP adapter、log/fake；`src/config/sms.rs` 当前字段主要绑定 Twilio，缺少具名多账号配置与渠道管理。
- `src/notifications.rs` 已有邮件/SMS 的数据库任务、固定 payload 与至少一次执行语义；复用生产 Provider、Container、配置仓储、共享 HTTP client、队列、I18n、事件和审计机制，不新造配置中心、验证码系统、模板引擎或重试器。
- 真实 Twilio 投递缺授权账号和目标；现有 mock、fake 与协议测试不算服务商受理/手机送达。保留现有证据，按新契约重新核对生产调用链。

## 来源与采纳

| 来源 | 已核事实 | 采纳与调整 |
| --- | --- | --- |
| 固定 Laravel docs `notifications.md` 的 SMS Notifications、Custom Channels | 短信以 Vonage 通知渠道和自定义渠道扩展；收件人与通知分离 | 公开体验以此为主，Rust 类型/async 适配先 Spec；不以泛化 SmsSender 替代 Notification 全行为 |
| OpenSpring `b8fce39945` 的 `openspring-core-sms`、`docs/zh-CN/sms.md` | 统一入口、具名供应商/多账号、sms4j 可选适配、配置快照、先构建后替换、Notification 通道 | 采纳职责与生命周期；不引入 JVM/sms4j、不复制全局静态工厂；显式指定连接不暗中切换，不默认复制遇异常即回退 |
| [EasySMS](https://github.com/overtrue/easy-sms)、[Laravel EasySMS](https://github.com/overtrue/laravel-easy-sms) | PHP 多渠道网关与 Laravel Provider 集成 | 参考具名连接、模板和第三方扩展体验；不是 Rust 依赖，也不要求照抄全部服务商 |
| [smskit](https://github.com/ciresnave/smskit) | Rust 多供应商统一接口、Twilio/Plivo/AWS、回调与 fallback | 候选参考，不把项目自述当成熟性证明；不整体引入其配置系统/路由层；仅在独立审计合格且确实降低维护成本时复用 adapter |
| [AWS 官方 Rust SNS SDK](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/rust_sns_code_examples.html) | 官方 Rust SDK 支持 SNS Publish；SNS 可向号码发 SMS | AWS 驱动优先评估官方 SDK，依赖可选 |
| [Twilio 官方 Rust 指引](https://www.twilio.com/docs/openapi/generating-a-rust-client-for-twilios-api) | 无官方 Rust SDK，可依据 OpenAPI 生成 | 对比现有薄 adapter、受控生成子集与社区 SDK，不为“用了 SDK”强行替换可靠小实现 |
| [腾讯社区候选](https://docs.rs/tencentcloud-sms-sdk)、[阿里云社区候选](https://docs.rs/alibaba-cloud-sdk-rust) | 搜索已发现候选；未做源码/维护/实发审计 | 不视作官方或已选型；核许可、维护、async/MSRV、签名、脱敏、端点、超时/重试和依赖体积后决定 |

实施时记录所选 SDK 的精确版本/commit、许可证与官方 API 版本至 stack/交付 Spec。阿里云、腾讯云官方 SDK 目录未列 Rust，不因社区包名称误标官方。候选失败可使用官方协议加现有 HTTP/签名基础组件的薄适配；不自研密码算法，不通过 PHP/Java/Go sidecar 增加默认部署负担。

## 对外契约建议

1. Notification 为主要业务入口：通知声明渠道、收件人路由、短信内容和排队策略；允许不创建用户模型的按需通知。低层 SMS 仍可直接发送，返回供应商受理回执。拟采用 `to_sms`、`connection(name)` 等名称仅作 Spec 候选，不冒称 Laravel 官方 API。
2. 沿现有 `SmsSender` 演进具名连接管理，区分连接名称（如 `transactional`）与驱动类型（如 `tencent`），同服务商可多账号。公开 driver/factory 由 Provider 注册，无需修改框架核心或维护穷尽枚举。复用框架既有 Manager 扩展惯例，不添加平行 Sms/SmsManager 内核。
3. 最小配置为 `SMS_CONNECTION` 选默认具名连接，`SMS_DRIVER` 定义该连接供应商；领域配置保存具名连接与供应商 typed 字段，凭据用 `TENCENT_SMS_*`、`ALIYUN_SMS_*` 等扁平 env。最终映射在 Spec 一次收敛，删除旧 Twilio 专属通用字段，不加无用户的兼容层。开发 log、测试 fake；生产显式选真驱动，禁止静默 log/fake。
4. 文本、本地翻译模板、供应商审核模板明确区分。厂商模板支持具名参数与有序参数的类型差异，不能把 BTreeMap 排序当腾讯模板参数顺序；模板/签名可按连接和 locale 选择，服务商不支持自由文本时明确拒绝，不暗改为其他模式。
5. 号码以 E.164 为传输边界；无国家码不擅自补 `+86`。可在已有验证/号码值类型处接成熟规范化库，地区由调用方明确提供，不在 SMS 新造地区解析器。语法合法不等于号码真实、可投递或已同意接收。
6. 官方 Vonage 渠道先核对固定官方包的消息格式、Unicode、sender、client reference、收件人路由等完整适用行为。通用模板消息不能替代官方专属类型，SDK 特性扩展只在对应渠道暴露。
7. 事件/Listener 复用既有 Notification 发送前、受理后和失败扩展；回调解析/验签由驱动提供公开方法，路由和权限由宿主定义。框架不默认开放管理、发送或 webhook 路由，不附带业务后台、短信登录或营销平台。

## 生命周期、持久化与错误

- 每次发送捕获同一配置 revision 的完整快照；管理员更新前先完整校验并构建候选客户端，成功后原子发布，新发送使用新实例，在途操作继续持有旧 Arc。失败保留旧实例并返回未激活状态；启停开关控制新准入，删除凭据不复活旧连接。
- 基础设施/feature 仍需部署；支持动态激活的业务连接字段复用统一 DB 设置，权限、secret 加密/脱敏、CAS、审计与多节点补偿均沿统一机制。节点尚未应用必须显式可见，不能将数据库保存成功等同所有节点生效。
- 排队时固定接收人、locale、已准备文本或供应商模板参数与显式连接选择，不写明文凭据。现行动态配置按每个 attempt 固定有效资源代次；本计划不升级为跨 attempt 永久绑定历史 revision 或保留旧 secret。SMS-3 先按 ADR-080/统一机制收敛：重试时连接目标、账号/模板变化、吊销及已可能受理任务如何处理，如何避免默认连接变更导致意外改投。只有经独立 Spec 确认确有必要时才增加引用历史代次的能力与保留/清理规则，不另建 secret 仓库，也不把本条当成历史凭据已可恢复。
- 业务事务内入队按统一 after_commit/回滚语义；不在业务持锁事务中调用短信供应商。数据库默认队列持久化；显式 Redis 队列沿现持久化责任，不承诺只迁移 DB 即保留 Redis 任务。
- 区分 `not_submitted`、`rejected`、`rate_limited`、`accepted`、`submission_unknown`；受理不等于送达。超时、断连、取消发生在可能提交之后，按 unknown 处理，不能简单标成“未发送”。错误保留安全的供应商code/request id，限制正文尺寸，不能透传含凭据/号码/正文的原始响应。
- SDK 自带 retry 与 worker retry 只能有一层明确策略，默认关闭隐藏自动重发。仅可证明未受理且可重试时重试/可选主备；鉴权、参数、停用、模板错误不盲重试，超时未知不默认切换供应商。显式指定连接时失败不暗换。
- 现有队列至少一次仍可能在受理后、确认前崩溃导致重复。优先复用供应商正式幂等键（若有）；本地去重/业务键不能冒称远程 exactly-once。保留人工核查与业务显式重试方式。
- 按需提供回执查询/回调能力；验签原始正文、重放与重复/乱序状态处理、消息ID与连接/租户归属需验证。不支持的能力明确拒绝，未知状态不伪装 delivered。不另造完整发送日志平台，必要 attempt/status 复用 Notification/queue 持久层。

## 实施顺序与交付

| 功能簇 | 前置与交付 | 退出条件 |
| --- | --- | --- |
| SMS-0 来源与 SDK 决策 | 核固定 Vonage 官方包、现公开调用、OpenSpring/EasySMS；审候选源码/锁/许可，明确默认、错误、schema 与 Rust 差异 | 确定 SDK/薄 adapter 理由、API草案、影响清单与验收，不加未经审计依赖 |
| SMS-1 渠道与连接契约 | 沿 R9 Notification 与现 SmsDriver 实现具名连接/公开factory，静态配置、文本/模板、log/fake、扩展接线 | 模块/生成项目用公开API；外部自定义driver不改src；默认及错误边界通过 |
| SMS-2 官方渠道与服务商 | Vonage 官方对应；保留并重验 Twilio；阿里云与腾讯云首批增强，AWS SNS后继独立驱动 | 每个驱动单簇真实受理/回执等适用能力验证，未启用无专属依赖，不因某账号缺失阻断独立驱动 |
| SMS-3 排队与安全重试 | R10事务派发/attempt资源快照、重试配置策略Spec、取消/崩溃/unknown处理、禁止不安全回退 | 先收敛跨attempt连接/账号变更与吊销边界，再验MySQL重启/并发及受理后故障；不虚报exactly-once |
| SMS-4 动态设置与回执 | 统一DB设置成熟后接入安全激活、多节点、停用/撤销与已定资源代次策略；供应商回执按能力提供 | 真实多节点配置一致、失败保旧、回调验签/归属/重复乱序及宿主公开挂载 |
| SMS-5 收口 | 文档、env、唯一模块、生成、依赖树、必要真实服务和发布全后端矩阵 | 逐服务商列已实现/已实发/未获外部条件，更新矩阵及本地提交，不推送/发布 |

官方渠道与国内扩展按既定两阶段归属；R9基础可直接承接，不将阶段一Notification内核推迟到增强之后。SMS-2各驱动可在共享契约冻结后由总控安排并行，manifest/lock、共享配置、迁移与main集成由总控串行。不要为方案立即启动所有供应商开发。

## 验收矩阵与外部条件

| 范围 | 高价值证据 |
| --- | --- |
| 公开行为 | 默认/指定连接、多账号、未注册/未编译驱动、缺配置/停用、文本/模板/Unicode、参数顺序、公开第三方driver、Notification与直接调用一致 |
| 协议与错误 | 官方签名样例、真实请求/错误码映射、边界长度、受理与送达区分、超时未知、SDK与worker不叠加重试；本地协议模拟仅证明构造/映射 |
| 真服务 | 每服务商授权测试号码、签名/模板、最小单次发送；区分供应商受理、手机实收、查询/回调三个证据；未经许可不购买或批量实发 |
| 动态与集群 | 真实MySQL共享设置/队列、两节点revision传播、丢通知补偿、更新失败保旧、在途资源快照稳定、跨attempt变更符合SMS-3确定的策略、重启与迁移后恢复 |
| 事件与回执 | Listener扩展失败边界、回调签名/重放/归属/重复乱序、不支持回执的明确结果、隐私脱敏 |
| 工程收口 | 唯一modules/example和生成项目实际调用、按feature不引入未用SDK；分簇集中门禁，日常MySQL、后端风险定向、发布全后端 |

测试按行为族组织，不每个driver复制完整manager/queue矩阵；共享契约测一次，各driver只补协议独有差异与真实服务。无凭据/许可/可用模板时保留具名待验，不以fake、编译或另一个服务商实发替代。

## 不在本计划默认范围

不引入短信群发运营后台、号码采购、SIM网关、价格竞价/自动成本优化或新的验证码登录产品。不把OpenSpring的23家映射当作Rust已实现清单；其余供应商通过公开扩展按实际需求增加。只有出现具体需求再扩展路由策略或原生批量接口，不堆规则引擎和适配器空壳。
