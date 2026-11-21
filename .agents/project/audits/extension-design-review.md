# 全量扩展方案复用与 Laravel 惯例审计

2026-11-21；核对提交 `91c9a36c2daa5e01b5fc5cd482d30dd4f3fc5252` 及当时工作区。范围为 `vendor/openrust/` 中除 framework 外的全部 18 个独立包，其中 package-discovery 是构建基础设施，另外 17 个承载扩展功能。清单包含 297 个 Rust 源文件、50,538 行；此计数用于界定范围，不是逐行正确性或上游功能覆盖率。

原审计收尾共享 HEAD 已更新为 `36c989366`，相对上述扩展提交只涉及 Docker／环境示例／部署文档；再次核对已验 Rust／Cargo 输入无漂移。其他协作者在途暂存与未暂存内容均保留，本轮没有操作其暂存区。

## 修复状态（2026-11-21）

用户随后授权“发现的问题，全部帮我处理好”。R1–R6 已按下面的关闭条件修正并验收；原审计结论保留为修复前证据。本次完成的是这六项审计问题及已交付接口范围，不是所有上游功能、PHP SDK 类型或全部官方 UI 的等价声明。

| 问题 | 修正与验证 |
|---|---|
| R1 | Horizon 读取 HORIZON_PREFIX／APP_NAME、HORIZON_PATH／DOMAIN，Provider 自动装配配置路由；保留显式路由并按应用去重，routes=false 关闭自动注册，默认拒绝访问。共享 Redis 总前缀下独立 Horizon 前缀、配置缓存、自定义域名／路径／中间件、根路径、显式优先／禁用均通过；实际自定义路径的桌面／手机和进程树通过 |
| R2 | 补 Stripe 原名客户／Portal／订阅远端对象入口，按供应商拒绝错误调用；恢复已消费的 Stripe 原生配置键与 Paddle 原生环境优先级，保留具名账户覆盖和原有可靠性机制。逐项接口／模型／配置映射见 [Cashier API 对照](cashier-api-mapping.md)。三库正反协议、归属、幂等、Webhook／事务与根配置缓存通过；JSON、显式参数、模型适配及未提供功能明确保留边界 |
| R3 | Support 重导出 ActivityLogger／PendingActivityLog／ChangeDetector，保留根路径且不复制对象；公开类型身份／编译导入和 Activitylog 包测试通过，三语正文同步 |
| R4 | 五包 NOTICE 区分源码适配与独立实现，保留六份固定原始许可证及 EasyWeChat 声明；补 SocialiteProviders 固定提交的 MIT／作者元数据（原树确无独立许可文件，不虚构声明）。根 THIRD_PARTY_NOTICES.md 汇集现有与新增来源声明；Docker runtime 显式复制。六份固定原文逐字节核对及分发包含检查通过，未重建或发布生产镜像 |
| R5 | Activitylog 两项、Horizon 两项、Pennant／Pulse 参考入口均在 require 前校验固定原文件摘要与来源；Scout 新入口实际执行固定 Builder／Engine／CollectionEngine／Str，覆盖 Unicode、筛选、映射及分页。七入口输出复现，每一个已声明源文件的篡改均拒绝；Scout 两类 reference 在三库原生实现对照通过 |
| R6 | [扩展复用取舍](../plans/extension-reuse-decisions.md)记录官方 Vue／Livewire、openidconnect-rs、async-stripe 的版本、许可、可复用部分与具体接入差异。修正“Rust 不能用 Vue／必然另一 HTTP 栈”之类不成立理由；当前保留单页面板与原生客户端，不新增管理器、HTTP／存储副本或生产依赖。不是仅改状态关闭，也没有用选型记录冒充额外供应商运行时验收 |

## 原审计结论（修复前）

现有扩展确实有固定 Laravel／Spatie／厂商源码调研，且大量复用现有框架和成熟 Rust 库；不能笼统认定为凭空设计或全部重复造轮子。但**不能认定全部严格遵守**：Horizon 配置及路由装配、Cashier 公开 API、Activitylog 公开命名空间存在可直接保持而未保持的差异；来源归属材料和部分参考测试的可复现约束也不完整。测试通过和路线 O01–O06 关闭，不消除这些问题。

