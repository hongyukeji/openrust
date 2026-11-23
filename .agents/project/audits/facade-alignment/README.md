# 门面公开契约对账

## 范围与依据

用户授权逐个核对全部门面，补齐缺口、实际验收后本地提交；不推送。此记录不能替代各所属模块的验收，也不把类型存在、底层方法存在或旧测试通过当成门面闭合。

基线：[framework v13.40.0](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Facades)。固定源码有一个基类及 46 个具体门面。[逐类型公开入口](upstream-entries.json)保存需要逐项检查的声明入口；当前 `pending` 明确表示尚未完成逐项签名、默认值、状态、装配和行为裁决。注解不包含所有动态委托入口，须继续检查实际目标、继承和 trait，不能把清单当成完整行为证明。

对照固定 Goravel 的应用级服务装配；公开职责与行为仍以固定基线为准。已检查 `openrust-v0.x` 对应响应、Session 和文件存储：旧版没有当前 Facade 类型体系，文件存储是受磁盘边界约束的存储接口，不能拿来冒充本地 `File` 服务。当前已具备的 HTTP 响应构造器、流、视图、重定向器和 Session Store 优先复用，不建立平行实现。

## 全类型复核队列

“已有入口”仅表示已确认公开导入；下面没有任何一行因此被计为完整对齐。

| 门面 | 当前公开入口／发现 | 状态与待办 |
|---|---|---|
| Facade | `support::facades::Facade` | 已补 resolved、应用读取／带作用域切换、is_fake；完成新路径与现有 spy 的实际验收。partialMock／shouldReceive／expects、专用 spy、defaultAliases 与剩余全局生命周期仍待闭合 |
| App | `support::facades::App` | 本批补容器注册/解析、别名/钩子/作用域、标签/工厂/调用与部分路径/环境入口；Application 生命周期、其余路径覆盖和宏等仍待核 |
| Artisan | `support::facades::Artisan` | 有 call/queue/command/add；继续核对 Kernel 的 all/output/resolve/启动回调及参数/退出状态 |
| Auth | `support::facades::Auth` | 逐项核对 Manager、默认 Guard、用户 Provider、认证状态与 fake/test 入口 |
| Blade | 已接 `support::facades::Blade` 与真实 `blade.compiler` 单例 | 原生编译产物、渲染、路径/缓存、指令/条件与类型格式化已实测；预编译阶段、组件完整入口及生成代码契约仍待补齐 |
| Broadcast | `support::facades::Broadcast` | 核对广播管理、频道注册/鉴权、连接、事件与测试替身 |
| Bus | `support::facades::Bus` | 核对 Dispatcher、批次/链、同步/提交后派发及 fake |
| Cache | `support::facades::Cache` | 核对默认仓储、Store、锁、tags、memo、替身及上下文隔离 |
| Cloud | 未开放对应门面 | 核对托管环境识别、托管队列连接及 managedQueues；真实云账号验收边界单列 |
| Concurrency | `support::facades::Concurrency` | 核对 Manager 选择、run/defer、驱动和生命周期 |
| Config | `support::facades::Config` | 核对全部仓储转发、类型读取、缺键默认、多个配置与 Serde 适配 |
| Context | `support::facades::Context` | 核对可见/隐藏上下文、增删、dehydrate/hydrate、事件及测试行为 |
| Cookie | `support::facades::Cookie` | 核对创建/队列、has/get、默认参数、过期与解密后的当前请求 |
| Crypt | `support::facades::Crypt` | 核对加解密、序列化适配、轮换、可用密钥和错误 |
| DB | `support::facades::Db` | 核对管理器和连接全部委托、事务、事件、查询、危险命令限制 |
| Date | 只有 `support::Date` 值对象 | 缺门面/工厂；核对时钟、解析、日历、语言、格式化、工厂替换及冻结时间，复用已有 jiff/测试时钟 |
| Event | `support::facades::Event` | 核对 Dispatcher 全部委托、事务后派发和 fake 的排除/作用域 |
| Exceptions | 已接 `support::facades::Exceptions` 与真实异常契约 | 已补报告/渲染/映射/上下文、运行期配置及具类型fake与实际Worker；withoutExceptionHandling、全局限流回调及共享基类测试能力仍待闭合 |
| File | 已接 `support::facades::File` 与 `filesystem::Filesystem` | 本地读写、锁、原子替换、目录、基础 glob、摘要、惰性行与宏已实测；require/条件代理、完整标志与平台边界仍待闭合 |
| Gate | `support::facades::Gate` | 核对策略、用户/资源、前后钩子、授权响应和事件 |
| Hash | `support::facades::Hash` | 核对管理器/驱动、校验、重哈希、配置校验与未知算法 |
| Http | `support::facades::Http` | 核对 Factory/PendingRequest 委托、fake/sequence、禁止逸出、批次/并发和断言 |
| Image | `support::facades::Image` | 核对图像工厂、驱动、解码、限制与公开委托 |
| Lang | `support::facades::Lang` | 核对 Translator/Loader、路径覆盖、namespace、消息选取、语言/回退与缺失回调 |
| Log | `support::facades::Log` | 核对渠道/堆栈、共享/请求上下文、级别、事件及 PSR 对应接口 |
| Mail | `support::facades::Mail` | 核对 Manager/Mailer/PendingMail 的入口、队列、附件、失败与测试替身 |
| MaintenanceMode | 已补 `support::facades::MaintenanceMode` 与 `foundation::MaintenanceModeManager` | 管理器七入口与四个驱动委托已实测；HTTP 改为逐请求解析实际 Contract。公共 Facade 基类继承测试接口仍待全局核对，不先宣布全门面闭合 |
| Notification | `support::facades::Notification` | 核对 Manager、路由通知、立即/队列派发、fake 和断言 |
| ParallelTesting | 未开放对应门面 | 缺测试运行器绑定、选项/token、进程/测试/数据库回调；不能只增加未被执行的回调注册表 |
| Password | `support::facades::Password` | 核对 Broker、Provider、令牌仓储及所有委托/默认值 |
| Pipeline | `support::facades::Pipeline` | 核对 Factory/实例构建、send/through/via/then/事务及 Context 隔离 |
| Process | `support::facades::Process` | 核对 Factory/PendingProcess、管道/池、fake、断言和退出/信号 |
| Queue | `support::facades::Queue` | 核对 Manager/连接、任务生命周期、连接选择、fake/except/for 与 Worker 真实消费者 |
| RateLimiter | `support::facades::RateLimiter` | 核对命名规则、命中/过期/清除、attempt 回调与缓存/Redis 接线 |
| Redirect | `support::facades::Redirect` | 核对全部 Redirector 入口；现有需显式请求的 guest/intended 要与请求级委托分别核对 |
| Redis | `support::facades::Redis` | 核对管理器、动态命令、连接、事务/管道、PubSub、事件及序列化 |
| Request | `support::facades::Request` | 225699817 有请求门面专项验收；继续核对基类继承、duplicate 深复制、全局启动入口等剩余适配，不把既有专项视作全量对齐 |
| Response | 此前仅宏；本批实现工厂转发中 | 构造/视图/流/文件/重定向接线；JSON 编码选项、默认参数表达和 ResponseFactory Contract 仍须核清，不先闭合 |
| Route | `support::facades::Route` | 核对 Router/Registrar/Binding/当前路由全部方法；注册期和运行期状态分别验证 |
| Schedule | `support::facades::Schedule` | 核对 Schedule/Event 的委托、默认参数、生命周期、命令与后台完成事件 |
| Schema | `support::facades::Schema` | 核对 Manager/Builder、所有操作、connection 及蓝图/方言边界 |
| Session | 此前只有 extend；本批补请求 Store 转发中 | 常用读写、闪存、轮换和请求生命周期；Manager 切换/forgetDrivers、handler、宏仍需补核；now/ageFlashData 已补并实测 |
| Storage | `support::facades::Storage` | 核对 FilesystemManager/Adapter 的全部入口、默认/云磁盘、fake、构建及错误策略 |
| URL | `support::facades::Url` | 核对 Generator 全部入口、当前请求/默认值、签名、密钥轮换与回调 |
| Validator | `support::facades::Validator` | 核对 Factory/规则扩展、解析器/存在性/回退消息及 make/validate 委托 |
| View | `support::facades::View` | 核对 Factory/编译器、命名视图、组件、共享数据、事件、缓存与宏 |
| Vite | 未开放对应门面及运行服务 | 核对 manifest/hot、资源标签、入口、CSP/SRI、SSR、预取及模板实际消费者；需要实际补模块，不能只导出同名类型 |

