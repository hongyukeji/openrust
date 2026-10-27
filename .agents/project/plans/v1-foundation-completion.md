# V1 通用底层能力补齐

> 受众：AI Agent
> 更新：2026-10-27
> 状态：F1 已完成双库专项验收；F2 已完成适用验收；F3 模板、生成路径及 M4 队列通知组合已完成代码和适用验收；F4–F10 仍按依赖顺序推进。

## 决策与版本边界

- V1 综合参考顺序为 **Loco、Laravel、Goravel、OpenSpring**；若依、芋道仅在 V2 开发时参考。各参考按整体框架能力评估，不把某一框架限定为唯一领域来源。OpenRouter 对标后续 AI模型平台的产品能力，该项目不进入本文件的 V1 必交范围。
- 下列 F1–F10 是 V1 必交范围；“可选”表示应用可以不启用，不表示开发时可以省略。复用成熟 Rust 组件、现有 Plugin/配置/错误与生命周期，不自建第二套 ORM、容器或队列。
- 先闭合 [todo.md](../todo.md) 当前唯一进行中功能簇，再按依赖逐簇推进。每簇先定公共 contract、选型和验收，再实现、验证和提交；本文件不创建另一份进行中任务。
- 组织/部门/套餐、租户管理页面和导入导出业务页面归 V2；租户隔离、查询授权、身份接入和数据交换的通用机制归 V1。
- 本轮只调整文档与交付范围，不安装依赖、不改变现有运行默认值；当前能力仍以代码和 `core/` 为准。依赖与具体 API 在实施 Spec 确定后更新 `stack.md` 及相关 ADR。

## 交付顺序

| 编号 | 交付 | 阶段与依赖 |
| --- | --- | --- |
| F1 | cron、IANA 时区与有界补跑 | M4；复用数据库时钟、slot 去重和任务派发，接在 worker 正确性修正后 |
| F2 | 国际化复数、选择与地区格式化 | M4；在既有 i18n 上扩展，先于邮件模板与通知组合 |
| F3 | 安全邮件模板与生成体验 | M4；复用 F2、Mailer/lettre，再接队列通知 |
| F4 | 缓存 tag、SWR 与同键加载合并 | M5；基础 Cache/真实 Redis、有界运行及共享锁之后 |
| F5 | 通用查询授权规则 | M5；复用 Policy、SeaORM 与已有查询组合任务，先于租户/数据交换 |
| F6 | 可选租户作用域及隔离 | M5；作用域/查询规则、命名缓存、队列与存储之后；RPC/实时接入时补组合验收 |
| F7 | 外部 OAuth2/OIDC/SSO 接入与 TOTP | M5；复用 AuthFlows、加密、审计、共享限流及出站目标策略，依次交付身份接入、MFA、SSO 登录与统一退出三个子簇 |
| F8 | CSV/XLSX 通用导入导出 | M5；F5/F6、Storage、Queue 和验证齐备后，分解析转换与任务化集成两簇 |
| F9 | 应用级生成模板覆盖 | M6；与剩余生成器一起完成，先于最终黄金路径验收 |
| F10 | 显式轻量测试数据工厂/fixtures | M6；复用 TestDatabase、实体与迁移，同步 TestApp/生成器验收 |

已有 gRPC、实时通信、出站韧性、生产迁移互斥等任务继续按原计划交付；本轮不复制这些能力。F6/F7 的应用层机制不得变成普通无认证或无数据库应用的必需依赖。

## F1 日历调度

- 保留 interval/UTC daily；新增 `cron-parser` 五字段表达式及 `chrono-tz` IANA 时区，不支持秒字段、自然语言或任意 shell。默认 UTC；DST gap 跳过、重复 wall minute 去重；默认跳过错过触发，catch-up 每 tick 上限 1–100。
- 仍使用共享数据库 UTC 时钟和同事务派发；表达式/时区摘要隔离计划版本，首次启用不回填历史；补跑跳过已经持久化的槽，避免 DST fold 后旧槽阻塞水位。细节以 [ADR-037](../decisions/adr-037-queue-worker-lifecycle.md) 与 [调度事实](../core/schedule.md) 为准。
- 验收：跨日/闰年、DST 重复与缺失、长时间停机、计划变更、双节点竞争和崩溃恢复；PostgreSQL/MySQL 均验证去重与有界补跑，不承诺外部副作用恰好一次。

