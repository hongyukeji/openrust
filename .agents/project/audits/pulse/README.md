# Pulse

## 调研与范围

2026-11-21联网读取GitHub官方仓库API，laravel/pulse为活跃MIT项目（1733 stars，仅作当次背景，不作为采用依据）；web搜索／网页解析未返回正文，实际使用GitHub API与固定源码归档。固定来源[laravel/pulse](https://github.com/laravel/pulse/tree/33cc6fc68cd3052aa06227e88e8d933dd3d70a51)，完整归档位于tmp/openrust-ecosystem-20261121/upstream/laravel-pulse。归档未含tests，另从raw.githubusercontent.com取得固定DatabaseStorageTest.php，存于tmp/openrust-pennant-20261121/pulse-storage-upstream-tests.php。旧版v0.x未发现独立Pulse包。

采用官方Entry／Value、数据库四种bucket、预聚合与tail组合、Storage／Redis／Null ingest、Recorder和命令职责；复用原生数据库／Redis／事件／缓存／生命周期及授权，不另建通用监控内核。许可证随包保留。PHP Livewire需转换为原生授权HTTP界面，静态类型注册代替动态类扫描，差异与验收分别记录。

## 当前进度

本节为实现期保存点，最终验收见文末。首先完成Entry／Value与三库SQL聚合存储，再实现ingest、记录器／生命周期、命令与授权面板，最后三语文档、浏览器及全组合验收。尚未验收通过，不计作交付。用户要求独立执行，不联系其他对话，问题与结果直接维护本目录及交付账本。

## 实现与验证保存点（2026-11-21）

已接入原生三表迁移、四种bucket、Storage／Null／Redis ingest、HTTP／队列／缓存／异常／慢SQL／外部HTTP记录器、服务器采样、四条Artisan命令及授权面板。默认应用保持可选包发现，配置／迁移／视图归包；发布视图通过原生Factory覆盖，已解析路径缓存遵循原生ViewFinder语义。Facade提供常用静态入口，ResolvesUsers可替换；默认用户详情复用原生认证Provider，读取不到记录时显示ID。

- 系统采样复用[sysinfo 0.38.4](https://github.com/GuillaumeGomez/sysinfo/tree/v0.38.4)，MIT，MSRV1.88；仅启用system／disk，原生OS调用替换上游shell管道。CPU／内存支持应用内替换检测器，SharedBeat按实例15秒节流，异步入口使用spawn_blocking。
- 慢查询早期实现使用 [backtrace 0.3.76](https://github.com/rust-lang/backtrace-rs/tree/775f6a1ba62e7d35a1fac76e64c61d9d4687b5f2)。当前使用数据库事件提供的编译期调用位置，并按实际 Cargo 包目录区分框架、Pulse 和第三方来源。无合格位置写空字符串，关闭位置记录写 `null`。发布构建与 Cargo 路径重映射验收见[扩展对齐账本](../extension-parity.md#pulse-slowqueries-数据库调用来源)。Rust Error 不保证携带调用栈，异常位置可以经 report／ExceptionReported 显式提供。
- Redis批量写入复用redis-rs原生Pipeline，核心Connection仅公开既有批处理执行入口与类型。流持久化成功后XDEL；存储失败／无效载荷保持条目便于恢复。不声称跨消费者或数据库成功后Redis删除失败的严格一次语义，上游同样不使用consumer group。
- PG NUMERIC之前由f64按错误wire类型绑定，真实PG返回insufficient data left in message。核心复用已有BigDecimal依赖与SQLx NUMERIC编解码，保留精确十进制文本，拒绝无效数字。服务端允许保留尾零，测试按十进制值与长小数精度验收，不改变字段scale。
- 原生RouteMetadata向诊断事件保留规范路由URI／domain／action；Kernel在请求入口清理旧匹配信息，避免提前拒绝的请求继承旧路由。

当前实际通过：Rust1.94包9项测试（含真实Worker），SQLite聚合窗口／并发／回滚，Redis批量／失败重试／保留，面板Gate拒绝与授权／原生模板覆盖，服务器命令；MySQL8.4.13两项最终存储测试通过。固定PHP8.5.12隔离容器运行未修改上游预聚合方法，125组数据生成fixture；124组可持久化组合逐项对照，全部null聚合按上游NOT NULL约束验证失败与事务回滚。容器sha256:1ff5b45922ab30f418447c6089dbe2eba5f01e0b2742863d0327ac349305d70f。PHP最小Collection／时钟适配不代表完整PHPUnit环境。

PG18.7扩展测试首次复验因1.2500与1.25文本断言不同失败，精度本身保持；已修正验收比较，待完整重跑。严格Clippy、根应用feature开关／配置缓存、三库原生入口、缓存驱动、浏览器、三语文档及最终组合仍待闭合，不能计作O05交付。

PHP关联数组的首次匹配顺序在Rust配置中用`[[pattern,value],...]`明确表达；对象形式仍支持，但顺序遵循JSON对象的既有排序，不全局开启serde_json preserve_order以免改变其他包。Entry／Value按Rust值所有权传递，返回Entry的聚合标志与buffer共享；其他公开数据字段的复制后修改不回写已缓冲快照。

## 原生集成与公开接口补验（2026-11-21）

新增真实TCP外部HTTP、原生SQL／缓存／带类型异常日志及已认证／访客连续请求测试，发现并修复生命周期结束后Pulse记录自身入库SQL：延迟回调持有旧scoped实例，而原生事件在新scope解析服务。忽略状态现在同时保留于当前异步任务并按应用Options身份隔离，Listener在解析新Pulse前检查；取消／嵌套忽略恢复及延迟回调失败已有回归。该保存点默认错误通过原生日志报告，最终按固定上游改为静默，可替换回调；用户解析保留原生认证错误来源。

按固定上游补齐公开Recorder契约、十类命名Recorder与register／recorders；编译型类型和显式事件名称替代PHP反射，禁用配置在启动时阻止注册，自定义原生事件与双应用隔离通过。CSS路径／可信HTML追加去重、缺文件错误与发布模板覆盖通过；css getter与withCss setter表达Rust不能重载的上游css，js返回原生面板脚本。面板保留自定义模板资源注入标记，不加载Livewire。

MySQL8.4.13别名模式最初因value列歧义失败，已按固定上游分别限定目标表与laravel_upsert_alias；核心Connection新增getConfig读取构造时解析配置。最终别名存储2项与PG18.7存储2项均通过。file、独立Redis8及database缓存SQLite／PG18.7／MySQL8.4.13均通过服务器按实例15秒节流、共享重启信号和锁竞争；独立Redis流失败重试／trim亦通过。Redis未清库，仅删除测试专有键并回收专属无卷容器。

启用Telescope的包测试14项通过，包括长期pulse:check在退出前逐轮存储Telescope条目、收到共享cache重启信号退出。默认LogWatcher只记录error，测试按真实默认级别修正探针，未降低产品默认。组合通过可选依赖，在仅启用Pulse时不装配Telescope。VAPOR_SSM_PATH的一次检查行为已接入，仍待独立验收。包与Telescope当前快照严格Clippy通过；后续源码补充仍须最终复验。

实际原生HTTP／生产Gate面板桌面与375px手机检查通过：10卡片、CPU／内存／磁盘进度条、队列SVG、四种period、刷新、访客403／无效period400、恶意文本无HTML执行。截图已人工检查，发现手机表头／数字折行并进行针对性样式调整，待最终浏览器复验。三语正文已补齐并通过内容检查，独立站点构建／浏览器仍待。

根原生配置／feature启用与默认骨架共3项通过，缓存命中后无效PULSE_ENABLED／APP_DEBUG不影响配置；关闭feature、三库根入口、最终相关核心回归与组合API尚待。磁盘停线后全部target已按授权安全清空（保留六锁inode与全部数据卷），接下来需要重建；既有通过证据保留，不将已删产物当作仍可运行。O05仍在验收，不记全部完成。

## O05单包最终验收（2026-11-21）

本节覆盖上方历史保存点的待重跑状态。Rust1.94最终包组合16项通过（含9项Provider、真实Worker、Telescope长期命令逐轮存储、缓存／Redis／聚合）。新增公开rescue成功／失败回调、只读用户解析器明确拒绝字段覆盖、legacy users批量解析与应用隔离、三语指标示例实际HTTP入口；report改为原生ExceptionReported派发，默认错误回调按固定上游静默。用户fields回调仍可自定义，错误source保留原生链。

存储在PostgreSQL18.7与MySQL8.4.13别名模式最终各2项通过；SQLite包含在包套件。缓存file、独立Redis8及database的SQLite／PG18.7／MySQL8.4.13通过节流、锁及重启信号；隔离Redis流重试／trim通过。根应用Pulse＋Telescope的配置与骨架3项、关闭feature3项、PG／MySQL原生配置入口各1项通过。三库根入口实际执行迁移、发布保留定制和配置缓存后无效环境变量隔离。

原生HTTP生产Gate桌面／375px手机重新验收通过，已人工查看手机长截图确认表头／数字不再逐字符折行。10卡片、资源进度、队列SVG、四个period、刷新、403／400及恶意内容纯文本均通过。VAPOR_SSM_PATH运行pulse:check不带--once，在10秒超时约束内正常退出。相关核心http_observation4项＋extension_core_contracts3项通过。包全部targets含Telescope严格Clippy、当时完整快照fmt通过；依赖自身rav1e／imgalifier／sqlx-sqlite既有警告不计自身lint通过。三语内容／独立站点构建、12项Pulse＋12项通用＋6项全部导航浏览器通过，实际支持状态页20篇，按正文动态计算。

证据位于tmp/openrust-pulse-20261121/：tests-final.log、storage-postgres-final.log、storage-mysql-alias.log、cache-*-final.log、configuration-final-combined.log、configuration-off.log、configuration-{postgres,mysql}.log、browser-final.log、clippy-final.log、fmt-final.log、core-regressions.log、docs-build-final.log及docs-*-final.log。Cargo.lock与隔离快照包集合和依赖内容一致，未替换其他协作者锁文件。

O05单包验收关闭，接续O06。语言适配明确保留：原生HTML／JS替换PHP Livewire；显式Recorder注册替代反射；css重载用withCss；aggregateTotal切片统一返回按类型结果；带类型异常由class／location表达并异步派发；用户默认通用认证Provider逐ID查询，自定义ResolvesUsers可批量；legacy users用ID映射；应用作用域自动选择容器，无setContainer变更操作。错误与取消不保证持久入库。最终全扩展／API组合及完整上游差异继续单列，不能计为整个路线图完成。

## 扩展审计修复（2026-11-21）

参考生成器固定 Entry／DatabaseStorage 原文件与来源，原聚合 fixture 复现并逐个拒绝篡改；Livewire／原生视图取舍补候选比较，未改生产功能。 统一命令、环境、结果和边界见[全量扩展修复记录](../extension-design-review.md#修复验收与交付边界)。

## CacheInteractions 缓存事件对照

PHP 8.5.12 使用固定 Laravel 的缓存 Repository、ArrayStore、固定 Pulse Recorder 和 Ingest 消费者验证命中与未命中。两种事件分别入库，值为空，时间戳取自事件发生时。多个分组规则命中时使用首个规则，忽略规则过滤原始键。事件进入延迟队列后将采样率设为零，实际消费时丢弃该条。证据为 `target/tmp/observability-parity/pulse-cache-interactions-upstream.log`。 [固定 CacheInteractions](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/CacheInteractions.php)

固定 PHP 探针将采集时间推后一秒后，命中和未命中仍保留事件发生时的秒级时间戳。Rust 通过应用 HTTP 路由访问实际缓存，并跨到下一秒后结束请求。通过数据库查询确认两条记录按未命中、命中排序，键均为首条匹配规则生成的 `feature:*`，值为空，时间戳均属于缓存操作期间。忽略键和采集前降为零的采样率均未增加条目，公开计数聚合对两类记录分别返回 1。

macOS 将 file、database、Redis 8.10.3 缓存与 SQLite、PostgreSQL 18.7、MySQL 8.4.13 存储逐一组合，九组均通过，同批完整缓存测试目标 3 项通过。固定 PHP 和 macOS 证据为 `target/tmp/observability-parity/pulse-cache-interaction-{source-time-upstream,matrix-macos}.log`。

Linux 7.0.14 ARM64 和 Windows 11 ARM64 使用相同源码复验 SQLite 上的 file、database、Redis 8.10.3 三种缓存。Pulse 库和缓存测试目标通过严格 Clippy，隔离快照全仓格式通过。本批的 PostgreSQL 和 MySQL 矩阵在 macOS 执行。平台证据为 `target/tmp/observability-parity/pulse-cache-interaction-{linux,windows,redis-linux,redis-windows}.log`。

## Recorder 模式修饰符与分隔符

PHP 8.5.12 与 PCRE2 10.44 的 12 组匹配与替换验证选项组合、Unicode 字符类、重复捕获名、ASCII 大小写折叠边界和分组替换。另有 13 组核对分隔符配对、转义、前置与后置空白、未转义内部分隔符及 `u` 对 `\C` 的限制。固定扩展的 `S`、`X` 保留默认行为。证据为 `target/tmp/observability-parity/pulse-pcre-options-matrix-upstream.log` 和 `target/tmp/observability-parity/pulse-pcre-delimiters-upstream.log`。 [固定 PHP PCRE 实现](https://github.com/php/php-src/blob/php-8.5.12/ext/pcre/php_pcre.c#L580)

Rust 直接设置 PCRE2 编译选项并按固定分隔符规则解析。真实 HTTP 文件缓存消费者核对 Unicode 与锚定忽略规则、首个匹配分组和延迟采样，SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 各通过。Rust 模式专项覆盖阈值。macOS 的 PCRE2 库 24 项、三个扩展默认 96 项、全仓格式、严格 Clippy、三语文档和 12 组浏览器检查通过。

Linux 7.0.14 ARM64 与 Windows 11 ARM64 使用相同源码通过模式专测及实际 HTTP 消费者。Linux 的 SQLite 专项确认动态链接系统 PCRE2 10.42，并核对该版本对 `r` 返回配置错误。同平台的静态 PCRE2 10.45 探针验证该选项的匹配行为。使用 `r` 需要 PCRE2 10.43 或更新版本。证据为 `target/tmp/observability-parity/pulse-php-pcre-{accepted,accepted-linux,bundled-linux-final,system-linkage,docs}.log` 和 `target/tmp/observability-windows/pulse-php-pcre-accepted-arm64.log`。 [PCRE2 版本记录](https://github.com/PCRE2Project/pcre2/blob/pcre2-10.43/NEWS)

### 分组的零宽匹配与替换转义

Rust 分组在零宽匹配后先在同一位置锚定重试非空匹配。重试失败时，`u` 修饰符按 UTF-8 码点推进，字节模式按字节推进，末尾零宽匹配只替换一次。PHP 8.5.12 探针核对 `/(?=a)|a/` 得到 `XX`、`/a*?/` 得到 `XXX`，以及多字节字符、CRLF、重试时的捕获组和替换转义。反斜杠转义的美元符号保留字面意义，花括号引用采用 `${1}` 形式。修复前后对照与新增用例的上游证据为 `target/tmp/observability-parity/pulse-pcre-zero-width-{upstream,native,oracle}.log`。 [固定 PHP 替换循环](https://github.com/php/php-src/blob/php-8.5.12/ext/pcre/php_pcre.c#L1757-L1819)

macOS 隔离快照通过 PCRE2 库 24 项、三扩展默认 97 项、快照全仓格式和框架及三扩展选定目标严格 Clippy。真实 HTTP 文件缓存消费者在 SQLite、PostgreSQL 18.7、MySQL 8.4.13 均保存分组键 `zero-width:XX`，原命中、未命中与动态采样行为同时通过。证据为 `target/tmp/observability-parity/pulse-pcre-zero-width-macos-verified.log`。

同源码在 Linux 7.0.14 ARM64 和 Windows 11 ARM64 各通过 Pulse 模式 4 项和 Provider 15 项，覆盖 SQLite 上的真实文件缓存行为。Linux 同批通过 Telescope Provider 15 项，按条件跳过 1 项。证据为 `target/tmp/observability-parity/pulse-pcre-zero-width-{linux,windows}-verified.log`。

分组键原先仅使用 UTF-8 文本。PHP 字节模式可在 UTF-8 码点中间插入替换内容。仅写 `(*UTF)` 而未用 `u` 修饰符时，PHP 的替换循环也按字节推进。实际固定 Pulse 缓存 Recorder 及 SQLite 存储保留两种模式产生的非 UTF-8 字节键，正常聚合并按原始字节计算哈希。同输入经 Rust 公开分组函数在字节模式返回文本转换错误，在仅写 `(*UTF)` 时返回 PCRE2 起始偏移错误。这两种输入的 Rust 字节键表示、持久化及面板序列化仍待对账。证据为 `target/tmp/observability-parity/pulse-pcre-zero-width-{oracle,storage-oracle,inline-native}.log`。

固定 PHP 8.5.12 的真实 CacheInteractions、Pulse 迁移与 DatabaseStorage 补验了 PostgreSQL 18.7 和 MySQL 8.4.13。两条分组规则产生的非法序列分别为 `58c358a958` 和 `59c359a959`。UTF-8 PostgreSQL 拒绝两条写入（SQLSTATE `22021`），严格 `utf8mb4` MySQL 返回 `HY000/1366`。混合合法与非法条目的批次也报错，预先存入的一条事件与四条聚合完整保留，混合批次的新条目和聚合均未留下。MySQL 显式关闭严格模式时，同样两个输入截断为 `X` 和 `Y` 并成功入库。以上是数据库实际处理分组原始字节的结果。修复前 Rust 的文本分组路径在存储前拒绝此输入，SQLite、非严格 MySQL、错误阶段及面板均有差距。证据为 `target/tmp/observability-parity/pulse-pcre-invalid-text-database-upstream.log`。

固定 PHP、Predis 2.4.0 和实际 Redis 8.10.3 补验。同一 Recorder 通过上游 RedisIngest 将两个分组键按原始字节写入 Stream。用一次受控存储失败确认两条仍留在 Stream，再用固定 SQLite DatabaseStorage 重试，成功保存后两条均被移除。SQLite 的 `typeof(key)` 为 `text`。固定聚合按原始键分别返回一次 miss，Cache Blade 模板的实际键插值经固定框架编译后显示 `X�X�X` 和 `Y�Y�Y`。探针覆盖实际插值，整个 Livewire 请求和快照序列化继续验证。证据为 `target/tmp/observability-parity/pulse-pcre-invalid-text-redis-display-upstream.log`。 [固定 RedisIngest](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Ingests/RedisIngest.php)

Rust 的真实文件缓存 HTTP 路由复现了缺口。两个请求均完成，但触发 Pulse 错误回调并丢弃观测条目，只有同一消费者的合法键对照入库。错误分别是文本转换失败及 PCRE2 非法 UTF 偏移。固定 PHP 多次匹配会跳过 UTF 检查。PCRE2 对可用起始偏移仍有前置条件，兼容方案需单独验证，避免从码点内部用未经验证的偏移调用底层匹配器。证据为 `target/tmp/observability-parity/pulse-pcre-invalid-text-native-baseline.log`。 [固定 PHP 替换循环](https://github.com/php/php-src/blob/php-8.5.12/ext/pcre/php_pcre.c#L1757-L1819) [PCRE2 API](https://www.pcre.org/current/doc/html/pcre2api.html)

原生 Rust 三库夹具验证了后续存储改动可使用的参数语义。SQLite 将二进制参数显式转换为 TEXT 后保持原字节及 TEXT 类型。PostgreSQL 的 `convert_from` 将字节转换为 UTF-8，数据库返回同一 `22021`。MySQL 将二进制参数直接赋给文本列时，严格模式报错而非严格模式截断，PHP PDO 的对照表明显式 `convert` 会改变严格模式的错误类型，需保留直接赋值。后续需在批量写入和 upsert 使用这些候选绑定并复验完整路径，Redis 的兼容编码与面板按字节区分聚合行也继续验证。证据为 `target/tmp/observability-parity/pulse-byte-key-{parameter-drivers,native-binding}.log`。

#### 缓存分组字节键进入原生消费者

CacheInteractions 的 PCRE 字节模式现在基于原始字节创建 Entry。有效 UTF-8 继续使用文本字段与既有 JSON 形式。原始字节可经 `Entry::key_bytes()` 读取，编辑后与原显示值不同的公开 `key` 按编辑值持久化。Redis Stream 仅将非 UTF-8 键编码为字节数组，旧读端无法将其解码为字符串并保留待消费项。旧文本条目可由新读端读取。数据库批量插入与 upsert 实际绑定原始字节。SQLite 转换成 TEXT 并按原字节生成哈希，读取非法 TEXT 时保留字节。PostgreSQL 在执行中按 UTF-8 转换，MySQL 按连接的严格模式决定拒绝或截断。原生 Cache 指标在展示边界替换无法显示的字节，两个显示相同的原始键仍保留独立计数。

macOS 真实 HTTP 文件缓存、SQLite、PostgreSQL 18.7 及 MySQL 8.4.13 专项通过。严格模式两库在存储触发错误回调，跨写入批次的合法项一并回滚。非严格 MySQL 的两组键按数据库行为截断并合并计数。独占 Redis 8.10.3 验证失败保留、重试、裁剪、旧文本载荷和原字节往返。另用实际 HTTP 缓存、原生 Redis ingest、SQLite 与面板 API 验证完整链路。Pulse 和 Telescope 默认 78 项通过、两项按环境跳过，Redis ingest 跳过项单独实跑通过。框架两个数据库目标、隔离快照格式、相关严格 Clippy、三语文档构建及桌面／移动检查通过。证据位于 `target/tmp/observability-parity/pulse-byte-key-{storage-macos,framework-regressions,storage-docs,storage-docs-chapter}.log`。

Linux 7.0.14 ARM64 使用 Rust 1.94.0 与相同源码通过 SQLite、真实 Redis 8.10.3 写入后再消费及独立失败保留／重试三项。证据为 `target/tmp/observability-parity/pulse-byte-key-storage-linux.log`。

仅写 `(*UTF)` 的模式仍受 PCRE2 安全起始偏移约束。该变体、完整 Livewire 请求对照及本批目标平台 Release 继续验收。

#### 含字节键的通用 Graph 与 Value

固定 PHP 8.5.12、Redis 8.10.3 与 SQLite 通过实际 Ingest 和 DatabaseStorage 核对。Graph 把两个原始字节键各保留为独立序列，均包含 60 个桶和一次计数。直接序列化 Graph 到 JSON 报 `JSON_ERROR_UTF8`。Value 的同键覆盖按提交顺序保留最后一项，即使其时间戳更小，按原始字节键查询也能命中。证据为 `target/tmp/observability-parity/pulse-byte-key-graph-values-upstream.log`。Rust 的文本 Graph API 维持 UTF-8 键契约。新增公开 `Pulse::graph_bytes`、Facade 和存储入口，复用原有按键排序、补齐及聚合查询，按原始字节独立返回各序列。自定义文本 Storage 的默认实现可转为字节；内置 DatabaseStorage 直接读取 SQLite 的原始字节。macOS 真实缓存 HTTP 后从另一个路由调用 Facade，确认两个原始键和合法文本对照在各自序列内都有 60 桶及一次计数，并按字节顺序返回。缓存、存储、快照格式、相关严格 Clippy 与三语文档构建和桌面／移动验证通过。证据为 `target/tmp/observability-parity/pulse-byte-key-graph-{macos,docs}.log`。

字节键仪表值已接通 `ByteValue`、`Pulse::set_bytes`、`Pulse::values_bytes`、Facade 与内置数据库存储。SQLite 保留原始字节与 TEXT 类型，原始键各自可查，即使显示相同。有效 UTF-8 字节键和同内容的文本键在批次内合并，同键保留最后提交的值。PostgreSQL 18.7 与严格 MySQL 8.4.13 拒绝非法 UTF-8，并回滚已经写入的前一批。非严格 MySQL 8.4.13 按库端规则截断。原文本 `Value` 保持既有结构与 Redis 编码，`Item::ByteValue` 使用新的 Redis 编码，须先更新消费者再开始发送。

macOS 的 SQLite HTTP Facade、三库严格性矩阵、Redis 8.10.3 到 SQLite 的 HTTP 写入和消费链路、独立 Redis 失败保留与重试、隔离快照格式及相关严格 Clippy 通过。Pulse 与 Telescope 默认 89 项通过，两项按环境跳过。三语文档构建和六组桌面／移动检查通过。Windows 11 ARM64 的文件缓存、字节 Graph 及 Value 各经 SQLite 和 Redis 8.10.3 通过，独立 Redis 失败保留、重试和裁剪通过。时间裁剪的测试使用固定历史消息 ID，避免跨主机时钟差影响结果。证据为 `target/tmp/observability-parity/pulse-byte-value-{macos,docs}.log` 和 `target/tmp/observability-windows/pulse-byte-value-arm64.log`。Linux 7.0.14 ARM64 同源码通过缓存分组、字节 Graph 和 Value 的 SQLite 及 Redis 8.10.3 路径，并补验独立 Redis 失败保留、重试与裁剪。证据为 `target/tmp/observability-parity/pulse-byte-value-linux.log`。本批 Release 继续验收。

同一固定 PHP 探针发现 `collapseValues` 的下一项差异。单批内 `01` 与 `1`、`1e0` 与 `1.0` 作为类型或键时按 PHP 宽松比较只保留较后提交的等值项。大小写有差异的文本、空文本与 `0`、相邻的大整数及同向溢出的整数字符串仍可独立保存。Rust 此批按原始类型和键字节精确比较，宽松比较继续对齐。证据为 `target/tmp/observability-parity/pulse-value-numeric-keys-upstream.log`。 [固定 DatabaseStorage](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Storage/DatabaseStorage.php)

## SlowJobs 任务尝试计时

固定代码在 `JobProcessing` 开始计时，遇到 `JobProcessed`、`JobFailed` 或 `JobReleasedAfterException` 后计入首个终止事件。PHP 8.5.12 经真实事件分派和上游 Ingest 消费者核对，跨秒的失败尝试为 75 毫秒，释放为 134 毫秒，成功为 49 毫秒。随后的第二个终止事件、`JobAttempted` 与 `sync` 连接均不增加慢任务记录。证据为 `target/tmp/observability-parity/pulse-slow-job-lifecycle-upstream.log`。 [固定 SlowJobs Recorder](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/SlowJobs.php)

修复前只读取 `JobAttempted.duration`，遗漏处理前监听器和队列结算耗时。Rust 已改为按连接、队列、保留任务标识和尝试次数记录开始时间，以第一个终止事件当时的时间戳计算毫秒耗时，再进入原有采样、过滤和聚合。`JobAttempted` 仅清理未产生对应终止事件的尝试。

macOS 上真实 Artisan SQLite Worker 经过 40 毫秒的处理前监听器，执行四次成功、一次终态失败和一次因异常释放。原实现入库的六条慢任务均未达到 40 毫秒，修复后的六条全部达到，并核对队列处理及释放计数。相同源码通过 Worker 和 Provider 共 18 项、涉及文件格式及 Pulse 库和两个测试目标的严格 Clippy。三语文档构建、渲染正文和 12 组三语桌面／移动浏览器检查通过。证据为 `target/tmp/observability-parity/pulse-slow-job-lifecycle-{baseline,restored,docs}.log`。

独立 Worker 达到退出型超时后由进程退出释放记录状态，嵌入式应用在同一进程内反复启动和停止 Worker 的超时清理仍需验证。其他目标平台及队列驱动的本批 Worker 计时仍需复验。