`Sms` 是已批准的框架扩展，单独跟踪，不计入上游 46 个具体门面的对齐数量。

## 填充与验收顺序

1. Response 和 Session 的开发者直达入口，验证与当前请求/视图/Session 使用同一对象；先完成实际消费者再收口。
2. App/Facade 的容器与生命周期入口，接上已有 Manager/Factory 的薄转发，逐项检查默认值及上下文关闭。
3. Blade、File、Date、MaintenanceMode、Exceptions、ParallelTesting、Vite、Cloud 的缺失公开类型与底层能力；按实际依赖顺序补齐各所属模块。
4. 其余已存在门面逐项核对全部声明入口和实际目标的公开委托；缺实现和缺验收继续显式登记。最后回归代码生成、默认应用、真实 HTTP/Artisan/Worker 及三语用法。

当前未完成全量对齐，不据此变更核心功能簇闭合状态。新增验收结果和实际提交在对应批次完成后写入下节。

## 本批状态

### App、Response、Session 第一批

- 基于已提交 `f171e1a3b` 的独立快照验收，随后合入已提交 `90d6e1c70` 的请求输入改动完成组合回归，不夹带 N-05 命名、请求输入易用性或其他在途改动。新增代码按当前 Rust 命名。
- ResponseFactory 新增响应构造、当前应用视图、JSON/JSONP、Markdown、异步流/SSE、文件/下载、路由/动作/guest/intended 重定向；类型绑定与 `response` 名称指向同一工厂。持有的工厂不会切到其他应用的请求上下文。
- Session 入口借用当前请求真实 Store，包含读写、闪存、旧输入、令牌/ID 轮换及会话缓存；没有请求时失败。Store 的 `now` 与 `age_flash_data` 补齐，保存与显式老化复用同一算法。
- App 新增既有容器/应用能力的静态委托，包含异步解析、注册、别名、解析钩子、实例/作用域、扩展器、标签、工厂、方法调用及部分路径/环境。借用回调可用 `call_callback`；`tagged` 和 `when` 通过回调保留底层借用与惰性语义。未补齐的入口仍在上表及逐项清单保留。
- 实际消费者发现整数路由参数缺少 `Into<UrlValue>`，补齐整数与布尔转换；没有在测试里改成手工字符串拼接。

#### 实际验收

工作目录 `target/tmp/facade-alignment/` 保留原始日志、固定上游及隔离源码；日志不是 Git 交付物。工具链 1.94.0；独占缓存为同目录 `cargo-target`，后续开发复用；此缓存关闭调试信息以控制体积，保持正常增量和并发。

- 最初请求/响应/Session/URL 六个目标共返回 88 项通过；其中 Session 测试存在 MySQL/Redis 环境缺失时提前返回的分支，这次不把这些分支算成真实驱动通过。
- 新增 App 2 项、响应/会话 8 项，另回归 Request 17 项、FacadeSpy 2 项、Config 2 项。最后扩展用例覆盖 App 标签/方法调用/借用回调，以及 Session 缓存同仓储、显式闪存老化；最终组合另包含请求输入易用性 5 项：六个目标共 **36 项通过**（`combination-tests.log`），不存在把源码测试数量当成方法覆盖率的换算。
- 独立 MySQL **8.4.13**：开启 `OPENRUST_REQUIRE_FACADE_MYSQL=1`，8 项测试全过，其中 6 项经真实 HTTP Kernel 使用 MySQL 会话，2 项是纯流/Store 算法。每个 Kernel 使用独立会话表；批次结束由 `tests/mysql_acceptance.py` 删除专属 tmpfs 容器，不碰开发数据库或数据卷。覆盖 ID 轮换后持久化、闪存消退、Session 缓存、guest/intended 的写入及一次性消费。
- 三语文档内容检查及站点构建通过；桌面/移动端共 **18** 组 App/Response/Session 章节和锚点检查通过。
- 初次 workspace Clippy 在 32 GB 磁盘停线被中断，不计通过；原日志保留。随后改用本工作流独占缓存，重新执行受影响库、两个新测试目标及请求输入目标的严格 Clippy，最终通过（`clippy-combination.log`，9.91 秒）；workspace 格式检查通过。初始测试中别名 `isShared` 和已缓存实例 `extend` 的预期与固定上游不符，查阅 `Container.php` 后纠正预期，失败日志未改写。

可复现命令（从根目录运行，独立快照路径换为当前验收源码即可）：

```sh
python3 -B tests/build_guard.py --workflow facade-alignment --target-dir target/tmp/facade-alignment/cargo-target -- cargo +1.94.0 test --manifest-path target/tmp/facade-alignment/source/Cargo.toml -p openrust-framework --test app_facade_api --test response_session_facades --test request_facade_full --test facade_spy --test config_facade_access
python3 -B tests/build_guard.py --workflow facade-alignment --target-dir target/tmp/facade-alignment/cargo-target -- python3 -B tests/mysql_acceptance.py -- sh -c 'export OPENRUST_TEST_FACADE_MYSQL_URL="$OPENRUST_TEST_MYSQL_URL" OPENRUST_REQUIRE_FACADE_MYSQL=1; cargo +1.94.0 test --manifest-path target/tmp/facade-alignment/source/Cargo.toml -p openrust-framework --test response_session_facades'
```

尚未据此验收真实 Redis、Windows/Linux 原生行为、所有数据库、全部 JSON 编码选项、ResponseFactory Contract、SessionManager 驱动切换/handler/宏及其他 43 个具体门面的完整公开契约。**本批是已验范围交付，不是全量对齐完成。**

### Cache：共享状态、管理器转发与宏（2026-11-24，已验收批次，门面仍 partial）

本批沿用固定 Framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 CacheManager、Repository、MemoizedStore、MemoizedTaggedCache 与 Macroable 调用链；不是只核门面注解。旧版 `openrust-v0.x` `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 的 `framework/src/cache.rs` 没有当前门面／管理器装配，复用当前驱动、仓储、锁、事件和 memo 生命周期，不移植旧 API，也未把旧版测试计入本批结果。

| 公开契约 | 本批实现与可观察行为 |
|---|---|
| 默认选择、解析与删除实例 | 门面开放 `get_default_driver/set_default_driver/resolve/repository`；保留 store/driver/build/extend/purge。`forget_driver(None)` 选择当前默认，`forget_drivers(&[...])` 表达上游数组参数；已取得 Arc 不因忘记实例而失效。resolve 新建而 store 复用。 |
| 仓储设置 | TTL、Store 与事件分派器改为短锁保护的共享状态，静态 setter 修改同一个仓储，不要求开发者取 root 或克隆绕行；Clone 复制设置并共享原后端。替换的对象在锁外析构，回调与 await 不持设置锁。 |
| 所有权适配 | Repository Contract 的 `get_store` 返回 `Arc<dyn Store>`，而非借用后端；Store 的 `get_prefix` 返回 `Cow<str>`，通常借用，动态适配器持有字符串。memo 的前缀与锁能力跟随当前后端。RateLimiter、Worker marker、Failover 与 Fortify 测试消费者同步处理持有期；没有不安全借用或泄漏对象。 |
| 事件快照与刷新 | 新仓储捕获事件分派器；容器替换绑定不会悄悄改变已解析仓储，`refresh_event_dispatcher` 才统一更新；`set_event_dispatcher` 直改所选仓储。Events Provider 提供实际分派器 Contract 绑定，允许自定义实现，无平行事件体系。 |
| setApplication | `set_application(&Application)` 惰性切换未来解析来源，弱持有应用，不丢弃已缓存仓储和自定义创建器；单次解析固定来源，创建器中切换应用也不混合配置、后端与宏注册表。 |
| 宏与 mixin | `r#macro/mixin/has_macro/flush_macros/macro_call` 在实际仓储上执行；同一应用的具名、标签和 memo 仓储共享注册表，应用之间隔离。回调可重入注册，无宏锁跨 await；未知名称明确报错。mixin 用具名 CacheMacro 迭代器表达可调用对象，replace 为显式 bool。 |
| 普通／默认读取 | 宏生成的 string/integer/float/boolean/array 使用普通读取、`_or` 和 `_or_else`；保留 Cache 数字字符串等已有转换，不套用 Request 规则。null/缺失走默认，错误类型与后端错误不被默认掩盖，命中不计算惰性默认。get_multiple 普通返回可选值映射，get_multiple_or 填充统一默认。 |
| memo 默认与锁 | 包装仓储默认 TTL 为 3600 秒、name=None、外层 events=None；标签视图继承外层配置。实际 I/O 与事件仍通过内层仓储。`lock_with_owner` 保留可选 owner，restore_lock 恢复同一令牌。 |

