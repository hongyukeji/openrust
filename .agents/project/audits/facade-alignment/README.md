# 门面公开契约对账

## 范围与依据

用户授权逐个核对全部门面，补齐缺口、实际验收后本地提交；不推送。此记录不能替代各所属模块的验收，也不把类型存在、底层方法存在或旧测试通过当成门面闭合。

基线：[framework v13.40.0](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Facades)。固定源码有一个基类及 46 个具体门面。[逐类型公开入口](upstream-entries.json)保存需要逐项检查的声明入口；当前 `pending` 明确表示尚未完成逐项签名、默认值、状态、装配和行为裁决。注解不包含所有动态委托入口，须继续检查实际目标、继承和 trait，不能把清单当成完整行为证明。

对照固定 Goravel 的应用级服务装配；公开职责与行为仍以固定基线为准。已检查 `openrust-v0.x` 对应响应、Session 和文件存储：旧版没有当前 Facade 类型体系，文件存储是受磁盘边界约束的存储接口，不能拿来冒充本地 `File` 服务。当前已具备的 HTTP 响应构造器、流、视图、重定向器和 Session Store 优先复用，不建立平行实现。

## 全类型复核队列

“已有入口”仅表示已确认公开导入；下面没有任何一行因此被计为完整对齐。

| 门面 | 当前公开入口／发现 | 状态与待办 |
|---|---|---|
| Facade | `support::facades::Facade` | 核对 resolved、替身、缓存清除、应用上下文、默认别名及异步生命周期；现有 spy/swap 不等于全部测试 API |
| App | `support::facades::App` | 本批补容器注册/解析、别名/钩子/作用域、标签/工厂/调用与部分路径/环境入口；Application 生命周期、其余路径覆盖和宏等仍待核 |
| Artisan | `support::facades::Artisan` | 有 call/queue/command/add；继续核对 Kernel 的 all/output/resolve/启动回调及参数/退出状态 |
| Auth | `support::facades::Auth` | 逐项核对 Manager、默认 Guard、用户 Provider、认证状态与 fake/test 入口 |
| Blade | 未开放对应门面 | 复用现有 BladeCompiler，补实际编译器绑定和公开转发，不以 View 工厂替代 |
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
| Exceptions | 未开放对应门面 | 需接现有 Handler/配置/报告器及测试替身，不能添加脱离实际 HTTP 生命周期的处理器 |
| File | 未开放对应门面 | 缺本地 Filesystem 服务，与 Storage 磁盘分开；核对读取/锁/写入/替换/目录/权限/链接/宏，PHP require 需明确语言适配 |
| Gate | `support::facades::Gate` | 核对策略、用户/资源、前后钩子、授权响应和事件 |
| Hash | `support::facades::Hash` | 核对管理器/驱动、校验、重哈希、配置校验与未知算法 |
| Http | `support::facades::Http` | 核对 Factory/PendingRequest 委托、fake/sequence、禁止逸出、批次/并发和断言 |
| Image | `support::facades::Image` | 核对图像工厂、驱动、解码、限制与公开委托 |
| Lang | `support::facades::Lang` | 核对 Translator/Loader、路径覆盖、namespace、消息选取、语言/回退与缺失回调 |
| Log | `support::facades::Log` | 核对渠道/堆栈、共享/请求上下文、级别、事件及 PSR 对应接口 |
| Mail | `support::facades::Mail` | 核对 Manager/Mailer/PendingMail 的入口、队列、附件、失败与测试替身 |
| MaintenanceMode | 只有底层契约和驱动 | 缺正式门面及 Manager 公开契约；必须复用 HTTP/调度器/命令现有实际维护状态 |
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
