# 错误转换治理（2026-11-23）

## 核对范围与设计

根应用与 vendor/openrust 下自有 Rust、测试、脚手架及 stub；排除第三方原始实现、历史归档和 target。实施前清单保存在 target/tmp/error-conversion-20261123/inventory.json：1968 个文件、4326 处 map_err、772 种文本形式及 248 处 From/TryFrom 声明。数量仅作定位线索，不能证明语义覆盖或完成率；后续协作变更可能改变这些数量。

已核对根指引、规范、当前交接和固定基线。Laravel v13.40.0 固定提交 1449bf241017087fbd38c118ecaabef3f6797fdc 的 Handler::render/prepareException/mapException 与 Kernel 保留 report/render 的 HTTP 边界职责；不将 PHP 异常捕获机制移植成 Rust 的全局错误类型。Rust 官方 From 要求无歧义、无损且不会失败，问号在返回边界调用 From；需要上下文时使用命名构造或局部映射。v0.x 的 core/error.rs 曾采用统一 Error，当前领域边界已分离，不复用旧版万能错误体系。

参考资料：Rust std::convert::From、Rust Book 9.2；上述固定 Laravel Handler、Kernel。原文及来源清单位于该工作流 research/，源码以 txt 保存，不新增 PHP 文件。

| 来源 → 目标 | 当前重复调用点与核对结论 | 处理决定与依据 |
| --- | --- | --- |
| ConfigError → HttpError | 欢迎页、Config Facade 测试、SetLocale、TrustedHosts、JSON API 配置均只包 Internal | HTTP 层增加具体 From，原始 ConfigError 保留，500 与脱敏不变 |
| BindingResolutionError → 各领域 Unavailable | 翻译、视图、数据库、缓存、队列、事件、日志、Hash、文件系统、Redis、Process Facade 和扩展 Provider | 在所属领域增加具体 From；Config 已有实现，复用，不依赖 HTTP |
| Binding/Config/Routing → BootstrapError | Application、HTTP 装配、ApplicationBuilder 重复包 ContainerFailed/ConfigurationFailed/RoutingFailed | 增加三种具体转换；Provider 名称、阶段、路径、生命周期映射保留 |
| Binding → RoutingError | Route 注册、Facade/Router 的 ContainerFailed | 只统一容器解析失败；URI、参数、正则及注册作用域仍显式分类 |
| Translation/View/Database → HttpError | App、View 渲染、资源、路由模型序列化及扩展控制器纯 Internal 包装 | 增加 HTTP 边界具体转换；数据库路由输出的 ModelNotFound→404 局部匹配保留 |
| serde_json / JoinError → HttpError | JSON Response、资源、流及 View 异步调度纯 Internal 包装 | 按实际重复来源统一；请求 JSON 解析的400及负载限制映射保留 |
| Cache/Queue/Event/URL/Session → HttpError | 已有具体 From；一些消费者仍重复包装 | 仅复用语义完全一致的转换，不重新实现 |
| Filesystem/Validation/Collection/Auth → HttpError | 已有分类转换，分别可能400/404、422/413、DumpAndDie、429或内嵌HTTP | 不将强制 Internal 的旧调用机械改为问号；保留边界差异 |
| Database/Cache/Session/Hash/Config/Binding → AuthError | UserProvider、SessionGuard、密码令牌、Sanctum 与应用 User 多处 backend 包装 | 核对实际来源后在 AuthError 增加反复出现的具体 backend 转换；HttpError 的 Backend 包装不替换为已有 Http 转换 |
| Translation 来源 IO/JSON/Join/Config | FileLoader 与语言设置；加载缺文件已有恢复逻辑 | 不新增单次转换；保持缺文件恢复、加载阶段和原始 source |
| Collection Source | Arr 深度、JSON、UTF8、IO、随机源等异构错误 | 按实际来源确认重复再处理；不添加泛型 From<E> |
| View CompileFailed/RenderFailed | 同种底层错误由编译/渲染阶段决定目标变体 | 保留阶段性映射和路径上下文 |
| 数据库 Driver/SchemaIo/FakerIo/事务/迁移/Seeder | 操作名、迁移名与不同IO职责不相同 | 保留 map_err，禁止一个 IO From 覆盖所有数据库错误 |
| Cache/Queue backend/execution/delivery | 存储、执行、投递、序列化与无效负载职责不同 | 不按 helper 名称批量替换；保留恢复、租约、重试及错误分类 |
| 仅用于应用示例／测试的日期、分页与OAuth／Scout适配 | 不以测试中一次指定的500策略为由新增领域→HTTP公开转换 | 保留局部适配；同一混合helper中已能确定为DB／Binding／View等来源的调用单独简化，Filesystem强制Internal和专用400／404转换仍区分 |
| ConsoleError / CLI 输出 | 大量 helper 映射为终端诊断字符串，公开契约为 message | 本轮不改终端错误体系；不把字符串映射冒充无损 From |
| 扩展 OAuth/Pay/Cashier | OAuth safe_hint、支付状态未知/验签/数据库等有意脱敏 | 保留安全映射，不附带可能含凭据和正文的原始错误 |
| Socialite 等已有 #[from] | Session/Request/Redirect/HTTP client 具体来源已有实现 | 可复用的构造调用简化；不同协议响应分类保留 |
| 生成器与公开示例 | resource stub 重复数据库→HTTP；job stub 为 InvalidPayload | 同步资源模板与实际消费者，保留任务无效负载分类 |
| 解析/范围/状态/恢复映射 | 无效请求、越界、404、重试、默认值、日志、释放锁等 | 保留有行为的 map_err；不以删光 map_err 为目标 |