验收日志统一在 `target/tmp/facade-alignment/cache/`：

- `final-file.log`：7 项公开门面 HTTP 路径通过，另 scope 关闭用例通过；其中无 Redis 环境的条件测试不计 Redis 验收。
- `sqlite-fixed.log`、`mysql.log`、`postgres.log`：上述 7 项分别在真实 SQLite 文件、独立 MySQL 8.4、独立 PostgreSQL 18 通过；数据库 fixture 使用独立表/容器，无开发数据库操作。
- `redis-final.log`：7 项公开门面测试及 2 项 memo 测试通过；真实 Redis 8 的 HTTP 与 Artisan `queue:work --stop-when-empty --sleep=0 --timeout=0 --tries=1` 分别验证请求与任务生命周期。独立临时 Redis 容器已清理，无共享缓存或卷操作。
- `typed-mysql-redis.log`：要求夹具存在的类型／批量消费者，在同批独立 MySQL 8.4.13 和 Redis 8.10.3 实际通过；同时覆盖 array/file。`consumer-tests.log`：根 cache 过滤组 20 项及事件 3 项返回通过，未设置外部服务的条件分支不计外部验收；实际 file/SQLite/array、Artisan cache 命令和事件路径已运行。`dependent-tests.log`：补跑仓储克隆设置 1 项与 Fortify 4 项通过。
- `fmt.log`：冻结源码 `cargo fmt --all -- --check` 通过。
- `clippy-final.log`：冻结源码 `cargo clippy --workspace --all-targets -- -D warnings` 通过。首次 Clippy 指出已有测试的冗余 mut，已修正；失败日志保留。
- `docs.log`：三语内容检查与站点构建通过；`browser.log`：三语、桌面／移动端四个 Cache 章节，共 24 次实际页面检查通过。
- 保留 `initial-tests.log`（事件名错误）、`sqlite.log`（夹具漏建 SQLite 文件）、`redis.log`（镜像无声明卷而夹具限制过严）、`redis-fixed.log`（旧 memo 标签名称断言）和 `clippy.log`，后续成功不覆盖失败记录。memo 名称断言修改依据是 CacheManager.memo 创建无 store 配置的外层 Repository，Repository.tags 再把外层配置传给标签视图。

**未闭合**：本批仅关闭已列路径的缺口。专用 Cache spy 与 memo 测试交互、`handleUnserializableClassUsing` 对 PHP 类反序列化的精确 Rust 裁决，以及其余委托的完整参数／默认／错误与真实消费者证据仍需完成。JSON/Bytes 不包含 PHP 类加载，但不能用无操作兼容方法宣称对齐；通用 Facade 替换也不代替专用 spy。完整门面保持 partial，不更新核心完成率；外部 Memcached/DynamoDB/S3 与发布矩阵未在本批重验。


### MaintenanceMode：管理器与实际运行状态（2026-11-24，已验收批次）

固定源码追踪 Facade -> MaintenanceModeManager -> Manager（含 RebindsCallbacksToSelf）及四个 Contract 方法；trait 仅有 protected 回调绑定，不另遗漏公开入口。FoundationServiceProvider 原行为为 manager singleton、MaintenanceMode Contract 动态绑定。本项目原先把具体驱动直接 singleton，并让 HTTP 中间件永久持有它，新增管理器后实测暴露 `forget_drivers` 后 Artisan 与 HTTP 看到不同 array 状态，现已修正。旧版 `openrust-v0.x` framework/src 内未找到对应维护管理器可复用；复用本仓已有三种后端、HTTP/命令/调度/Worker 消费者，缓存解析注册表提取现有成熟实现供两种 Manager 共用，没有另造平行维护状态。

- 正式入口：`support::facades::MaintenanceMode`、`foundation::MaintenanceModeManager`（也可从 `foundation::maintenance` 导入）。公开构造器接受 `&Arc<Container>`，捕获 Config 并弱持有容器；Provider 创建 singleton 管理器，Contract 使用 bind -> 当前 driver。
- 管理器七入口与动态委托四入口的参数、返回、默认、错误、生命周期和对应测试记在原 JSON。`driver(None)`/空字符串选默认，未配置默认 file；extend 的显式类型回调接收实际容器，可解析其他驱动；循环依赖明确报错。clear 不删除 creator，既有 Arc 继续有效；并发仅创建一次，回调、析构不持注册表锁。
- set_container 只换容器，保留最初配置、驱动与 creator，符合 Manager 原机制。新 file 驱动使用所选容器 Paths；过往 file 驱动保留原 Paths。弱持有的应用释放后 get_container 明确返回 Unavailable。
- HTTP 每个未排除请求重新解析实际 MaintenanceMode Contract，支持热切换默认、清空实例与用户显式替换；公开中间件 new(mode,except) 仍保留显式固定服务用法。命令、调度器和 Worker 继续使用 Application.maintenance_mode 同一正式 Contract，不增加旁路。
- cache 后端现在保留管理器与具名 store，而不是永久持有第一次解析的 Repository；每次操作重新解析，因此 purge 后能看到替换。写入使用仓储默认 TTL（初始 3600 秒、None 永久），与固定基线 put 默认行为一致，不再无条件 forever。store 未指定时按捕获配置的 cache.default 选择，set_container 不偷换配置来源。
- Rust 差异已明确测试：data 返回 Option<Map>，无状态为 None，激活空载荷为 Some(empty)；PHP array 驱动 inactive 为 []。Rust async 返回 Result，把后端和拒绝写入作为错误传递，调用方不能把错误当成非维护状态。驱动自定义接收显式容器和捕获值，不需要 PHP Closure 的隐式 this 重绑定。该适配实际验证重入创建器、循环错误及并发，而非以静态语言为由跳过入口。
- 应用/生成骨架均复用已有默认 Provider 装配，不需要额外样例 Provider 或预置安装资源。三语配置文档增加正式门面、驱动、容器切换、有效期与进程边界。

已保留验收证据（`target/tmp/facade-alignment/maintenance/`）：

- `third.log`：7 项 Cache 公开门面回归、3 项初始维护门面及 4 项既有维护状态通过，证明提取注册表未改 Cache 调用行为。
- `framework-final.log`：初始维护 3 项、状态 4 项及真实 Artisan 调度器 2 项通过；普通任务跳过维护、允许维护任务执行、应用隔离和分钟内维护状态记忆均实际运行。
- `facade-worker.log`：5 项维护专项全部通过，含独立 Redis 8.10.3 的 Artisan queue:work --once --sleep=0 --timeout=0，验证阻止预约、--force 处理及 up 后恢复；任务处理器从 MaintenanceMode 门面读取真实状态。
- `http-artisan-redis.log`：根应用 3 项真实 HTTP/Artisan 测试通过，含独立 Redis 支持的两个 HTTP 实例共享维护状态及预渲染。临时 Redis 容器已清理，未动数据库或数据卷。
- `docs-resumed.log`：三语内容检查与文档构建通过；`browser.log`：三语桌面／移动端维护章节共 6 次浏览器检查通过。
- `first.log`（测试调用了错误的既有 API）和 `second.log`（暴露旧 HTTP 驱动持有问题）保留，修正后不覆盖。`clippy.log` 在全工作区检查末尾遭磁盘停线，不计通过；`docs.log` 因同一停线未启动，不计通过。
- `cleanup.log`：守卫持有全部 Cargo 锁后清理项目编译/文档产物，释放 71,288,389,632 字节，清后可用 102,876,880,896 字节；日志、源码、未提交改动、下载依赖、数据库和卷保留。先前停止尝试遇到 Operation not permitted，随后确认无活跃 Cargo 后才重试清理，未无锁删除。