## F2 国际化表达与格式化

- 保留普通消息及 `{name}` 兼容入口，在同一 catalog/locale 体系增加计数复数、有限选择和数字/日期地区格式化；已选择 ICU4X `icu_plurals`、`icu_decimal`、`icu_datetime` 与 `icu_locale` 2.3.x 稳定组件及编译数据，不自创表达式语言或执行模板代码。
- JSON structured message 仅允许 `plural` 类别映射或 `select` 有限字符串映射，二者必须有 `other`；参数名、类别、分支数量、模板长度在装载时校验。缺参/无效 plural 输入按 `other`，普通模板缺参仍保留原占位符。`format_number` 用精确十进制字符串；日期时区由调用方明确转换，不修改 API 机器字段、存储时间或稳定错误码。
- 已验收：中/英/波兰语复数与选择、零/小数/无效值/缺参回退、精确数字、显式时区跨日格式、并发请求 locale；workspace default/all-features tests、clippy、warnings-denied docs、deny、MSRV 和干净源码副本生成应用 check/test 通过。gRPC 组合在 G2 接入后补验；F2 无数据库行为，不要求新增双库集成测试。

## F3 邮件模板

- 使用可选 MiniJinja 2.24 渲染编译期嵌入的可信应用资源；支持布局继承，输出主题、纯文本和可选 HTML，交给现有 `lettre::Message` 与 Mailer。HTML 默认转义，缺必需变量明确失败；不开放任意文件或网络读取，渲染燃料和输出大小有界。
- 主题复用 F2 `I18n`，正文按 locale 精确/语言/配置 fallback 选择。既有 HTML/multipart/附件能力直接复用，不新增发送、附件或重试系统。
- 生成器产出模板、示例和测试；队列消息明确采用固定渲染结果或固定模板版本，重试不因模板变化悄然改变消息，持久载荷与日志遵守敏感信息边界。
- 已验收：HTML 注入、路径拒绝、缺参、语言回退、布局继承、MIME alternative、生成器资源/feature 注册；Mailpit v1.32.0 收到实际 SMTP 消息，API 确认 Subject、Text、HTML。生成项目 clean-source check/test 与全工作区门禁在提交前执行。
- 队列通知组合已在 PostgreSQL 18/MySQL 8.4 隔离库验收事务、活动唯一键、worker 服务解析和重试重复投递边界；Mailpit 验收实际队列 SMTP 发送。Twilio 外部实发仍需授权目标与凭据。

## F4 高级缓存

- 基础 `get_or_insert_with` 继续仅承诺便利加载；另提供显式同键加载合并，注明单进程范围，限制在途 key、等待者与 loader 时间。key 包含 store/namespace、类型与加载契约，取消和失败不会遗留等待者。
- tag 按 store/namespace 隔离，以版本代际等真实原子机制失效，不扫描全部缓存。防止旧 loader 在失效后回填当前代以及元数据过期造成旧值复活。Redis 必须验证跨节点，memory 仅进程内；file 等不具备能力时明确不支持。
- SWR 明确 fresh/stale/最终过期窗口；后台刷新有任务所有者、并发上限、超时及停机回收，不能无限延长陈旧值。tag 失效优先于陈旧命中。跨节点刷新协调复用共享锁，租约可能失效，不保证 loader 恰好一次。
- 这些能力用于可重建缓存，不作为权限、会话撤销或其他权威状态的替代；缓存故障仍报错，不静默回退。
- 验收：真实 Redis 双节点失效/回填竞争，时间边界、失败/取消、慢 loader、容量与停机回收；单进程与跨节点保证分别记录，不能仅靠本机冻结时钟验证共享时钟。

## F5 查询授权规则

- 在既有 Policy/SeaORM 接口上提供公开、类型化规则注册与组合，把可信身份转换成受约束查询条件；与用户过滤按 AND 合并，调用方不能覆盖权限条件。
- 覆盖列表、详情、更新、删除、批量、关联与导出；明确规则失败默认拒绝，以及原生 SQL/自定义驱动的显式逃生边界。不做任意字符串 SQL 改写，不内置部门树或复制 ORM。
- 验收：双库越权 ID、过滤扩大范围、批量混入无权记录、关联泄漏、规则错误与绕过边界；独立 consumer 只用公开入口增加规则。原有显式查询仍有效，新的受保护入口与普通入口必须可辨认。