本轮是设计／契约审计，检查清单、配置、Provider、公开导出、关键实现、上游参考生成器和现有验收证据；没有重新执行所有包的运行时矩阵、完整上游 PHPUnit 或逐行证明 50,538 行正确。以下问题均未在本轮修改生产代码，按关闭条件接续，不能计作已修复。

## 本轮联网与证据方法

- 网页检索和页面工具本轮未返回可用正文，改用真实 HTTPS 请求 GitHub REST API、固定提交的 raw 源码及 OpenID 官方规范，没有用离线记忆冒充联网结果。
- 查询 Laravel、Spatie、Yansongda 等相关仓库的维护状态、许可证元数据和项目说明，同时检查成熟 Rust 候选。不是 GitHub Trending 榜审计，不以 Star 数选择实现，不宣称某包是全生态最优。
- 固定上游源码用于判定项目既有契约；本轮补充候选的当前提交只用于选型比较，不悄悄升级固定基线。
- 本轮原始响应、下载成功／失败、来源 URL 和临时盘点在 `tmp/extension-design-audit-20261121/`，其中 sources.json／sources-followup.json／alternatives.json／inventory.json 可复核。失败的猜测路径没有作为依据；ActivityLogger 的实际路径为 `src/Support/ActivityLogger.php`，不是旧版的 `src/ActivityLogger.php`。
- 与上一轮 `api-wip-info.json` 的 2,445 个输入核对，只有根 README 和 Scout／Telescope 两份内部审计文本有变化，Rust 源码与 Cargo 输入没有漂移。运行时验收仍引用各包既有记录，不将指纹一致等同本轮重新执行测试。

## 全包检查表