`resumed.log`：清理后实际重新编译，维护专项 5 项（含新增构造器、显式容器设置、无效默认配置与真实 Redis Worker）、状态 4 项、调度器 2 项再次通过。`clippy-resumed.log`：随后全工作区 all-targets 严格 Clippy 通过，完成最后的惰性错误构造修正；不把先前中断计通过。通用 Facade 基类测试 API 尚未全量裁决，因此本门面仍保持 partial；其余七个缺失核心门面仍在原清单，不因此变更整体完成率。

### 本地交付复核（2026-11-24）

- Cache 批次：`2312f8cfb988d6ce066829df28f3089bc73238ce`。
- MaintenanceMode 批次：`11bad74faea68f4b0ac9fc77c91cbb9ff2fac758`。提交后复查剔除了三语配置页混入的并行 EnvironmentFile 章节，原文留在工作区，随后由所属功能的独立提交 `8e881a75f` 收口；没有将环境文件代码夹带进本批。未发布的临时维护提交 `5938b1b69` 已被上述最终提交替代。
- `maintenance/docs-scoped.log` 与 `maintenance/browser-scoped.log` 是剔除并行段落后的文档构建和三语桌面／移动端 6 次页面复核，均通过。维护批次的所有代码与这份冻结验收源码逐字一致；后续新环境文档不属于维护批次。
- `maintenance/clippy-combination.log`：以已提交 `8e881a75fe0de35375b8c244615174e257d25f82` 组合两批门面改动及环境文件功能，全工作区 `cargo clippy --workspace --all-targets -- -D warnings` 通过。既有维护／HTTP／Worker 行为证据继续按各自实际源码和日志范围使用，不把静态组合检查当成额外行为验收。
- 均为本地提交，未推送。当前共有 39 个已提供入口的核心门面；新增 MaintenanceMode 不能直接算完整对齐。其余七个缺失入口是 Blade、Cloud、Date、Exceptions、File、ParallelTesting、Vite；原有门面、Facade 基类、Sms／扩展门面的剩余公开契约继续在本对账核对，未达全量闭合条件，不修改核心阶段完成率。


### Facade 基类：解析回调、应用切换与 Fake 识别（2026-11-24）

直接补已授权剩余缺口，沿用原对账。固定 Facade.php 的 resolved / getFacadeApplication / setFacadeApplication / isFake 已逐项映射；检查了真实缓存／容器行为与 Fake 标记，未将 Http 或 Process 注册桩误判为 Fake。旧版 framework/src 内没有当前 Facade 类型体系可移植；复用现有 Container.after_resolving、声明类型投影、Tokio 作用域与各服务已有 Fake 状态，不引入依赖或平行容器。

- `resolved` 用实际服务 Arc 和当前容器回调，支持保存准确服务句柄、无尺寸 Contract、同步／异步解析。未解析时不提前创建；已解析即时执行；即时失败不注册后续钩子；异步失败不缓存坏根，重试后可正常缓存。回调不持门面锁，错误不吞掉。
- 发现 App 原有 `resolved(key)` 查询遮蔽基类回调：改为 `App::is_resolved(key)`，底层 Container.resolved(key) 保留，`App::resolved(callback)` 恢复正式入口。现有 App 实测消费者和三语文档同步更新，无旧重载别名。
- App 构造完成后使用现有弱入口解析一次，发布已经构造的应用解析状态；没有将强应用引用保存回容器。此前真实 App::resolved 的即时回调未触发，现已补齐，并实际验证应用释放后容器弱引用失效。
- `get_facade_application` 返回当前 App 门面根，作用域外为 None。setter 采用 `set_facade_application(Some(&app), future).await`，临时 None 可隐藏应用；同应用嵌套共享执行缓存，不同应用与并发任务隔离。完成、取消、panic 展开恢复外层，关闭新作用域。该具体 Rust 适配避免 PHP 静态全局变量影响并发请求，没有把“静态语言不支持”当作省略入口的理由。
- `is_fake` 只读取已经缓存的根，未缓存／无上下文为 false，不触发服务构造。Mail、Notification、Event 对接真实 Fake 状态，含临时 Event fake；HTTP、Process 桩和普通记录 spy 不等于 Fake 标记。原生自定义门面可以覆盖类型安全的 is_fake_root；未添加另一个全局 Fake 标记表。
- 常规 HTTP／Artisan／Worker 不需要开发者手工包作用域；公开 setter 供嵌入／测试和显式新任务使用。直接门面读取、容器修改和现有默认请求状态继续共用原运行链路。

实际证据保存在 `target/tmp/facade-alignment/base/`：

- `oracle-final.log`：PHP 8.5.12 容器加载固定真实 Facade.php 与 Fake 接口，最小容器双件只驱动这些基类分支；得到解析前空、即时 [first1,second1]、重解析 [first1,second1,first2,second2]，Fake [false,false,true,false]。这是基类行为对照，不宣称验收了整个上游容器或 Mockery。容器无网络、只读挂载，执行后已自动移除。
- `lifecycle-final.log`：8 项公开基类消费者、2 项原有 spy（含 HTTP/Artisan）、2 项 App 消费者通过；`delivery.log` 在最后 getter 调整后重复受影响测试并完成全工作区严格 Clippy。回调、同服务 Arc、真实 Config Contract、App 回调、异步错误重试、嵌套／并发／取消／panic 恢复、Fake 类型区别及弱所有权均覆盖。
- `combination-fixed.log`：新基类与 Cache／MaintenanceMode、App、现有 spy 组合回归通过；无 Redis 变量的条件用例不计实际 Redis。`redis-worker.log`：独立 Redis 8 下重新运行基类、spy、cache_memo 与维护专项，实际 Artisan Worker 的请求／任务生命周期、维护阻止／force／恢复均通过；临时容器已删除，无数据库／数据卷操作。
- `docs.log` 与 `browser.log`：三语文档内容／构建通过，三语桌面／移动端新增章节共 6 次浏览器检查通过。失败的 first / fourth 日志为测试调用既有 API 的错误，combination 日志暴露 App 即时回调缺口，均保留，不覆盖为成功。

本批尚不能关闭 Facade：原 JSON 明确保留 partialMock／shouldReceive／expects、专用 spy、defaultAliases 以及剩余缓存／转发契约。其余七个缺失门面和全部已有门面仍按原范围继续实施，不因本批通过而结束整体任务。

### File 本地文件服务第一批