## F6 可选租户隔离

- 默认关闭；启用后从可信身份或明确受信的应用解析器确定租户，不能直接信任请求传来的 tenant ID。复用调用作用域，区分身份、租户与授权；声明受保护的资源缺少租户时拒绝操作。
- 给已登记的数据表/查询、缓存与锁 key、持久任务、私有文件和审计提供统一接入。租户信息经过显式、受校验的载荷传播；任务执行时重新验证适用授权，不保存原始请求凭据。
- 明确关联表、批量/原始 SQL、自定义驱动、公共资源及管理操作边界，不宣称拦截所有连接；跨租户访问须显式授权和审计，嵌套作用域不得无授权切换。单库行级隔离为首个完整路径，不含分库分表。
- 验收：两个租户的并发读写、关联/批量、缓存、文件、任务重试/恢复和审计不串用；两 API 节点共享后端；gRPC/实时启用后补同类隔离。关闭时普通应用无额外业务模型或配置负担。

## F7 外部身份与 TOTP

- 保持 JWT-first；以公开 Provider 契约接入外部 OAuth2/OIDC 身份及 SSO 登录。采用成熟协议组件，授权码/PKCE、state 与回调地址校验；OIDC 验证 nonce、issuer、audience、有效期及签名/JWKS。出站 discovery/JWKS/UserInfo 复用目标策略和总预算。
- 外部身份以 provider/issuer 与 subject 绑定本地主体，不能仅凭同邮箱自动合并；绑定/解绑需重新认证并审计。外部身份验证成功后复用现有 AuthFlows 与凭据签发，不建立第二套本地认证系统。
- 可选 TOTP 与恢复码使用现有加密/限流/审计；启用前验证，密钥受本地轮换保护，恢复码仅存哈希并原子单次消费。明确挑战有效期、时间窗、重放防护、失败限流和重新认证边界；MFA 完成前不能签发具有完整权限的凭据。
- 验收：可控 OIDC 服务的真实协议流程、伪造/重放/issuer 混淆/PKCE/JWKS 轮换、绑定冲突，以及 TOTP 重放、恢复码竞争和双库共享状态；至少一个真实兼容 IdP 集成。第三方账号/短信实发仍单独授权，不用 fake 冒充外部送达。
- V1 交付外部身份客户端接入、TOTP 及下述 SSO 闭环。独立 OAuth 授权服务器、Passkeys/WebAuthn、SAML 另列后续评估，不能混称本簇已支持。

### SSO 登录与统一退出

本节是 F7 SSO 补齐范围与验收的唯一维护入口；F7 双应用 SSO/退出专项已完成，依赖的 OIDC 与 TOTP/恢复码子簇均已交付。复用 ADR-053 的身份客户端与现有 AuthFlows/共享存储，不重做已交付的 OIDC 登录；实现与状态模型以 [ADR-055](../decisions/adr-055-openid-connect-sso.md) 为准。

