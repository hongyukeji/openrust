# Telescope 诊断扩展

2026-11-21：O03 原生诊断链路已实现并完成下列定向验收。该结果不代表完整 Vue 面板、所有上游公开 Watcher 类或最终全扩展组合已经等价；尚存差异列于末节。保留共享工作区其他扩展及核心改动，本轮未暂存、提交或推送。

## 固定来源与复用

- 官方 Laravel Telescope 固定提交 `39a8ea8ad79915df6c3ac9b4e72659ade808eba3`，源码位于项目 `tmp/openrust-ecosystem-20261121/upstream/laravel-telescope`。核对 Entry、Repository、18类Watcher、配置、迁移、路由、安装器、命令与输出格式，保留MIT许可证。
- 本轮再次请求官方GitHub网页与raw入口，web工具没有返回可读正文；使用已取得的固定官方源码归档核对，不宣称核实实时热门榜或Star排名。缓存问题另通过官方raw入口取得 Laravel Framework 固定提交 `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 `Illuminate/Cache/DatabaseStore.php`，确认 `forever` 使用315360000秒。
- `v0.x` 固定参考 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 无独立Telescope包。内部复用当前数据库Query Builder／迁移、应用作用域、原生defer、日志观察、事件分发、队列／Worker、缓存、Redis、HTTP路由／会话／防伪与SourceInstallation；未另造观测总线、任务执行器、RESP解析器或通用安装框架。

## 已实现与语言适配

- 18种EntryType、UUIDv7、主机名、标签、已解析认证身份快照、异常family与occurrences、对象／数组content、公开存储契约及自定义仓储覆盖。三张原生迁移表；UUID前缀查找、序号游标、tag／batch／family过滤、监控、合并更新、异常合并、prune与clear。
- 原生HTTP／Artisan／Worker执行作用域隔离；共享回调配置，defer刷新；filters／batch filters／tags／暂停恢复／监控标签／脱敏。抑制守卫支持取消恢复；不使用进程全局当前用户。硬终止不保证刷新。
- RequestReceived在中间件前加载记录状态；RequestHandled记录真实元数据、耗时和已解码输入，绝不读取未消费正文。请求／响应参数支持点路径脱敏。
- LogManager只观察实际格式化后的消息；异常使用原生错误快照，未知Rust源码位置保持null并使用独立family，不伪造堆栈。
- JobQueueing在实际入队前、唯一锁准入后触发；批次bulk同样触发。整个Worker attempt隔离于queue:work命令，处理前与终态事件共享作用域。已有Horizon投递后事件保留。缺失EntryUpdate通过原生队列处理，最多重试三次。
- 原生邮件记录真实收件地址、原始mailable、排队标识、HTML／文本与MIME；同步／排队通知保留实际通知类型、notifiable及响应。通知复用PreparedMail时显式传入queued状态。
- Eloquent在原生typed监听后发出真实模型生命周期快照，保留事务上下文、取消及事件抑制；hydration按模型累计。应用事件通过显式 `Event::properties()` 提供可序列化诊断；不反射任意Rust字段。
- 调度使用after-hooks结束位置的ScheduledTaskCompleted，避免后台启动误计完成；保留命令、表达式、实际时区和非append输出。回调失败但after-hooks执行时仍记录，schedule:run原生退出码保持0。
- 视图复用Factory composer，记录名称、文件路径与数据键；不持有Factory强引用形成循环。Gate记录实际结果与可表示参数。Redis通过原生redis::Cmd迭代参数，在成功命令后记录耗时和连接，无启动联网副作用。
- `support::dump`仅格式化一次并在原生事件被捕获时抑制标准输出；页面启用4秒缓存开关或always配置。IncomingDumpEntry关联首个Request／Job／Command入口。同步dbg／println／LazyCollection dump不被拦截。
- 授权Web API、Nonce CSP诊断面板、自定义path／domain、会话防伪、监控标签及暂停／恢复；页面用textContent呈现不可信存储内容。邮件预览使用上游sandbox策略，下载固定MIME类型并以解析后UUID构造文件名。`Sec-Fetch-Site: same-origin`按框架默认防伪机制允许；缺可信metadata及token的写请求拒绝419。
- 八条Artisan命令。list按类型列出关键字段；show支持latest、前缀、batch类型筛选、JSON与Unicode安全截断；`--full`实际取消内容截断。安装器复用单源码锁登记编译型配置／Provider，重复执行保留定制。

## 验收环境与证据

所有Cargo使用Rust1.94.0，原生增量与并发。独占快照 `tmp/openrust-extensions-20261121/source`、target `tmp/openrust-extensions-20261121/target`、TMPDIR `tmp/openrust-extensions-20261121/runtime`。不在活跃工作树编译，不在构建中同步快照。

| 实际验证 | 结果／证据（相对项目tmp/openrust-extensions-20261121） |
|---|---|
| 包默认测试 | `telescope-current-tests.log`：custom_storage1、entries3、installation1、provider15、storage6，共26项通过；Redis1明确ignored，单独实机运行 |
| PostgreSQL18.7 | `telescope-postgres.log`：storage6通过；`telescope-worker-pgsql.log`：原生HTTP派送→database Worker→批次完成／失败／日志与作用域通过 |
| MySQL8.4.13 | `telescope-mysql.log`：storage6通过；`telescope-worker-mysql.log`：同一原生Worker行为通过 |
| Redis8.10.3 | `telescope-redis.log`：隔离Redis8原生命令参数／连接／耗时1项、Redis缓存生命周期1项通过；复用Horizon Redis夹具，不复制造轮子 |
| database缓存三库 | `telescope-cache-pgsql.log`、`telescope-cache-mysql.log`、`telescope-cache-sqlite.log`：永久值、读写、暂停恢复、过期、删除及两owner互斥锁均通过；file同一函数在默认包测试通过 |
| 核心回归 | `telescope-core-regression.log`：console_observation1、eloquent_events2、events8、http_observation4、mail_notifications31、queue_worker6、scheduling17；子进程夹具单独运行1。数据库／外部服务条件缺失导致的提前返回不计为该服务验收；外部数据库采用上面的REQUIRE隔离夹具结果 |
| 真实源码安装 | `telescope-install.log`：原生root binary安装、模块与Provider只登记一次、重复安装／发布保留定制通过 |
| 原生HTTP／Chrome | `telescope-browser.log`：桌面1440×900、手机375×812授权、CSRF、XSS、详情、监控／暂停恢复、布局通过；自定义路径／域名、disabled不写入通过，子进程已收尾 |
| 根应用配置与骨架 | `telescope-root-tests.log`：默认骨架2项＋配置发布／覆盖保留／无效环境不穿透缓存1项通过 |
| 已认证身份与出站HTTP | 包provider实际验证已解析身份无二次认证、无密码与跨请求泄漏；真实本地TCP HTTP验证JSON／form与query合并、双向脱敏、网络耗时及原生SELECT诊断；三库分别见`telescope-client-pgsql.log`、`telescope-client-mysql.log`与默认包日志 |
| 严格lint／Rustdoc | `telescope-clippy.log`：framework＋telescope全部targets通过；`telescope-workspace-clippy.log`：全workspace全部targets、`-D warnings`通过，修正Pulse测试未检查读取量的一行lint；`telescope-rustdoc.log`：Telescope `RUSTDOCFLAGS=-D warnings --no-deps`通过；第三方patch依赖既有警告不冒充本包警告 |
| 三语文档 | `telescope-docs-content.log`及`--prefix docs run build`通过；`telescope-docs-browser.log`12组通用，`telescope-docs-scope.log`6组全导航通过，支持状态页数量由英文正文声明计算，三语一致（当前20篇，含并行Pulse正文） |

常用复现入口：

```sh
cargo +1.94.0 test --locked --offline --manifest-path tmp/openrust-extensions-20261121/source/Cargo.toml -p openrust-telescope --tests
TELESCOPE_TEST_DRIVER=pgsql python3 -B tests/postgres_acceptance.py -- <storage-test-binary> --nocapture
TELESCOPE_TEST_DRIVER=mysql python3 -B tests/mysql_acceptance.py -- <storage-test-binary> --nocapture
python3 -B vendor/openrust/telescope/tests/install_acceptance.py <root-binary>
python3 -B vendor/openrust/telescope/tests/http_acceptance.py <telescope-acceptance-binary>
```

运行前设置上述项目内TMPDIR、独占CARGO_TARGET_DIR；HTTP夹具设置已安装的DOCS_PLAYWRIGHT_MODULE和DOCS_BROWSER_PATH。Provider的三库函数每次用新夹具数据库精确筛选，不让多TestApp争用固定表。Redis明确要求TELESCOPE_TEST_REDIS_URL，调用ignored函数时必须通过隔离Redis8夹具。

## 本轮发现的核心缓存缺陷

database缓存此前把引擎毫秒时间戳及i64::MAX永久哨兵直接写入Laravel标准integer expiration列。SQLite容纳了这些值，但PG18报integer parameter错误、MySQL8拒绝写入。修复在DatabaseBackend边界以Unix秒持久化、读取恢复引擎毫秒；永久记录按固定Laravel使用十年，亚秒租约向上取整，防止立即过期。公开API仍按原TTL单位使用，三语缓存正文已明确数据库精度与十年边界。新验收使用标准integer列，不能通过改成bigInteger掩盖错误。现有毫秒格式缓存不是新存储格式的兼容目标，应用升级时应按自身维护窗口清除旧缓存。

## 完整上游对齐仍需单列的边界

- 面板为原生安全诊断UI，未使用上游Vue前端或avatar；列表、过滤、详情、监控标签、暂停恢复均已实际验收。18类公开Watcher已在下节补齐独立注册，内部共用格式化帮助器，不再用私有总监听器替代公开结构。
- Gate 已覆盖授权入口及同应用嵌套授权。Query 已覆盖核心数据库、分页、模型写入、直接关系读取、关联写入、模型与集合关系加载、多对多中间表和常规 Schema。普通异步包装器、关系变体和驱动特有的 Schema 操作仍待核对。源码重映射的已验范围与部署条件见[发布构建的源码路径重映射](../extension-parity.md#发布构建的源码路径重映射)。EventWatcher 默认忽略框架事件，应用内共享的 ignoreFrameworkEvents 开关已在下节补齐。
- 服务端已捕获已知缓冲正文及实际处理器／控制器、已解析路由中间件类型；不透明Body／流仍不读取。HTTP 客户端记录构建器提供的 multipart 元数据，来源边界见下文。View的闭包composer名称为null。通知Mail响应为实际收件人快照；不冒充PHP SentMessage对象反射。
- 命令以普通文本表格及通用字段详情输出，尚未逐项复现上游颜色、相对时间、分类详情布局／batch统计。`--full`、JSON与查询语义已经验证。prune仅接受非负整数小时。
- 正式托管服务、其他操作系统和最终全扩展组合／完整API导出未由本批通过结果覆盖。

缓存保留供后续验收复用；未到32GB停线时不清热target。夹具只回收本批标签容器和子进程，不删除任何数据卷。后续开发应按本节真实差异接续，不能把包测试通过等同完整上游移植。


## 公开 Watcher 与缓冲响应补齐（2026-11-21）

复查固定上游 Watcher／EventWatcher／RequestWatcher／ClientRequestWatcher 源码，重新通过官方 raw 取得 Watcher.php，证据为项目 tmp/openrust-telescope-followup-20261121/upstream-watcher.php。新增公开 Watcher trait 和全部18类同名结构，持有公开 options，new(options)／register(app).await? 可由应用 Provider 独立使用；包自身也使用这些入口登记监听器，非仅添加空壳导出。只订阅各自原生事件，日志和视图复用既有 LogManager／Factory 接口。生命周期准入独立登记一次。options.enabled=false 不登记；手动登记前须关闭对应默认配置避免重复采集。过滤模式接受上游的字符串或数组。

Telescope.ignoreFrameworkEvents 使用应用共享 Arc<AtomicBool>，默认 true，提供与PHP静态字段职责对应的公开设置；跨请求共享但跨应用隔离，不引入进程全局状态。EventWatcher 的手动 ignore 即使开启框架事件仍生效。明确序列化事件字段继续使用原生 Event::properties，无任意Rust字段反射。

核心 Response::getContent／setContent 保存已知缓冲字节，JSON／文本构造共享 Bytes，不复制内容；未知Body／文件／流保持None。为了不扩大现有Response内联枚举体积，快照持有一个可选Box；不会改写LimiterResponse等既有公共枚举。take_body、HEAD、空响应、缓存验证器及文件准备维护快照一致性。RequestHandled.responseContent 分享字节；观察事件不轮询、消费或缓冲流。RouteMetadata补充实际解析中间件类型，Telescope读取已有action，避免额外反射或虚构字段。RequestWatcher按固定intdiv字符长度限制处理JSON／text/plain并应用响应脱敏；ClientRequestWatcher保留其不同的除法比较边界。

新增 tests/watchers.rs 的两个真实HTTP用例：独立注册／禁用／自带选项、框架事件开关／手动ignore／应用隔离，以及JSON嵌套脱敏／999与1000个中文字符边界／不透明正文／真实路由元数据。在SQLite、PG18.7、MySQL8.4.13分别实际运行通过；远端每个函数使用单独包装器数据库，避免夹具互相污染。原包26项保持通过，新增2项，Redis1明确ignored后用独立Redis8.10.3实际通过。

证据：tmp/openrust-telescope-followup-20261121/tests.log、watchers-postgres.log、watchers-mysql.log、redis-final.log；core-tests.log覆盖HTTP观察4／核心扩展契约3与原生分页，http-response-regression.log另有23个真实HTTP输入／响应／缓存验证／文件／流／下载回归。分页日志中的MySQL条件函数未提供连接时提前返回，不计为MySQL验收；Scout同组真实三库另有专用结果。

同目录http-browser-final.log覆盖真实服务器桌面／手机授权、防伪、XSS、详情、监控、暂停恢复、自定义路径／域名及disabled；docs-build.log、docs-telescope.log专项12、docs-check_browser.log通用12、docs-check_scope.log导航6均通过。三语正文与生命周期说明同步了Watcher和已缓冲响应的使用及支持限制。全workspace/all-features/all-targets严格Clippy、fmt、API与全部扩展组合见[最终组合记录](../extension-final.md)。

本节关闭公开Watcher、框架事件开关及已知缓冲响应／路由字段的实现缺口。原生UI布局与PHP反射、调用源码过滤、multipart元数据及CLI装饰仍按上节明确支持边界描述，不冒充完整Laravel逐API或像素级等价。没有联系其他对话，没有替代性通用事件／观测内核。

## 出站 multipart

固定 `ClientRequestWatcher::input` 使用结构化字段，普通字段保留原值，文件或流使用文件名、十进制 KB 长度与部件头部，同名字段取请求中最后一项。Laravel `PendingRequest::parseHttpOptions` 先加入普通字段，再追加登记的附件。OpenRust 调整线上的字段顺序，通过 `RecordedRequest.multipart` 将原值和 `attach_body` 元数据传给 Watcher。未知流长度记为 `null`，流只由 HTTP 传输消费。字段、部件头部和整个载荷复用既有脱敏开关。

`attach` 读取随仓 reqwest `Part` 的构造类型及配置。原生文本和可解码为 UTF-8 的缓冲字节保留字段值，流和其他字节记录文件名、已知长度及配置头部。流即便由缓冲字节构造，也只记录元数据。快照复用现有 `RecordedRequest.multipart`，同时保留部件的线上编码、原请求中的顺序、末项覆盖和脱敏。中间件保留原 multipart 边界但替换正文时，快照仍对应构建参数。

固定 Laravel HTTP Client 与 Telescope 的 PHP 8.5.12 探针验证了预格式化数组。Guzzle 生成的正文包含文本、具名和无名流及非 UTF-8 字节。发送及响应诊断均保留普通文本和有效 UTF-8 字节，即便文本配置了文件名。流与无法编码为 JSON 的字节字段只记录名称、十进制 KB 长度和部件头部。重复字段取末项，未提供头部时输出空数组。证据为 `target/tmp/observability-parity/multipart-native-equivalent-upstream.log`。Rust 通过原生 `Part` 对齐这些部件类型。

直接预格式化入口的固定探针确认，列表保留提交顺序及重复部件，普通对象中可嵌入 `name`／`contents` 描述项。有效的布尔与数字保留原诊断类型。普通字段允许空值，空值描述项及嵌套数组会被 Guzzle 拒绝。证据为 `target/tmp/observability-parity/multipart-descriptor-shapes-upstream.log`。

固定补充探针核对了字符串名称与文件名、显式 `Content-Disposition`、文件名去目录及空列表。自定义 disposition 能改写线上的字段名，诊断仍使用描述项原名称；显式空列表发送结束边界，诊断为空数组。Rust 的 `as_multipart` 接入描述项列表和混合对象，保留描述项顺序、标量诊断和既有 `attach` 追加规则。普通字段与描述项的非空正文均按编码字节数默认补上部件级 `Content-Length`，显式提供的长度仍由调用方控制。证据为 `target/tmp/observability-parity/multipart-descriptor-extra-upstream.log`。

命名为 `0`、`1`、`0` 的上游探针得到 `["last","one"]`，倒序、稀疏和带前导零的名称得到 JSON 对象。脱敏键 `1` 可遮盖连续数组的第二项，`01` 只匹配同名对象字段。Rust 保留既有 multipart 字段映射，通过新的 `RecordedRequestDataSource::Multipart` 携带字段首次出现顺序。Telescope 先按原字段名脱敏，再将顺序连续的数字字段呈现为数组。公开诊断来源的穷尽匹配需处理新增变体。证据为 `target/tmp/observability-parity/multipart-numeric-{fields,redaction}-upstream.log`。

PHP 浮点字段在发送时按字符串转换精度编码，并在诊断中保留原数字。固定探针观察到默认 `precision=14`，`1.0` 发送为 `1`，`1e20` 为 `1.0E+20`。舍入后指数小于 -4 或达到 14 时进入科学计数法，负零发送为 `-0`。独立 Rust 原型与 PHP 的 4105 个相同二进制浮点输入逐字节一致。真实 HTTP 普通字段与预格式化部件共用固定默认精度编码，录制数据继续携带原始数字值。证据为 `target/tmp/observability-parity/multipart-float-{fields,precision}-upstream.log` 和 `target/tmp/observability-parity/float-comparison/{php,rust}.tsv`。

固定的中间件矩阵核对成功及连接失败各 10 组。发送前回调替换媒体类型时，成功诊断仍使用回调前的 multipart 参数。请求中间件换成 JSON、表单或纯文本时，成功诊断随事件媒体类型返回原始部件描述列表。保留 multipart 类型时，边界变化仍保留原字段诊断。连接失败依据实际上线请求的媒体类型，解码 JSON／表单，multipart 与纯文本为空。Rust 已接入对应的事件分支，现有 `Http::recorded` 仍反映实际请求的快照。证据为 `target/tmp/observability-parity/multipart-middleware-types-upstream.log`。

原描述列表为空时，经过请求中间件改写的 JSON 和表单由实际上线正文解码。原列表非空时，保留重复项及附加字段，并在清单末尾追加登记附件。流序列化为空对象，非 UTF-8 字节采用替换字符，诊断不消费流。Rust 在此分支继续对配置的部件头部和参数脱敏。公开诊断来源增加 `MultipartDescriptors` 变体，穷尽匹配需涵盖。固定探针为 `target/tmp/observability-parity/multipart-middleware-{empty,attachments,binary}-upstream.log`。

macOS 的回环 HTTP 同时核对线上字节、重复字段、敏感部件头部、完全脱敏、302 后原始字段、307 重放及重试。未知长度的原生流在发送前未被轮询，传输中消费一次。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 的专项入库通过。框架 HTTP 客户端 23 项、Telescope Provider 15 项及独立 Redis 8.10.3 的观察和 Redis 缓存用例通过。

Linux 7.0.14 ARM64 同源码通过框架 HTTP 客户端 23 项、Telescope Provider 15 项，独立 Redis 用例按条件忽略。Windows 11 ARM64 通过两项 multipart 真实传输和一项 Telescope SQLite 入库。macOS 严格 Clippy、快照全仓格式、随仓 reqwest 模块格式、三语文档构建及浏览器检查通过。证据为 `target/tmp/observability-parity/multipart-native-part-{macos,macos-redis,linux,windows,fmt,docs}.log`。

预格式化入口在 macOS 和 Linux ARM64 各通过 HTTP 客户端 24 项、Telescope Provider 15 项，Windows 11 ARM64 通过三项 multipart 真实传输及一项 Telescope SQLite 入库。macOS 另通过 PostgreSQL 18.7、MySQL 8.4.13 和 Redis 8.10.3 缓存专项。实测涵盖发送顺序、部件头部、空边界、非法数据在发送前被拒绝、重复字段末项入库和敏感字段脱敏。首次 macOS 构建触及磁盘停线，空间恢复后用相同隔离目录重新构建。相关目标严格 Clippy、隔离快照格式、三语文档构建及浏览器检查均通过。证据为 `target/tmp/observability-parity/multipart-descriptors-{macos,macos-final,macos-redis,linux,windows,docs,fmt}.log`。

数字名称专项通过真实出站请求验证重复覆盖、按字段名脱敏、连续数组、倒序对象、稀疏对象及前导零对象。macOS 和 Linux ARM64 各通过客户端 24 项和 Provider 15 项，Windows 11 ARM64 通过三项 multipart 真实传输和一项 Telescope SQLite 入库。macOS 的 PostgreSQL 18.7、MySQL 8.4.13、Redis 8.10.3 缓存专项及独立 Redis 观察通过。相关目标严格 Clippy、隔离快照格式、三语文档构建和浏览器检查均通过。证据为 `target/tmp/observability-parity/multipart-numeric-{macos,macos-redis,linux,windows,docs}.log`。

浮点专项通过两种真实输入验证 12 个发送样例和对应部件字节长度，覆盖正负零、舍入后的指数切换、极大值和次正规数。框架录制核对原浮点位值，Telescope 入库核对长小数诊断。macOS 和 Linux ARM64 各通过客户端 25 项、Provider 15 项，Windows 11 ARM64 通过四项 multipart 真实传输和一项 Telescope SQLite 入库。macOS 另通过 PostgreSQL 18.7、MySQL 8.4.13 和 Redis 8.10.3 缓存专项。相关目标严格 Clippy、隔离快照格式及三语站点构建和浏览器检查通过。证据为 `target/tmp/observability-parity/multipart-float-{macos,macos-redis,linux,windows,docs}.log`。

中间件回归在实际回环传输和拒绝连接上逐项验证 23 条入库记录，其中 10 条为失败，另有 3 条空列表回退。断言覆盖线上媒体类型和正文、原始描述顺序和重复项、附件、字节替换及嵌套脱敏。macOS 与 Linux ARM64 各通过客户端 25 项和 Provider 16 项，Windows 11 ARM64 通过客户端 multipart 四项及 23 条记录的 SQLite 回归。macOS 另通过 PostgreSQL 18.7、MySQL 8.4.13 及 Redis 8.10.3 缓存专项。相关目标严格 Clippy、隔离快照全仓格式、三语站点构建和浏览器检查通过。证据为 `target/tmp/observability-parity/multipart-media-{macos,macos-redis,linux,windows,docs}.log`。

### Query 事务和保存点的调用来源

固定 PHP 8.5.12 以真实 SQLite、Laravel 事务及 QueryWatcher 探针验证。过滤定义了查询回调的助手文件后，Facade 事务、连接事务及嵌套事务分别回退到各自的调用行。后续独立查询继续使用自己的调用行。PHP 也能通过调用栈取得普通函数包装器的外层位置。证据为 `target/tmp/observability-parity/query-transaction-upstream.log`。 [固定 FetchesStackTrace](https://github.com/laravel/telescope/blob/39a8ea8ad79915df6c3ac9b4e72659ade808eba3/src/Watchers/FetchesStackTrace.php)

Rust 真实 HTTP 挂起并恢复事务内的查询。修复前忽略助手文件时，三个事务查询均被省略。事务入口现在与已有查询入口一样，在构造 Future 时捕获位置，在轮询时传播，覆盖 Facade、连接、业务服务的默认事务实现及保存点。回滚与重试继续走原事务执行路径。修复前的结果为 `target/tmp/observability-parity/query-transaction-baseline.log`。

Rust 1.94.0 的独立双次轮询探针在含调试信息的构建中可以看到普通异步包装器及外层源文件行号，同一代码经优化且无调试信息时收不到这些位置。普通应用异步包装器的透明来源仍开放。证据为 `target/tmp/observability-parity/query-ordinary-async-probe-{debug,release}.log`。

macOS 的 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各用真实 HTTP 验证五条 SQL 的精确文件与行号。默认 Watcher 17 项及框架数据库 32 项通过，涵盖事务事件、保存点和取消测试。隔离快照全仓格式及所涉目标严格 Clippy 通过。Linux 7.0.14 ARM64 使用 Rust 1.94.0 的 Release 构建通过 SQLite 上全部 17 项 Watcher 回归，包括同样的事务调用来源测试。其他目标平台继续验收。证据为 `target/tmp/observability-parity/query-transaction-{macos-final,linux-release}.log`。

### MySQL 批量清理的查询来源

固定 PHP 8.5.12 的实际 Laravel Schema Facade 和 Telescope QueryWatcher 使用真实 MySQL 8.4.13。批量删视图记录目录查询和删除。批量删表记录目录查询、停用外键检查、删除以及恢复外键检查。空目录只记录相应的目录查询，每条 SQL 归到实际调用行。探针通过 Laravel 的 UUID 生成覆盖点使用 Symfony UUID，保留固定 QueryWatcher 和 Schema 实现。证据为 `target/tmp/observability-parity/query-mysql-cleanup-upstream.log`。 [固定 MySqlBuilder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/MySqlBuilder.php#L12-L46)

修复前，Rust 通过物理连接直接批量清理，实际 HTTP 回归中缺少这些查询条目。现复用 `SchemaSession`、MySQL 目录读取和 DDL 入口，在同一独占写会话保留外键初始状态。后台任务继承查询来源和有效应用作用域。空结果只查询目录。

最终 macOS 隔离快照使用真实 MySQL 8.4.13，Telescope HTTP 验证四个调用点、八条 SQL 的文件与行号以及清理后约束状态。原生 Schema 回归确认跨库隔离、前缀与无前缀对象、独立视图保留及单写连接外键恢复。SQLite 默认 Watchers、隔离快照全仓格式及相关严格 Clippy 通过。证据为 `target/tmp/observability-parity/query-mysql-cleanup-{baseline,final}.log`。MySQL 取消路径和目标平台继续验收。

### 出站响应内容形状与大小限制

固定 PHP 8.5.12 的真实 PSR 响应和 `ClientRequestWatcher::response` 确认，`0` 在 HTML 和 JSON 标量回退时为 `Empty Response`，`text/plain` 保留字符串，302 优先报告目标地址。关联式 JSON 解码会把空对象以及从 0 开始且按顺序排列的数字键对象变成数组。逆序和带前导零的对象键保留。上限按千字符比较，整数、带小数的数字和数值文本均可设置。证据为 `target/tmp/observability-parity/telescope-client-response-shape-upstream.log`。 [固定 ClientRequestWatcher](https://github.com/laravel/telescope/blob/39a8ea8ad79915df6c3ac9b4e72659ade808eba3/src/Watchers/ClientRequestWatcher.php)

Rust 响应诊断先按原始键脱敏，再递归保持上游 JSON 输出形状。真实回环 HTTP 与 SQLite、PostgreSQL 18.7、MySQL 8.4.13 各验证 11 项入库，包含 40 个字符时保留、41 个字符时清除及多字节对照。默认 Provider 回归 17 项通过，独占 Redis 项按条件跳过。隔离快照全仓格式与相关严格 Clippy 通过。其他平台与 Release 继续验收。最终夹具修改后同一三库矩阵复跑通过。证据为 `target/tmp/observability-parity/{telescope-client-response-shape-macos-verified,telescope-client-response-shape-final,observability-response-restart-verified}.log`。