## 实际修改

| 错误边界 | 实际修改与消费者 | 保留的分类与上下文 |
| --- | --- | --- |
| HTTP | http/error.rs 增加 Config、Binding、Translation、View、Database、JSON、Join、Log、Redis、InvalidHeaderValue、IO、Mail、Notification、Hash、Image 的具体 Internal 转换；Config Facade、响应、资源、异步视图、控制器、应用及扩展消费者改为问号 | 路由模型结果的 ModelNotFound→404、请求解析400、验证422／413、认证429／内嵌HTTP与其他特化转换不改；强制 Internal 的特殊来源保持显式包装 |
| 领域与容器 | Translation、View、Database、Cache、Queue、Event、Log、Hash、Filesystem、Redis、Process、Context、Maintenance、Image、Concurrency 在自身错误类型中接收 BindingResolutionError→Unavailable；Facade 不再重复构造相同变体 | 不引入领域到HTTP的反向依赖；需要中间领域来源的调用显式 map_err(DomainError::from)，不跳过错误链 |
| 装配与路由 | BootstrapError 接收 Binding／Config／Routing；RoutingError 接收 Binding；应用装配与路由解析复用 | Provider 名称、注册／启动阶段、配置路径、迁移与Seeder上下文保留 |
| 认证 | AuthError 接收 Binding、Database、Cache、Hash、Session、Config、Event、Cookie、JSON、Url→Backend；用户Provider、Guard、密码流程、Sanctum、Passport及根 User 复用 | HttpError 的显式 Backend 包装不能改成已有 Http 转换，否则可能从500变为404等状态 |
| Collection | JSON→Serialization，ArrValueError／DataPathError→Source；实际集合消费者复用 | UTF8／IO等单次或有阶段差异的来源不新增转换；DumpAndDie既有语义保留 |
| 缓存与日志 | Cache 接收IO／Database／Redis→Backend；Log 接收IO→WriteFailed；file／database缓存与Redis limiter消费者复用 | 缓存损坏、租约、操作上下文及重试恢复保留；JSON的Backend与其他模块Encoding／InvalidPayload不混同 |
| 图片 | IO→Io用于文件打开、流读取与格式探测；Image→HTTP Internal用于Responsable与请求上传入口；缺失文件的实际dimensions调用验证I/O类型及HTTP来源 | 保留图片编码器、存储、网络与限制分类，IO不因NotFound改为HTTP404 |
| 并发 | IO→Io、JSON→Encoding、Process→Process；任务协议与驱动复用 | 调度／执行／进程分类保留；不扩大为通用Http转换 |
| 扩展HTTP边界 | Horizon面板、Pulse面板、Pennant中间件重复同类型Internal包装，具体From放在各自dashboard／http／middleware中；公开Result、真实Facade／Dashboard消费者、报告回调、500脱敏及领域来源链单独验证 | 不让核心HTTP反向依赖扩展，内嵌Auth／Request错误不自动展开改变原状态 |
| 脚手架及扩展 | resource stub 使用 Ok(Value::Object(model.toArray()?))；Fortify公开actions、Telescope、Pulse、Passport、Horizon、Pail、translation-database等实际调用同步 | job的InvalidPayload、Fortify通知的强制Internal、OAuth safe_hint与支付脱敏保持 |
| 文档与规范 | 三语errors正文说明问号、具体转换、中间领域和闭包推断；conventions补充可复用原则 | 不承诺所有map_err可删除，不增加万能错误管理器 |

### 独立核对并修复的既有错误链缺口

1. concurrency::Error 已保存 Process／Encoding／Io／Unavailable，但 Error::source 原先返回空。补充对应来源，不改变变体或Display。
2. cache/file_store.rs::io_context 原先 format! 将IO来源变为字符串。私有 CacheIoError 保留原始IO与操作说明，外层IO种类和Display不变。新增真实FileStore失败测试验证操作前缀、ErrorKind和原始来源；不在其他错误中扩大包装层。

3. ArrValueError::Path 已保存 DataPathError，但默认 Error::source 未暴露。补充 Path 分支，保持其余变体、Display与公开From不变；使用真实 data_set 深度失败经 ArrValueError 和 CollectionError 两层转换验证来源链。

### 验收边界

