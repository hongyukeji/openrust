# Pulse

## 调研与范围

2026-11-21联网读取GitHub官方仓库API，laravel/pulse为活跃MIT项目（1733 stars，仅作当次背景，不作为采用依据）；web搜索／网页解析未返回正文，实际使用GitHub API与固定源码归档。固定来源[laravel/pulse](https://github.com/laravel/pulse/tree/33cc6fc68cd3052aa06227e88e8d933dd3d70a51)，完整归档位于tmp/openrust-ecosystem-20261121/upstream/laravel-pulse。归档未含tests，另从raw.githubusercontent.com取得固定DatabaseStorageTest.php，存于tmp/openrust-pennant-20261121/pulse-storage-upstream-tests.php。旧版v0.x未发现独立Pulse包。

采用官方Entry／Value、数据库四种bucket、预聚合与tail组合、Storage／Redis／Null ingest、Recorder和命令职责；复用原生数据库／Redis／事件／缓存／生命周期及授权，不另建通用监控内核。许可证随包保留。PHP Livewire需转换为原生授权HTTP界面，静态类型注册代替动态类扫描，差异与验收分别记录。

## 当前进度

本节为实现期保存点，最终验收见文末。首先完成Entry／Value与三库SQL聚合存储，再实现ingest、记录器／生命周期、命令与授权面板，最后三语文档、浏览器及全组合验收。尚未验收通过，不计作交付。用户要求独立执行，不联系其他对话，问题与结果直接维护本目录及交付账本。

## 实现与验证保存点（2026-11-21）

已接入原生三表迁移、四种bucket、Storage／Null／Redis ingest、HTTP／队列／缓存／异常／慢SQL／外部HTTP记录器、服务器采样、四条Artisan命令及授权面板。默认应用保持可选包发现，配置／迁移／视图归包；发布视图通过原生Factory覆盖，已解析路径缓存遵循原生ViewFinder语义。Facade提供常用静态入口，ResolvesUsers可替换；默认用户详情复用原生认证Provider，读取不到记录时显示ID。

- 系统采样复用[sysinfo 0.38.4](https://github.com/GuillaumeGomez/sysinfo/tree/v0.38.4)，MIT，MSRV1.88；仅启用system／disk，原生OS调用替换上游shell管道。CPU／内存支持应用内替换检测器，SharedBeat按实例15秒节流，异步入口使用spawn_blocking。
- 慢查询定位复用[backtrace 0.3.76](https://github.com/rust-lang/backtrace-rs/tree/775f6a1ba62e7d35a1fac76e64c61d9d4687b5f2)，MIT或Apache2，MSRV1.82；排除框架／Pulse／依赖内部帧。发布时未保留调试符号可能无法恢复源码位置，返回空字符串；Rust Error不保证携带调用栈，异常位置可显式通过report／ExceptionReported提供。
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

O05单包验收关闭，接续O06。语言适配明确保留：原生HTML／JS替换PHP Livewire；显式Recorder注册替代反射；css重载用withCss；aggregateTotal切片统一返回按类型结果；带类型异常由class／location表达并异步派发；用户默认通用认证Provider逐ID查询，自定义ResolvesUsers可批量；legacy users用ID映射；应用作用域自动选择容器，无setContainer变更操作。无调试符号时不保证源码定位，错误与取消不保证持久入库。最终全扩展／API组合及Horizon／Telescope完整上游差异继续单列，不能计为整个路线图完成。