| 包 | 成熟来源与实际采用 | 当前判断／应关注项 |
|---|---|---|
| activitylog | [Spatie Activitylog fdcc5ee](https://github.com/spatie/laravel-activitylog/tree/fdcc5ee063cbf91114679336c4fbf92837196419)；日志模型、流式接口、LogOptions、ChangeDetector、操作者、模型事件、缓冲与清理；复用 Eloquent／Auth／defer | 主要结构有依据；公开 Support 路径不全、上游许可材料缺失、参考生成器未自行固定输入，见 R3–R5。动态模型关系和原始 ID 的异步解析已有语言适配说明。 |
| cashier | [Cashier Stripe d5c0154](https://github.com/laravel/cashier-stripe/tree/d5c015490ff77f2c138fe79b86d2a982787fe3ac)、[Cashier Paddle 82bfc68](https://github.com/laravel/cashier-paddle/tree/82bfc68eb72c38e666a4900baeecca3621840259)；厂商 SDK 签名参考；复用原生 HTTP／SQL／密码学 | 并非原名原结构的完整移植，而是自定义统一计费接口，见 R2。事务收件箱与 Paddle 操作认领是有具体失败场景支撑的设计，不能仅因多几张表认定过度设计。 |
| fortify | [Fortify 1.43.0 / 3db382a](https://github.com/laravel/fortify/tree/3db382a3d5d6a0d537de32a190ce3b945e621713)；Actions／Contracts／Controllers／Features／Provider，密码学复用 AWS-LC、二维码复用 qrcode | 本轮没有发现需要另造认证框架的情况；沿用原生 Guard／Session／验证器。Passkeys 已在后续单元接入，不沿用早期“默认功能未交付”的过时结论。 |
| horizon | [Horizon b86d5aa](https://github.com/laravel/horizon/tree/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa)；监督树、分仓储、AutoScaler、命令、标签及指标，复用 Worker／Redis／进程管理 | 核心设计有直接来源；配置和路由习惯仍有缺口，见 R1。显式标签、内存计量、Unix 信号及轮询 listen 已声明；面板复用取舍见 R6。 |
| oidc | [OIDC Core](https://openid.net/specs/openid-connect-core-1_0.html)、Discovery／退出规范；复用 jsonwebtoken、AWS-LC、原生 HTTP／Session／原子缓存 | 没有固定官方 Laravel RP 对应物，独立 Manager 合理；但自有 Discovery／Claims／流程代码应补成熟 OIDC 库比较，见 R6。没有重写 JWT 签名算法。 |
| package-discovery | Laravel 包发现职责，使用 Cargo metadata／build.rs／既有 Provider，单一生成入口 | 编译型语言适配，非业务扩展；不需要运行 Composer 或另加插件运行时。本轮未发现重复的包发现体系。 |
| pail | [Pail 1.2.5 / fdb73f5](https://github.com/laravel/pail/tree/fdb73f5eacf03db576c710d5a00101ba185f2254)；命令／Options／File／Printer／来源对象，复用日志及原生进程 | 有固定逐项参考及许可证。文件监听和命令输出是本包本职，不能仅因有本地实现认定另造日志框架；PHP 运行时来源信息按已有 Rust 适配核对。 |
| passkeys | [passkeys-server 0.3.0 / 2e7f893](https://github.com/laravel/passkeys-server/tree/2e7f893429d35acff45ac16d05ddcf883372a20e)、官方 JS 0.4.1、webauthn-rs-core 0.6.2 | 属于实际复用成熟实现的明确例子：未自行实现 WebAuthn 验签，复用官方客户端；自定义模型／AAGUID／资源发布已有后续验收。通过 Fortify 或显式 Provider 使用，不应误判没有 discovery metadata 就是缺陷。 |
| passport | [Passport 13.10.0 / f5f5e9f](https://github.com/laravel/passport/tree/f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7)；Bridge／Contracts／Guards／模型／命令／迁移，复用原生认证、SQL 和密码学 | 已有固定迁移／协议／PHP 对照和分阶段验收，主要命名与结构有依据；OAuth 服务端与 OIDC RP 各有职责，没有把 Passport 冒充 OIDC OP。 |
| pay | [Yansongda Pay 01cf002](https://github.com/yansongda/pay/tree/01cf002156d99f61fcba6308be810554dfd879d3)、[Laravel Pay b2b2594](https://github.com/yansongda/laravel-pay/tree/b2b2594e97f2ee2e1e672b55f8085b25819b9612)、微信／支付宝官方 SDK | 场景、Provider／Facade、回调验签有成熟依据；PayManager／具名商户／类型化金额为已声明适配，未迁入 PHP 插件流水线，不自写密码算法。只支持已声明的直连商户范围，不扩大成完整 SDK。 |
| pennant | [Pennant 9046466](https://github.com/laravel/pennant/tree/9046466feb0146b6223db8b8e9c589aeba23f7f0)；FeatureManager／Driver／PendingScopedFeatureInteraction／事件／中间件 | 公开词汇、默认 database／array、作用域及批量判断有直接来源；类型化注册与异步认证为合理适配，本轮没有发现必须新建特性平台的理由。参考输入校验见 R5。 |
| pulse | [Pulse 33cc6fc](https://github.com/laravel/pulse/tree/33cc6fc68cd3052aa06227e88e8d933dd3d70a51)；Recorder／Storage／Ingest／聚合，复用 sysinfo、backtrace、Redis pipeline | 核心复用充分。sysinfo 替代 shell 采样有明确理由；Livewire 依赖 PHP 运行时，原生视图属于有依据的替代，但不等同全套 Livewire 组件兼容。参考输入校验见 R5。 |
| reverb | [Reverb 1.8.1 / 70e3d28](https://github.com/laravel/reverb/tree/70e3d28ed31466da34de0c055f3681b75a5a538c)；Pusher 协议、事件、频道、命令；复用 Axum／Redis／HTTP | 有固定来源；Redis 成员租约和内部消息是已记录的 OpenRust 分布式适配，不兼容混跑原 PHP 节点的保证。当前 HTTPS 等 WIP 保留，不代作已提交内容或新增厂商验收。 |
| scout | [Scout 6f6560d](https://github.com/laravel/scout/tree/6f6560dbcda3ec99d9ba90bd6eebed5e0f16e363)；Searchable／Observer／Builder／EngineManager，复用模型事件、事务回调及 Worker | 交付 collection／null 与自定义 Engine 契约，命名主线基本遵循。其余数据库和外部引擎明确未交付；原生数据库全文搜索不能替代 Scout DatabaseEngine。参考测试强度见 R5。 |
| settings | [Spatie Settings dc7c4c4](https://github.com/spatie/laravel-settings/tree/dc7c4c42bb3789c74493d756eaca2f183e9a68d5)；类型化分组、仓储、加密、save／refresh | 复用原生容器／SQL／事务／加密，没有另建后台或配置平台。组版本锚点、并发冲突与锁定字段报错均为显式设计差异；后者不是 Rust 必然限制，不能称完整等价。 |
| socialite | [Socialite 5.33.0 / 8032bb1](https://github.com/laravel/socialite/tree/8032bb1fa9d74aae54438f216e4d45db90d27ea9)、SocialiteProviders 微信／QQ、EasyWeChat MiniApp | Manager／One／Two／Provider／User 保留生态职责；成熟 JWT、密码学及真实 PHP 源码样本复用充分。`wechat` 配置统一是用户明确要求的 OpenRust 分组，不是 Laravel 原生配置名。 |
| telescope | [Telescope 39a8ea8](https://github.com/laravel/telescope/tree/39a8ea8ad79915df6c3ac9b4e72659ade808eba3)；18 Watcher／Entry／仓储／命令，复用原生各类事件和生命周期 | 公共 Watcher、模型、缓冲响应等已补齐；反射、来源过滤、multipart 和 Vue UI 仍有差异。不能因 UI 是 JS 就说 Rust 无法复用；选型证据见 R6。 |
| translation-database | [Spatie Translation Loader 2.8.3 / 17fa7d2](https://github.com/spatie/laravel-translation-loader/tree/17fa7d211bca1423a0e424823e7fddd5c2bc4ab6)；language_lines 结构、数据库覆盖文件、命名空间行为 | 精简 Loader 接现有 Translator，没有重写翻译系统；显式装配和应用迁移符合其只读词条加载范围。Translation Manager UI 与模型字段 Translatable 职责不同，未误引入。 |

表中“未发现”限于本轮检查维度，不表示全 API 等价、安全认证或生产上线批准。详细历史行为证据仍以各主题审计为准。

## 原问题与关闭条件

### R1 — P2：Horizon 未保持配置入口及路由装配习惯

位置：`vendor/openrust/horizon/config/horizon.rs:11`、`src/provider.rs:48`、`src/dashboard.rs:116`。

- 配置加载器完全不使用 Environment，Redis 前缀固定 `horizon:`。固定官方配置读取 `HORIZON_PREFIX`，缺省由 APP_NAME 的 slug 派生。应用按 Laravel 习惯配置该环境变量时，当前包仍使用固定前缀。
- 核心 `config/database.rs:140` 已有按应用名生成的 Redis 总前缀，所以不能断言默认不同应用必然串数据。但当应用显式共享 REDIS_PREFIX／同一 Redis 数据库并期望 HORIZON_PREFIX 单独隔离 Horizon 时，环境变量不生效会共享 Horizon 键空间，影响指标、监督器记录及按该仓储执行的控制操作。
- 固定上游具有 domain／path／middleware 配置并由 Provider 装配路由；当前包没有这些配置入口，`Horizon::routes()` 固定 `/horizon`，自定义路径依靠 `routesAt` 和应用路由组。Rust 路由已经能表达这些配置，现有记录不足以解释必须改成当前使用方式。

关闭条件：按固定上游补环境变量、应用名默认前缀及配置驱动路由，明确保留安全授权；兼容已文档化的显式路由模式时防止重复注册。验收共享 Redis 总前缀下两个 HORIZON_PREFIX、配置缓存、自定义 path／domain／middleware、禁用路由和授权拒绝。无需新造路由或配置管理器。

### R2 — P2：Cashier 的统一 API 偏离上游，不能仅归因于 Rust

位置：`vendor/openrust/cashier/src/billing.rs:11`、`:47`、`:71`、`:752`；`src/client.rs`、`config/cashier.rs`。

固定 Stripe 上游保留 `createAsStripeCustomer`、`createOrGetStripeCustomer`、`updateStripeCustomer`、`asStripeCustomer`、`billingPortalUrl`、`redirectToBillingPortal` 等入口，Stripe 与 Paddle 本身是独立包。当前合为一个具名驱动 Manager，用 `createOrGetCustomer`、`updateCustomer`、`billingPortal` 及 `SubscriptionHandle`／数据 Subscription 分离设计，许多方法返回通用 JSON。现有文档承认没有逐方法兼容层，但“有声明”不等于符合优先保留原名的约束。

Billable 从 PHP trait 变成 Rust struct 可以有类型和借用理由；以上方法改名、供应商语义合并并非语言强制。当前不宜拆成重复客户端或整体重写数据库，而应先建立两家方法／返回值映射，保留可直接表达的原名，以适配入口复用当前客户端和存储；不能把两家同名但不同语义的方法强行视为等价。

关闭条件：逐项映射已声明的计费 API、模型与默认配置，说明真正需要的 Rust 差异；以原名调用编译示例及供应商正反协议验证。保留现有操作认领、归属校验、Webhook 事务等已经验证的可靠性机制。

### R3 — P3：Activitylog Support 公开路径缺口

位置：`vendor/openrust/activitylog/src/lib.rs:26`、`src/support.rs`。

固定上游 `Support\ActivityLogger`、`Support\PendingActivityLog`、`Support\ChangeDetector`，Rust 当前主要从 crate 根导出这些类型，support 模块没有相应重导出。命名相同但公开命名空间不全，熟悉该版本 Spatie 的使用者无法按目录直接对应。

关闭条件：在 support 补准确的公开映射并核对文档及编译导入；保留现有根导出即可，不必为别名移动内部文件或增加包装对象。`models::concerns::LogsActivity` 当前已经存在，不列为缺口。

### R4 — P2：上游来源归属材料不齐

Activitylog 包没有上游许可文件，而固定原文提供 Spatie MIT 及原作者声明。Cashier、Settings、Socialite、Reverb 包也没有包内上游许可／NOTICE；根 LICENSE 目前只声明 OpenRust 作者，内部审计链接不能替代随可分发代码携带的来源说明。

本项是来源追溯和交付材料缺口，不据此推断所有“参考设计”都构成复制、也不作法律结论。应区分源码翻译、直接资源复制、独立协议实现和仅设计参考，尤其检查 Activitylog 翻译逻辑及测试 fixture，以及 Socialite／Reverb 的对应移植代码。Horizon／Pennant／Pulse／Scout／Telescope／Passport／Passkeys／Pail／Translation Loader 已有相应文件，不能一概称全仓遗漏。

关闭条件：逐项确定实际采用的来源及许可，保留适用的原作者声明；独立实现仅记录参考而不虚构来源。包与最终分发材料均可追溯，不引入一个新的许可证管理系统。

### R5 — P3：参考测试的固定输入与证明范围不统一

位置：Activitylog `tests/{changes_reference,support_reference}.php`，Horizon `tests/{autoscaler_reference,worker_reference}.php`，Pennant／Pulse `tests/reference.php`，Scout `tests/reference.php`。

前几项确实 require 实际上游源码，不是把 Rust 测试结果冒充 PHP；但生成器直接接收任意源码目录，没有像已有 Socialite／Pay 参考入口那样先验证关键文件摘要。提交号目前在审计或 fixture 中说明，独立重新生成时仍可能误用其他版本。

Scout 的生成器只运行 PHP 标量／JSON／匹配表达式，没有实例化官方 CollectionEngine；还直接使用 `strtolower`，固定 CollectionEngine 实际调用 `Str::lower`。现有中文与标量样本不证明非 ASCII 字母的大小写等价。这能证明对应 PHP 运行时表达式，不证明整个上游 Engine 流程。不能把它描述为完整官方 Engine 对照，更不能以这些 fixture 推断 PHPUnit 全通过。

关闭条件：复用已有参考输入校验方式，固定关键原文件及源码来源，加入篡改输入拒绝；Scout 至少增加实际固定 CollectionEngine 的关键搜索／过滤／映射路径参考，表达式 fixture 可继续作为补充。无需执行无关厂商端到端服务来证明这些局部行为。

### R6 — P3：原生面板与协议客户端缺少完整的候选比较记录

Horizon／Telescope 已声明重写为原生安全 HTML／JS，官方包中已有可浏览器执行的前端资源。PHP 后端不适用不等于 Vue 前端不可复用；现有记录尚未给出复用原前端所需 API、资源许可、依赖维护和部署成本的比较。Pulse 的 Livewire 依赖 PHP，有更明确的运行时替代理由，不能混为一谈。

OIDC／Cashier 当前有成熟密码学或 SDK 源码参考，但未见对现成 Rust 协议库／Stripe 客户端的系统取舍记录。本轮补查：

- [openidconnect-rs e20ddbb](https://github.com/ramosbugs/openidconnect-rs/tree/e20ddbb01ca68356f2979bc125f5613ab45b4e7e)，manifest 为 4.1.0，MSRV 1.88，MIT；默认 HTTP 依赖可关闭，不能以“必然强加第二 HTTP 客户端”为由排除。不过该提交有 chrono 依赖，需与本项目 Jiff 方向、既有算法／声明策略及 Session／缓存流程实际比较。
- [async-stripe 19d6990](https://github.com/arlyon/async-stripe/tree/19d6990265e23d1257602696ea065376c07d802b)，workspace 为 1.0.0-rc.10、MSRV 1.89、MIT OR Apache-2.0；存在客户端／类型／Webhook 分包。应比较固定 Stripe API 版本、预发布维护成本、按需功能和原生 HTTP 观察接入，不能只因“已有 SDK”就整体换库。
- [Jumbojett OIDC 5539c0b](https://github.com/jumbojett/OpenID-Connect-PHP/tree/5539c0b58f2cbb4fba78850f16a04654d1fea6fc) 是 PHP 生态参考，不是 Rust 可直接链接的依赖。Laravel 的 Socialite 也不等同完整 OIDC RP；不应为套名而合并职责。

关闭条件：给出简短、可检验的采用／不采用依据；只有实际减少代码与维护成本、且保留当前原生观察／安全契约时才替换。该项属于选型证据不足，不能从未做比较直接推断应推倒重写或已有运行时故障。

## 已有合理适配及保留边界

- 编译型 Provider 发现、显式类型注册、Future／Result 和受控 trait 对象有语言依据；默认应用 Provider 只注册应用模型，未预装可选包业务示例。
- 原生数据库／事件／队列／缓存／HTTP／授权及生命周期在各包重复利用，没有另建一套通用任务、认证或观测内核；直接服务于本包的 Recorder／Watcher／Engine 抽象符合上游职责。
- Settings 的保留版本行和锁定更新报错、Reverb 的成员租约、Pay 的具名商户与整数金额是已声明设计差异；不自动算错，但不能写成 Laravel／Spatie 原始行为。新增抽象是否过度应按具体场景判断，不能以 Rust 模块数或上游 PHP 文件数直接下结论。
- Scout 所有外部引擎、Telescope 任意 PHP 反射及 UI 完整等价、真实商户／托管服务、其他 OS／设备、正式 API 发布均不在既有本地通过结论中。本轮不把已知范围限制当作漏实现的运行时 bug，也不抹掉它们。

## 原审计验证和接续顺序

本轮执行 `node docs/tests/check_content.mjs`：三语均为 109 个导航章节、111 个 Markdown 文件，内容检查通过。完整盘点包 manifest／公开导出／配置／Provider／测试与许可证文件，固定来源下载结果留临时目录；手工核对上述问题的两端原文。现有源指纹未漂移的组合测试／Clippy／fmt／三库／Redis／浏览器结果继续以 [extension-final](extension-final.md) 和各包审计为证据，本轮没有重新执行这些矩阵。

接续建议：先 R1 配置和隔离，再 R4 来源材料、R3 公开路径及 R5 参考输入；R2 先做准确接口映射再最小补齐；R6 比较之后再决定是否替换实现。每项按对应的关闭条件验收并更新其包审计，禁止仅改状态文字关闭问题。本轮没有修改生产代码、暂存、提交、推送或联系其他对话；保留 Docker／Reverb 等共享 WIP。

## 修复验收与交付边界

独立源码快照 `tmp/openrust-pennant-20261121/source`；复用本工作流独占 `tmp/openrust-core-ecosystem-20261121/target`，TMPDIR 为项目 tmp/ 内绝对路径。Rust 1.94.0，原生并发／增量；没有查额度、设置人为停线、联系其他对话或新增并行代理。共享工作区期间其他协作者继续修改认证、启动、Docker／Reverb 等；本次不归属或提交这些改动，自己的源码／配置／测试与快照逐项核对。

日志位于 `tmp/extension-repairs-20261121/`，主要命令与实际结果：

- `cargo +1.94.0 test --locked -p openrust-horizon -p openrust-cashier -p openrust-activitylog -p openrust-scout`：65 通过；默认忽略 10 项，其中 Horizon 的 9 项以专属 Redis 入口另行全部通过，Cashier 的外部官方 SDK 参考入口本轮未重跑，不计通过。SQLite 为实际驱动。
- `tests/{postgres,mysql}_acceptance.py -- ... sql_matrix.py {pgsql,mysql}`：各 17 通过，覆盖 Cashier protocol／persistence／callback_http 和 Scout integration。服务由 REQUIRE 变量与批次专属容器门禁验证为 PG18／MySQL8.4；不使用开发数据库。最终源码补充断言后再次通过，日志 postgres-final／mysql-final。
- `vendor/openrust/horizon/tests/redis_acceptance.py -- cargo ... -p openrust-horizon -- --ignored`：9 通过，实际 Redis 8.10.3；新案例在同一 Redis/global prefix 下通过两个 .env 中的 HORIZON_PREFIX 验证 pause 状态双向隔离。未改共享缓存实现，因此不把此项冒充三种缓存驱动复验。
- 根 `cargo ... test -p openrust --all-features --test horizon_configuration --test cashier_configuration --test application_skeleton`：4 通过；检查包发现、配置发布及保留定制、缓存与默认骨架。Horizon config:show 在环境漂移后仍保持缓存中的 prefix/path。
- `cargo +1.94.0 clippy --workspace --all-targets --all-features --locked -- -D warnings` 与 `cargo +1.94.0 fmt --all --check` 通过。编译器对工作区外 patched rav1e/imgalifier 依赖的既有 warning 保留，未扩大修改范围。
- `tests/extension_reference_acceptance.py --upstream ... --framework ...`：PHP 8.5.12 镜像，七入口复现并逐文件拒绝篡改。AutoScaler 浮点值允许 1e-13 的同平台数学库尾差，索引／目标／排序等其他值严格比较；现有约 4.848098118036503 与 4.8480981180365035 的尾差如实保留，未改 fixture 掩盖差异。没有声称全套 PHPUnit。
- Horizon `process_acceptance.py`：实际 Worker／supervisor／master 进程树、重启替换、伸缩、停止／继续、失败重试、排空／信号及 Redis 失联清理通过。另以 `HORIZON_TEST_BROWSER=1 HORIZON_PATH=operations/queue` 复验：桌面／手机授权、详情、监控写入、指标、CSP 刷新与布局通过。
- 三语内容检查、VitePress 构建及 `/`／`/docs/` 的 12 项通用浏览器、6 项全导航检查通过；各语言 109 个导航章节、111 个 Markdown 文件。Horizon 默认前缀改变的已有部署迁移说明同步三语，最终补充说明后的构建／内容检查一并验收。
- 包内六份固定 MIT 原文与真实 HTTPS 下载逐字节一致；根运行时 NOTICE 含这些原文，Docker runtime COPY 入口存在。源码分发包含包内声明；没有将许可证文件检查称为已重建生产镜像或法律意见。

测试过程中先后修正了本次夹具未转义空格的 dotenv 值、测试缺失 Arc／Repository 导入，以及新增 Scout 断言的 needless_borrow；最终修复后重新执行受影响测试及严格检查，不以失败日志计通过。调研的 404 路径已记录且未作为证据；实际 async-stripe 默认 API 为 `2026-10-28.endive`，不同于本项目固定 `2026-11-18.endive`。

真实商户、托管身份商、实际资金、其他平台／设备、完整官方 UI／未提供 API、正式 API 导出与发布仍按既有边界；不把完成六项修复扩大为全部功能或生产验收。本轮未暂存、提交、推送、部署或发布，按当次 Git 授权保留工作区改动。未触及 31,250,000 KiB 停线，继续保留在复用缓存及日志；专属数据库／Redis 容器经既有入口回收，不删除现有数据卷。

最终交付复核：46 个本次源码／配置／测试／fixture／指南输入与已验快照逐字节一致；补充迁移说明后再次通过三语构建及 12 项浏览器／6 项导航，来源分发检查也已重跑，见 `tmp/extension-repairs-20261121/final-verification.log`。已验快照再次运行全 workspace fmt 通过。活跃工作区的另一次 fmt 检查报告其他协作者正在修改的 `tests/auth_models.rs`、`tests/startup.rs`、framework auth／install_routes 格式差异；保留其在途改动，未将此检查描述为活跃工作区整体通过。共享 HEAD 为 `b90a66c8a`，该结论不覆盖其后继续变化的核心 WIP。

用户随后授权本地提交：按六项修复核对暂存范围，46 个已验源码／配置／测试／fixture／指南输入仍与快照一致；其他协作者改动保留，不推送。提交号由 Git 历史追溯。