- **双应用单点登录**：两个独立应用以不同 client ID、回调地址和本地凭据接入同一 IdP。浏览器在 A 登录后，B 在 IdP 会话有效且策略允许时复用该会话；本地身份映射与授权各自独立，不共享应用 JWT、Cookie 或签名密钥冒充 SSO。明确重新认证、授权同意、IdP 会话过期及本地 MFA 的边界；外部认证成功不能绕过本地 MFA，未经显式信任约定的 `amr/acr` 不能直接替代本地验证。
- **统一退出**：应用保留本地退出，并提供显式的身份中心退出入口。V1 实现 [RP-Initiated Logout](https://openid.net/specs/openid-connect-rpinitiated-1_0.html) 与 [Back-Channel Logout](https://openid.net/specs/openid-connect-backchannel-1_0.html)，以身份中心通知各应用撤销关联登录状态；前通道退出按实际兼容需求另行评估，不作为本簇默认必交项。先完成本地退出，再请求 IdP 退出，外部失败不得恢复本地登录。提供 IdP 能力检查与接入说明；不支持通知或配置不完整时，明确只能保证本地退出，不能报告所有应用已退出。
- **协议安全**：服务端 discovery/JWKS 等请求复用受限出站策略；浏览器退出跳转仅使用已验证的 IdP 退出端点，校验协议与允许来源，不接受用户任意 URL，也不承诺控制浏览器的 DNS/代理。应用发起退出要防 CSRF，退出后的回调地址采用注册白名单并校验一次性 state。后通道不依赖浏览器 Cookie，须独立按标准验证 Logout Token 的签名算法、issuer、audience、时间、events、jti 及 sid/sub 约束并禁止 nonce，拒绝跨用途令牌；以共享状态处理重放与幂等。协议回调使用标准 HTTP 状态和响应，不让应用的 AlwaysOk/业务包装掩盖失败；敏感令牌不进入日志。
- **精准撤销与竞态**：持久关联 provider/issuer/client、外部 sid 或 sub 与本地 refresh family、可选 Session、待完成登录/MFA challenge。区分单次 IdP 会话退出与该主体在对应客户端的全部关联会话退出；不误撤其他 provider、其他主体、无关设备或 PAT。复用共享事务或撤销代际，验证 logout 与回调、MFA 完成、refresh 轮换竞争，防止旧登录流程在退出后重新签发有效凭据。
- **集群与生效时限**：撤销通知命中任一副本后，所有副本按声明时限执行同一结果；数据库/Redis 共享后端是权威状态，不依赖黏性会话、进程内名单或仅 Pub/Sub。保持既有 JWT 默认本地验签，退出不会自动使已签发 access token 即时失效；实施 Spec 必须给出其最大剩余有效窗口。需要即时撤销时提供显式共享状态校验接入方式并验证故障时拒绝访问的语义，不能把 refresh 撤销写成 access token 即时撤销。通知失败、重试/重复、存储故障、滚动重启及未送达结果须可观测且有界，不承诺第三方 IdP 无条件可靠送达。
- **真实验收**：在 Keycloak 注册两个 client，使用同一浏览器会话验证 A 登录后 B 不重复输入密码，并验证过期/重新认证及本地 MFA。两个应用各至少两个 API 副本，使用各自应用状态与凭据，经负载均衡分别在 PostgreSQL 18/MySQL 8.4 完成回调、应用发起退出、IdP 发起退出、跨副本 refresh/Session 撤销和 JWT 时限验证；覆盖伪造/重复通知、乱序与旧 challenge 竞态、通知或共享存储故障及重启。Redis Session 启用路径另以真实 Redis 验证。记录拓扑、版本、命令、声明时限和实测结果；单 client 授权码测试、两个副本或 fake 均不能替代双应用 SSO 验收。
- **交付边界**：示例复用根目录集成宿主与根集成测试，补配置、中文使用说明及适用生成模板，保持未启用外部身份的应用无需 IdP。V1 提供可接入现有身份中心的客户端机制；不新增自研身份中心，也不将本节规划标记为现有能力。M5 完成本簇专项，M6 按完整集群门禁复验。

## F8 通用数据交换

- 先支持 CSV 与 XLSX：列映射、类型转换、统一验证、逐行错误和有界分块；解析与业务保存分离，允许公开自定义转换器。复用查询授权、租户隔离、Queue、Storage 和私有下载，不新建任务平台。
- 同步小文件与异步大文件采用同一转换契约；明确整批/分批事务、部分成功、取消、幂等及错误报告留存。限制文件、展开后数据、行/列/单元格与缓冲规模。
- 不执行公式/宏；导出文本明确防公式注入策略；不自动支持旧 XLS、报表设计器或业务表格 UI。
- 验收：编码/日期/空值/类型、重复表头、超大与压缩文件、公式注入、取消和资源回收；双库分批写入与失败报告，租户/权限过滤、导出文件归属及过期；生成最小独立扩展示例。

## F9 应用模板覆盖

- 提供模板清单、导出、预览与应用级覆盖，优先级为应用覆盖→内置模板；公开变量 schema 与模板版本，局部覆盖允许回退，错误覆盖不能悄然退回内置模板。
- 模板读取/输出限制在明确资源根与生成目标内，处理相对路径和符号链接逃逸；默认不覆盖已有业务文件。缺变量、语法和版本不兼容给出位置与修复建议，升级时可审阅差异。
- 使用成熟模板工具，按内容正确转义 Rust/TOML/文本，不把 HTML 转义规则直接用于代码生成。无需修改 CLI 源码即可自定义生成物。
- 验收：默认/局部覆盖、错误诊断、路径逃逸、冲突与升级；无库及 PostgreSQL/MySQL 生成项目 check/test，快照来自真实产物。

## F10 轻量数据工厂

- 生成普通 Rust builder/fixture，支持默认属性、覆盖、批量上限、显式关系组合和可选固定随机种子。构造与保存分开，复用已有实体、迁移和 TestDatabase；不提供隐式 ORM Factory/Seeder trait 或第二套 fixture 数据库。
- 固定种子只承诺锁定依赖版本内可复现；测试数据不能自动调用真实通知供应商或污染共享开发数据。seed 仍由应用普通函数决定幂等和业务内容。
- 验收：可复现数据、关系约束、失败正常返回、用例隔离、双库插入及清理；生成项目展示如何替换属性和组合关联，而无需理解框架内部实现。

## 后续适配登记

以下按已确认建议登记为后续适配/评估，不加入当前 V1 必交清单，也不据底层库支持宣称已实现；进入开发时再登记明确版本、需求与真实验收。

| 方向 | 边界与前置 |
| --- | --- |
| 更多数据库 | SQLite、MariaDB、SQL Server 等逐项评估；正式支持需驱动、迁移、认证、队列和生成项目的真实矩阵 |
| 外部消息中间件 | RabbitMQ/Kafka 等公开适配；区分任务队列、发布订阅、重放，确认 ack/重试/顺序/事务边界，不以 driver 数量证明可靠性 |
| 搜索 | 可选索引驱动、同步/重建和权限过滤；复用数据与队列，不自研搜索引擎 |
| 运行时功能开关 | 与 Cargo feature 区分；显式作用域、持久化和命中稳定性，不建设实验运营平台 |
| SSR 与静态/SPA Starter | 可选模板、静态资源、fallback/缓存头及表单安全；保持 API 默认路径，复用现有 HTTP/CSRF |
| 更多身份协议 | Passkeys/WebAuthn、独立 OAuth 授权服务器及 SAML 分别评估；不把外部 OIDC 登录宣传成授权服务器 |

GraphQL、任意 DAG、服务治理控制面、分库分表/跨库事务等原有边界不因本轮改变。V2 参考若依/芋道后复用 V1 通用机制，组织/套餐/UI 模型仍由后台套件决定。

## 参考与完成定义

- Loco：[模板覆盖](https://loco.rs/docs/how-to/override-templates/)、[调度](https://loco.rs/docs/how-to/schedule-jobs/)、[显式租户辅助](https://loco.rs/docs/how-to/multi-tenancy/)。
- Laravel：[缓存](https://laravel.com/framework/docs/cache/config)、[邮件](https://laravel.com/framework/docs/mail)、[工厂](https://laravel.com/framework/docs/eloquent-factories)、[Socialite](https://laravel.com/framework/docs/socialite)、[Fortify](https://laravel.com/framework/docs/fortify)。官方可选包不等于内核默认启用。
- Goravel：[国际化](https://www.goravel.dev/digging-deeper/localization.html)、[工厂](https://www.goravel.dev/orm/factories.html)、[调度](https://www.goravel.dev/digging-deeper/task-scheduling.html)。
- OpenSpring 只读参考基线 `bfe09b0e7b3d07572ddd06d44726ca445e61bb57`：`openspring-core-tenant/TenantAutoConfiguration`、`openspring-core-scheduler/SchedulerManager`、`openspring-starter-iam/OidcOAuthProvider`、`openspring-starter-task/AsyncTaskExportOperations`；核实实际源码与许可证后再适配，不把接口存在当作完整安全或性能保证。
- F1–F10 均须有公开可用入口、默认/可选配置、成功/失败与边界测试、中文文档及示例；数据路径双库，缓存/文件/跨节点路径使用真实服务。有效 feature 组合和干净源码应用生成纳入最终验收。
- 文档规划只运行链接、旧引用和 diff 检查；实际代码功能簇按仓库门禁执行，不把本轮文档提交计作任何能力已实现。