隔离基线为 9db399c1110087905c23ad06b66c37393da8c69c，加本任务交付差异。共享的View、Pagination、Facade、根User从该基线重建本任务差异，排除其他协作者后续Blade对象、模型字段宏等在途功能。最终提交候选再以 c3f0b38f067abe2efbd81416be45ab6f7fc85f40 重建，仅叠加本次差异；排除未提交的图片Manager与模型字段功能。通过结果不代表这些协作功能已验收，交付工作区保留其全部改动。

现有忽略的参考PHP夹具不会进入git archive；仅在验收快照中将该夹具按原始字节保存为txt，并改快照测试的两处路径。交付测试和既有PHP未修改，未新增PHP。

Rust 1.94.0，项目build_guard入口，独占构建缓存位于 target/tmp/locale-discovery-20261123/cargo-target；源码、参考、脚本及日志归 target/tmp/error-conversion-20261123/。磁盘停线已由守卫持锁清理编译产物，保留源码、日志、依赖、数据库和数据卷；之后继续验收。

### 已执行验收

| 项目 | 实际结果与证据 |
| --- | --- |
| 工具链／静态检查 | Rust 1.94.0；隔离workspace/all-targets严格Clippy通过（第三方依赖自身警告未修改）；fmt通过，日志 clippy-final.log／fmt-final.log；最终提交组合另记 clippy-delivery.log |
| 新增边界及Config公开消费者 | error_conversions 29项、config_facade 3项、config_facade_access 2项通过；覆盖同步与异步问号、容器成功／失败、结构化来源、Config实际HTTP报告回调与响应脱敏、缓存操作上下文、图片缺失文件、并发来源链。core-tests.log |
| 框架消费者 | 169项通过，涵盖异常报告／渲染、Collection／LazyCollection、装配语言、视图／翻译、认证授权、真实模型路由404及序列化500、Mail／Notification、Concurrency／Process；framework-consumers.log |
| 应用与模板 | 47项应用入口／生命周期／欢迎页／资源生成器／生成资源编译通过；另4项本地认证模型、2项Fortify安装actions、10项Passport生命周期通过；application-consumers.log及对应独立日志 |
| 三库／缓存 | PostgreSQL 18.7、MySQL 8.4.13均在各自独占tmpfs夹具通过缓存事务和认证模型；SQLite 3.54.1（运行时查询确认）通过缓存事务／认证模型／API资源／分页；Redis 8.10.3专属tmpfs实例PING／值共享／所有者锁通过，file缓存跨应用及Artisan通过。matrix-postgres.log／matrix-mysql.log／cache-*.log；测试容器已回收，未操作开发数据 |
| 三语文档 | 内容、构建、双路径浏览器及导航检查通过；docs-results.log与四份详细日志 |

framework-consumers中三项原有ignored：artisan_worker_fixture与artisan_mail_fixture由实际进程测试启动；smtp_delivery_fixture需要独立SMTP，未以真实SMTP投递验收。其他引擎／操作系统、完整workspace测试、发布及性能未验。本次不将编译／扫描数量等同于全仓所有错误语义已穷尽。

最初图片增量快照曾混入另一任务的Manager与参考数据组合，导致上传文件名断言失败；不修改交付逻辑来迎合测试，改用提交基线及对应参考夹具重建，22项图片、28项初版边界测试及模型路由／配置消费者通过，见delivery-tests.log。最终提交组合的fmt及workspace严格Clippy通过，扩展后续检查见clippy-extensions.log。

补充复核：44项框架消费者及9项应用生命周期通过；Horizon／Pulse／Pennant新增8项公开Result、真实HTTP上报脱敏与来源链测试通过；Pulse 10项、Pennant 3项、Horizon 2项面板授权消费者通过。健康检查、维护及请求／缓存头／文件下载37项通过。日志分别为remaining-framework.log、remaining-application.log、extension-errors.log、pulse-consumers.log、pennant-consumers.log、horizon-consumers.log、final-http.log。事件缓存的两项独立进程夹具由父测试启动，未当作独立业务通过项。

复核补齐json_options序列化、应用语言、事件事务／广播、限流、分页视图、混合管理器属性注入与Fortify账户调用，保留日期、UTF8、HTTP客户端、正则、专用Filesystem和OAuth等仍需局部指定策略的映射。HTTP缓存头／维护／下载中IO、Cookie、URL、JSON与Header来源复用已验证的具体转换，操作文本及恢复分支不变。Fortify账户／邮件验证／密码重置14项通过（fortify-consumers.log）；SQLite版本由独立内存连接查询，未提交版本探针。

最终来源链回归：29项error_conversions、76项Collection、6项LazyCollection及8项扩展错误测试通过（final-sources.log／extension-errors.log）；最终fmt与workspace/all-targets严格Clippy通过（fmt-final.log／clippy-final.log）。扩展转换实现归HTTP入口模块，领域定义不反向引用核心HTTP装配。

用户随后授权本次本地提交，不含推送；提交仅包含本任务差异，其他协作者改动继续保留。