- 新增 `File` 门面，`files` 名称与类型别名解析同一个应用单例，独立于 Storage 磁盘根目录。Facade 宏生成的实际转发纳入测试，不能只看普通 `fn`。
- 二进制读写、共享/独占锁、先取锁再截断、并发追加、原子替换（跟随目标链接/显式权限）、删除全部尝试、MD5/SHA/xxh128、权限、路径、目录深度/隐藏/VCS过滤、链接、基础glob、惰性行、JSON默认/严格及应用级类型宏均有公开消费者。
- `FileError` 接入 HTTP 正常异常路径。缺本地文件不是 HTTP404；通过实际 HTTP Kernel 的 JSON500 和 Artisan Console Kernel 消费者验证。没有把测试Kernel称作外部HTTP监听/独立进程Worker。
- 已复核旧版文件系统：受磁盘根目录限制的 Storage 不能代替本地File。MIME识别直接复用现有 UploadedFile 内容识别，不复制一套嗅探器。
- 成熟组件采用 [glob0.3.4](https://docs.rs/glob/0.3.4/glob/)、[pathdiff0.2.3](https://docs.rs/pathdiff/0.2.3/pathdiff/)；两项无默认额外依赖。锁使用 [std File锁](https://doc.rust-lang.org/std/fs/struct.File.html)，Windows锁定追加使用写句柄取锁后seek，避免仅追加句柄不支持锁。
- 固定 PHP8.5.12 容器直接加载固定 Filesystem/两个trait；实际运行6摘要+大写算法、chmod零值/显式模式、非法JSON、删除失败后继续、SplFileObject行尾/空文件。日志 `target/tmp/facade-alignment/file/oracle.log`；容器自动删除，所有可写数据在项目target/tmp。
- 本批11项File专项、基类/spy组合与严格Clippy结果以 `file/` 实际日志为准。三语文件存储页同步新增本地File章节并单列当前MIME、JSON/glob标志与平台限制。
- File仍为 partial。逐入口结果和未实现/待验收项在同一JSON，未提前关闭File或46门面总账；继续处理原任务剩余缺口。

### Blade 编译器门面第一批

- `blade.compiler` 名称及 BladeCompiler 类型别名复用 View 工厂中的同一个 Arc；没有添加独立编译器。编译器引擎由持有值改为持有 Arc，Factory/Engine 的现有公开访问方式保持。
- `compile_string` 返回可复用原生指令对象，`Blade::render` 带入真实Factory上下文；路径选择、正常缓存发布/失效、指令/条件、双重编码及类型格式化直接转发已有能力。实际 View 渲染器消费同一批设置。
- `get_custom_directives` 返回实际类型回调映射；上游 `if` 另生成的四种指令回调尚未在此映射呈现，已登记明确差异。编译对象快照与上游运行时静态回调查找的差异也未略过。
- 固定 PHP 容器实际加载原 BladeCompiler、Compiler、Concerns 和框架辅助类，验证 stripParentheses 的字节截断、if 生成指令与紧邻字母的结束标签边界；原始日志 `blade/oracle.log`。最初不合法模板 `yes@endallowed` 在固定PHP也不会编译结束标签，测试已改为合法HTML边界，没有改解析器规避失败。
- 原生字节截断用例包含未闭合UTF8输入，结果与PHP十六进制一致。旧版使用独立视图服务，没有当前BladeCompiler/Facade；复用本版已验收的指令树、渲染上下文及缓存机制。
- 本批5项门面专项，与既有 Blade声明、Finder、真实HTTP应用和原生值消费者组合验收；严格workspace Clippy和三语文档证据在 `target/tmp/facade-alignment/blade/`。该记录不替代JSON中的剩余项，Blade仍为 partial，整体任务继续。

### Exceptions 实际契约接线第一批

- 新增 Exceptions 直达报告、资格/重试判断、HTTP/Console渲染和当前处理器读取。访问名称是实际 ExceptionHandler 类型契约，没有旁路处理器。
- 发现并修复：ApplicationState 持有固定具体 Handler，HTTP/显式报告/Artisan绕过替换后的容器实例。移除固定字段，按实际契约解析；HTTP请求只保留容器弱引用，在错误边界重新解析，因此请求内部 swap 后抛错也生效。
- 为契约增加默认 `render_http` 扩展钩子，现有具体 Handler 覆盖它继续使用原请求日志上下文、验证重定向与HTML/JSON响应。现有自定义 trait 实现无需补写方法；缺失绑定返回错误，不静默退回最初实例。
- 新增3项门面/真实Kernel/Artisan契约替换测试，回归5项异常渲染、7项异常报告（含真实TCP HTTP、并发配额和Artisan），全部15项通过。Worker原有路径已解析此契约，仅源码核实，没有把它算成新增fake验收。
- 仍为 partial：fake必须保留可供类型/谓词断言的真实异常，不能用借用错误的字符串快照冒充；全部配置转发和 shouldReport 与限流资格差异继续登记在同一JSON，下一批继续补齐。

### Exceptions 运行期配置与报告状态

- 原处理器支持运行期配置：reportable/renderable、忽略/恢复、重试抑制、dontFlash、日志级别、上下文、响应/JSON策略、重复报告抑制及带类型的限流策略。既有启动配置保留，不新建处理器或重置已报告实例。
- 配置读取使用独立快照，实际用户回调与异步日志运行时不持有配置锁；写入先保留原快照，旧回调在解锁后销毁。实际验证报告回调中注册新规则，不发生重入死锁。
- 纠正初步实现的概念误判：固定 Handler::isReporting 是正在报告的同一异常实例比较，不是资格查询。PHP原类经Reflection仅初始化该属性的实跑结果为 `[false,true,false,false]`；Rust用处理器身份+异常身份的任务局部作用域，验证回调期间同一/不同异常、报告结束、回调panic后恢复。
- shouldReport 同步对齐为纯报告资格；之前会消耗采样/限流配额，现移动至实际报告阶段。验证重复查询不调用策略，真实报告仅调用一次；原真实TCP并发限流与采样回归保持通过。
- 门面拥有所有权的错误采用 SharedException 保存真实对象，已有SharedException不二次包装；重复抑制和类型回调保留身份。未以字符串快照替代真实错误。
- `exceptions-config/test-fifth.log`：当前门面10项+既有报告7项+渲染5项，共22项通过。原first日志的编译失败为测试调用既有API写法错误；初始eligibility版isReporting未提交，已按固定源码纠正，相关失败与中间成功日志完整保留，最终以本节行为和交付日志为准。
- 自定义处理器不支持配置或当前报告查询时显式返回错误，不提供空操作或默认true来冒充支持。fake/原始异常所有权传播、map、context getters、ReportableHandler链式停止及全局throttle回调仍未闭合，继续原任务。

### ReportableHandler 与全局报告回调

- `Exceptions::reportable` 返回已注册的正式 ReportableHandler；`.stop()` 在回调执行后阻止后续报告器与默认日志，注册后修改或克隆句柄均作用于同一标志。`reportable_all` 支持所有异常，开发者不必绕行到底层配置。
- 正式类型提供 `new`（所有异常）、`for_type`（具名Rust类型）、`handles`、`invoke` 和 `stop`。直接调用不匹配类型时返回类型错误；`ReportDecision::Stop` 对应上游回调的 false。类型选择可检查原错误及transport源链，不假造Rust类继承。
- 继续复用现有配置的报告器有序列表。启动配置 `report` 仍返回配置对象以兼容原链；新增 `reportable` 可取得句柄，未另起一套分派或状态系统。
- `reportable/test-second.log`：ReportableHandler3项+Exceptions10项+既有报告/渲染12项，共25项通过；PHP固定原类真实执行确认匹配、错型、前后停止、回调调用次数和false停止，日志 `reportable/oracle.log`。全workspace严格Clippy、三语构建与6项浏览器检查通过，日志同目录。
- 固定参考目录是源码档案，不是独立Git仓库；不能用它的 `git rev-parse` 结果判定上游版本（该命令会落到父OpenRust仓库）。已重新联网逐字核对 Facade、Filesystem、BladeCompiler、异常Handler与ReportableHandler五份源码，均与固定1449bf提交的官方raw文件相等；URL及SHA256证据为 `target/tmp/facade-alignment/reference-verified.json`。
- ReportableHandler调用/匹配/停止的当前原生路径已完成；Exceptions整体仍有fake、映射、完整上下文和全局限流回调等缺口，继续执行原门面对齐任务，不以本批通过停止。

### 异常记录的源对象所有权前置

- 为实际异常fake保存可判定类型的原对象，QueueError 的不透明源载荷改用标准 Arc，错误自身可clone；源对象不要求Clone/Serde，变体与错误分类保持。提供execution/backend工厂，容器错误继续通过From/`?` 转换。同步所有实际生产者、Addon、测试和三语队列示例，队列载荷/数据库/Redis协议不改。
- `exception_ownership`两项直接验证：源为未实现Clone且包含Mutex的错误，捕获后仍指向同一对象、能看到原字段更新，最后一个持有者释放才析构；解析错误克隆继续保留同一BindingResolutionError及其类型。`error_conversions`29项与现有Exceptions10项通过。
- `queue_worker`默认目标中的外部环境提前返回不算真实驱动通过。独立MySQL8.4与Redis8.10.3夹具强制设置对应要求变量，分别完整实跑真实Artisan Worker的重试/失败/上下文目标（64.20s/73.29s），全部通过；SQLite进程消费者亦实际通过。SQL Server/RedisCluster本批未提供环境，不计入实际驱动结果。两个临时容器均已清理，保留数据卷和既有数据库。
- 额外构建生成应用时发现此前遗漏：`http.blade.html` 的 @rust 块仍有 currentLocale/getFacadeRoot。改为Rust原生current_locale和正式View::render；同步队列原生渲染任务也改走正式门面，不保留get_root绕行。普通Blade表达式语法不动。
- 独立生成项目初次直接编译未装配workspace补丁，暴露pcre2/SQLx缺方法；这是夹具准备方式错误，不记通过，也未改第三方源码掩盖。沿用既有blade_native_acceptance.py的补丁与锁文件装配后，实构建/部署/self-test/三语HTTP均通过，原生队列任务与多应用/并发断言亦通过。临时部署执行文件直接删除，实测额外释放145,022,976字节。
- 记录在 `target/tmp/facade-alignment/error-ownership/`：test-second.log、worker.log、native-consumer-resumed.log、clippy.log。初次全workspace检查触及32GB停线，已安全持锁清理66,105,139,200字节并重建；该中断不计通过，源码/日志/下载/数据库完整保留。源码目录与可复用增量缓存按现行规范保留。
- 已核对旧版core/error与foundation/exceptions：旧统一Error仍含Box源，旧report_shared用Arc/Weak保留实例身份。本版沿用已有SharedException及其回归，不搬旧统一错误体系，也未发现可直接替代当前Fake契约的旧实现。
- 本单元仅闭合错误共享所有权前置；异常fake实现另在途，不夹带入此提交，也不把Exceptions整体宣称完成。

### Exceptions：具类型异常替身与实际入口接线（2026-11-24，仍 partial）

- 固定上游 ExceptionHandlerFake 原类已用 PHP8.5.12 实际执行，精确根类型筛选、保留对象、忽略规则、首条错误、记录后抛错、handler/setHandler 均核对；`exception-fake/oracle.log`。旧版没有此类型，复用当前容器、Handler 和 SharedException，不增加平行日志体系。
- 新增 `Exceptions::fake/fake_with`、`ExceptionType::of/including_sources`、具类型条件/正反/数量断言、reported、原处理器解包及替换。记录保留真实共享错误和来源，条件回调不持有记录锁；重复 fake 不嵌套。fresh facade scope 未解析前 is_fake=false 与基类缓存契约一致，不误改为自动解析。
- HTTP 增加拥有 Arc<HttpError> 的 handle_http 及单独 render_http_response 钩子，真实 Next/Kernel 使用它；具体 Handler 保留日志请求上下文、验证重定向与响应钩子。替身筛选后报告，渲染仍委托原处理器且不重复日志。Artisan 报告拥有的 ConsoleError；Worker、任务中间件、隔离任务、健康检查、延迟回调与流错误保留 SharedException。应用开放 report_owned，借用 report 仍可用；只有短期借用的不透明错误无法安全长期保留，替身明确报错，不伪造快照。
- `tests-fourth.log`：7项新替身、10项门面和原报告7/渲染5，共29项通过。`workers-second.log`：强制独立MySQL8.4及Redis8环境，各通过1项真实 Artisan queue:work；验证独立队列消费失败任务，记录QueueError并保留非Clone任务源错误。未配置服务的默认早返回不计实测。临时容器和唯一测试表已清理。
- `clippy.log`：冻结工作区全workspace/all-targets严格Clippy通过。失败日志（错误目标名、测试夹具缺trait/捕获loader、fresh scope缓存断言）均保留，后续成功不覆盖。
- 仍有明确缺口：throw_on_report 目前在直接报告中以Result返回原始错误，HTTP/Artisan/Worker消费日志失败的边界尚未自动传播；withoutExceptionHandling模式、完整map/context/register和继承Fake/spy契约继续开发，不能将本批算作整个Exceptions或全部门面闭合。

- 本批三语文档构建、异常测试章节桌面/移动端6项浏览器检查、全workspace格式检查通过：`exception-fake/docs.log`、`browser.log`、`fmt.log`。增量缓存按当前规则保留，临时容器已删除，验收记录保留。

### Exceptions：重抛的原生异步传播（2026-11-24）

- 前批公开的自动传播缺口已补：词法任务局部捕获通道将原始SharedException返回受保护入口，丢弃报告所在Future，使报告后的代码不继续执行；不通过线程全局状态、字符串或日志快照模拟异常。
- `TestApp::call/get` 自动捕获并返回原始错误；新增 `try_artisan` 返回原生Result。保留原 `artisan -> TestCommand` 签名：只有显式throw模式才以SharedException原始payload panic，调用方可选fallible API避免panic。任意其他异步入口用正式 `Exceptions::catch_reported(future)` 捕获；捕获外直接report返回Err，不会无限挂起。
- `exception-propagation/tests-final.log`：16项替身目标返回通过，其中4项外部环境早返回不计实测；实际12项覆盖原有7项及HTTP/Artisan传播、嵌套恢复、取消、显式兼容panic及并发应用隔离；另原门面/报告/渲染22项通过。
- `exception-propagation/workers.log`：强制独立MySQL8.4.13、Redis8.10.3，共4项真实Artisan Worker（两后端分别正常记录与显式重抛）通过，仍可断言原任务Failure源类型/状态。测试表与临时容器已清理。没有将默认无环境的早返回算成后端通过。
- 此批闭合的是受保护原生测试调用传播；没有宣称生产HTTP可跨网络传送Rust对象，亦未将Exceptions剩余map/context/register及整个门面对齐任务计为完成。

- 本批严格workspace/all-targets Clippy复核、fmt、三语站点构建及6项浏览器检查通过（`clippy-second.log/fmt.log/docs.log/browser.log`）。失败Clippy的err().expect()已改正，原日志保留。

### Exceptions：纠正报告资格的限流语义（2026-11-24）

此前“should_report 是不消耗准入的纯查询”是错误裁决，已用固定 Handler 的实际执行纠正：shouldReport 调用 shouldntReport，后者在忽略/重复规则后执行 throttle；throttleUsing 也是追加有序回调，不是覆盖全部回调。旧日志保留，仅代表旧实现，不能继续作为该契约对齐证据。

- 固定PHP Handler真实执行：两次 shouldReport 配置必输Lottery，结果false/false且回调2次；`exception-eligibility/oracle-final.log`。早期oracle缺完整自动加载时被上游rescue吞掉错误，产生true/true/0，已用受保护throttle预检排除此假阳性，保留失败及中间日志。
- should_report 现在包含采样/限流准入；实际 report/HTTP report 删除第二次准入。被忽略/已重复错误短路不计费。替身沿用原处理器资格，采样拒绝不记录。
- `exception-eligibility/tests.log`：12项门面、既有报告7/渲染5和12项实际本地替身测试通过（默认外部4项早返回未计后端验收）。新增Limit每分钟1次验证查询会消耗配额；独立键实际报告只计费一次，第二条拒绝；Lottery查询次数、fake采样和ignore短路均通过。
- 当前三语文档已同步纠正，is_reporting 仍只查询当前报告实例，不消耗准入。完整上下文、映射、register和其他门面仍继续开发。

- 本批 workspace/all-targets严格Clippy、格式检查、三语站点构建及6项浏览器检查通过；日志归入 `exception-eligibility/`。

### Exceptions：公开上下文与注册入口（2026-11-24，仍 partial）

- 新增正式 register/build_context_for_exception/context_for_exception 转发，注册钩子的标准行为是空操作，自定义处理器可覆写；Fake均转发原处理器。不是以配置对象或get-root绕行充数。
- ExceptionContext将JSON字段与原始借用Exception分别保留。既有单参数build_context_using保持兼容，新增双参数build_context_using_with_context接收累积字段，按注册顺序覆盖。正式exception_context::<E>复用现有类型回调机制，可显式接入E::context，不假定Rust Error含动态可选方法。
- 修复SharedException包装漏掉原类型上下文字段；JSON快照剥离透明所有权包装，不添加假previous，也不让原类型context回调重复执行。真实来源链仍保留。日志处理器实际复用公开上下文构建。
- 当前默认上下文在异常字段后合并已解析默认Guard的userId，并按PHP假值规则省略0/空标识；实际HTTP确认请求结束后身份不泄漏。上游Auth::id的未解析懒认证仍待收口，因此contextForException仍partial。
- `exception-context/oracle.log` 固定PHP原类执行验证phase1->phase2、后回调看见累积值、默认userId覆盖异常字段、原异常对象引用及register空操作；`tests-second.log` 新5项通过，连同既有异常消费者、错误转换29与真实日志15，共85项实际本机测试通过（另4项无外部环境早返回不计后端验收）。真实HTTP、结构化文件日志、Fake原处理器、回调重入及透明包装仅调用一次均已验收。首次测试漏导入Facade已修正，失败日志保留。

- 本批全workspace/all-targets严格Clippy、格式、三语站点构建及上下文章节6项浏览器检查通过：`exception-context/clippy.log/fmt.log/docs.log/browser.log`。

### Exceptions：类型映射与真实 JSON 策略（2026-11-24）

- 新增正式map::<来源,目标>，同来源覆盖保持顺序、首个命中停止、不递归映射；map_optional按Rust枚举变体筛选，None保留原异常并继续；显式inner_exception适配Rust Error未定义的getInnerException，优先解包共享原实例。动态PHP类名构造在Rust用类型和构造回调表达，非法错误类型由编译器拒绝，不另造类名注册系统。
- 报告前映射，原始should_report不映射；HTTP报告与渲染分别映射，Fake记录原始报告但渲染照常映射。新增Mapped语义让映射后只按新根匹配报告/忽略/渲染规则，真实previous仍可诊断，避免误触发旧异常规则。映射到HttpError保留状态与处理路径，原生DumpAndDie不被映射。
- JSON策略原先对非HttpError用合成Application exception占位，现将should_render_json_when的第二参改为实际Exception，可检查映射后的真实业务类型。根应用/模板中只按Request判断的现有闭包保持原写法；全workspace/all-targets实际编译检查其消费者。
- `exception-map/oracle.log` 固定PHP原类确认覆盖后UnexpectedValueException、保留previous、内层异常优先和非法动态参数报错。`tests-final.log`：11项映射专项及其他异常消费者共52项实际本机测试通过（另6项无后端环境早返回不计外部验收）；覆盖回调重入、替身、HTTP状态/钩子、来源规则隔离、真实JSON参数、内层错误实际指针和可选映射。
- `exception-map/workers.log`：独立MySQL8.4.13和Redis8.10.3各1项真实Artisan queue:work通过；任务错误在原处理器映射为WorkerMapped，再进入新类型报告器，保留原始任务Failure源值42，不被未选中替身截获。临时容器与测试表已清理。旧未受影响的队列所有权/重抛证据沿用，不混算新覆盖。
- 初始HTTP映射接线漏入渲染已被专项测试发现并修复；JSON回调签名遗漏亦已修复，保留全部失败日志。该批没有把懒认证、withoutExceptionHandling、完整全局限流回调、共享Facade基类或其余门面计为闭合。

- 最终源码再次实跑两后端Worker（`workers-final.log`）通过；workspace/all-targets严格Clippy、fmt、三语构建与6项浏览器检查均通过（`clippy.log/fmt.log/docs.log/browser.log`）。临时容器已清理，保留增量缓存和全部验收证据。

### Exceptions：惰性认证上下文与能力错误（2026-11-24）

- context_for_exception变为async Result；实际HTTP错误边界借出Request执行默认Guard的惰性认证。新增context_for_exception_with_request及report_for_request，让持有可变请求的路由不用绕行root就能报告带真实身份的错误；也可沿用request.with_container借出请求后用环境门面。build_context_for_exception保持同步且不执行认证。
- 认证结果错误保留原异常继续报告；认证内部显式Fake重抛也由局部捕获转换为认证失败，不能替换原始HTTP错误。使用按应用Container标识的词法栈，避免同应用递归，又不阻止嵌套应用解析自己的身份；取消自动恢复。用户ID合并优先级和PHP假值过滤沿用前批。
- 自定义处理器缺少register/build_context/context能力时明确返回不支持错误；标准Handler显式实现空register，只有确实支持且无字段时才能返回空上下文。Fake继续转发真实结果，不以默认空值冒充支持。
- `exception-auth-context/oracle.log`：固定PHP原类验证buildContext不认证、contextForException调用Auth::id一次、认证异常被捕获且保留原对象、自定义无元数据方法实际报错。旧版无当前门面体系，继续复用现有Guard/Request上下文与共享错误，不新增认证或日志系统。
- `tests-combination.log`：14项上下文（新增9项）及既有映射/门面/报告/渲染/日志/错误转换，共105项实际本机消费者通过；另6项无外部环境早返回未计后端验收。实际HTTP与JSON文件日志验证惰性认证、显式请求及环境借出、认证错误、认证中再次报告、取消恢复、嵌套应用身份、严格自定义能力和认证内Fake重抛。初始Result字段接线遗漏已修复，失败日志保留。
- 本批不更改数据库/队列协议，复用已通过的独立MySQL8.4与Redis Worker报告路径证据；withoutExceptionHandling、全局限流回调和其余门面仍继续处理，不计全量闭合。

- 本批全workspace/all-targets严格Clippy、格式、三语站点构建和6项浏览器检查通过：`exception-auth-context/clippy.log/fmt.log/docs.log/browser.log`；保留全部验收日志，未新增临时数据库或数据卷。

### Exceptions：关闭处理模式与共享身份（2026-11-24）

- 新增TestApp::without_exception_handling、with_exception_handling、handle_exceptions、handle_validation_exceptions。保存首个原处理器，重复关闭不叠加包装；Fake保留记录并切换内部处理器。关闭后report为空、should_report=false，Fake仍记录被忽略的类型；恢复后使用原资格规则。
- 真实HTTP未处理错误返回原SharedException，404补充请求方法和去掉查询的URL，保留原HttpError来源；except只渲染、不重复报告。正式Exceptions::render返回原生Result；只有借用且无法持有的错误返回明确ExceptionOwnershipError，不伪造类型快照。原始Response兼容入口在显式关闭模式中以真实共享错误payload抛出，TestApp自动捕获。
- 修复共享错误再次包装及映射后的重复报告身份；显式downcast_chain_ref检查诊断来源（包含Mapped），最多32层，循环source不会阻塞报告。
- 固定PHP原Trait、Fake、Container实际执行通过：关闭资格、原对象、except、恢复及Fake保留；oracle仅补PSR接口声明，不将其当作完整PHP HTTP/Console部署验收。证据without-exceptions/oracle.log。
- tests-combination.log共125项返回通过，其中8项缺外部环境早返回不计，实际本机117项通过；最后所有权整理后ownership-final.log再次跑新模式12项及Fake消费者。workers.log另用独立MySQL8.4.13、Redis8.10.3各1项真实Artisan queue:work验证关闭模式记录被忽略的任务原始错误、恢复资格；测试表/容器已删除。
- workspace/all-targets严格Clippy、fmt、三语站点构建及6项桌面/移动浏览器检查通过，证据clippy-final.log/fmt.log/docs.log/browser.log。保留失败与全部成功日志。全局throttle回调、继承Facade测试接口和其他门面仍待补齐，未宣称完整闭合。

### Exceptions：全局限流策略与实际类型分组（2026-11-24）

- 固定Handler::throttleUsing的有序回调集合与首个非null结果继续复用；新增正式Exceptions::throttle_using_all和配置throttle_all，与原具类型策略混合注册。None继续、Unlimited停止、Lottery及Limit执行现有限流器；不增加平行计数器。旧版foundation/exceptions.rs有全局回调但基于统一Error及本地计数，不适配当前契约，继续使用当前RateLimiter和共享错误。
- SharedException拥有入口保留具体Rust类型名，重复共享包装保留身份及元数据；新增from_typed_arc、Exception::typed保留具体Arc和借用信息。默认Limit按实际根类型计费，映射后按目标类型；显式by键可跨类型共享。既有已擦除dyn Error无运行时类名反射，使用显式键或新构造器；无元数据的全局无键策略按限流失败放行，绝不将未知类型混为一组。三语文档明确此原生约束，未笼统认定擦除路径完全等价。
- exception-throttle/oracle.log：固定PHP原Handler实际执行全局None->具类型Unlimited、全局拒绝及首个结果停止。tests-delivery.log的7组共85项返回通过，其中8项缺后端环境早返回不计，实际本机77项；新增顺序、默认类型/不同消息、跨类型显式键、重入注册、跨应用隔离、映射目标配额及报告只计费一次。
- workers.log：独立MySQL8.4.13及Redis8.10.3各1项真实Artisan queue:work通过，原始QueueError类型和来源42保留，全局无键Limit只调用一次。初次选择器拼写错误导致0项，workers-zero-not-accepted.log保留且不计验收；修正后实际各跑1项。临时容器/测试表已清理。
- 全workspace/all-targets严格Clippy、格式、三语构建及6项桌面/移动浏览器通过：clippy.log/fmt.log/docs.log/browser.log。共享Facade测试契约、剩余门面和逐入口最终裁决继续处理，未将本批等同全部完成。

### File：编译模块加载与只执行一次（2026-11-24，仍 partial）

- 固定Filesystem::getRequire/requireOnce已实际PHP执行：具名数据、内部路径变量不被输入覆盖、getRequire每次执行并记入包含集合、失败后requireOnce仍跳过、缺文件报错。file-modules/oracle.log。旧版没有可移植的现代File加载器，复用当前FileMacro的具类型回调及files应用单例，不新增解释器或动态编译。
- 新增register_module/get_require/require_once及RequiredFile。应用注册编译后的Rust函数，实际源码路径须存在；具类型参数代替extract，回调局部状态不能被用户数据覆盖。路径canonicalize后共享包含身份，首次返回Value(T)，重复为AlreadyIncluded，与实际true返回值区分；get_require重复执行，包含标记在执行前，失败/panic/递归不重执行once。替换注册不重置包含集合。
- 注册表及包含集合应用级隔离，不照搬PHP进程全局表；并发只认领一次，其他线程可在首个回调完成前返回AlreadyIncluded。回调及旧注册析构不持注册表锁。未注册文件、缺失/删除路径和输入输出类型不符明确报错，不把原始源码文本当作执行结果。Rust源码改动需重新构建。
- tests.log：5项新模块与11项原File消费者通过，涵盖HTTP/Artisan真实Kernel共享、具类型参数与返回值、失败、panic、重入、8线程、应用隔离及Unix符号链接别名。workers-second.log另独立MySQL8.4.13、Redis8.10.3各1项真实Artisan queue:work通过，每后端2个任务完成但模块执行一次，Worker后门面仍见已包含；测试表和临时容器已清理。首轮Worker缺Application trait导入的编译失败保留，未计通过。
- 全workspace/all-targets严格Clippy通过；追加Worker夹具后相关目标严格Clippy再次通过（clippy-final.log）。fmt、三语站点构建及6项浏览器通过。未将动态解释器、Windows原生链接矩阵、所有glob/JSON/hash标志或File全部继承契约计为完成。

### File：条件回调、回退与高阶代理（2026-11-24，仍 partial）

- 固定Conditionable/HigherOrderWhenProxy经实际PHP Filesystem执行，file-condition/oracle.log：字符串0跳过、unless回调保留原false、标量0/false不误回退、null返回原服务、条件闭包一次、高阶查询捕获与文件副作用选择均核对。复用当前Collection/Stringable已有原生FnOnce条件风格，不添加动态方法解释器或外部依赖。
- 正式File及Filesystem提供when/unless、*_else、*_test及*_condition/*_capture。ConditionValue保留原始具类型参数，支持数值、布尔、字符串、Option、集合和JSON；业务对象可实现原生trait。ConditionalResult把Target与Value(T)分别表达，None回调保留原服务，Some(false/0)保留标量；高阶操作返回None仍是Value(None)，不套用回调空值回退。
- HigherOrderWhenProxy通过具类型call闭包执行实际方法；捕获查询只一次，try_capture传播真实查询错误，未捕获明确MissingCondition。门面代理保留原应用Arc，跨作用域不重新解析；直接服务保留原借用。不以get-root绕行替代公开入口，Rust后缀与联合结果的适配已写三语文档。
- tests-resumed.log：5项新条件测试+11项File+5项实际模块测试，共21项本机通过；另2项后端缺环境早返回不计。覆盖非Clone业务对象、回退实例身份、空值/标量、真假副作用、查询/操作失败、跨应用原服务以及实际HTTP首次写入/重复跳过/JSON500和Artisan读取。初次内部方法可见性、HTTP夹具闭包路径借用问题已修复，失败日志保留；tests-final.log来自未完成脚本同步的旧源，不作为新增HTTP验收，最终以resumed为准。
- workers.log：独立MySQL8.4.13和Redis8.10.3各1项真实Artisan queue:work通过，实际模块通过条件回调写入，两个任务完成、模块执行一次；测试表和临时容器已清理。全workspace/all-targets严格Clippy、fmt、三语构建和6项浏览器通过：clippy.log/fmt.log/docs.log/browser.log。
- 本轮期间32GB磁盘守卫安全持锁清理本项目编译profile及生成站点，build-guard-events.jsonl记录释放62,832,758,784字节、清理后94,699,384,832字节；源码、未提交改动、日志和依赖保留。后续重新编译及文档构建通过，当前增量缓存按规范保留。File其余JSON/glob/hash/MIME/平台与共享Facade测试契约仍未闭合。


### Vite：清单、资源标签与真实渲染接线（2026-11-24，仍 partial）

- 固定Vite.php已从官方提交核对SHA256并与本地归档逐字一致，参考记录`vite/reference.json`。旧版未发现可复用的Vite服务；复用当前URL生成器、容器、View编译器、HtmlString和现有md5。没有新增前端工具包装或平行模板引擎。
- 正式Vite门面接应用单例与HTTP/Artisan/Worker作用域配置；Provider中boot默认值供新作用域继承，nonce/属性回调/预加载记录的执行期覆盖不会泄漏到下个请求或任务。清单缓存应用独立；flush只清预加载。22个已验收公开入口逐项登记在JSON，其余仍pending。
- 实现生产清单入口、静态imports与CSS、去重/排序、SRI、nonce、属性与自定义URL回调、热更新标记、React前导、asset/content/manifest_hash；content保持原字节，清单哈希不读取hot内容。回调在锁外执行，回调错误保留真实source，支持重入。
- @vite/@viteReactRefresh走真实编译器内置指令，用户自定义同名指令优先，不污染get_custom_directives。保留已提交@error else；缓存格式递进为47。模板的Vite读取入口同步接正式服务，非同名占位。
- 新增HTTP域名反例发现同步模板线程丢失请求URL：现在渲染只继承具应用身份的只读URL快照，不复制可变请求/会话。真实HTTP验证配置域名与当前请求域名、并发不同域名和nonce；Artisan退回应用域名。保留其他请求功能与协作者修改。
- 固定PHP原类+真实Collections/Macroable等及已声明Symfony polyfill实际执行（oracle-final.json），比较全部HTML、预加载、哈希、热模式、React前导及动态URL9次调用顺序；只用夹具提供public_path/asset，不冒充完整PHP部署。前两次缺辅助依赖失败保留，不计通过。
- 本机回归复用已通过Blade App12、表达式5、Facade5、查询6、请求配置9、翻译7；新增及受影响消费者最终日志`tests-final-origin.log`包括Vite12（其中1项无浏览器环境早返回不计）、Blade语句6、Request11、ResponseSession8、原生视图6。最终Clippy/fmt见`clippy-final-origin.log`/`fmt-final.log`，失败和修复日志完整保留。
- `browser.log`：实际Vite8.3.7构建+Rust HTTP Kernel生成HTML+Chrome读取JS/CSS；开发服务器WebSocket连接和CSS热更新通过。HTML由本地夹具服务器提供，不计Rust生产监听/部署；React仅原前导字符串等价，未计真实React应用HMR。
- `workers-final.log`：独立MySQL8.4.13与Redis8.10.3各1项实际Artisan queue:work，每后端消费2任务，逐任务nonce/属性/预加载隔离通过。临时容器与测试表已清理。最后仅将MutexGuard显式词法块化，不改变发送或Worker逻辑；工作区全目标严格Clippy验证该测试。
- 三语Vite页与前端概览链接已更新，`docs-verified.log`构建和`docs-browser.log`三语桌面/移动6项检查通过。早期被旧父树覆盖的文档构建不作为最终证据。
- 32GB停线时守卫释放62,557,171,712字节，恢复94,595,948,544字节；重建后保留热缓存、全部源码与日志。prefetch、正式fonts接线、宏与共享Facade测试能力继续开发，不将本批或已有@fonts计为完整Vite闭合。
