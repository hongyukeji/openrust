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

仅写 `(*UTF)` 的零宽分组存在明确的匹配边界。固定 PHP 8.5.12 与 PCRE2 10.44 对 `é` 按字节推进，返回 `58c358a958`。该步骤会在 UTF-8 码点中间继续匹配。PCRE2 的受检接口要求偏移落在码点首字节或字符串末尾。Rust 保留该检查，遇到码点内偏移时返回配置错误。ASCII、消耗完整多字节字符的模式及显式 `u` 按对应规则工作。PHP 和 Rust 均对带 `u` 的用例得到 `XéX`。macOS 的五项规则测试、相关严格 Clippy、全仓格式、三语站点构建和六组浏览器检查通过。证据为 `target/tmp/observability-parity/pulse-inline-utf-boundary-{upstream,macos,docs}.log`。 [固定 PHP 替换循环](https://github.com/php/php-src/blob/php-8.5.12/ext/pcre/php_pcre.c#L1757-L1819) [PCRE2 受检起始偏移](https://www.pcre.org/current/doc/html/pcre2api.html#SEC25)

完整 Livewire 请求对照及目标平台 Release 继续验收。

#### 含字节键的通用 Graph 与 Value

固定 PHP 8.5.12、Redis 8.10.3 与 SQLite 通过实际 Ingest 和 DatabaseStorage 核对。Graph 把两个原始字节键各保留为独立序列，均包含 60 个桶和一次计数。直接序列化 Graph 到 JSON 报 `JSON_ERROR_UTF8`。Value 的同键覆盖按提交顺序保留最后一项，即使其时间戳更小，按原始字节键查询也能命中。证据为 `target/tmp/observability-parity/pulse-byte-key-graph-values-upstream.log`。Rust 的文本 Graph API 维持 UTF-8 键契约。新增公开 `Pulse::graph_bytes`、Facade 和存储入口，复用原有按键排序、补齐及聚合查询，按原始字节独立返回各序列。自定义文本 Storage 的默认实现可转为字节；内置 DatabaseStorage 直接读取 SQLite 的原始字节。macOS 真实缓存 HTTP 后从另一个路由调用 Facade，确认两个原始键和合法文本对照在各自序列内都有 60 桶及一次计数，并按字节顺序返回。缓存、存储、快照格式、相关严格 Clippy 与三语文档构建和桌面／移动验证通过。证据为 `target/tmp/observability-parity/pulse-byte-key-graph-{macos,docs}.log`。

字节键仪表值已接通 `ByteValue`、`Pulse::set_bytes`、`Pulse::values_bytes`、Facade 与内置数据库存储。SQLite 保留原始字节与 TEXT 类型，原始键各自可查，即使显示相同。有效 UTF-8 字节键和同内容的文本键在批次内合并，同键保留最后提交的值。PostgreSQL 18.7 与严格 MySQL 8.4.13 拒绝非法 UTF-8，并回滚已经写入的前一批。非严格 MySQL 8.4.13 按库端规则截断。原文本 `Value` 保持既有结构与 Redis 编码，`Item::ByteValue` 使用新的 Redis 编码，须先更新消费者再开始发送。

macOS 的 SQLite HTTP Facade、三库严格性矩阵、Redis 8.10.3 到 SQLite 的 HTTP 写入和消费链路、独立 Redis 失败保留与重试、隔离快照格式及相关严格 Clippy 通过。Pulse 与 Telescope 默认 89 项通过，两项按环境跳过。三语文档构建和六组桌面／移动检查通过。Windows 11 ARM64 的文件缓存、字节 Graph 及 Value 各经 SQLite 和 Redis 8.10.3 通过，独立 Redis 失败保留、重试和裁剪通过。时间裁剪的测试使用固定历史消息 ID，避免跨主机时钟差影响结果。证据为 `target/tmp/observability-parity/pulse-byte-value-{macos,docs}.log` 和 `target/tmp/observability-windows/pulse-byte-value-arm64.log`。Linux 7.0.14 ARM64 同源码通过缓存分组、字节 Graph 和 Value 的 SQLite 及 Redis 8.10.3 路径，并补验独立 Redis 失败保留、重试与裁剪。证据为 `target/tmp/observability-parity/pulse-byte-value-linux.log`。本批 Release 继续验收。

同一固定 PHP 探针发现 `collapseValues` 的下一项差异。单批内 `01` 与 `1`、`1e0` 与 `1.0` 作为类型或键时按 PHP 宽松比较只保留较后提交的等值项。大小写有差异的文本、空文本与 `0`、相邻的大整数及同向溢出的整数字符串仍可独立保存。Rust 在写库前倒序处理同批仪表值，复用框架固定 PHP 字符串比较函数。普通键按字节分组，数字候选再按 PHP 语义比较，只与已保留的项对比。负零、指数和大数边界通过，跨批次仍由数据库精确 upsert。macOS 的 SQLite、PostgreSQL 18.7、MySQL 8.4.13 严格及非严格模式、独占 Redis 8.10.3 重试与裁剪、Pulse 和 Telescope 默认 90 项、快照格式和相关严格 Clippy 均通过。当前批次 Release 及其他目标平台继续验收。证据为 `target/tmp/observability-parity/pulse-value-numeric-keys-macos-verified.log` 和 `target/tmp/observability-parity/pulse-value-numeric-keys-upstream.log`。 [固定 DatabaseStorage](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Storage/DatabaseStorage.php)

## 面板慢操作、异常与用量排序

固定 Pulse 的四个慢操作组件以 `max` 降序为默认，接受各自 URL 参数上的 `count`。异常以 `count` 降序为默认，`exceptions=latest` 改按最大时间戳。Usage 可选请求、慢请求或任务，并在解析用户信息前把查询限制在前十名。 [固定 Pulse 面板组件](https://github.com/laravel/pulse/tree/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Livewire)

原生面板过去对四张慢操作卡一律按次数查询，且用量读取最多 101 项。Rust 现通过授权的指标端点查询正确的顺序和上限，在浏览器中提供独立排序和用量选择。页面 URL 保留选项，刷新继续使用当前选项。保留原生单独的任务用量卡。修复前真实请求在慢请求卡错误地将高频项排在最长耗时项之前，证据为 `target/tmp/observability-parity/pulse-dashboard-order-baseline.log`。

固定 PHP 8.5.12 与 SQLite 执行五张卡片及 Usage 的实际 `render` 方法和数据库查询，验证排序与三种用量的前十名。夹具提供 Livewire 基类、即时查询和视图数据捕获，完整 Livewire 请求生命周期继续验收。macOS 的 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 均经真实授权端点通过相同排序及人数断言，非法用量参数返回 400。Pulse Provider 默认 18 项通过。生产 Gate 的真实桌面与移动浏览器验证排序、用量选项及重新加载，保留授权、图表和转义检查。隔离快照格式、相关严格 Clippy、三语构建与六组正文检查通过。其他平台 Release 继续验收。证据为 `target/tmp/observability-parity/pulse-dashboard-order-{upstream-final,macos-final,docs}.log`。

### 用量卡片的用户资料

固定 `Users::find` 与 `LegacyUsers::find` 的 PHP 8.5.12 对照确认，未找到用户时返回 ID、空补充信息和 Gravatar 占位地址。原生认证资料的空邮箱和文本 `0` 使用占位地址，有效邮箱按 PHP 的 ASCII 小写和默认 `trim` 计算 SHA-256。显式头像包括空字符串，按原值返回。固定用量模板在用户名下显示补充信息，并按需加载头像。证据为 `target/tmp/observability-parity/pulse-user-display-upstream.log`。 [固定 Users](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Users.php) [固定用量卡片](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/resources/views/components/user-card.blade.php)

Rust 前态的授权 HTTP 返回空补充信息为 JSON `null`，改为与固定上游一致的空字符串并得到正确头像。真实认证 Provider 与用量聚合通过 SQLite、PostgreSQL 18.7、MySQL 8.4.13 七组资料。移动和桌面 Chrome 验证头像可解码、按需加载、补充资料作为文本渲染，原有筛选与轮询仍通过。头像占位请求由浏览器内合成响应返回，使用假资料。Pulse Provider 28 项、Telescope Provider 33 项、相关格式、Pulse 全目标严格 Clippy 和三语文档的 6 个视口通过。日志为 `target/tmp/observability-parity/pulse-user-display-{baseline,macos-retry,docs}.log`。

固定 PHP 从 Eloquent Provider 用 `findMany` 在一条 SQLite 查询中读取多个模型。真实原生授权 HTTP 的旧实现为四个指标键执行四条用户查询，现经 Eloquent Provider 的模型主键查询合并为一条。SQLite、PostgreSQL 18.7、MySQL 8.4.13 验证 `1` 和 `01` 返回同一用户、未知用户显示 ID。通用 Provider 继续逐个按 ID 读取。框架认证与扩展契约合计 11 项、Pulse Provider 29 项、Telescope Provider 33 项、快照格式、相关严格 Clippy 和三语文档桌面及移动检查通过。证据为 `target/tmp/observability-parity/pulse-eloquent-users-{upstream,baseline,verified-retry,docs}.log`。

旧版 `Pulse::users` 回调现在可用 `User::from_profile` 分开提供邮箱、补充信息和头像，并与默认认证资料复用格式转换。固定 PHP 验证仅邮箱、补充信息覆盖、空值覆盖、文本零邮箱及未找到用户五组。Rust 的实际授权 HTTP 在 SQLite、PostgreSQL 18.7、MySQL 8.4.13 得到相同结果。直接返回的原生 `User` 字段仍由回调决定。Pulse Provider 30 项、Telescope Provider 33 项、快照格式、严格 Clippy 和三语文档桌面及移动 6 组通过。证据为 `target/tmp/observability-parity/pulse-legacy-user-profile-{upstream,macos,docs}.log`。

### 首屏卡片的延迟加载与隔离

固定 Pulse 的九类指标卡片声明 `#[Lazy]`。支持范围内的 Livewire 3.6.4 与 4.0.0 会为默认惰性组件生成占位框，在进入可视范围时启动加载，首轮使用独立更新。常规轮询更新可合并。 [固定 Pulse 组件](https://github.com/laravel/pulse/tree/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Livewire) [Livewire 3.6.4](https://github.com/livewire/livewire/blob/v3.6.4/src/Features/SupportLazyLoading/SupportLazyLoading.php) [Livewire 4.0.0](https://github.com/livewire/livewire/blob/v4.0.0/src/Features/SupportLazyLoading/SupportLazyLoading.php)

Chrome 前态的首个全量指标请求挂起时卡片容器为空，返回 500 后整页面无卡片。原生面板现在先生成占位框，进入可视范围后独立请求卡片。已加载的卡片可合并轮询，首轮仍在等待的卡片不会阻塞其他轮询。失败可在卡片内显示，合并请求失败时按卡片复验，支持自动刷新和手动恢复。授权指标入口接受 `cards` 选择卡片，独立复用卡片缓存，保留不带选择参数的完整指标快照。

SQLite、PostgreSQL 18.7、MySQL 8.4.13、具名 Redis 8.10.3 的授权 HTTP 验证卡片选择、无效值、未选查询的隔离和完整 JSON。真实 SQLite 看板及 Chrome 桌面、手机验证独立加载、延迟与错误隔离、恢复、周期、手动和定时刷新、焦点与触控，未授权的部分响应返回 403。首轮浏览器检查发现卡片节点替换时滚动目标失效，保留卡片容器后复验通过。Pulse Provider 27 项、Telescope Provider 30 项、快照格式、Pulse 严格 Clippy、三语文档与六组浏览器检查通过。证据为 `target/tmp/observability-parity/pulse-card-lazy-{browser-baseline-retry,macos,macos-browser-retry,docs}.log`。

### 后台、离线和页面移除的轮询

按固定 Pulse 模板的五秒轮询配置，直接执行 Livewire 3.6.4 与 4.0.0 的指令源码，确认后台抽样值 0.94 跳过、0.95 请求，离线暂停、恢复在线后继续，组件移除后结束轮询。原生面板按同一阈值处理后台自动请求，并在离线和面板移除时停止相应轮询。 [Livewire 3.6.4 轮询](https://github.com/livewire/livewire/blob/v3.6.4/js/directives/wire-poll.js) [Livewire 4.0.0 轮询](https://github.com/livewire/livewire/blob/v4.0.0/js/directives/wire-poll.js)

受控 Chrome 复现后台仍发请求，修复后通过可控时钟和采样值验证两侧边界。真实 SQLite 看板在 Chrome 测试客户端离线、恢复和移除容器时的 HTTP 请求数，并通过已有桌面、手机、定时刷新及图表焦点回归。三语构建和六组浏览器检查通过。证据为 `target/tmp/observability-parity/pulse-poll-pauses-{upstream,browser-baseline,browser,macos,docs}.log`。

两个固定 Livewire 版本还验证了同一时钟下分别采样每个组件。原生前态对整个批次只抽样一次，导致两张卡片一起跳过。修复后每张已加载卡片独立抽样，选中的卡片共用一次 HTTP 请求。受控页面和真实 Chrome 中各用 0.94 和 0.95 验证只请求其中一张卡，公开指标、轮询、授权、桌面与移动回归及三语文档均通过。证据为 `target/tmp/observability-parity/pulse-poll-card-samples-{upstream,baseline,browser,macos-retry,docs}.log`。

### 慢操作、异常和用量的采样计数

固定 PHP 8.5.12 使用实际 Blade 编译器渲染四类慢操作、异常和用量计数。采样率 0.4 时原始计数 3 显示为 `~8`，提示包含采样率与原始值。全量计数按千位分隔。用量的请求、慢请求、任务分别读 UserRequests、SlowRequests、UserJobs。Rust 的 `sample_rates` 随当前配置返回，卡片原始行及五秒缓存仍保留数据库计数。面板识别数据库返回的数值和数字文本。采样率为零时保留历史原始计数。 [固定卡片模板](https://github.com/laravel/pulse/tree/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/resources/views/livewire)

原生前态的 JSON 缺少采样率。SQLite、PostgreSQL 18.7、MySQL 8.4.13 和具名 Redis 8.10.3 通过实际授权 HTTP，验证各 Recorder 的采样率、三种用量及缓存命中时的配置刷新。真实 SQLite 看板在桌面和移动浏览器验证估算、提示、零采样率和排序切换。Pulse Provider 26 项、Telescope Provider 30 项、隔离快照格式、Pulse 全目标严格 Clippy、三语文档和六组浏览器检查通过。证据为 `target/tmp/observability-parity/pulse-card-counts-{upstream,macos-baseline,macos-verified,docs}.log`。

### 周期链接和浏览器历史

固定 PHP 8.5.12 的 `HasPeriod` 以 `1_hour`、`6_hours`、`24_hours`、`7_days` 选择四个周期。实际 Blade 中选择最近一小时会清除 URL 的 `period`，其他周期写入固定名称，并保留其余查询参数。原生前态打开 `?period=6_hours` 仍请求最近一小时。修复后面板接受固定链接及已有短写，切换周期保留浏览器前进和后退记录；JSON 指标接口把两种写法归到同一查询周期和缓存项。 [固定周期选择器](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/resources/views/livewire/period-selector.blade.php) [固定周期区间](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Livewire/Concerns/HasPeriod.php)

受控 Chrome 桌面和移动视口核对有效及无效链接、旧链接、默认值、其他参数及连续前进后退。SQLite、PostgreSQL 18.7、MySQL 8.4.13 和具名 Redis 8.10.3 配置分别经授权 HTTP 验证不同写法共用已缓存的曲线。真实 SQLite 应用的桌面与手机浏览器验证四种链接及指标请求，并通过原有授权、排序和定时刷新回归。Pulse Provider 25 项、Telescope Provider 30 项、隔离快照格式、Pulse 全目标严格 Clippy、三语文档与六组浏览器检查通过。证据为 `target/tmp/observability-parity/pulse-period-route-{upstream,browser-baseline,browser,macos,docs}.log`。

### 慢操作与异常卡片的五秒缓存

固定 Pulse 的 `RemembersQueries::remember` 在配置的缓存仓储中按组件类、查询键和周期保存结果。PHP 8.5.12 使用实际 ArrayStore 核对，四秒时保留原查询结果、耗时和执行时间，第五秒重新查询。变更排序、周期或组件类会选取独立缓存项。证据为 `target/tmp/observability-parity/pulse-card-query-cache-upstream.log`。 [固定 RemembersQueries](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Livewire/Concerns/RemembersQueries.php)

Rust 前态在每次授权指标请求中查询四类慢操作及异常。当前通过框架 Cache 的 `remember` 在 `pulse.cache` 指定仓储中按卡片、实际排序和周期缓存汇总行五秒。真实 HTTP 在 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 上确认短时复用、独立排序与周期、到期读取最新数据。同一用例在独占 Redis 8.10.3 证明选择了配置的具名仓储。Pulse Provider 22 项、Telescope Provider 20 项、快照全仓格式、相关严格 Clippy、三语构建和六组浏览器检查通过。独占 Telescope Redis 用例沿用环境条件。证据为 `target/tmp/observability-parity/pulse-card-query-cache-{macos-baseline,macos-final,docs}.log`。其余卡片和查询时间的验收见下文。

### 用量、缓存与队列卡片的查询缓存

固定用量卡片在五秒内共用同一类型及周期的计数与已解析用户，缓存卡片分别记忆总计和逐键查询，队列卡片按周期缓存历史。Rust 在已授权指标入口复用同一 Cache 记忆路径。用量按指标类型分键，缓存总计与明细分键，采样率继续随配置读取。 [固定 Usage](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Livewire/Usage.php) [固定 Cache](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Livewire/Cache.php) [固定 Queues](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Livewire/Queues.php)

SQLite、PostgreSQL 18.7、MySQL 8.4.13 及 Redis 8.10.3 的授权 HTTP 通过复用、切换周期和到期刷新回归。默认空总计读入缓存后，短时保留空结果，切换周期验证 102 个命中键的总计及 101 行明细上限。Pulse Provider 23 项、Telescope Provider 20 项通过，独占 Telescope Redis 用例按环境条件跳过。隔离快照格式、相关严格 Clippy、三语站点构建和六组浏览器检查通过。证据为 `target/tmp/observability-parity/pulse-activity-card-cache-{macos-baseline,macos-final,macos-retry,docs}.log`。服务器卡片见下节。

### 队列峰值、采样率和连接名

固定 Queues 通过 PHP 8.5.12、真实 SQLite 聚合及 Blade 峰值表达式确认，采样率 0.4 时原始峰值 3 显示为 `~8`。只有一种连接时隐藏连接前缀，出现多个连接时显示完整名称，队列名内部的冒号保留。原生指标响应保留原始曲线，同时返回当前采样率。面板据此显示峰值估算及原始值提示，零采样率显示已有计数。SVG 省略相邻两个零值之间的线段。 [固定队列面板](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/resources/views/livewire/queues.blade.php)

SQLite、PostgreSQL 18.7、MySQL 8.4.13 及具名 Redis 8.10.3 通过真实 HTTP，验证缓存曲线和查询时间保持一致时可应用新的采样率。真实 Pulse 应用的 Chrome 桌面与手机版面验证峰值、标签和图形；受控指标响应另测单双连接切换及零采样率。Pulse 缓存 6 项、Provider 24 项、Telescope Provider 27 项通过，独占 Telescope Redis 用例沿用测试条件。全目标 Pulse 严格 Clippy、隔离快照格式、三语文档及六组浏览器检查通过。证据为 `target/tmp/observability-parity/pulse-queue-card-{upstream,macos-retry,browser-baseline,browser,docs}.log`。

### 队列采样点读数与刷新

固定队列图的提示回调显示选中时点的采样计数。按固定 Blade 中的回调核对，采样率 0.4 时原始计数 3 和 1 分别显示为 `~7.5` 和 `~2.5`。原生图显示本地时间及该时间点各队列事件的读数，支持鼠标、触摸和键盘，五秒刷新后保留仍在图中的时间点及键盘焦点。 [固定队列面板](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/resources/views/livewire/queues.blade.php)

Chrome 在受控指标数据上验证估算值、键盘、触摸与定时刷新，在真实 SQLite 应用上回归指标请求、服务器图及队列图交互。三语站点构建和六组浏览器检查通过。证据为 `target/tmp/observability-parity/pulse-queue-trend-{upstream,macos,docs,docs-retry}.log`。

### 面板刷新与请求采集

固定 PHP 8.5.12 以具名 Livewire 路由和实际请求快照调用 `LivewireRoutes::resolveRoutePath`，刷新请求归入 `/operations/pulse`，原路由保存在 `via` 中。固定 SlowRequests 和 UserRequests 的默认规则过滤此面板路径，保留 `/operations/pulse-report`。原生面板使用独立具名指标入口，将该请求按已注册面板路径交给相同过滤逻辑，继续支持应用清空忽略规则后采集该请求并显示其来源。 [固定路径解析](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/Concerns/LivewireRoutes.php) [固定配置](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/config/pulse.php)

修复前两次面板刷新均产生慢请求记录。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 的真实授权 HTTP 验证冷、热刷新均按配置过滤，普通业务路径继续计入慢请求与用户请求。清空忽略规则后两类采集均恢复，慢请求附带指标入口来源。Pulse Provider 25 项、隔离快照格式和全目标严格 Clippy 通过。证据为 `target/tmp/observability-parity/pulse-dashboard-request-{upstream,macos-baseline,macos-verified}.log`。

### 服务器卡片的心跳与历史缓存

固定 Servers 在同一次五秒缓存查询中读取系统当前值、指标曲线并判断最近上报。Rust 将心跳状态和 CPU／内存历史保存在同一缓存项，按周期及已加载的服务器配置分区，使配置改动可立即选择新结果。忽略期限的输入校验在读缓存前完成。 [固定 Servers::render](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Livewire/Servers.php)

真实 HTTP 前态会立即读到新增的第二台服务器。修复后，SQLite、PostgreSQL 18.7、MySQL 8.4.13 及具名 Redis 8.10.3 验证五秒内心跳和曲线一致、切换周期独立读取、到期后旧心跳变为失效且新节点与相应曲线出现，以及相同周期可立即采用改变后的服务器排序。Pulse Provider 24 项、Telescope Provider 20 项通过，独占 Telescope Redis 用例按环境条件跳过。首轮验收守卫在磁盘清理期间发现仍活跃的构建，恢复空间后重试。隔离快照全仓格式、相关严格 Clippy、三语站点构建及六组浏览器检查通过。证据为 `target/tmp/observability-parity/pulse-server-card-cache-{macos-baseline,macos-final,macos-retry,docs}.log`。

### 查询耗时和执行时间

卡片缓存同时保存查询数据、单调计时器测得的毫秒耗时，以及应用时钟在查询开始时给出的 UTC 时间。五秒内重用同一次查询的数据与时间。已授权的指标响应在 `query_timing` 中提供这些值。原生面板将执行时间转换为浏览器所在时区，在标题提示中显示，缓存总计与逐键明细分别显示，服务器主机名使用服务器卡片的同一次查询时间。新缓存键与旧格式隔离，切换版本时旧条目按原五秒期限自然到期。 [固定模板与时间呈现](https://github.com/laravel/pulse/tree/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/resources)

实际 HTTP 前态缺少时间字段。修复后在 SQLite、PostgreSQL 18.7、MySQL 8.4.13 和具名 Redis 8.10.3 验证首次执行时间精确到秒、缓存命中沿用原时间与结果、到期后更新。缓存已热时仍验证生产模式拒绝未授权的指标访问。Pulse Provider 24 项和 Telescope Provider 20 项通过，另一个 Telescope 测试沿用独占 Redis 验收条件。实际 Chrome 以受控指标响应验证原生脚本在 375 和 1440 像素宽度渲染各类时间提示，包括切换排序。格式、严格 Clippy、三语构建及六组文档浏览器检查通过。证据为 `target/tmp/observability-parity/pulse-query-timing-{macos-baseline,macos-final,browser-baseline,browser,docs}.log`。

### 卡片缓存的自采集隔离

固定 PHP 8.5.12 调用 `Pulse::defaultVendorCacheKeys`，确认默认模式 `/^laravel:pulse:/` 排除卡片查询的缓存键。原生组合在自定义 CacheInteractions 分组下发现查询缓存的冷请求写入了额外的应用缓存计数。原生卡片通过已有的 `Pulse::ignore` 执行内部缓存操作，冷热请求均保持原有查询快照和缓存仓储，避免自有诊断计入用户的缓存 Recorder。 [固定缓存键](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Livewire/Concerns/RemembersQueries.php)

SQLite、PostgreSQL 18.7、MySQL 8.4.13 的严格与非严格模式及独占 Redis 8.10.3 Ingest 验证真实缓存键、冷热请求和指标稳定。专用 Redis 消费入口确认两次面板查询未产生待消费计数。基于干净 `8df774457` 加本批两个源码／测试文件，三扩展默认测试 138 项通过，25 项使用独立环境。全仓格式与 Pulse 严格 Clippy 通过。证据为 `target/tmp/observability-parity/observability-release-candidate-macos.log` 及 `target/tmp/observability-parity/pulse-card-cache-suppression-{upstream,macos}.log`。

### 缓存汇总和采样展示

固定 PHP 8.5.12 与 SQLite 执行 Cache 的实际 `render` 和数据库聚合。102 个命中键加一次未命中得到全量汇总 102 与 1，明细查询 101 项，Blade 取前 100 项。用固定模板实际表达式核对采样率 0.5、一次命中及两次未命中显示 `~2`、`~4` 和 `33.33%`。证据为 `target/tmp/observability-parity/pulse-dashboard-cache-upstream.log`。 [固定 Cache 组件](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Livewire/Cache.php)

Rust 指标入口保留 `cache` 明细，新增 `cache_totals` 提供独立聚合的原始计数和采样率。前端按相同规则显示全局及逐键命中率，明细展示最多 100 项。采样率为零时显示已有原始计数。修复前 HTTP 的汇总字段缺失，证据为 `target/tmp/observability-parity/pulse-dashboard-cache-baseline.log`。

macOS 三库的实际授权 HTTP 验证空汇总、超过明细上限的键及采样率。桌面与移动浏览器验证估算值、全局和逐键原始值提示、截断后的全局与逐键命中率。Pulse Provider 19 项、Pulse 缓存和存储 8 项、Telescope 存储 7 项、相关严格 Clippy、隔离快照格式、三语构建和六组正文检查通过。平台 Release 与完整 Livewire 请求仍待验收。证据为 `target/tmp/observability-parity/pulse-dashboard-cache-{macos-final,regressions,docs}.log`。

### Servers 历史曲线与最近上报

固定 PHP 8.5.12 的 Servers `render` 与真实 SQLite 将主机快照关联到 CPU、内存的平均历史序列。样本 13／33 和 256／512 生成 23 和 384；31 秒前的主机标为未最近上报。默认按主机名升序，`ignoreAfter=15` 可隐藏过期快照。证据为 `target/tmp/observability-parity/pulse-dashboard-servers-upstream.log`。 [固定 Servers 组件](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Livewire/Servers.php)

同样数据经原生真实 HTTP 返回最近的主机快照。CPU 历史断言因缺少序列而失败，最近上报状态、排序及按时限过滤一并保留为接续对齐项。原生复现为 `target/tmp/observability-parity/pulse-dashboard-servers-baseline.log`。

Rust 指标接口保留快照原有字段，增加 `recently_reported`，并通过 `server_graphs` 返回可见主机的 CPU／内存平均历史。默认按主机名升序，保留旧快照。可在 `pulse.dashboard.servers` 配置标量排序字段、方向和过期时限。原生面板保留瞬时读数和磁盘用量，显示两类历史及上报状态，历史缺采样处断开。

同一 macOS 隔离快照的真实 HTTP 通过 SQLite、PostgreSQL 18.7、MySQL 8.4.13，验证平均值、稀疏序列、新旧状态、排序覆盖及数字／时长过期配置。Pulse 与 Telescope Provider 共 37 项通过，独立 Redis 的 Telescope 用例按环境跳过。实际授权页面通过手机和桌面 Chrome 检查，涵盖历史曲线、上报状态、四种时段、仪表、刷新和文字转义。全仓格式、相关严格 Clippy、三语文档构建及六组正文检查通过。证据为 `target/tmp/observability-parity/pulse-dashboard-servers-{verified,docs}.log`。

固定面板的 Servers、Cache、Queues、Usage、Exceptions 及四类慢操作模板均声明五秒轮询。原生面板由每十秒改为每五秒统一请求，保留请求尚未结束时跳过该轮的规则。真实 Chrome 使用可控浏览器时钟验证首次加载、五秒前未请求及五秒时响应成功，桌面和移动功能回归同时通过。证据为 `target/tmp/observability-parity/pulse-dashboard-polling-verified.log`。 [固定面板模板](https://github.com/laravel/pulse/tree/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/resources/views/livewire)

固定历史曲线以最近的采样点显示本地时间和数值。原生 SVG 为每个实际采样点提供覆盖最近区域的提示，显示本地日期、时间、时区和 CPU 百分比或内存 MiB。Chrome 在两个视口检查命中区域、时间和数值。证据为 `target/tmp/observability-parity/pulse-dashboard-server-tooltip-verified.log`。 [固定 Servers 视图](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/resources/views/livewire/servers.blade.php)

实际 Chrome 页面确认触摸点选、键盘切换读数和 Escape 关闭提示。用可控浏览器时钟触发真实五秒请求，重新渲染后同一历史采样点和键盘焦点均保留。移动用例使用 Chrome 触控模拟。证据为 `target/tmp/observability-parity/pulse-dashboard-server-input-verified.log`。

固定 PHP 8.5.12 的 Carbon 相对时间探针验证逗号和 `and` 连接、正负分量、`ago`、周／月／年、相对日期、毫秒和微秒，也复验 `addSeconds` 的实际时间偏移。Rust 通过框架相对区间解析器处理 `ignore_after`，单位换算沿用 Carbon 默认规则，期限比较保留微秒。既有 `Request::interval` 的 ISO／紧凑语法和 Pulse 保留期限配置沿用各自入口。固定上游探针为 `target/tmp/observability-parity/pulse-dashboard-server-interval-upstream.log`。

框架区间 12 项通过，包含既有固定 PHP 样本及本批复合区间回归。实际指标 HTTP 在 SQLite、PostgreSQL 和 MySQL 验证复合时限、月单位、负时限及隐藏主机的历史清理。Pulse 与 Telescope Provider 共 37 项通过，独立 Redis 用例按环境跳过。桌面和移动浏览器、三语站点构建及六组正文检查通过。证据为 `target/tmp/observability-parity/pulse-dashboard-server-interval-{verified,docs}.log`。

服务器当前内存和磁盘读数按固定视图以 1024 换算 MB、GB、TB，内存最多一位小数，磁盘取整数。原生仪表用原始读数计算使用率。实际 Chrome 桌面和移动视口验证内存 `2GB / 8GB` 与 25%、磁盘 `100MB / 200MB` 与 50%。证据为 `target/tmp/observability-parity/pulse-dashboard-server-size-verified.log`。

固定 PHP 的实际 `render` 确认数值主机名按数值比较，`cpu`、`memory`、`storage` 可作为集合排序字段，`storage.0.used` 支持嵌套取值。未知字段保留原始存储顺序。Rust 复用框架路径读取与 PHP 数值解析，按这些字段排序并保留等值记录的读取顺序。真实 HTTP 前态将 10 排在 2 之前。相同数据在固定 PHP、SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 验证数值主机名、CPU 历史、磁盘集合、嵌套字段和未知字段。Pulse 与 Telescope Provider 共 38 项通过，独立 Redis 用例按环境跳过。全仓格式、相关严格 Clippy、桌面和移动 Chrome、三语站点及六组正文检查通过。证据为 `target/tmp/observability-parity/pulse-dashboard-server-sort-{exact-upstream,baseline,verified,docs}.log`。

混合字段可能出现循环比较。Rust 使用稳定排序处理此类输入，存在循环时的确切输出顺序仍需与 Zend 排序逐项对照。 [PHP 8.5.12 比较实现](https://github.com/php/php-src/blob/php-8.5.12/Zend/zend_operators.c#L3258-L3305) [PHP 8.5.12 排序实现](https://github.com/php/php-src/blob/php-8.5.12/Zend/zend_sort.c#L241-L298)

PHP 相对时间语法的其余边界、Livewire 更新事件和缓存包装仍需消费者验收。目标平台与发布组合继续验收。

## 采集暂停回调中的显式状态切换

固定 Pulse 的 `ignore` 进入时暂停采集，回调可用 `startRecording` 临时恢复，退出时恢复入口状态。PHP 8.5.12 通过实际 `record` 和 `wantsIngesting` 确认原先开启／关闭、嵌套回调与异常退出。证据为 `target/tmp/observability-parity/pulse-ignore-control-upstream.log`。原生前态的真实 HTTP 回归在回调结束后仍保持错误的采集状态，见 `pulse-ignore-control-macos-baseline.log`。 [固定 Pulse](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Pulse.php#L225-L263)

原生回调在当前应用任务和同一 Pulse 实例内临时切换采集。嵌套回调结束或取消后恢复之前状态，其他容器实例仍沿用内部遥测抑制。真实 HTTP 和自定义 Recorder 验证默认暂停、主动恢复、嵌套切换、取消和应用隔离，实际写入 SQLite、PostgreSQL 18.7、MySQL 8.4.13。Pulse Provider 27 项、隔离快照格式、Pulse 库和 Provider 严格 Clippy、三语文档构建及桌面和移动视口通过。证据为 `target/tmp/observability-parity/observability-host-and-{pulse-ignore-macos,ignore-docs}.log`。最终 Release 部署组合继续验收。

## pulse:check 的心跳实例与 Vapor

固定 `CheckCommand::handle` 通过 `Env::get` 判断 Vapor，使用 `Str::random()` 的默认 16 个字母数字字符作为一次命令运行的 `SharedBeat.instance`。PHP 8.5.12 的命令探针接入真实事件 Dispatcher 和 ArrayStore，替换数据写入及 Telescope 收集边界。空值、`0`、大小写混合的 `false`、`(false)`、`(null)`、`EMPTY` 和带成对引号的 `0` 使用普通实例。带成对引号的字符串 `false` 和参数路径启用 Vapor，只检查一次并派发 `vapor`。证据为 `target/tmp/observability-parity/pulse-check-instance-fixed-command.log`。 [固定 CheckCommand](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Commands/CheckCommand.php) [固定 Env](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Env.php#L252-L276)

前态的真实 Artisan 普通心跳给出 32 位十六进制标识，且把非空环境保留字误判为启用。Rust 复用框架 `Str::random(16)`，按固定环境规则转换保留字和成对引号。实际 Artisan 用独立应用环境文件捕获公开的 SharedBeat，按八个环境值检查实例形状，并在 Vapor 用例省略 `--once` 验证自动结束。首轮编译因宿主磁盘停线退出，空间恢复后重跑。macOS 的 cache 6 项、Provider 21 项、隔离快照格式、相关目标严格 Clippy、三语站点构建及六组桌面／移动页面通过。Docker Redis 首次出现缓存后端错误。同一测试二进制使用官方 8.10.3 源码编译的独占本机 Redis 后，通过心跳命令及既有命名缓存、锁和重启回归。随后使用项目提供的认证容器夹具复跑，两项也通过，记录于 `target/tmp/observability-parity/pulse-check-instance-macos-redis-retry.log`。证据为 `target/tmp/observability-parity/pulse-check-instance-{macos-baseline,macos-baseline-retry,macos-final,macos-lint,macos-native-redis,docs}.log`。其他平台及发布组合另行验收。

## SlowJobs 任务尝试计时

固定代码在 `JobProcessing` 开始计时，遇到 `JobProcessed`、`JobFailed` 或 `JobReleasedAfterException` 后计入首个终止事件。PHP 8.5.12 经真实事件分派和上游 Ingest 消费者核对，跨秒的失败尝试为 75 毫秒，释放为 134 毫秒，成功为 49 毫秒。随后的第二个终止事件、`JobAttempted` 与 `sync` 连接均不增加慢任务记录。证据为 `target/tmp/observability-parity/pulse-slow-job-lifecycle-upstream.log`。 [固定 SlowJobs Recorder](https://github.com/laravel/pulse/blob/33cc6fc68cd3052aa06227e88e8d933dd3d70a51/src/Recorders/SlowJobs.php)

修复前只读取 `JobAttempted.duration`，遗漏处理前监听器和队列结算耗时。Rust 已改为按连接、队列、保留任务标识和尝试次数记录开始时间，以第一个终止事件当时的时间戳计算毫秒耗时，再进入原有采样、过滤和聚合。`JobAttempted` 仅清理未产生对应终止事件的尝试。

macOS 上真实 Artisan SQLite Worker 经过 40 毫秒的处理前监听器，执行四次成功、一次终态失败和一次因异常释放。原实现入库的六条慢任务均未达到 40 毫秒，修复后的六条全部达到，并核对队列处理及释放计数。相同源码通过 Worker 和 Provider 共 18 项、涉及文件格式及 Pulse 库和两个测试目标的严格 Clippy。三语文档构建、渲染正文和 12 组三语桌面／移动浏览器检查通过。证据为 `target/tmp/observability-parity/pulse-slow-job-lifecycle-{baseline,restored,docs}.log`。

固定 PHP 在会退出的超时路径结束进程。Rust 的嵌入式 Artisan 可以只返回退出码，使应用及 Recorder 实例继续运行。前态保留了这类尚未结算尝试，测试向同一个 Recorder 重放超时事件所携带的尝试标识，错误地产生一条慢任务记录。

Rust 将时间戳清理挂到所属 Worker 尝试已有的延迟回调集合。集合执行或丢弃时清理该代标记，保留首个正常终止事件的计时。相同标识的新尝试拥有独立标记。真实 SQLite、PostgreSQL 18.7、MySQL 8.4.13 上分别验证退出型超时保留队列任务并清除时间戳，以及可重试超时正确记录释放和后续成功两次。两个 Worker 回归、默认 Provider、隔离快照格式及所涉严格 Clippy 通过。证据为 `target/tmp/observability-parity/pulse-worker-timeout-{baseline,final}.log`。 [固定 Laravel Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php#L326-L368)

同一原生 Artisan 入口另外使用独占 Redis 8.10.3 队列及 SQLite 指标存储，两个 Worker 用例均通过。验收通过当前驱动的队列大小确认超时保留和正常清空。三种数据库队列组合同时重跑通过。证据为 `target/tmp/observability-parity/pulse-worker-timeout-queue-matrix.log`。其他目标平台与外部队列驱动继续验收。
