# 框架对齐开发计划

日期：2026-11-22。状态：用户已授权接手全部差异的实现与验收，正在持续实施。用户追加授权：已完成并验收的依赖闭合批次及时本地提交；未授权推送。原审计及计划生成时的事实保留为历史，当前结果以下方实施记录及统一数据中的逐项状态为准。

本次实际执行的需求、编号、依赖、路径、联网来源与状态核对见[计划重做核对](#plan-review)。该记录不是框架功能验收结论。


## 本地分批提交（2026-11-22）

用户已明确要求“能提交的文件，处理完成后就提交，不要持续堆积”。已核对原有验收记录并按依赖提交：

| 提交 | 范围与验收依据 |
|---|---|
| c84c4a9fb | 磁盘守卫、规范入口；本次重跑8项隔离测试通过。启动前、运行中、正常/失败/取消退出后复查，Cargo锁及协作者保护。 |
| ba651bbbd | JsonSchema类型、工厂、反序列化、三语用法及来源；此前6项Rust与39组真实固定PHP对照。 |
| 8ff569d77 | manager/哈希/日志/译器/进程/URL公开接口与三语正文；使用既有定向及严格Clippy/三语站点验收，不称本次全部重跑。 |
| 3de36e733 | 事务管道与缓存锁；既有三库事务及file跨进程、Redis/database锁矩阵。管道用法随后随helpers正文提交。 |
| 244683407 | 验证器、消息袋、Lottery及HTTP测试依赖簇；既有77规则/25,499真实Validator/HTTP固定oracle、严格Clippy及三语站点验收。 |
| b74bfa561 | 将前一提交误纳入的并行新增blade_query_helpers测试移回未跟踪WIP，源码保留。对应编译器改动不属于本轮已验批次。 |

以上是受支持范围的提交，不能据此关闭完整API裁决。Query C16–C19仍在补验，未混入这些提交；欢迎页、语言控件、版本查询及新模板改动均保留为协作者WIP。索引操作以逐文件/部分正文为单位，不提交全部工作区。未推送。

Q03补充：原临时守卫只在运行中检查、未自动清理且漏掉退出瞬间；新tests/build_guard.py覆盖上述边界，并将真实清理事件写入target/tmp/build-guard-events.jsonl。绕过入口的raw Cargo仍不能自动拦截，须执行规范中的阶段检查。第四次实际安全清理见disk-cleanup-query.json：12个profile，锁inode保留，清理前28,469,579,776字节可用，之后94,545,403,904，实测净释放66,075,824,128字节。此为此前阶段检查触发的实际清理，不冒称由新守卫执行；本次守卫测试只清隔离假项目。仍保留正常增量与并发。当前其他协作者构建继续保留，不触线不清理热缓存。

## 多态聚合入口（2026-11-23）

C25补Model::loadMorphMax/loadMorphMin/loadMorphSum/loadMorphAvg，四个方法直接委托既有loadMorphAggregate及RelationAggregateQuery，不引入另一套聚合引擎。固定Framework13.40.0 Model.php:1063–1104为同样四个委托入口；v0.x对应model无这些接口，沿用当前已验多态目标/聚合/快照回写。公开参数仍是既有Rust类型化relation/alias而非PHP动态关系映射，返回Result<()>，这项语言适配不能记成签名1:1。

真实PHP8.5.12 PDO SQLite生成16组四种聚合×有数据/空关系/其他目标/空目标参考，Rust1项集成用例全部核对通过，另覆盖全局scope过滤、自定义属性别名、original同步、SQL错误和错误目标类型不改缓存。PostgreSQL18.7与MySQL8.4.13隔离库各实际执行同一用例并通过；数值按驱动返回可能为文本/整数/浮点，不伪装类型一致。fmt、workspace/all-targets严格Clippy、三语build、12组合浏览器和双前缀全导航通过。证据：target/tmp/framework-parity-20261122/eloquent-morph-reference.log / morph-{postgres,mysql}.log / clippy-morph.log / fmt-morph.log / docs-{build,browser,scope}-morph.log；参考生成脚本和JSON纳入tests/reference。C20/C21/C23/C24及更广泛ORM审计继续，未全量关闭。

## ORM 主键与单值（2026-11-23）

C22补whereKey列表、whereKeyNot/orWhereKey/orWhereKeyNot、value/sole/soleValue/valueOrFail；复用原Conditions整数/集合/分组、scoped水合及关系查询既有sole_match基数错误检查。主键带表限定，OR不越过全局scope；单值读取经过真实模型访问器/转换/检索生命周期，不直接取底层SQL标量。原有投影保留，SQL NULL和无行分别按对应方法职责处理。公开Key枚举是PHP单值/数组参数的Rust表达，已有Into<Value>标量仍可调用，Vec和数组直接支持；模型参数显式取getKey。PHP Expression与动态模型/Arrayable输入全集仍需后续签名裁决。

固定来源为Framework13.40.0 Eloquent Builder 285–358/880–920及现有Query/Model/relations实现。旧版v0.x只有旧model元数据/SeaORM组织，无当前builder.rs，未直接复制不兼容的旧架构。联网页面工具未返回内容，采用已有固定源码与真实PHP8.5.12 PDO SQLite，136组主键+45组属性/基数=181组参考由纳入测试的脚本生成，Rust全部通过。

本轮eloquent_key_values4项通过（含181参考），SQLite模型既有回归2项实际执行；eloquent_models中的MySQL测试因无其专属变量只提前返回，不计实际MySQL。另经隔离入口，PostgreSQL18.7及MySQL8.4.13各3项主键/模型属性用例实际通过，表名按用例隔离，正常并发运行。fmt、workspace/all-targets严格Clippy、三语build、12页面浏览器、根及/docs/全导航通过。证据：eloquent-key-third.log / eloquent-{postgres,mysql}.log / clippy-eloquent-key.log / fmt-eloquent-key.log / docs-{build,browser,scope}-eloquent-key.log，均位于target/tmp/framework-parity-20261122/。首次helper可见性与测试类型推断编译失败已修复，保留原日志，不算作最终通过。

## Query 与重定向补验（2026-11-23）

C16–C19新增来源、值连接、二进制/整数集合/行值/组合OR条件、HAVING组合、随机排序、ID翻页、rawValue与惰性存在回调，沿用既有AST/绑定/方言。参考固定上游Builder/Grammar与现有查询实现，真实PHP固定源码156组SQL/绑定对照。Composer使用真实symfony/polyfill-php86 v1.36.0补齐SortDirection，未编造枚举；联网页面读取未返回内容，不能称新增联网结论。

本轮新守卫入口验收：database_query22、database_query_writes18、query_parity4项通过，另1项PG专项明确ignored，非空跑通过。隔离PostgreSQL18.7显式执行3项查询及1项PG编码专项通过；MySQL8.4.13实际3项通过。非法rowValues运算符在添加时立即拒绝并覆盖and/or。PG服务端将无类型参数推断为文本时按TEXT/VARCHAR/BPCHAR/NAME文本编码，覆盖标量/null/JSON/bytes/非法UTF8及强类型bigint恢复；不通过强制SQL cast掩盖原问题。查询失败不调用存在回调，回调错误传播。C20分类绑定尚未实现。

C11–C15追加真实Session默认/login命名错误袋的返回上一页断言，正反例、缺key/错误bag/无错误反例通过；本轮test_response_assertions9项与metadata7项全部通过。

本轮完整快照fmt --all --check、workspace/all-targets严格Clippy通过。Query三语正文补使用方式与当前typed/driver边界，三语构建、12页面组合浏览器、根及/docs/双前缀全部导航通过。证据：target/tmp/framework-parity-20261122/{query-current-guard,query-postgres-explicit,query-mysql-explicit,redirect-errors-current,fmt-query-current,clippy-query-current,docs-build-query,docs-browser-query,docs-scope-query}.log。只提交这些明确文件，协作者新模板与版本/欢迎页不计通过。

保留差异：整数raw集合仅Vec<i64>而非PHP动态转换；existsOr返回类型化Satisfied/Callback(T)；空HAVING回调在Rust不添加条件，而固定PHP对应完全空闭包抛TypeError；binary只提供MySQL语法；PostgreSQL无类型投影可能为Value::Text。完整API、动态参数及所有方言行为尚未关闭，不把本轮通过扩展为全框架完成。

## 原生日志访问验收（2026-11-23）

C49在已有通道解析器、sink、格式器和处理器上拆出真实 `NativeLogger`。`Logger::getLogger` 返回实际使用的 `Arc<NativeLogger>`，`LogManager::getLogger` 与 `Log::getLogger` 解析默认通道并返回同一实例；forget后已持有句柄继续写入，新解析使用新实例。原生写入只用调用上下文，不合并频道上下文或Context元数据，不派发MessageLogged/Pail；真实输出和处理器失败直接返回，框架写入继续保留原有恢复职责。发布前tap沿用现有配置入口，不另建日志引擎或增加Rust依赖。

固定Framework v13.40.0 Logger.php和LogManager.php、Monolog3.9.0与psr/log3.0.2为参考。真实PHP8.5.12/Monolog输出与事件JSON对照通过。官方固定raw文件经curl核对，与已有固定源码9401字节及SHA256 `b7382fcab927960291e1c6b595cb0755b46e4fe6d7d94f91e7372a268a61fdd1` 一致；来源 https://raw.githubusercontent.com/laravel/framework/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/Logger.php 。实际PHP确认低于级别的框架日志不派事件，现有实现正确，未提交临时错误判断产生的改动。旧版v0.x的logging.rs及logging/manager.rs使用不同async LogSink组织，未搬入不兼容架构。

`native-log-final.log`中新增3项和既有logging15、context3、listeners7项全部通过（共28项）；真实应用booted生命周期分别调用Facade、manager、channel验证同实例，缺失Facade作用域的首次失败已修复，未绕过公开入口。覆盖级别过滤、处理器、格式器、真实文件、错误返回、应急恢复、懒解析、缓存、withName及上下文/事件隔离。`fmt-native-log.log`、`clippy-native-log-final.log`通过，后者为隔离快照workspace/all-targets严格Clippy；三语build/browser/scope通过，证据`docs-build-native-log-final.log`、`docs-browser-native-log.log`、`docs-scope-native-log-final.log`。PHP参考安装记录在`native-log-composer.log`，未将参考依赖声明为通过安全审计。

构建触及32GB停线后中断的Clippy和scope未计通过；协作守卫持14个Cargo profile锁清理编译/文档产物，保留锁inode、源码、WIP、日志、下载和数据。`target/tmp/build-guard-events.jsonl`记录可用31,805,612,032至133,673,054,208字节，净释放101,867,442,176字节；恢复后重新构建并执行最终检查。原中断及ENOENT日志保留。

Rust适配：返回拥有型Arc及Result，原生类型为Rust执行器而非PHP Monolog对象；共享通道运行期可变Monolog对象接口和全部Monolog动态API未伪造。getLogger/getChannels本批职责已实现并验收，不据此关闭Log全部接口、真实HTTP sink或整个框架。以上日志除全局清理事件外均在`target/tmp/framework-parity-20261122/`。

## 当前实施与验收（2026-11-22）

本节记录正在实施的事实，不能将清单登记或API出现当作全部完成。源码基线为b214e46be及实时WIP；协作者SetLocale实例回调与其三语正文、规范和验收记录保留。本批未操作开发数据或Git索引。

- C01补JsonSchema类型层、工厂与Deserializer；固定PHP8.5.12执行39组对照，Rust6项通过。静态/实例工厂、Result和128层栈深度保护是明确Rust适配，完整边界继续裁决。MIT来源纳入既有THIRD_PARTY_NOTICES；golden生成脚本纳入测试reference。
- C02/C03补有序MessageBag及真正ViewErrorBag，named/default bag、session flash与View共享错误已接通，5项消息袋及相关验证/HTTP回归通过。C03的PHP稀疏/关联内层键、递归数组merge和JSON flags仍未处理。
- C28补五项URL资源与origin/default接口，含清除CDN、显式协议、子路径、实例隔离；urls6项通过。Facade服务的可变来源配置契约和输入防护差异继续核对。
- C32进程顺序/负向/零执行断言已补，15项通过含断言反例。assertRan/assertRanTimes/assertDidntRun已改为记录与Option结果双参数，factory返回&Self；typed闭包替代动态匹配，失败记录职责仍需核对。
- C33邮件实例清理、C42哈希默认参数setter、C50会话配置查询已补；对应manager2项/hash9项通过，完整历史行为回归仍按清单推进。Session配置为装配快照，不声称可实时读取后续Config修改。
- C34补可替换Selector与getLoader，2项选择器及6项Blade通过；实际临时文件Loader读取追加用例通过。C45只补Response状态Builder（authorization4项通过），Gate策略发现相关接口未完成。C49只补getChannels，真实写入/懒解析/快照已验收，logging15项在短项目内TMPDIR通过；原长路径3项Unix socket失败保留。底层logger适配待裁决。
- C40在既有trait补forceRelease/currentOwner/restoreLock原名，不造新锁引擎；实际PostgreSQL18.7/MySQL8.4.13/SQLite/Redis8.10.3/file矩阵通过，含争抢/等待/所有者/恢复/强制释放/过期，welcome11项通过；真实跨进程file争抢/错误owner/restoreLock释放/重获已在file-lock-cross-process.log通过。

- C09补运行期值/规则/消息/属性/值名/译器及typed异常工厂；6项mutation含4组真实固定PHP操作序列通过，上传标量替换移除旧文件引用，真实HTTP自定义400与普通422隔离通过。getRules已应用Precognition展开字段过滤。getRules读取时展开、变更后显式重验和typed异常替代PHP类名为明确Rust适配；扩展/replacer及全生命周期仍开放。
- C53的withinTransaction改为仅配置，then/thenReturn触发连接解析及事务；显式Transaction上下文贯穿阶段/终点，finally在提交/回滚后及恐慌/取消时执行一次。SQLite/PostgreSQL18/MySQL8.4实际成功/SQL错误/取消/嵌套复用通过；新增默认连接及缺失连接finally在SQLite复验。当前mutation-pipeline-second.log共40项通过、1浏览器专项未执行；clippy-mutation-pipeline.log严格workspace/all-targets通过，fmt及三语build通过。先前Precognition键排序预期不符已修，失败日志保留；via/setContainer等动态职责仍继续裁决。

- C08在现有执行器内补Factory的extend/extendImplicit/extendDependent/replacer/resolver、trait/Facade及实例单项/复数注册；工厂快照和resolver无锁执行保留实例隔离。include/excludeUnvalidatedArrayKeys及公开策略已生效，修正原投影对任意父数组过宽的裁剪，现只裁无参数array/list且有子声明的父字段；validated/safe随策略即时重算。自定义replacer接管规则参数替换，不先执行:min等内置替换。extensions-fifth.log55项通过，含92组真实PHP扩展、16组数组策略、typed min/dyn Factory、真实HTTP两应用并发隔离；既有25499组Validator/199属性/4mutation保持通过。严格Clippy、fmt、三语build/browser/scope均通过。precognition-browser-extensions.log补验官方助手2.0.0在Chromium153真实成功/拒绝/提交/上传/省略文件通过。回调只读/Option/Result、拥有型resolver及动态Class@method/子类边界明确；通用:index等占位符、全typed输入域与完整API继续裁决。
- 该批构建再次触发32GB停线，guard安全停构建后取得10个Cargo profile锁并保留inode，清除本项目全部编译内容和docs dist，保留源码、WIP、日志、下载、独立交付文件及数据。实测清理前36.351328256GB可用，清理后94.875521024GB，净释放58.524192768GB；停止构建后的清理前实测与触发瞬间不同，均按日志记录，不混写。证据extensions-first.log及disk-cleanup-extensions.json；继续保留正常增量/并发与复用缓存。

- C11/C12补状态、头缺失、正文/文本/顺序/负向、JSON子集/片段/缺失/路径/批量/严格结构等断言。test-response-json-expanded.log中3035组真实正文/实体、60组允许标签和4064组JSON固定PHP+PHPUnit12.5.8对照通过，含7HTTP/2字符串/6断言测试。共享去标签改为固定PHP状态机，修正HTML401 lang/rang，不用正则或HTML5表冒充HTML401；旧Stringable属性中>保留预期已用实际PHP纠正。动态callback/完整混合排序/深度保护等输入域仍开放。
- C13追加具名Session错误包、闪存输入、valid/invalid与JSON错误消息；2128组真实Store/PHPUnit参考已生成，当前验收进行中。C15补按TestApp实例的ResponseCaptureOptions/withResponseCapture，保留1MiB/5秒默认保护；新增大小/时间/取消/恢复测试待验。启用Session暴露相对URL未补origin的真实测试入口问题，已按MakesHttpRequests复用应用URL生成器准备地址；夹具只对自己的fixture端点显式CSRF例外。未以首轮419/500或测试未运行计通过。
- 正文构建触发停线后安全清理11个profile，保留Cargo锁inode与源码/日志/下载/数据。disk-cleanup-content.json实测可用31.815520256GB至95.333986304GB，净释放63.518466048GB；继续使用正常增量与并发。

- C11–C15当前追加验收：89项HTTP/request/strings/view/time回归通过，后增地址专项与metadata7项通过。2128组真实Session对照与跨请求闪存生命周期、3项大小/超时/取消恢复测试已验；原始视图/creator/composer、签名路由/上一页、流及文件、Cookie live clock、17组真实PHP编码参考通过。修复响应头忽略大小写和包含断言。严格workspace/all-targets Clippy及三语build/browser/scope通过；后续Query不在该Clippy快照。动态参数/混合JSON排序/完整Cookie与下载边界仍继续裁决，全任务未完成。日志test-response-metadata-fifth.log、test-response-origins-second.log、clippy-testing-second.log和docs-*-testing.log。

证据统一在`target/tmp/framework-parity-20261122/`。`regression-current.log`共86项通过，`translation-selectors.log`另8项通过；`clippy-third.log`为该快照workspace/all-targets严格Clippy通过，之后译器/日志等改动仍须重新运行。三语内容、站点构建、12项页面组合及`/`、`/docs/`完整导航通过；后续追加正文同样重验。初次Clippy的type_complexity与needless_borrow已修，第二次查出welcome旧MessageBag调用已修；保留各失败日志，不掩盖。

构建使用项目内隔离源码、TMPDIR与空闲的welcome-locale工作流Cargo缓存，不复制缓存、不禁用增量、不限并发。Clippy期间触及32GB停线，guarded_run安全停构建后确认无Cargo进程，取得10个profile排他锁并保留inode，清除本项目全部编译内容与文档dist/cache；实测从31.952089088GB可用恢复92.411977728GB，净释放60.459888640GB（见disk-cleanup.json）。历史日志、源码、WIP、下载和数据保留。已证实快照路径/profile变体与多缓存叠加增长，临时guard只停止并不会替代理执行清理；Q03完整入口修复尚未完成。

- C04补Lottery胜负回调、强制结果/sequence/reset与异步scope；5项Lottery、7项异常报告及9组固定PHP对照通过。chooseTimes、typed回调、同步线程/异步任务作用域、非有限概率拒绝和异步scope外静态控制拒绝是明确适配。
- C07的77条原缺失字符串规则均已接入。199组属性方法、25,499组完整固定PHP Validator对照及41项定向测试通过，含真实multipart排除/文件KB比较、三时区冻结时钟及真实DNS A/AAAA/NXDOMAIN/SERVFAIL/超时/取消恢复。当前快照严格Clippy、fmt、三语构建、12浏览器组合及双前缀全部导航通过，证据validation-77-second.log、clippy-validation-77-final.log及docs-*-validation-77.log。
- 日期规则复用Calendar；timezone元数据由同一固定IANA2025b/PHP8.5.0生成器提取canonical与国家信息，PHP8.5.12的2026.5库598标识分组对照全部一致，不升级运行期时区数据。DNS采用成熟Hickory0.25.2的真实传输层，避免高层localhost合成/hosts查询冒充DNS记录。fakeDnsLookups为验证器实例开关，setDnsResolver提供服务器/超时/重试配置；不声明PHP全局静态、缓存或所有特殊域名语义已等价。
- 失败回归修正数值分类实际每条规则重置、浮点消息与null同型大小、主机字符串0的PHP假值、DNS本地合成结果误通过。保留全部失败日志；新增psr/container2.0.2等真实PHP依赖的source refs见统一数据。原77名称有实现不等于整个C07完成：既有/typed规则矩阵、动态PHP输入、特殊DNS/IDNA及JSON表达等继续审计；输入预算、JSON低于128层、指数±1000和Result适配仍明确保留。

Q02全部声明/类型/模块/19包裁决、余下基础组件、Validation其余规则、Testing/Query/ORM、真实流式sink与Reverb signin等仍按下方依赖顺序实施；本节不是最终交付结论。

## 模型保存删除变体验收（2026-11-23）

C24 已补 `saveOrFail`、`updateOrFail`、`updateQuietly`、`deleteOrFail`、`saveOrIgnore`，复用现有保存引擎、模型连接事务和驱动 `insertOrIgnoreReturning`。22组真实固定 PHP/PDO SQLite 参考覆盖成功、空模型、不存在模型、保存/创建/更新/删除取消、唯一冲突、保存/删除后监听器失败、quiet、忽略冲突与事件修改。SQLite、独立 PostgreSQL 18.7、MySQL 8.4.13 各1项集成通过，另测任务取消后数据库与审计写入回滚。MySQL/SQL Server 的忽略插入 RETURNING 按固定 Grammar 明确不支持；MySQL 验证 Unsupported，不称22组全部支持。

真实对照发现原实现提前同步 original：保存后监听器失败会错误清除 dirty，成功监听器修改却保留为 dirty。红测试 `model-write-original-red.log` 复现；修复为保存后事件/owner touch成功才从最终实例属性同步，22组版本通过。普通模型与事件5项回归通过，其中旧MySQL缺变量早退不计真实数据库；关系6项在 `RUST_MIN_STACK=16777216` 下通过，默认线程栈曾溢出（`model-write-final.log`），只证明增栈可通过，默认栈原因仍需定位，不掩盖为原环境通过。

`fmt-model-write.log`、`clippy-model-write.log`、`docs-{build,browser,scope}-model-write.log` 均通过，包含严格workspace/all-targets Clippy、三语构建、12页面组合与双前缀全部导航。证据及隔离驱动位于 `target/tmp/framework-parity-20261122/`；源码参考固定 Framework v13.40.0 的 Model.php，来源及许可沿用既有记录。新联网读取仍为空，未宣称额外联网核实。真实PHP参考补用 serializable-closure v2.0.10 测试依赖，Composer报告该包6项advisory，仅用于隔离参考工具，不将其声明为已通过安全审计。

后续已解除默认栈问题：只将忽略插入的驱动 Future 装箱，避免它扩大普通 save 的状态体；`model-write-boxed.log` 同一关系6项在默认线程栈通过，写入1项及隔离PG/MySQL重验通过，无需设置 RUST_MIN_STACK。`clippy-model-write-boxed-clean.log` 与 fmt通过。初轮全量复验混入协作者尚未完成的语言配置类型/格式错误，已在独立快照使用HEAD加本批修复验收，未修改其工作区；失败日志保留。此结果覆盖此前“默认栈仍待定位”的限制。

Rust通过 `saveInTransaction`/`deleteInTransaction` hook执行事务变体；已有外层事务显式传入，未实现PHP环境式事务或动态save覆盖及options数组。模型内存状态不会因数据库回滚自动复原，符合实测参考。C20/C21/C23及其他未完成任务继续实施，此处不是全任务完成结论。

## 模型作用域与查询构造器验收（2026-11-23）

C21已实现 `withGlobalScope`、`withoutGlobalScopesExcept`、`removedScopes`、`applyScopes`、`getQuery`、`setQuery`、`getEagerLoads`、`setEagerLoads`，增加 `Scope::extend` 注册钩子和 `Model::newEloquentBuilder` 默认查询工厂。采用现有 Scope/Conditions/EagerLoads，不引入依赖或第二套查询器。注册表按顺序覆盖同名项，移除历史保留顺序/重复，清空后新增与移除后重注册可生效；软删除成为公开 `SoftDeletingScope` 并使用同一注册/移除路径。Rust类型名通过 `std::any::type_name` 引用，无PHP类名别名。

23组真实固定PHP/PDO SQLite参考覆盖默认/覆盖/移除/仅保留/OR保护、软删除、查询替换、未应用及重复应用作用域、预加载替换与关系模型隔离。SQLite、独立PG18.7、MySQL8.4.13各1项集成通过，另验证工厂公开消费、extend/apply时机、错误恢复及并发查询隔离。相关事件/主键181参考/模型/聚合/写入12项，默认栈关系6项、factory/force-fill/路由9项通过；旧MySQL缺变量早退不算真实库。首轮测试夹具签名/关系输出类型错误已修，日志保留；登记表堆存放后关系回归默认栈通过。fmt、严格workspace/all-targets Clippy、三语build/12页面组合/双前缀全导航通过，日志 `scopes-{fourth,third,regression,postgres,mysql}.log`、`clippy-scopes.log`、`fmt-scopes.log`、`docs-*-scopes.log`。

来源为固定Framework v13.40.0 Builder.php/SoftDeletingScope.php；本地旧版v0.x的Model元数据与Query使用不同SeaQuery装配，查阅后未直接搬入。联网raw读取仍为空，采用既有固定源码与真实PHP oracle，不宣称新增联网验证。`getQuery`/`applyScopes`用消费所有权代替PHP共享对象/隐式克隆，工厂返回框架typed Builder而非任意运行期派生类；应用额外方法用Rust trait/wrapper，文档已明确。动态Model静态scope注册/宏等全域不据此冒称完成。C20分类bindings独立未实现，此批未依赖其公开API，未把依赖状态改成已交付。

## 严格属性与关联更新时间（2026-11-23）

C23补 shouldBeStrict、三项prevent策略、丢弃/缺失处理钩子及 withoutTouching/withoutTouchingOn/isIgnoringTouch，沿用应用容器、现有属性转换、模型水合与关联touch路径。严格配置按应用和模型类型隔离，显式传入app；类型化钩子替代PHP共享静态回调。原生/枚举缺失读取、访问器、SQL NULL、新建/最近创建实例及处理器返回值按固定源码执行。getAttribute/getRawOriginal保留原始Option低层接口；严格读取使用getAttributeValue/getEnum，这些签名与静态配置差异不称为字面1:1。多模型水合在Retrieved之后记录lazy策略快照，单模型不自动设置，已水合实例保留原策略。

真实PHP8.5.12/PDO SQLite生成51项参考，50项对应一致行为，1项记录有意差异：同类型嵌套withoutTouching的固定上游结果为[true,true,false,false]，Rust恢复外层作用域为[true,true,true,false]。任务局部上下文在Future轮询时继承，并在成功、Err、panic、取消后恢复；并发任务不串扰。抑制的是目标模型关联时间戳，仍保存子模型并派发owner Saved及遍历后续关联。此项修复式适配已明确保留，不能把C23记成全部行为相同。

批量fill按字段顺序执行；后续setter失败保留前面成功赋值，四份旧模型/强制填充/哈希/加密测试原有整次回滚断言已按真实PHP参考纠正，单个setter失败原子性仍验。最终5目标13项通过，其中无变量的MySQL专项目标早退不计真实库；此前其余事件/factory/主键181参考/多态/关系默认线程栈/路由/作用域/写入回归已通过。SQLite、隔离PostgreSQL18.7、MySQL8.4.13各实际执行严格策略与touch集成；MySQL另实际通过模型完整持久化、哈希、加密及HTTP forceFill四项。fmt、workspace/all-targets严格Clippy、三语build/browser/scope通过；第三方imgalifier-avif既有cfg警告不等于依赖安全审计通过。首轮旧断言失败已保留日志，未冒称通过。

来源：固定Framework13.40.0 Model.php/HasAttributes.php/Builder.php、既有转换与事件实现；旧版v0.x元数据/SeaQuery装配不同未直接搬入。联网raw依然无内容，采用现有固定源码与真实PHP oracle，不宣称新增联网验证。源码/参考见tests/eloquent_strictness.rs与tests/reference/eloquent-strictness.php/json；证据位于target/tmp/framework-parity-20261122/strict-{eighth,sixth,postgres,mysql,mysql-models}.log、strict-reference-final.json、fmt-strict.log、clippy-strict.log及docs-*-strict.log。C20及更广泛公开API裁决仍未完成。

## Query 分类绑定（2026-11-23）

C20补 getRawBindings/setBindings/addBinding/mergeBindings，保留九类顺序，复用现有AST、Grammar和参数执行器。手动变更记录已有自动绑定前缀，后续条件继续追加，select/reorder清空对应类别；来源替换保留已有from绑定。HAVING嵌套将手动绑定带入条件组，子查询在嵌入时保留最终绑定，快照深拷贝变更；分页计数暂时移除排序绑定后恢复。普通update/delete与PostgreSQL updateFrom按实际SQL位置取绑定；updateFrom拆分来源与连接条件，覆盖含参数子查询及原WHERE。没有拼接不可信值或建立第二套查询器。

真实PHP8.5.12按固定v13.40.0编译SQLite/PG/MySQL共93组SQL、九类原始绑定、展平绑定及错误参考，全量对照通过。SQLite实际验证投影/where/having/union、注入文本、聚合、计数排序恢复、groupLimit、update/delete及beforeQuery快照；PG18.7/MySQL8.4.13独立库实际执行同一用例，PG另覆盖updateFrom。三库各3项既有查询实际回归通过，PG无类型编码专项显式执行通过；主机原22查询/18写入/4查询对照通过，1项PG专项ignored不冒称主机执行。原MySQL专项无连接早退不计实际库。关系6项默认线程栈、scope/strict各1项及新绑定2项通过。新增状态最初触发关系栈溢出，改为按需堆存放groupLimit状态后默认栈全部恢复，没有调大线程栈；失败日志保留。最终fmt、workspace/all-targets严格Clippy、三语build/12页面组合/双前缀全部导航通过。

明确差异：接口用消费所有权与Result、Option类别和BindingValues/Value表达，非PHP任意动态输入；类别读取会校验相关SQL结构。getRawBindings保留原逻辑类别，现有groupLimit不会像上游toSql那样改写类别/offset，getBindings返回实际窗口排序及绑定数值限额。数值限额占位而非上游直接内联，以及PG updateFrom参数按真实SQL位置重排均保留为已有执行适配，不称字面1:1。无占位符的手动绑定属于调用方错误；未覆盖全部无效查询组合、其他数据库版本/SQLServer实际驱动与所有公开API，不据此关闭全Query或全框架。

来源为固定Builder.php:4695–4810、Grammar/PostgresGrammar、现有AST/写入/分页。旧版v0.x同路径builder使用SeaQuery/共享QueryState/get_bindings，无这四项分类API，未搬入旧锁与状态架构。联网raw仍无内容，采用本地固定源码与真正PHP oracle。参考tests/reference/query-bindings.php/json；证据target/tmp/framework-parity-20261122/bindings-{second,sixth,eighth,postgres-final,mysql-final}.log、clippy-bindings-final.log、fmt-bindings-final.log及docs-*-bindings.log。C23已本地提交7aba9cc19；其他剩余继续，未推送。

本批再次触及32GB停线，通知相关开发对话暂停新增构建后，核对无Cargo/rustc进程并持13个profile排他锁清理全部项目编译与docs dist，保留锁inode、源码/WIP/日志/依赖下载/数据。实测可用31,797,075,968→135,643,385,856字节，净释放103,846,309,888字节；证据disk-cleanup-bindings.json。清理后通知恢复并沿用正常增量/并发，未关闭用户应用。

## 目标与依据

按固定上游逐项补齐可实现的目录、命名、公开接口、默认值、装配顺序、生命周期和错误行为。Rust 必要适配与用户确认的额外功能单独登记，保留可追溯的差异；不能把这些差异删除后称为完成，也不能将其算作字面 1:1。

- 输入：[全仓审计](#historical-audit)、[审计快照（audit_snapshot）](framework-parity-data-2026-11-22.json)、[固定基线](laravel-rebuild-baseline.md)、[项目规范](../conventions.md)、[当前交接](../current.md)、[待办](../todo.md)。旧审计只代表当时的源码。
- Framework：v13.40.0，`1449bf241017087fbd38c118ecaabef3f6797fdc`；应用骨架：v13.11.0，`06d016a364a37430eec9cbc52209adce3d7667ce`；文档：`960eedf0911155ad16326ade49c0ea5d18eda0b8`。不得自动升级比较基线。
- 原审计本地 HEAD：`d4a40d7f46e708e6e2b12dee3033148042984638`，当时另有配置、语言、Blade、欢迎页等在途修改。当前HEAD已为 `8277bdb4a4d8a5156780ea8fc959a140d7bc56c6`，且有middleware与三语正文／测试在途修改。执行前重新核对实时HEAD、全部WIP和占用进程，保留协作者修改。
- 输入包含 54 组 C 差异、188 行选定方法缺口、77 项缺失字符串规则、14 项 R 适配、24 项 X 扩展、19 项 E 生态边界；10550 处公开方法声明中，10229 行尚待契约裁决。另有类型、helper、文档边界线索。各数字有交叉，不相加为缺陷数或对齐百分比。
- 该审计是全仓静态清点与选定契约复核，尚非全量语义认证。方法名出现或没出现，都不能直接裁定兼容性。


## 从对话要求到可执行工作

本批审计与计划材料集中在两个文件：本文包含历史审计全文、[26条需求对照](#requirements)、调研依据、开发计划和[核对记录](#plan-review)；[统一任务清单](framework-parity-data-2026-11-22.json)包含116项主任务及77条规则子任务。实施直接接续本文与统一数据，不再保留独立Agent提示词；任务及验收全部完成后统一归档本批材料。数据文件同时保留14张原始审计表和audit_snapshot；work_items共193行，使用item_type区分主任务和规则，parent关联规则所属C07，不能将子项累加为独立缺陷。

116项由C54＋R14＋X24＋E19＋Q5构成，为有交叉的工作分解，不是独立缺陷数。每个C项已经包含具体修改与完成条件；188条方法逐条关联原始表，不能以所在组关闭代替逐方法核对。77条规则逐条有稳定子编号；其余现有规则和typed Rule对象仍按完整规则矩阵核对。

| 补充任务 | 具体执行范围 | 验收条件 |
| --- | --- | --- |
| Q01 当前基线与增量 | 核对HEAD、WIP及自旧快照后的语言、欢迎页、控制器trait、中间件变更；保留原审计，新账本使用当前路径 | 已完成事实／历史结果／当前待验分开；不存在的旧docs/zh_CN锚点只作历史，不用作当前消费证据 |
| Q02 全部候选逐项裁决 | 10550处声明、1651类型、120 helper线索、163文档线索，逐项追踪所属类型与生命周期；覆盖38模块19包，新增问题并入任务 | 原名存在不自动通过；名称缺失不自动缺陷；没有未裁决行才可称本清单核对完成，不能据此推导全部输入语义证明 |
| Q03 构建增长根因 | 核对真实进程／路径、TMPDIR和target对子进程传递、缓存复用、退出与失败收尾、停线和锁；查明能证实的失效入口再改 | 有增长来源与失效链路证据；验证相关入口正常／失败／取消和缓存复用；保持增量／并发，不以重复冷构建或只改规范代替修复 |
| Q04 品牌规范实际落实 | 覆盖源码、注释、提示、Cargo元数据、README、发布和生成模板、三语正文；当前Cargo description仍需逐项裁决 | 自有说明改写，外部包／协议／许可真实保留；同步调用方与生成器，不以关键词零命中验收 |
| Q05 历史决定回归保护 | 标准队列三表、合并迁移、旧辅助机制不恢复；rust-toolchain.toml持续不存在，最低版本与子进程一致 | 新库迁移／回滚／ledger、批次失败重试与链流程实际验证；既有库只交付升级说明，不擅删数据或改迁移账本 |

### 当前事实更新

原审计的54组是在旧快照上确认，并非已经对当前HEAD重跑54组。原审计时ViewErrorBag为占位，当前已接通named/default bag，PHP内层稀疏键等仍有差异；sink入口仍为路径参数；其他组先重核具体契约。C38旧语言输出事实已被BCP 47提交替代，但规范化、输入拒绝和上游原样locale行为仍是有意差异，继续保留于结果表。X01及微信／QQ等已有实现和历史本地验收，不应从零再造；真实供应商结果仍未验。

原计划制定时未运行验收；用户随后授权实施，当前实际结果以上方“当前实施与验收”为准。未进行本批Git暂存、提交或推送。

## 复用与调研顺序

每个功能簇先读固定源码及其测试，再按规范检查 `openrust-v0.x` 或已有 Git 引用，最后查官方文档、生态项目和成熟 Rust 库。沿用现有职责和依赖；记录来源版本／提交、采用理由、许可和必要差异。Star 数不能替代契约验证。

固定源码及来源在 `target/tmp/laravel-code-audit-20261121/`。重点参考包括上游 `Support/MessageBag.php`、`Support/ViewErrorBag.php`、`Validation/Concerns/ValidatesAttributes.php`、`Testing/TestResponse.php`、`Database/Query/Builder.php`、`Http/Client/PendingRequest.php` 及对应测试。对照入口可从[固定 Framework 源码](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc)定位。

本次重做已通过HTTPS实际读取固定Blueprint、PostgresGrammar、应用jobs迁移、PendingRequest、三种SocialiteProviders驱动及EasyWeChat MiniApp源码，具体采用与失败边界见[对话要求与调研记录](#本次实际调研与复用)。实施阶段仍按功能簇补充成熟方案调研，不自动升级基线。

## 必须保留的决定

1. 数据库表／字段／状态值注释使用简短开发者惯例，如“用户表”“用户名”“状态：0开启，1关闭”；含义必须来自真实枚举。沿用迁移 comment API 与可扩展翻译资源，按配置语言写入，不硬编码只有三种语言。迁移文件名、结构与方法尽量保持上游习惯。
2. 微信网站扫码、公众号网页授权、小程序继续统一在 `services.wechat.web`／`official_account`／`mini_app` 下配置；保留已有驱动名与独立小程序 code2session 语义。QQ 保留 `qq`。避免并造第二套登录管理器。
3. [BCP 47 迁移方案](locale-bcp47.md)是较新的用户决定，优先于旧审计的语言标识描述。默认三语 `en`／`zh-Hans`／`zh-Hant`，覆盖资源、公开 API、事件、配置、文档目录／URL、生成部署及自有持久化数据；规范化与浏览器选语分离。`zh-CN` 不等于 `zh-Hans`，不得将地区与文字体系强行合并。该方案已由 `02fdc4ce8` 实施，并有本地验收记录；欢迎页回退后续由 `24563cc34` 修正。当前继续保护该实现，仅对受影响行为复验；其他部署自有数据／旧载荷升级与真实目标／Windows仍单列。不能按旧审计重复迁移或回退公开标签。本轮没有重跑其验收。
4. 公开代码和开发者文档以 OpenRust 为主体，注释及未国际化提示优先简体中文。内部对照资料可注明上游；真实协议、外部标识和许可声明保留。
5. 不恢复 `rust-toolchain.toml`，保留 Cargo `rust-version`，日常显式 Rust 1.94.0。保留正常并发、增量和可复用缓存，不为减小占用全局限并发／关闭增量。
6. 保留功能和数据。可选扩展的表与服务应由所属 Provider 装配；迁出根应用默认迁移须处理已有数据库与迁移账本，不删表或重跑破坏性迁移。`jobs`／`job_batches`／`failed_jobs` 属于正常上游骨架，不因表多而删除。
7. 资源防护不能为了模拟无限输入而直接移除。逐项核对默认值、可配置范围、失败行为和安全代价；仍存在的输入域差异明确登记，不能冒充等价。

## 执行账本与状态

实施者直接复用本文与统一数据：在统一数据文件中的work_items维护任务、状态、实际命令和证据位置，在本文集中记录阶段验收、决定及未完成条件；详细运行日志放项目target/tmp，不逐项新增分散文档，也不覆盖原审计快照。既有已实施事实引用对应提交与原验收，当前未执行的复验保持未验；不预填新实现或通过结果。

每条记录分别保存“与上游的关系”“实现状态”“验收状态”，不因保留扩展或语言适配而自动通过。至少包括：稳定 ID、所属阶段／负责人、上游类型及源码锚点、本地类型及公开消费入口、参数／默认值／返回值／失败与执行时机、裁决及理由、依赖、代码和文档位置、测试名称、实际命令／环境／结果、剩余条件。大表可用 CSV；命令和结果放同主题验收记录，不复制整份源码。

状态分别使用“待复核、待实现、实现中、待验收、验收通过、Rust适配、保留扩展、不适用、外部阻塞”。后三类差异必须有依据；外部阻塞不算验收通过，Rust适配与保留扩展不称字面 1:1。确认组关联行也要核对具体契约，不能仅因归组就整体通过。

## 阶段与依赖

以下是本计划的执行顺序，并非声称各功能均未实现；已有实现先复核再补缺口。P1／P2 沿用审计处理优先级，不表示实测事故级别。

| 阶段 | 工作内容 | 依赖及完成条件 |
| --- | --- | --- |
| S0 基线与逐项裁决 | 核对当前 WIP、固定来源、旧修复；建立 C/R/X/E、38 个核心模块和19个包账本。逐行核对全部方法、类型、helper、文档线索，并将新缺口拆成稳定任务 | 先完成基线与账本，再随每批实现持续裁决；名称相同也检查所属类型、签名和时机。10229 行待裁决、2948 行名称候选及其余映射均有明确去向，不能只做54组 |
| S1 基础契约 | JsonSchema 类型工厂与还原、ViewErrorBag、MessageBag、Lottery、宏支持矩阵、Concurrency／Pipeline 生命周期 | S0；先建立可公开消费的真实类型，再补调用端。PHP 动态行为逐项映射，禁止空类型／常量返回／静默 no-op |
| S2 校验与测试工具 | 全规则清点、Factory／Validator 扩展、TestResponse 与 Process fake 断言、捕获策略 | S1 错误包完成后处理会话／验证断言；77条逐项实现及测试，其余已存在规则也核对语义。保留已修复的数值、regex、相对签名等行为 |
| S3 数据与分页 | Query、Eloquent、绑定、关系、事务、严格模式与分页 resolver | S0/S1；共享 SQL 行为通过三库实际执行；不以生成 SQL 相似替代实际绑定、结果和事务验证 |
| S4A HTTP与文件 | Client 配置和流式 sink、Request／URL resolver、请求限制、File／Storage、Image | S0；sink 优先按P1实施。文件中断／句柄／大响应、非法路径和图像资源边界验证 |
| S4B 身份与状态 | Gate／Guard／AuthManager、SessionManager、Cache Lock／RateLimiter、Hash 参数和资源上限 | S0/S1/S3相关能力；跨请求／任务隔离、并发锁、异常恢复与失败路径均验证 |
| S4C 消息与服务 | Events、Queue failed providers、Bus Queueable、Mail、Translation、Notifications、Log | S0及相关S3/S4B；事务后投递、失败驱动、缓存清理、解析顺序等通过真实公开入口验证；语言改动接续BCP 47方案 |
| S5 协议与包 | 优先完成 Reverb signin；为19个包逐一固定参考和映射接口、装配、驱动、命令、默认值、生命周期与错误 | 相关核心依赖先满足；包实现者可直接修改必需核心代码。不能用framework方法表代替包审计 |
| S6 扩展与默认应用 | 执行X清单处置，保护已实施BCP 47并处理剩余影响；梳理默认Provider／迁移／配置／生成应用 | 复核语言既有实现／验收与实际剩余升级条件；不把已完成本地迁移重复列待实现，不擅改实际数据。可选扩展独立启用，既有业务能力继续可用 |
| S7 生态边界 | 按E清单裁决核心／可选包／PHP工具职责；形成有范围的实施任务或明确替代／暂不提供边界 | 先完成基础核心依赖；优先资源构建／前端联通与Prompts，其他按实际需求立项，不为同名创建空包 |
| S8 集成与交付核对 | 重验受影响生成应用、正式入口、全feature组合、文档及最终差异表；核对所有账本行 | 所有内部可执行任务闭环；真实服务／Windows条件不足继续单列。没有未裁决行才可称本轮清单裁决完成，有限测试不等于所有输入语义证明 |

S4A／S4B／S4C 是范围划分，不自动授权多Agent并行；单Agent按依赖连续处理。有外部阻塞时登记条件，继续可独立完成的项目。

### C清单分派

原问题、188行方法明细和证据分别保留在[差异表（audit_tables.confirmed_differences）](framework-parity-data-2026-11-22.json)、[方法表（audit_tables.confirmed_api_gaps）](framework-parity-data-2026-11-22.json)及[规则表（audit_tables.validation_rules）](framework-parity-data-2026-11-22.json)。下表每个C编号只设一个主归属；跨阶段依赖不重复计完成。

| 编号／阶段 | 具体修改要求 | 完成条件 |
| --- | --- | --- |
| C01／S1 | 补齐JsonSchema工厂、类型层次、组合类型、约束、序列化及反序列化；沿固定类型与原名导出，不只补anyOf名字 | 逐类型公开构造与JSON往返；组合／必填／默认／非法Schema及错误传播对照上游；新测试目标由实施者真实创建后记录 |
| C02／S1 | 将ViewErrorBag占位改成具名MessageBag集合，实现hasBag/getBag/getBags/put/count及上游默认bag职责 | 默认和自定义bag、空bag、覆盖、缺bag、视图及Session真实消费；错误输入和恢复，不能只测构造成功 |
| C03／S1 | 补MessageBag完整key/format、keys/merge/has/hasAny/all/unique/forget及格式化行为，保留可表达的原名 | 重复消息、通配key、空集合、首条／全量格式、合并顺序、删除和序列化；ViewErrorBag与校验消费不回退 |
| C04／S1 | 补winner/loser回调、alwaysWin/alwaysLose/fix/sequence测试控制，核对静态控制作用域 | 概率两分支、回调返回、序列耗尽、嵌套与异常复位、并行测试不串扰；不能让测试控制泄漏请求 |
| C05／S1 | 逐类列Macroable目标、注册／替换／清理／查询／调用映射；先复用现有注册机制，动态PHP调用记录typed入口 | 每个目标类单独公开消费、未知宏失败、冲突替换、清理和请求隔离；ResponseFactory支持不能替全部类作证 |
| C06／S1 | 核对process/sync和fork驱动职责；复用Registry与序列化任务，明确闭包／平台边界，可实现生命周期继续补齐 | 任务正常／异常／伪造输出／取消、延后执行、注册缺失、子进程清理及不支持驱动显式失败 |
| C07／S2 | 逐条处理77条缺失字符串规则，复核其余34条及typed规则对象；接入现有parser/validator/消息，不造第二套规则引擎 | 每规则正反例、缺失/null/空串/类型转换、参数和数组通配、依赖规则、大小／文件／日期／DB／网络；外部条件不足不能跳过后算通过 |
| C08／S2 | 补Factory隐式／依赖规则注册、扩展、消息replacer和unvalidated array keys策略原名入口，接入现有Validator | 自定义普通/隐式/依赖规则真实执行、嵌套数组键排除、替换器次序、错误规则和跨验证器隔离 |
| C09／S2 | 补Validator运行期值／规则／译器／异常重设及其缓存失效职责，参数与返回参照所属类型 | 修改前后再次执行、错误包与validated结果刷新、stop/bail、异常类型及错误消息，不能返回旧缓存 |
| C10／S2 | 盘点字段／路径／展开／输入／深度／匹配／规则数限额的来源，评估现有配置入口；逐项记录与上游不同输入域 | 每限额边界-1/边界/边界+1、畸形输入和配置覆盖；保留防护，未等价边界明确列出 |
| C11／S2 | 补正文／文本／顺序／否定及client/server error等TestResponse原名断言，核对HTML转义与文本提取 | 每断言正确值通过且错误值失败，文本与原始HTML区别、顺序和状态边界；失配输出有诊断 |
| C12／S2 | 补JSON匹配／count／fragment／missing／path集合／严格结构断言，不以ExactJson替代其余语义 | 对象／数组／嵌套路径／类型／顺序／缺键／null与严格结构，每项均验证错误值确实失败 |
| C13／S2 | 补Session错误包、闪存输入和valid/invalid断言，使用真实Session与ViewErrorBag | 默认及具名bag、指定字段／消息、old input、valid/invalid相反分支及跨请求消费 |
| C14／S2 | 补redirect route/action、header missing、Cookie过期、view与streamed等断言，沿现有Response生命周期 | 重定向参数／签名、header大小写、Cookie时间／加密、视图数据、流内容及否定断言，不吞流错误 |
| C15／S2 | 核对1MiB/5秒捕获策略与可配置接口，保留真实传输测试入口，typed测试适配独立记录 | 超1MiB正文、长响应、无限流超时取消与可配置覆盖；测试工具限制不误判生产传输行为 |
| C16／S3 | 补fromRaw/joinWhere/leftJoinWhere/rightJoinWhere，区分ON列比较与WHERE值绑定 | 三库实际连接、别名／子查询／原始片段／空与转义值、binding位置；错误SQL如实传播 |
| C17／S3 | 补binary/raw integer/row values与orWhereAll/Any/None等条件入口，按grammar保留原参数与嵌套逻辑 | 三库支持矩阵、空集合、null、布尔分组、二进制与行值、原始整数绑定和注入反例 |
| C18／S3 | 补havingNull/NotNull/NotBetween和嵌套having，保留逻辑分组及参数顺序 | 三库聚合后过滤、null／区间边界／嵌套or、bindings及不支持语法明确失败 |
| C19／S3 | 补随机排序、按ID前后分页、reorderDesc/rawValue/existsOr/doesntExistOr，核对回调执行时机与结果 | 三库空／单行／多行、稳定分页无重漏、前后方向、存在与不存在回调各只执行正确分支 |
| C20／S3 | 补getRawBindings/setBindings/addBinding/mergeBindings；使用现有binding分类与grammar，不拼接不可信值 | join/where/having/union等混合绑定顺序、类别错误、合并、clone隔离及三库实际执行 |
| C21／S3 | 补动态全局scope和Builder替换等可实现职责；模型元数据与typed scope建立所属类型映射 | scope注册／移除／组合、模型隔离、关系查询和自定义Builder公开消费；不以同名元数据替代生命周期 |
| C22／S3 | 补whereKeyNot/orWhereKey/orWhereKeyNot/soleValue/valueOrFail及其返回与失败语义 | 三库单／多主键、字符串键、零行／一行／多行、or分组、错误类别与原结果值 |
| C23／S3 | 补shouldBeStrict与静默属性丢弃／缺属性读取／withoutTouching策略，保留已支持lazy loading防护 | 策略开关、嵌套作用域、异常恢复、属性缺失与关系touch、并发模型或任务不串扰 |
| C24／S3 | 补updateOrFail/updateQuietly/saveOrIgnore/saveOrFail/deleteOrFail等变体，逐项核对事件和事务职责 | 三库成功／唯一冲突／事件取消／SQL失败／事务回滚，quiet不发事件但写入正确，非quiet不漏事件 |
| C25／S3 | 补loadMorphMax/Min/Sum/Avg原名与多态类型分组聚合，复用既有关系加载链 | 多态多类型／空关系／null聚合／条件约束、查询数及三库实际结果，不能退成逐行N+1 |
| C26／S4A | 补withUserAgent/withNtlmAuth/withoutVerifying原名及真实职责；先核对传输库能力，NTLM不能用普通Authorization假代 | 本地服务器捕获User-Agent；受控证书验证开关；NTLM受支持握手或明确真实缺口，不以名字存在宣布支持 |
| C27／S4A | 改sink为传输中分块写入，支持上游路径／资源职责的Rust表达，复用现有HTTP重试／回调链 | 大分块响应可观察边收边写及内存上界、中途断连／超时／取消／磁盘错误／句柄输入，失败文件状态与重试明确 |
| C28／S4A | 补secureAsset/assetFrom/getDefaultParameters/useOrigin/useAssetOrigin；不重开已支持的相对签名 | 协议／主机／子路径／CDN／默认参数覆盖、代理上下文和相对／绝对签名回归 |
| C29／S4A | 补getUserResolver/getRouteResolver/setRouteResolver，沿现有Request身份与路由解析链 | 自定义／默认／缺失resolver、路由参数与用户更换、请求隔离；Request::user仍沿当前正确入口 |
| C30／S4C | 补hasWildcardListeners/setQueueResolver/setTransactionManagerResolver/getRawListeners，保留解析顺序 | 普通／通配监听、注册／清除、注入resolver、事务提交／回滚、队列监听及fake作用域 |
| C31／S4C | 补数字database/file/dynamodb failed providers与配置装配、记录／查询／删除职责；沿失败任务契约 | 三库数字ID存储、文件持久化与损坏／并发写、DynamoDB本地协议和真实权限分别记录；retry/prune/forget实际入口 |
| C32／S2 | 补assertRanInOrder/assertNotRan/assertNothingRan及上游匹配职责，复用Process fake记录 | 命令／参数／执行顺序、零次／多次／负例／嵌套fake恢复；每断言错误时确实失败 |
| C33／S4C | 补MailManager.forgetMailers并真正释放／清空已解析实例，保留配置快照职责 | 不同mailer缓存、清理后重新解析、请求作用域及发送／取消／地址默认值历史回归 |
| C34／S4C | 补Translator.getSelector/setSelector/getLoader并保留原调用职责，与已完成BCP 47协同 | 自定义复数selector生效、Loader对象公开读取与真实查询、显式locale／fallback／损坏资源，BCP 47不回退 |
| C35／S4A | 补flop及可实现transformUsing；GD／Imagick专有驱动按实际原生后端映射，不能假装存在PHP扩展 | 水平／垂直方向、转换顺序、自定义转换失败、backend不支持时显式错误，输入格式与输出实际核验 |
| C36／S4A | 盘点图像字节／边长／像素／帧／总像素限额、色彩和JXL延迟归一，明确可配置范围和驱动边界 | 各限额边界、恶意图片与动画、metadata／色彩／延迟实测；不为对齐移除防护或声称逐像素等价 |
| C37／S5 | 实现完整pusher:signin服务端状态与客户端处理，复用既有authenticateUser签名入口而非止步HTTP | 真实协议客户端登录／拒绝／签名错误／身份串扰／重放与重连；客户端事件和服务器状态一致 |
| C38／S6 | 纠正旧审计现状：BCP 47已本地实施，保留有意差异；核对事件／API／资源／文档／持久化消费者并接续剩余升级边界 | en/zh-Hans/zh-Hant及精确zh-CN独立、关闭浏览器策略、旧输入拒绝与文档URL；其他部署旧载荷／数据迁移按授权和冲突审阅 |
| C39／S3 | 补currentPathResolver/currentPageResolver/queryStringResolver等原名接口，保留typed数据与请求隔离 | 自定义resolver、默认请求、非法页码／query、游标、多个应用与并发请求无全局状态泄漏 |
| C40／S4B | 在现有Lock／LockProvider契约补forceRelease/currentOwner/restoreLock等原名与所有实现映射，不另造锁引擎 | Redis8/file/database三库实际owner、过期、阻塞／超时、恢复与强制释放；动态trait对象可直接调用原名 |
| C41／S4B | 核对bcrypt和Argon执行上限配置及生成／校验职责，明确旧高成本哈希迁移与资源拒绝行为 | 可配置边界和旧hash验证、超限显式错误、正常密码错误；仅受控成本运行，不靠删防护换对齐 |
| C42／S4B | 补Bcrypt.setRounds与Argon.setMemory/setTime/setThreads原名，接入现有Options／执行限额 | setter后生成参数、invalid／超限、校验／needsRehash及实例隔离；参数实际生效 |
| C43／S4A | 补Filesystem工具类和File Facade真实入口，如sharedGet/replaceInFile/relativeLink/copyDirectory/cleanDirectory | 公开Facade解析、本地文件／目录／锁／相对链接、权限错误、符号链接边界和失败恢复；不以std::fs示例冒充框架API |
| C44／S4A | 补Adapter assertExists/assertMissing/assertDirectoryEmpty/assertEmpty/fileMissing/directoryMissing/copyToDisk/moveToDisk | 本地跨盘读写、目标失败与move源保留、空目录、断言正反例；远程对象存储本地协议与真实目标分列 |
| C45／S4B | 补Gate.getPolicyFor/guessPolicyNamesUsing/resolvePolicy与Response.withStatus/asNotFound，使用既有容器与策略注册 | 策略推断／覆盖／未找到、允许与拒绝、404/自定义状态、响应修改及公共HTTP消费 |
| C46／S4B | 补attemptWhen/getLastAttempted/setRememberDuration/getSession/getRequest/getTimebox，核对条件登录和remember职责 | 条件拒绝不登录、凭据错误、remember时间、上次尝试、timebox与Session轮换、真实HTTP及并发用户隔离 |
| C47／S4B | 补resolveUsersUsing/hasResolvedGuards/forgetGuards，清除正确作用域中的守卫缓存 | 自定义用户resolver、解析前后状态、forget后重建、任务／请求之间无身份泄漏 |
| C48／S4C | 补ChannelManager.deliversVia/deliverVia和readNotifications/unreadNotifications等关系入口 | 通道选择与覆盖、数据库通知读／未读关系、标记读取、队列与locale恢复；不要重造发送管理器 |
| C49／S4C | 补可提供的getChannels与typed底层logger访问；Monolog对象不适用单列，不能把所有内省一起豁免 | 已解析channel枚举、懒解析不误触发、logger句柄真实记录、监听和上下文隔离；无PHP类型伪壳 |
| C50／S4B | 补shouldBlock/blockDriver/defaultRouteBlockLockSeconds/defaultRouteBlockWaitSeconds/getSessionConfig，保留已存在阻塞机制 | 配置默认／覆盖／缓存、真实Session阻塞与超时、manager读取和路由选项一致，不能误报缺锁 |
| C51／S4C | 补allOnConnection/allOnQueue/withoutDelay/beforeCommit/withDeduplicator，核对typed Job／Chain级传播 | 链中每任务连接／队列／延迟、事务前后提交／回滚、去重标识作用与失败重试；不引回额外队列账本 |
| C52／S4A | 将Request正文／文本／字段固定限制逐项映射配置及代理行为，沿现有解析器与受控错误 | Content-Length／chunked／multipart、边界和部分读取、代理限额一致性、可配置覆盖及413等响应 |
| C53／S1 | 对照Pipeline.via/setContainer与withinTransaction链式配置职责，保留typed pipe但修正可保持的执行时机 | 配置时不执行、then触发、连接选择、异常回滚、嵌套管道和顺序；上游PHP动态method不适用部分单列 |
| C54／S4B | 补RateLimiter.resetAttempts/retriesLeft/cleanRateLimiterKey，沿现有计数和清理实现 | key清洗、剩余次数边界、窗口过期、重置及多客户端并发；Redis/database/file受影响路径实际验证 |

现有测试入口、原始锚点、188条方法子项关联及当前状态详见[统一任务清单](framework-parity-data-2026-11-22.json)。测试路径是实施时复用入口，不是本轮已执行结果；不存在的旧测试名明确标为待核实／新增。

### 按功能验收

| 功能簇 | 必要验证 |
| --- | --- |
| 基础类型／宏／管道／并发 | 公共消费示例、空值／错误类型、序列化还原、回调顺序、状态复位、事务实际执行时机、取消与任务清理；宏检查每个目标类而非全仓同名 |
| Validation／Testing | 每条规则成功和失败、缺失／null／空值／类型转换、数组通配与依赖、消息替换／三语、bail／停止顺序；所有断言既能正确通过也能对错误输入失败；捕获限额和超时可观察 |
| Query／ORM／分页 | PostgreSQL18、MySQL8.4、SQLite的SQL执行、绑定顺序、空结果和错误、事务回滚、scope／事件／quiet／touch、关系聚合、游标和分页边界；至少覆盖真实应用消费入口 |
| HTTP／文件／图像 | 跳转／头／TLS配置与失败、分块大响应流式落盘及内存证据、中途断连和取消、句柄／路径sink、跨盘失败／恢复、图像尺寸／动画和无效输入；不声称不同编解码器逐像素一致 |
| 身份／Session／缓存／Hash | 条件登录拒绝、策略发现／错误状态、守卫复位、并行请求与worker无状态串扰、锁owner／恢复／过期／强制释放、限流计数、哈希旧高成本值；共享缓存覆盖Redis8／database／file，database覆盖三库 |
| Events／Queue／Bus／通知等 | 容器真实解析、事务后执行与回滚、失败存储及重试／重复、链连接和队列传播、邮件实例清理、通知读未读关系、日志channel内省；可用本地协议测试与真实供应商结果分列 |
| Reverb／扩展包 | 真实协议客户端signin成功／拒绝／重连及状态隔离；包发现／Provider／迁移／Artisan／Worker／HTTP生成应用入口，禁用可选包的默认应用也能工作 |
| BCP 47／扩展与文档 | 按独立方案覆盖所有消费者、旧值冲突与数据迁移；数据库注释实读表／字段和状态值；微信三入口和QQ协议／配置回归；三语内容、目录、链接、子目录部署、构建和浏览器检查 |

## R适配逐项处置

下列是核对任务，不是对现状的新增豁免。所有可保留的上游原名和行为仍应实现。

| 编号 | 主阶段 | 处置与证据要求 |
| --- | --- | --- |
| R01 | S0/S6 | Cargo／模块／二进制对应目录与命令映射，核对打包后正式入口 |
| R02 | S1/S6 | 编译型注册保留装配顺序，生成应用验证配置／包／迁移／Seeder／命令 |
| R03 | S1 | 类型化容器解析、注入／作用域／失败与释放映射，不模拟PHP任意反射 |
| R04 | S5 | Cargo包发现、features／禁用包／重复注册与缓存生命周期 |
| R05 | 全阶段 | 每个Result／Future／Option记录上游异常／可空语义与执行时机 |
| R06 | S3 | 模型元数据、关系与引导映射，不以静态类型豁免scope／事件职责 |
| R07 | S3 | SQLx／TDS底层句柄适配，实际三库事务测试；PDO专用入口注明替代 |
| R08 | S1/S6 | Blade表达式支持矩阵，逐项辨别可实现能力与PHP代码执行边界 |
| R09 | S1/S6 | directive职责、view:cache、表达式／块预算和失败时机，补消费用例 |
| R10 | S4B | 单Redis客户端适配，核对原名、连接配置和Redis8实际行为 |
| R11 | S4B/S6 | 保留常驻进程请求／任务隔离；复验Config、Locale、Facade清理 |
| R12 | S4C | Worker取消、派生任务生命周期及关停边界，不将future取消说成进程终止 |
| R13 | S0/S6 | 日期／时区／年份／数据版本与消费者，列不等价输入并验证常用途径 |
| R14 | S2 | Cargo TestApp与typed断言适配，补可保留的原名和真实失败断言 |

## X扩展逐项处置

| 编号 | 主阶段 | 开发任务与保留边界 |
| --- | --- | --- |
| X01 | S3/S6 | 保留多语言数据库注释，验收表／字段／枚举说明与配置选语，扩展新语言不改核心分支 |
| X02 | S6 | BCP 47本地迁移已有实现与验收，复核影响及部署剩余；不恢复旧下划线公开标准 |
| X03 | S5 | 保留微信网站扫码，核对web配置、state、回调和账户绑定 |
| X04 | S5 | 保留公众号网页授权，核对scope／state／回调与权限拒绝 |
| X05 | S5 | 保留mini_app独立code2session，核对code失败、会话密钥边界和身份归属 |
| X06 | S5 | 保留QQ驱动，核对state／token／openid与撤销失败 |
| X07 | 全阶段 | 遵守OpenRust品牌／中文规范，不改协议、外部标识或删许可 |
| X08 | 全阶段 | 沿用工具链／数据库／缓存／构建管理规范，不引入工具链文件 |
| X09 | S6 | 核对派生数据库名、63字节约束及空值／非法名称；对照上游默认值并记录现有差异和变更影响，不自行搬迁开发数据 |
| X10 | S6 | 逐个核对额外app键的调用方，保留必要原生入口，额外策略明确配置归属 |
| X11 | S6 | 保留时区／算法可配置能力，核对默认值及错误输入与加密旧数据影响 |
| X12 | S6 | 核对APP_URL缺省推导及正式URL／代理／链接行为，明确与骨架默认差异 |
| X13 | S6 | 保留当前首页语言功能，按BCP 47方案把浏览器匹配与Translator分开 |
| X14 | S6 | 核对staticDisk装配、目录穿越与不存在文件，明确原生服务职责 |
| X15 | S4C | 保留SMS／Outbox／幂等恢复，核对事务与Worker；不额外引入业务队列表 |
| X16 | S6 | 评估将sms_deliveries迁移归属所属Provider，处理已有迁移记录，不删除功能或用户表 |
| X17 | S6 | 保留gRPC独立入口，核对可选装配／关闭／错误与关停 |
| X18 | S5 | 保留Pay微信／支付宝支付，明确直连支持范围，真实商户验收另列 |
| X19 | S5 | 保留OIDC RP／OP／SSO，明确与Socialite／Passport职责及持久化边界 |
| X20 | S5/S6 | 保留Settings；复验受影响的语言／缓存／加密，不重开已完成底座 |
| X21 | S5/S6 | 保留数据库翻译；BCP 47涉及JSON键／冲突／缓存／发布／旧值迁移 |
| X22 | S5 | 保留Activitylog，核对脱敏、主体与事务，不混为框架核心默认能力 |
| X23 | S3/S5 | 保留原生搜索，与Scout驱动分别登记、实际数据库验收 |
| X24 | S0/S8 | 记录本地patch来源／许可／上游状态；有等价上游API后再移除并回归，不盲升依赖 |

X09—X24的存在不证明均由用户明确要求，不将历史默认差异直接解释为已获用户确认的永久例外。执行者先核对现行决定和消费者；可按规范修正装配与契约，涉及无依据的产品选择或破坏性迁移保留明确待决项。

## 19个包的独立任务

依[包清单（audit_tables.package_coverage）](framework-parity-data-2026-11-22.json)逐包建立基线；已有映射可复用。缺少固定包来源时先补来源，不以Framework版本冒充各包版本。

| 包 | 核对重点 | 实际代码／测试入口及完成条件 |
| --- | --- | --- |
| framework | 38模块、全部候选与内置Sanctum，不能只看C清单 | `vendor/openrust/framework/`；现有tests入口：api_resources、api_response、api_throttling、application_locale；全部38模块、类型／方法／helper裁决、C/R/X实现及内置Sanctum实际入口；不以19包清点取代API核对 |
| package-discovery | 编译期发现、features、Provider顺序、生成应用 | `vendor/openrust/package-discovery/`；按实际Cargo清单定位或新增消费测试；别名／features／禁用包／Provider顺序／重复发现／生成manifest及配置缓存；独立生成应用实际启动 |
| fortify | actions／路由／view、Session；保留偶发Storage失败调查，定位前不宣布修复 | `vendor/openrust/fortify/`；现有tests入口：accounts_http、auth_http、email_verification_http、features；登录／注册／验证／重置／TOTP与Session真实HTTP；复现一次Storage失败的连接／事务证据，未定位仍列调查 |
| socialite | 官方驱动对照、OAuth1/2接口、微信三入口／QQ、state和账户归属 | `vendor/openrust/socialite/`；现有tests入口：china、docs_example、drivers、facebook；官方驱动全集映射、OAuth1/2 state与token流程、微信三入口／QQ配置和身份隔离、真实本地账号绑定；供应商联调另列 |
| passport | 授权流程、模型扩展、token生命周期、退出／撤销 | `vendor/openrust/passport/`；现有tests入口：authorization_code、client、client_contracts、client_lookup；各已承诺grant/token/revoke/client模型扩展、授权浏览器流程、状态／错误及SQL持久化；不以单grant证明全集 |
| passkeys | WebAuthn、凭据格式与升级边界；真实设备验收单列 | `vendor/openrust/passkeys/`；现有tests入口：aaguids、ceremony_driver、configuration、contracts；真实本地WebAuthn协议、challenge一次性／origin／RP校验与账户归属；PHP凭据格式不直接迁移，真机另列 |
| reverb | 完整signin、协议状态、TLS支持范围及真实客户端 | `vendor/openrust/reverb/`；现有tests入口：channels、client_options、laravel_user_auth、signatures；真实协议客户端signin／拒绝／断连重连／身份隔离、TLS与广播受影响路径；HTTP签名端点不是完整验收 |
| cashier | 两厂商行为、Paddle客户绑定策略、JSON／SDK类型／PDF差异及已有映射 | `vendor/openrust/cashier/`；现有tests入口：callback_http、persistence、protocol、provider；逐厂商订阅变更／退款／重复乱序通知与HTTP失败；Paddle客户归属、JSON类型、SDK/PDF未提供明确保留差异 |
| horizon | worker／指标／保留期／队列状态、原生面板接口，不将外观相似当完整等价 | `vendor/openrust/horizon/`；现有tests入口：configuration、installation、options、runtime；真实worker队列状态／指标／保留与清理，Redis实际消费、控制台和面板浏览器；UI和指标不同逐项列出 |
| pulse | 采集／存储／卡片／布局接口，原生面板与Livewire边界 | `vendor/openrust/pulse/`；现有tests入口：cache、concerns、provider、redis_ingest；真实采集器与存储、缓冲／聚合／保留、卡片与布局扩展、桌面手机面板；Livewire机制与原生替代分列 |
| telescope | watcher事件字段、source／callstack、Query／Gate过滤、multipart和Redis捕获范围 | `vendor/openrust/telescope/`；现有tests入口：custom_storage、entries、installation、provider；实际watcher事件／字段／存储／清理、Query/Gate过滤、multipart与Redis捕获、面板HTTP；缺source/callstack不伪填 |
| pail | 日志解析、Artisan、终端输出、信号与多平台 | `vendor/openrust/pail/`；现有tests入口：auth_timing、capture、command、files；真实终端日志解析／过滤／格式、文件轮换／信号退出、错误和受支持平台；终端格式不同说明 |
| pennant | typed scope、发现、缓存／持久化／事件 | `vendor/openrust/pennant/`；现有tests入口：auth、events、features、provider；不同scope、批量查询、持久化／缓存／事件与发现；并发任务隔离，官方对应接口逐项消费 |
| scout | CollectionEngine及官方外部驱动缺口，自定义Engine不能冒充内置全集 | `vendor/openrust/scout/`；现有tests入口：integration；模型同步／搜索／分页、CollectionEngine、失败传播／自定义Engine；Algolia/Meilisearch/Typesense内置缺口逐个列出 |
| pay | X18，签名、回调幂等／乱序和错误恢复 | `vendor/openrust/pay/`；现有tests入口：callback_http、failures、notifications、protocol；APIv3／RSA2签名、本地回调重复乱序／失败恢复／退款，商户真实授权与资金结果边界明确 |
| oidc | X19，RP／OP／SSO、认证策略与撤销边界 | `vendor/openrust/oidc/`；现有tests入口：flow、sso；RP与OP的发现／授权／令牌／claims／撤销／会话，SSO跨应用隔离；真实IdP与本地协议分列 |
| settings | X20，组／加密／请求任务缓存与可选装配 | `vendor/openrust/settings/`；现有tests入口：process、provider、runtime、settings；组属性类型／默认／加密／保存／请求任务缓存、可选Provider／应用覆盖／配置缓存；不重开已完成后台底座 |
| translation-database | X21，文件覆盖／缺表错误／reload／BCP 47数据 | `vendor/openrust/translation-database/`；现有tests入口：database_loader；三库language_lines、文件覆盖／精确locale／缺表／reload、BCP47旧新键冲突／回滚／重跑；真实旧环境操作另授权 |
| activitylog | X22，事件、主体、脱敏、清理及事务 | `vendor/openrust/activitylog/`；现有tests入口：activitylog、provider、support；actor/subject、事件／事务、脱敏／清理、包启用与禁用；文档公开消费，不默认注册示例业务 |

## E生态逐项处置

以下均保留在差异清单内。排序建议不构成用户批准的排除项或默认豁免；未实现继续保持未提供／待开发状态，只有具体职责有依据时才能判不适用。跨语言能力缺口不能直接写“不适用”；PHP专用运行时／工具则给真实替代，不创建同名空实现。未立项的可选能力不阻塞核心修复，但仍是未提供的边界。

| 编号 | 建议处理顺序／任务 |
| --- | --- |
| E01 MongoDB | 可选驱动后续任务：BSON／查询／模型／迁移职责及成熟Rust驱动调研，未立项前明确不支持 |
| E02 Vite | S7优先：入口manifest／开发服务器／生产资源／Blade消费／部署路径；复用Vite产物格式 |
| E03 入门套件 | E02/E04后续：选定真实模板与认证流程再实施，不将Fortify无界面认证当UI完成 |
| E04 前端 | 与E02联动，明确Vue／React／Inertia各自范围；没有实现仍保留缺口 |
| E05 Agent开发 | 与E06/E07分别调查职责和需求，保留独立可选任务，不凭外部HTTP调用声称框架SDK |
| E06 MCP | 可选包后续：协议版本、传输、发现、认证及官方兼容测试，复用成熟协议库 |
| E07 AI SDK | 可选包后续：模型／工具／流／embedding／向量与供应商真实边界 |
| E08 Boost | 工具后续：与E05/E06区分，调查Rust开发指引生成与工具安装实际需求 |
| E09 Head | 可选页面能力后续：复核固定官方包来源与职责，布局标签不当独立包等价 |
| E10 Dusk | PHP包不直接移植；记录成熟浏览器测试方案及真实应用用例，WebDriver封装能力仍单列缺口 |
| E11 Envoy | 可选部署任务后续，现有Process／Concurrency不是远程任务DSL；不在本计划擅自部署 |
| E12 Pint | PHP格式化职责不适用Rust；用rustfmt／Clippy验证，明确不是Pint实现 |
| E13 Mix | 旧资源构建兼容按需求后续，优先E02；明确mix manifest／helper未提供 |
| E14 Folio | 可选页面路由后续；核对路由发现／命名／中间件和编译型映射 |
| E15 Octane | PHP运行时适配器不适用Rust；复验原生常驻服务作用域／热状态／关停，可保留的管理接口另列 |
| E16 Sail | 优先复核已有Docker／Compose的启动与持久化；不为同名包装命令造额外层 |
| E17 Prompts | S7优先：按官方输入／选择／密码／验证／取消／非TTY列完整矩阵，复用现有Artisan及终端库 |
| E18 Valet | 可选本地DNS／TLS工具后续；监听服务与反向代理不是Valet完整替代 |
| E19 Homestead | PHP虚拟机镜像不直接移植；说明现有容器方案，VM供应能力仍未提供 |

## 执行与验收规则

- 所有临时源码、日志、构建和生成应用均在项目根 `target/tmp/`，传递项目内 `TMPDIR`；Cargo在完整独立源码快照和按工作流复用的独占 `CARGO_TARGET_DIR` 中执行。快照必须含当前WIP和生成输入，不能只归档HEAD遗漏未提交修复。
- Rust使用 `cargo +1.94.0`，会间接运行Cargo的夹具传递 `RUSTUP_TOOLCHAIN=1.94.0`。先从实际Cargo清单和测试读取target／变量名，再执行定向测试、fmt和严格Clippy；不编造测试名、不将缺变量跳过计为通过。测试须证明公开行为，避免只镜像实现。
- 共享SQL：PostgreSQL18＋MySQL8（现有夹具8.4）＋SQLite；共享缓存：Redis8＋database＋file，database覆盖三库。使用现有 `tests/postgres_acceptance.py`、`tests/mysql_acceptance.py` 及实际测试的URL／REQUIRE变量，使用隔离库、专属Redis和临时文件，保留开发数据与数据卷。日常验收环境不等于把默认骨架DB驱动改掉。
- 正式应用入口、CLI、worker及必要生成应用按受影响范围验证；默认应用不提前装配可选业务包。开发者文档三语同步；涉及站点时运行现有内容检查、文档构建和浏览器验收。内部计划本身无需Rust构建或文档站构建。
- 新大构建和阶段边界检查磁盘。可用空间≤32 GB（31,250,000 KiB）按规范停新增构建，协调现有构建安全退出、核对归属并取得Cargo锁，自动清理本项目全部编译产物；保留锁inode、源码／WIP／日志／下载／独立交付物／数据库／数据卷。不得整体删除根target，不全局关闭增量或固定低并发。
- L01—L22已有修复和独立复核记录先重新核对，避免按旧报告重做或回退；历史9/9等结果不代表当前源码重新通过。Fortify偶发Session失败保留原失败证据，未找到根因不写“彻底修复”。
- 微信／QQ／OIDC／Passkeys真机、支付、实际投递、AWS、托管广播及正式HTTPS仍受当前待办中的外部条件约束；本地协议／容器／虚拟设备结果单列。Windows共享文件系统os error87仍未完成验收，需可构建的项目内原生文件系统环境。
- 外部阻塞先记录原因和恢复条件，继续其余可执行任务。发布、数据破坏、提交、推送、tag、Release或部署不由本计划授予额外授权。

### 可直接复用的验收入口

以下是已核对存在的命令／目标，不是本轮执行记录。全部在完整隔离源码快照中运行，提前设置项目内TMPDIR、工作流独占CARGO_TARGET_DIR和RUSTUP_TOOLCHAIN=1.94.0；按当前功能／feature／真实环境筛选，不在活跃工作树启动构建。

```sh
cargo +1.94.0 fmt --all --check
cargo +1.94.0 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.94.0 test --locked -p openrust --test application_skeleton
cargo +1.94.0 test --locked -p openrust-framework --test locale_bcp47
cargo +1.94.0 test --locked -p openrust-framework --test laravel_alignment_validation
```

root `schema_comments` 已有PG真实用例 `all_builtin_migrations_write_localized_comments_on_postgres`，读取 `OPENRUST_TEST_POSTGRES_URL`／`OPENRUST_REQUIRE_POSTGRES`；MySQL实际分支读取 `OPENRUST_TEST_MYSQL_URL`／`OPENRUST_REQUIRE_MYSQL`。可由现有夹具运行目标，但需确认测试实际执行了服务分支：

```sh
python3 -B tests/postgres_acceptance.py -- cargo +1.94.0 test --locked -p openrust --test schema_comments all_builtin_migrations_write_localized_comments_on_postgres -- --exact
python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test --locked -p openrust --test schema_comments builtin_migrations_run_on_mysql_and_sqlite -- --exact
```

其他测试的URL／REQUIRE须从所属测试真实读取处映射，不能将上述变量套用于所有模块；其余三库／缓存行为按主矩阵定向补验。多用例复用同一容器也必须隔离表／数据库，异步等待关闭与回收，不能固定test-threads=1掩盖共享表冲突。

涉及开发者站点时，在快照运行 `node docs/tests/check_content.mjs`、`npm --prefix docs run build`，以及文档维护指南中的真实浏览器检查；本次内部计划未改站点，不运行无关构建。

## 交付条件

- [ ] C01—C54每组及每条展开API均有当前源码裁决、实现／适配依据及验收结果；77条规则与其余现有规则分别说明。
- [ ] 38模块、19包、全部公开声明／类型／helper／线索均有去向，新发现缺口已纳入同一账本；未裁决项如实列出，不宣称全量对齐。
- [ ] R01—R14、X01—X24、E01—E19全部有处置与证据，用户额外功能保留，BCP 47与默认装配有完整消费者验收。
- [ ] 受影响的三库／三缓存、真实应用入口、失败恢复及文档检查有实际记录；外部条件与Windows矩阵明确列为未验收。
- [ ] 最终表分别列已验证等价、必要Rust适配、保留扩展、未提供能力、外部阻塞和未完成任务。保留实际差异，不用整体百分比或“全部1:1”替代证据。
- [ ] 记录修改范围、实际测试、未验证范围和资源收尾；按用户当次授权决定是否提交／推送，本轮计划不包含Git写操作。

<a id="requirements"></a>

## 对话要求与开发任务对照

2026-11-22 重做。依据本对话中用户逐条发来的要求、当前项目规范、固定上游以及现存实现／验收记录整理。本文是计划的需求依据，不表示重新执行了框架验收。原审计保留不动；当前源码优先于旧快照。

### 对话要求逐项落实

引文摘录用户原意；连续的“处理好／继续”合并到对应功能，历史的“提交”只属于当时批次。本轮授权为重做开发计划和提示词，不扩为框架开发、数据操作或Git提交。

| 编号 | 对话要求 | 必须落实的工作与不得遗漏的边界 | 任务对应 |
| --- | --- | --- | --- |
| U01 | 继续指定对话未完成的任务；“中途不停止，直至开发完成” | 按当前交接核对旧任务结论；已有实现、待实现、待真实条件分别交接。遇单项外部阻塞继续其他工作，不能把“有记录”当完成 | Q01、Q02、全部C/R/X/E |
| U02 | “都处理好了吗？还有遗漏的吗？”“目前还剩下哪些工作没有处理？” | 建立逐项可追溯清单，收尾列真实剩余；同名方法、历史测试通过不证明整体完成 | Q02、各阶段交付条件 |
| U03 | jobs／job_*过多、复杂；“像laravel靠齐”“按照laravel优秀的机制实现” | 保留标准jobs、job_batches、failed_jobs；不恢复已删除的额外队列恢复账本、辅助表和专有恢复命令。SMS自己的Outbox职责单独处理，不能借队列简化误删业务能力 | Q05、C31、C51、X15、X16；[队列既有记录](../audits/queue-laravel-schema.md) |
| U04 | 指定dee8ae7fb；删除根rust-toolchain.toml，同步文档，保留rust-version | 已移除则保持；核对生成器、Docker、CI、README及子进程工具链，不增加另一份固定工具链文件 | X08、Q01、Q05 |
| U05 | 日常环境Rust1.94＋PostgreSQL18＋Redis8；此前提到MySQL切换 | 按最新规范采用Rust1.94.0，日常共享SQL验PG18／MySQL8.4／SQLite，共享缓存验Redis8／database／file。不能用旧“仅MySQL”或“仅PG”缩小验收；不因此改默认骨架DB或搬迁开发数据 | X08、R07、R10；[最新规范](../conventions.md#日常数据库与缓存验收) |
| U06 | 创建表和字段缺少注释，直接打开数据库难阅读 | 盘点根迁移、包迁移、发布模板和生成应用；PG／MySQL实读注释，SQLite说明不保存注释的边界 | X01、S3/S6 |
| U07 | “直接简单点，例如用户表、用户名、邮箱、手机号、状态” | 注释简短、准确，不塞设计文档或冗长解释。不存在的字段不为示例新增；状态值取真实代码 | X01 |
| U08 | “状态0开启1关闭”，开发者一看就懂 | 枚举／状态注释明确值与含义；不能把字符串SMS状态伪写成0／1。时间、JSON、哈希等有必要的存储含义简明标注 | X01、X15 |
| U09 | 三语默认根据配置，后续扩语言，原设计“不科学” | 复用现有Translator、FileLoader与分组资源；新增语言增加资源文件，不改核心switch／固定三语结构；回退／缺键／损坏资源在DDL前失败或沿既定契约返回 | X01、X02、C34、C38 |
| U10 | 迁移名称、内容、结构、API尽量1:1 | 上游三条初始迁移对应users/cache/jobs，jobs迁移包含标准三表；文件名和顺序、正向／回滚／pretend／ledger核对。已有库迁移历史另有维护边界，不删表重建 | X01、X16、Q05、S3/S6 |
| U11 | 反复要求联网调研GitHub热门项目和Laravel生态，避免重复造轮子和过度设计 | 每簇先查固定源码和现存实现、旧版，再调研成熟生态及Rust库；记录版本／提交、实际可复用内容与许可；不能只留一句“以后调研” | 全任务；下文来源与复用表 |
| U12 | “我让你额外加入的，都可以按照这种方式进行处理” | U11适用于所有扩展及后续改造；使用成熟内部机制优先，不为对齐再造并行Manager／配置层／新DSL | X01—X24、S5/S7 |
| U13 | 微信、QQ“能接入吗”；两种微信入口都需要 | 网站扫码与公众号授权分别实现，不能只做一种。保留Socialite授权／state／错误和用户映射职责，账号绑定由应用决定 | X03、X04、X06、Socialite包 |
| U14 | “微信小程序登录支持吗？”“需要接入” | 独立code2session，不把小程序当浏览器OAuth；会话密钥不回传为公开身份凭据，真实设备和供应商联调单列 | X05、Socialite包 |
| U15 | 三种微信“不能都放在wechat中吗”；QQ独立qq；“按照你的建议处理好” | 保持services.wechat.web／official_account／mini_app和wechat公开模块归组，保留weixinweb／weixin／qq驱动名；凭据、回调和账户命名空间隔离 | X03—X06；[归组记录](../audits/api-s01/wechat.md) |
| U16 | 编译产物不停增长，找出规定为何没生效、避免再发生 | 盘点实际产物／进程／环境传递／锁／复用路径／退出收尾，找到能验证的原因并修复实际入口；不能仅加一条规范或宣称已清理 | Q03、X08 |
| U17 | 对“关闭增量、并发3”指出“反向优化、负增益” | 保留Cargo正常并发、增量、IDE调试和热缓存；禁止全局关闭增量、固定低并发或每批冷编译；确有单命令资源约束须有实测原因 | Q03、X08 |
| U18 | “请现在阅读项目相关文档，务必掌握本项目知识” | 实施前读指引／规范／交接／现行计划，按当前HEAD与WIP重建事实；阅读摘要不能替代当前记录 | Q01 |
| U19 | 从头到尾审计，与Laravel是否1:1；“所有不是1:1，包括自己说的额外改动，都列出来” | 覆盖38核心模块、19包、类型／方法／helper、默认骨架和运行入口；C/R/X/E全部保留。用户额外功能也是差异，保留功能但不能从结果表消失 | Q02、C01—C54、R01—R14、X01—X24、E01—E19 |
| U20 | “给我提示词，我发给AI Agent处理” | 交付完整可复制提示词，写清任务、顺序、文件、验收、状态与剩余；不能只让Agent再写计划 | 提示词已交付，2026-11-23按用户要求删除失效文件；后续直接接续本文与统一数据 |
| U21 | “Agent已经处理完成，请核实一下”“你帮我处理好” | 验收核对实际代码及运行结果，不采信完成口号；历史L01—L22不机械重开，当前失败定位后再修 | Q01、Q02、S8；[独立复核](../audits/laravel-fixes-verification-2026-11-22.md) |
| U22 | 代码与文档尽量不要Laravel／Goravel品牌，先方案，后“改好并提交” | 延续已批准品牌规范，对项目自有代码、提示、Cargo元数据、README、生成模板与三语正文逐项核对。来源／许可／外部包和协议保留；不能改契约来消除关键词 | Q04、X07、S6/S8 |
| U23 | 历次“提交” | 历史授权有批次范围；本轮未再次要求提交。计划与未来提示词不扩大暂存／提交／推送／发布权限 | 全阶段Git边界 |
| U24 | “清理构建产物” | 历次清理结果是历史事实；当前不因重做计划再清理。真正开发时按现行磁盘停线、归属与Cargo锁清理，保留源码、WIP、日志、下载、交付和数据 | Q03、X08 |
| U25 | “根据资料和建议，整理成开发计划记录下来，然后给我提示词” | 落盘具体任务，逐项包含问题／实现要求／依赖／代码入口／验收／状态；同步待办入口。计划不得把未实现写成完成 | 本文与统一JSON数据；提示词曾交付，现按用户要求移除 |
| U26 | “按照要求重做，不要偷工减料” | 纠正旧计划过期BCP 47状态；用116项可执行主任务、77条规则子任务和原始188行方法映射约束覆盖；现行状态、历史证据、未执行验证分开写 | 本次重做与文件一致性检查 |

### 并行对话已进入现行规范的决定

当前源码HEAD为 `8277bdb4a4d8a5156780ea8fc959a140d7bc56c6`。本轮首次读取时已有middleware配置／三语文档及api_throttling测试在途；随后又看到请求语言解析及欢迎页等协作者改动。这些均不是本轮修改，实施者每次开始须重新读取实时状态。

BCP 47已由 `02fdc4ce8` 实施，后续 `24563cc34` 修正欢迎页回退；依据当前源码及[本地验收记录](../audits/03-07/locale-bcp47-2026-11-22.md)，资源与文档已迁至 `zh-Hans`／`zh-Hant`。原审计C38关于旧输出的描述是历史快照，不能继续作为当前未实现事实。当前残留边界为ICU支持范围、其他部署环境自有数据／旧载荷升级以及真实目标／Windows验收，详见[待办](../todo.md)。本次仅核对代码及记录，没有重跑该批测试。

本地完成不等于与上游语言标识字面一致。BCP 47为现行用户要求的有意差异，仍保留在C38／X02／R11对应映射中，不回退目录、不制造旧URL别名。新发现的控制器trait及中间件在途改动也进入Q01增量范围，不能只比较旧HEAD。

### 本次实际调研与复用

2026-11-22本次重做通过HTTPS实际读取下列固定官方／项目源文件；未使用浮动最新版。浏览工具未返回可读正文，改用只读HTTPS取得源码。只记录读到的事实，不将调研当运行验收。

| 来源 | 本次核对内容 | 采用方式与边界 |
| --- | --- | --- |
| [Framework Blueprint固定源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php) | 存在comment($comment)表注释入口 | 继续使用既有.comment与Translator；不新增注释国际化Manager或固定三语参数 |
| [PostgresGrammar固定源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/PostgresGrammar.php) | compileComment／compileTableComment分别处理列／表 | 对照现有PG注释生成、转义及变更；MySQL和SQLite沿各自grammar验证 |
| [应用jobs迁移固定源码](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/database/migrations/0001_01_01_000002_create_jobs_table.php) | 同一迁移创建jobs、job_batches、failed_jobs | 维持标准三表；不重引已删辅助恢复表／专有命令 |
| [PendingRequest固定源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/PendingRequest.php) | sink($to)公开入口与传输配置职责 | C27沿已有传输链补真正流式消费，不用先全缓冲再写路径冒充sink |
| [SocialiteProviders Weixin](https://github.com/SocialiteProviders/WeChat/blob/e61ce5d0f0ebf1c1ff57c6645815985203538908/Provider.php) | 实际读取授权、令牌和用户映射源码 | 复用现有Socialite驱动，保留weixin名和公众号职责 |
| [SocialiteProviders WeixinWeb](https://github.com/SocialiteProviders/WeChat-Web/blob/8b19355caf083bc092c7b25fe1189ae0744dbfbf/Provider.php) | 实际读取网站扫码驱动 | 网站与公众号配置归组但不合并凭据和scope |
| [SocialiteProviders QQ](https://github.com/SocialiteProviders/QQ/blob/b2b3652ef74f49d6eb0ba9804cfe118511a6c467/Provider.php) | 实际读取QQ令牌／OpenID／用户映射 | 沿用qq，不新建统一Tencent协议或自动账户合并 |
| [laravel-wechat配置固定源码](https://github.com/overtrue/laravel-wechat/blob/133828b7950acceff9b11deb442b65a9cf203292/config/easywechat.php) | 核对实际easywechat.php，包含official_account配置及open_platform／mini_app注释模板 | 借鉴按应用类型分组及明确可选入口；不将注释模板说成默认启用，web分组继续是本项目批准适配 |
| [EasyWeChat MiniApp Utils](https://github.com/w7corp/easywechat/blob/ba8022698a613e37b10ed1e4db604c5a96af0a63/src/MiniApp/Utils.php) | codeToSession(string $code)；本次取得源码SHA256与既有PHP参考校验一致 | 保留已有miniApp().codeToSession，复用HTTP与错误处理，不造浏览器OAuth适配 |

EasyWeChat源码本次校验SHA256为 `de24fde19d0210fcd1fdb22462111b1d60d591deef299ae8cbaec7d9fa500f21`，与仓内 `vendor/openrust/socialite/tests/reference/mini_app.php` 的固定来源校验相同。上表依赖许可由现有[腾讯记录](../audits/api-s01/tencent.md)、[小程序记录](../audits/api-s01/mini-app.md)提供，实施时引入／修改依赖仍需核对实际版本许可。

`overtrue/laravel-wechat`最初两个猜测路径返回404；随后实际查询固定提交的Git树，定位并读取真正的 `config/easywechat.php`（6652字节），核对分组与可选模板。集中配置的已批准决定另见[项目归组记录](../audits/api-s01/wechat.md)。`services.wechat.web`是本项目批准的适配，不宣称Laravel原生默认即如此。

数据库多语言注释的既有调研指出：translation-loader用于翻译来源，translatable用于业务数据，sql-commenter用于查询追踪，均不是必须引入的数据库元数据多语言机制，见[注释既有调研](../audits/schema-comments.md)。保留现有成熟组合，避免为满足“复用”增加无必要依赖。该生态结论引用既有调研，本次没有重新认证这些包全部API。

### 计划交付的核对边界

- 已重新读取用户所发对话、现行指引／规范／交接／待办、固定基线、审计分类表、相关源文件及历史验收；完整大表通过程序读取，候选未逐条做当前语义复验。
- 已补工作项、规则子任务、需求对照和提示词，并检查编号、依赖、路径和状态；不以计划整理冒充完成框架修复。
- 116项为任务分解数量，不是独立缺陷总数或对齐率。77条规则属于C07；188条方法属于对应C组；10229行是历史未裁决声明，不是已确认缺陷。

<a id="plan-review"></a>

## 计划重做核对记录

2026-11-22。范围：按用户发送的完整对话要求重做开发计划与Agent提示词。仅修改内部计划、工作项和待办，未进行框架开发、数据库操作、构建清理、Git暂存／提交／推送。

### 已实际执行的核对

| 检查 | 实际结果 |
| --- | --- |
| 对话需求 | 26条U编号已对应具体任务和保留决定；历史Git授权不跨批次沿用 |
| 当前事实 | 读取HEAD `8277bdb4a4d8a5156780ea8fc959a140d7bc56c6`、WIP及现行指引／规范／交接／待办；未覆盖其他协作者的中间件／语言解析／欢迎页在途改动 |
| BCP 47状态 | 已读取当前实现、提交与本地验收记录；纠正旧计划“尚未实施”，保留与固定上游不同的用户标准及实际环境升级边界；本轮未重跑相关测试 |
| 主任务覆盖 | C54＋R14＋X24＋E19＋Q5共116行，唯一、无缺编号；每条有具体修改要求、完成条件、现状、关系／实现／验收三个独立字段 |
| 依赖 | 全部引用可解析到主任务、无自依赖、无循环；Q02逐项裁决贯穿开发，不以一万行审查全部写完作为先修明显缺口的前置条件 |
| 校验子任务 | 77条C07-V编号与原validation-rules.csv缺失项逐条一致、无重复；其余现有规则和typed Rule对象仍需语义复核 |
| API关联 | 原confirmed-api-gaps.csv的188行全部关联到所属C工作项的类型与方法；未将关联等同当前已修复 |
| 审计输入 | 通过CSV实际读取原始10550声明／1651类型／120 helper线索／163文档线索／38模块／19包，数量与snapshot一致；并未逐条重新证明当前语义 |
| 测试与路径 | 声明为现有的所有测试路径实际存在；新增能力未虚构现有target；计划示例命令来自真实Cargo包、测试与URL／REQUIRE读取处 |
| 文档结构 | 新计划／提示词／需求对照相对链接存在，围栏闭合，C/R/X/E主表编号完整，未勾选未执行开发；git diff --check通过 |
| 联网调研 | 实际HTTPS读取固定Blueprint／PostgresGrammar／jobs迁移／PendingRequest、三种SocialiteProviders驱动、EasyWeChat MiniApp与laravel-wechat配置；后者经固定Git树定位正确路径；依据与采用边界见需求对照 |
| Git | git diff --cached --stat为空；本轮未暂存、提交、推送 |

### 未执行和不得推导的结论

未启动Rust测试／Clippy／构建、数据库／缓存服务、浏览器或真实供应商验收。本次的编号／依赖／链接检查不能作为116项功能完成、当前全仓API通过或Laravel字面1:1的证据。

外部服务、实际部署自有数据升级和Windows仍按当前待办记录条件。116项是主任务分解数量，77条规则及188行方法是子项与关联，不能累加计算缺陷或换算对齐率。

临时编写脚本保存在项目 `target/tmp/parity-plan-redo-20261122/`，没有在项目外创建临时文件，没有为计划整理启动无关构建或清理缓存。

### 本次文件合并

按用户要求，本批交接材料合并为开发计划、统一任务清单和完整Agent提示词三个文件。需求对照、调研及既有计划核对全文并入本文；规则子任务与主任务合并，保留全部原字段值和稳定编号。原始审计正文全文并入下方历史附录，14张表及快照合并到统一数据文件；其他主题文档仍作为既有来源引用。此次只调整文件组织和引用，未执行框架开发、功能验收或Git提交。

<a id="consolidation"></a>

## 审计与计划整体合并核对

2026-11-22：此前只合并计划材料，遗漏16个原始审计文件。本次将整批19个文件收敛为3个：本文、统一JSON数据、完整Agent提示词。旧审计目录和旧任务CSV移除；历史正文只调整标题层级和链接，历史结论不改写为当前事实。

2026-11-23：按用户要求删除已无用途的独立Agent提示词，并清理现行入口引用；以上三文件描述保留为历史整理事实。当前任务尚未全部完成，本文和统一JSON继续作为接续入口，任务及验收全部完成后统一归档。

统一JSON使用`columns`与`rows`保存表格：第n个单元格对应`columns[n]`，字段顺序、空字符串、原始行序和全部字段值均保留。`work_items`包含193条任务；`audit_snapshot`保存原JSON快照；`audit_tables`包含14张审计表；`audit_reviews`预留后续逐条契约裁决，不改写历史表。任务实施状态仍在`work_items`中维护。

已将写入后重新读取的数据与全部原始表及快照逐项比较：14张审计表共26449行、193条任务、快照对象全部一致；主任务116项、规则子任务77项、编号唯一、父项存在、依赖可解析且无环。审计正文整段保留，仅调整标题层级与数据文件链接；删除旧文件前再次检查来源未被并发修改。此次仅整理材料，未进行框架开发或运行时验收，未暂存、提交或推送。

<a id="historical-audit"></a>

## 原始审计全文（历史快照）

以下为原审计正文，所有“当前”均指该审计快照时点；表格与快照链接现在指向统一JSON的对应数据集。不得将历史静态清点当作本次全量语义验证。

## 当前框架与固定上游的全仓差异审计（2026-11-22）

**当前不能认定为 Laravel 1:1 对齐。** 本报告将缺失能力、接口缺口、语言适配、主动扩展和验收空白分别列出；用户确认要增加的能力同样属于相对固定上游的差异，但不因此建议删除。

### 基线与审计边界

- 框架：Laravel Framework v13.40.0，`1449bf241017087fbd38c118ecaabef3f6797fdc`。
- 默认应用：v13.11.0，`06d016a364a37430eec9cbc52209adce3d7667ce`；不能把应用版本误写成13.40.0。
- 本地：`d4a40d7f46e708e6e2b12dee3033148042984638`，另有并行协作者未提交的语言／模板／配置改动。最后观察到的文件状态与SHA256见 [snapshot.json（audit_snapshot）](framework-parity-data-2026-11-22.json)；工作区仍可能继续变化，本报告不代表后续改动。
- 使用项目既有的固定上游完整源码和来源记录（`target/tmp/laravel-code-audit-20261121/provenance.json`）。本次在线打开固定GitHub页面未取得可读正文；结论依据本地固定源码，未改用浮动最新版。
- 不改生产代码，不运行生成／迁移／构建／测试，不暂存、不提交、不推送。只生成内部审计交付文件；不干扰其他Agent。

扫描器使用正则提取显式public声明和每文件主类型，非PHP／Rust AST，也不解析继承、trait展开或宏生成。Rust名称索引包括非public方法和其他包，仅用于候选过滤；候选之外也可能存在所属类型／可见性错误。

**“全量”在本轮指全仓文件和声明清点，不代表已逐条证明所有方法、参数及输入组合的语义等价。** 未完成逐条裁决的候选完整保留，不能把它们全部说成缺陷，也不能声称已找齐一切行为差异。方法在其他类同名、trait、宏和Facade转发都会影响扫描结果。

### 总量与可交接清单

清点固定上游 **1746 个PHP文件**、本地 **1393 个Rust源文件**；提取 **10550 处公开方法声明**，不是去重API数。原名候选 **2948 处**；明确分组登记 **54 组**，其中选定原名缺口展开 **188 行**；内置字符串规则缺失 **77 项**。另列 **14 类运行时适配**、**24 项项目额外设计**、**19 项生态边界**。这些表有交叉，不能相加当作独立缺陷总数。逐声明裁决队列中 **321 行**关联已确认组，**10229 行**仍待契约裁决；这些数字不是对齐率。

| 文件 | 用途 |
|---|---|
| [confirmed-differences.csv（audit_tables.confirmed_differences）](framework-parity-data-2026-11-22.json) | 本文C清单：明确差异、影响、代码锚点、处理建议 |
| [confirmed-api-gaps.csv（audit_tables.confirmed_api_gaps）](framework-parity-data-2026-11-22.json) | 选定缺失原名逐方法展开，保留所属类型及上游行号 |
| [validation-rules.csv（audit_tables.validation_rules）](framework-parity-data-2026-11-22.json) | 固定ValidatesAttributes内全部validateX规则的存在性核对，逐项列出缺失字符串规则 |
| [project-additions.csv（audit_tables.project_additions）](framework-parity-data-2026-11-22.json) | 用户额外要求与项目现有扩展，区分来源可信度 |
| [runtime-adaptations.csv（audit_tables.runtime_adaptations）](framework-parity-data-2026-11-22.json) | 语言、运行时和输入限制的对应关系 |
| [ecosystem-boundaries.csv（audit_tables.ecosystem_boundaries）](framework-parity-data-2026-11-22.json) | 已声明未提供／部分提供的生态能力 |
| [api-inventory.csv（audit_tables.api_inventory）](framework-parity-data-2026-11-22.json) | 全部公开方法声明；名称存在不证明类型、签名和语义等价 |
| [api-review-queue.csv（audit_tables.api_review_queue）](framework-parity-data-2026-11-22.json) | 每一处声明的人工裁决状态与已确认差异组；其余明确标待核对 |
| [api-candidates.csv（audit_tables.api_candidates）](framework-parity-data-2026-11-22.json) | 全部原名缺失候选；未人工裁决行不能直接安排“修复缺陷” |
| [helper-inventory.csv（audit_tables.helper_inventory）](framework-parity-data-2026-11-22.json) | 非主类型文件内的具名function线索；含匿名类方法，所属入口仍需裁决 |
| [type-inventory.csv（audit_tables.type_inventory）](framework-parity-data-2026-11-22.json) | 全部PHP类型清点，弥补方法名碰巧同名造成的漏查；同名类型仍需命名空间对应 |
| [module-coverage.csv（audit_tables.module_coverage）](framework-parity-data-2026-11-22.json) | 全部38个Illuminate模块的覆盖／候选计数 |
| [package-coverage.csv（audit_tables.package_coverage）](framework-parity-data-2026-11-22.json) | 全部19个本地包；生态包不能用framework核心API表代替审计 |
| [documented-boundary-clues.csv（audit_tables.documented_boundary_clues）](framework-parity-data-2026-11-22.json) | 文档全部限制关键词线索；存在“限制”字样不自动证明与上游有差异 |

### 已确认的核心差异

P1表示优先处理明显能力／协议／资源行为缺口；P2表示迁移兼容缺口。不是本轮实测故障严重度。原名缺失并不必然表示SQL或底层功能无法通过其他方法实现。

| 编号 | 性质 | 优先级 | 范围 | 当前差异 | 影响／边界 | 本地证据 | 上游示例证据 | 处理建议 |
|---|---|---|---|---|---|---|---|---|
| C01 | 能力缺口 | P1 | JsonSchema | 整个 JsonSchema 类型构造／反序列化体系无对应模块；并非只有 anyOf 三个名字缺失 | 无法按原API创建／还原 JSON Schema | [vendor/openrust/framework/src/lib.rs:21](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/lib.rs:21) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/JsonSchema/JsonSchemaTypeFactory.php:8](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/JsonSchema/JsonSchemaTypeFactory.php:8) | 补齐原名和契约，并补成功／失败对照测试 |
| C02 | 占位实现 | P1 | Support / ViewErrorBag | 导出的 ViewErrorBag 只有私有占位字段，无 hasBag/getBag/getBags/put/count 等实现 | 命名错误包不能作为上游 ViewErrorBag 直接使用 | [vendor/openrust/framework/src/support/view_error_bag.rs:2](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/support/view_error_bag.rs:2) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Support/ViewErrorBag.php:12](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Support/ViewErrorBag.php:12) | 补齐原名和契约，并补成功／失败对照测试 |
| C03 | 接口及签名缺口 | P2 | Support / MessageBag | 仅 add/get/first/len/is_empty/messages；first/get 无 key/format 的完整重载，缺 keys/merge/has/hasAny/all/unique/forget/format 等 | 不是完整可替换的错误消息容器 | [vendor/openrust/framework/src/support/message_bag.rs:5](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/support/message_bag.rs:5) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Support/MessageBag.php:170](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Support/MessageBag.php:170) | 补齐原名和契约，并补成功／失败对照测试 |
| C04 | 接口缺口 | P2 | Support / Lottery | 只提供 odds/choose，缺 winner/loser 回调及 alwaysWin/alwaysLose/fix/sequence 等测试控制 | 不能迁移完整概率回调及测试机制 | [vendor/openrust/framework/src/support/lottery.rs:22](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/support/lottery.rs:22) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Support/Lottery.php:85](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Support/Lottery.php:85) | 补齐原名和契约，并补成功／失败对照测试 |
| C05 | 机制差异 | P2 | Macroable | 只有 ResponseFactory 等局部宏注册；没有通用 Macroable trait 和 PHP 任意成员动态转发 | 不能把一个类存在 macro 名称当成 Collection/Str/Request 等均支持宏 | [vendor/openrust/framework/src/http/response_factory.rs:41](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/http/response_factory.rs:41) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Macroable/Traits/Macroable.php:12](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Macroable/Traits/Macroable.php:12) | 逐类建立宏支持矩阵；按 Rust 能力提供明确调用入口，不伪造任意动态方法 |
| C06 | 能力及语言适配 | P2 | Concurrency | 仅 process/sync，无 fork；任务须显式 Registry 注册并序列化输入，不能直接传任意闭包到子进程 | 驱动名和任务输入方式不完全相同 | [vendor/openrust/framework/src/concurrency/facade.rs:74](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/concurrency/facade.rs:74) |  | 明确记录 fork 适用边界；保留编译型注册并验收生命周期 |
| C07 | 能力缺口 | P1 | Validation / 规则全集 | 内置字符串规则只有已列 parser 分支；缺失的规则逐项列入 validation-rules.csv | alpha/ip/json/digits/decimal/gt/gte/required_with/prohibited 等会返回 InvalidRule，翻译消息存在不等于支持规则 | [vendor/openrust/framework/src/validation/rule.rs:114](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/validation/rule.rs:114) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Validation/Concerns/ValidatesAttributes.php:45](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Validation/Concerns/ValidatesAttributes.php:45) | 补齐原名和契约，并补成功／失败对照测试 |
| C08 | 接口缺口 | P2 | Validation / Factory | 缺隐式规则、依赖规则、消息替换器及 unvalidated array keys 策略原名入口 | Rule::custom 不等同扩展 Factory 的完整机制 | [vendor/openrust/framework/src/validation/factory.rs:9](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/validation/factory.rs:9) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Validation/Factory.php:212](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Validation/Factory.php:212) | 补齐原名和契约，并补成功／失败对照测试 |
| C09 | 接口缺口 | P2 | Validation / Validator | 缺规则／值／译器／异常运行期重设入口 | 复杂动态校验不能按原 API 迁移 | [vendor/openrust/framework/src/validation/validator.rs:16](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/validation/validator.rs:16) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Validation/Validator.php:1263](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Validation/Validator.php:1263) | 补齐原名和契约，并补成功／失败对照测试 |
| C10 | 额外限制 | P2 | Validation / 输入硬限制 | 1024字段、路径1024字节、展开4096、输入20MiB、深度64、匹配预算1Mi步；单字段规则64条、规则字符串4096字节 | 增加了框架级可观察拒绝条件，并非 PHP 配置的同一组限制 | [vendor/openrust/framework/src/validation/path.rs:5](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/validation/path.rs:5) |  | 逐项评估默认值和可配置性；记录与上游输入边界的差异 |
| C11 | 接口缺口 | P2 | Testing / 正文与状态断言 | 缺 assertSee/SeeText/SeeInOrder/DontSee 及 client/server error 原名入口 | 常规 HTTP 测试迁移需重写断言 | [vendor/openrust/framework/src/testing/response.rs:15](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/testing/response.rs:15) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Testing/TestResponse.php:713](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Testing/TestResponse.php:713) | 补齐原名和契约，并补成功／失败对照测试 |
| C12 | 接口缺口 | P2 | Testing / JSON 断言 | 缺 assertJson/count/fragment/missing/path集合及严格结构等接口 | 现有 ExactJson/JsonPath/JsonStructure 不能覆盖上游全部断言 | [vendor/openrust/framework/src/testing/response.rs:40](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/testing/response.rs:40) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Testing/TestResponse.php:854](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Testing/TestResponse.php:854) | 补齐原名和契约，并补成功／失败对照测试 |
| C13 | 接口缺口 | P2 | Testing / Session和验证断言 | 缺 Session 错误包、输入闪存、valid/invalid 等断言 | 不能把 assertSessionHas 与 assertJsonValidationErrors 视为完整测试能力 | [vendor/openrust/framework/src/testing/response.rs:117](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/testing/response.rs:117) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Testing/TestResponse.php:1704](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Testing/TestResponse.php:1704) | 补齐原名和契约，并补成功／失败对照测试 |
| C14 | 接口缺口 | P2 | Testing / 跳转头Cookie视图流断言 | 缺跳转 route/action、header缺失、Cookie过期、view及streamed等断言 | API全集未实现 | [vendor/openrust/framework/src/testing/response.rs:106](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/testing/response.rs:106) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Testing/TestResponse.php:295](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Testing/TestResponse.php:295) | 补齐原名和契约，并补成功／失败对照测试 |
| C15 | 额外限制 | P2 | Testing / 捕获行为 | 测试响应捕获最多1MiB、五秒，测试入口为 Rust TestApp/Cargo | 长请求／大正文的测试结果和原测试体系不相同 | [docs/zh_CN/testing/http-tests.md:22](/Users/Home/Codes/Rust/openrust/docs/zh_CN/testing/http-tests.md:22) |  | 保留传输测试替代入口；评估可配置捕获限制 |
| C16 | 接口缺口 | P2 | Query / from 与 join | 缺 fromRaw/joinWhere/leftJoinWhere/rightJoinWhere 原名 | 可用其他SQL构造不代表该入口已对齐 | [vendor/openrust/framework/src/database/query/builder.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/query/builder.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Query/Builder.php:390](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Query/Builder.php:390) | 补齐原名和契约，并补成功／失败对照测试 |
| C17 | 接口缺口 | P2 | Query / 条件 | 缺 binary/raw integer/row values 及 orWhereAll/Any/None 原名 | 条件构造 API 不齐 | [vendor/openrust/framework/src/database/query/builder.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/query/builder.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Query/Builder.php:1322](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Query/Builder.php:1322) | 补齐原名和契约，并补成功／失败对照测试 |
| C18 | 接口缺口 | P2 | Query / having | 缺 havingNull/NotNull/NotBetween/嵌套 having 等原名 | 聚合条件链式接口不齐 | [vendor/openrust/framework/src/database/query/builder.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/query/builder.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Query/Builder.php:2839](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Query/Builder.php:2839) | 补齐原名和契约，并补成功／失败对照测试 |
| C19 | 接口缺口 | P2 | Query / 排序分页执行 | 缺 inRandomOrder/forPageBeforeId/AfterId/reorderDesc/rawValue/existsOr/doesntExistOr 原名 | 需按其他路径编写，调用和返回契约未等价 | [vendor/openrust/framework/src/database/query/builder.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/query/builder.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Query/Builder.php:3144](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Query/Builder.php:3144) | 补齐原名和契约，并补成功／失败对照测试 |
| C20 | 接口缺口 | P2 | Query / bindings | 缺 getRawBindings/setBindings/addBinding/mergeBindings 原名 | 底层查询扩展接入不一致 | [vendor/openrust/framework/src/database/query/builder.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/query/builder.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Query/Builder.php:4715](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Query/Builder.php:4715) | 补齐原名和契约，并补成功／失败对照测试 |
| C21 | 接口缺口 | P2 | Eloquent / Scope与Builder | 缺动态全局scope及builder替换相关原名 | 静态 globalScopes 元数据可以实现部分职责，但不是同一调用契约 | [vendor/openrust/framework/src/database/eloquent/model.rs:52](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/eloquent/model.rs:52) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Eloquent/Builder.php:202](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Eloquent/Builder.php:202) | 补齐原名和契约，并补成功／失败对照测试 |
| C22 | 接口缺口 | P2 | Eloquent / 主键与结果接口 | 缺 whereKeyNot/orWhereKey/orWhereKeyNot/soleValue/valueOrFail 原名 | 查询便利接口和失败返回契约不齐 | [vendor/openrust/framework/src/database/eloquent/builder.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/eloquent/builder.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Eloquent/Builder.php:311](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Eloquent/Builder.php:311) | 补齐原名和契约，并补成功／失败对照测试 |
| C23 | 接口缺口 | P2 | Eloquent / strict与touch策略 | 缺 shouldBeStrict/preventSilentlyDiscardingAttributes/preventAccessingMissingAttributes/withoutTouching 等原名 | 已支持 preventsLazyLoading 不等于全部严格模式 | [vendor/openrust/framework/src/database/eloquent/model.rs:33](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/eloquent/model.rs:33) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Eloquent/Model.php:585](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Eloquent/Model.php:585) | 补齐原名和契约，并补成功／失败对照测试 |
| C24 | 接口缺口 | P2 | Eloquent / 保存删除变体 | 缺 updateOrFail/updateQuietly/saveOrIgnore/saveOrFail/deleteOrFail 原名 | Result 风格可能承载错误，但方法变体及事务职责不能自动认为相同 | [vendor/openrust/framework/src/database/eloquent/model.rs:33](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/eloquent/model.rs:33) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Eloquent/Model.php:1200](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Eloquent/Model.php:1200) | 补齐原名和契约，并补成功／失败对照测试 |
| C25 | 接口缺口 | P2 | Eloquent / 关系聚合 | 缺 loadMorphMax/Min/Sum/Avg 原名 | 多态关系聚合入口不全 | [vendor/openrust/framework/src/database/eloquent/model.rs:33](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/eloquent/model.rs:33) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Eloquent/Model.php:1063](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Database/Eloquent/Model.php:1063) | 补齐原名和契约，并补成功／失败对照测试 |
| C26 | 接口缺口 | P2 | Http Client / 配置便利入口 | 缺 withUserAgent/withNtlmAuth/withoutVerifying 原名 | withHeader 可设置 User-Agent，不能由此声称该原名接口存在；NTLM/TLS策略需另外核对底层替代 | [vendor/openrust/framework/src/http/client/pending_request.rs:79](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/http/client/pending_request.rs:79) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Http/Client/PendingRequest.php:551](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Http/Client/PendingRequest.php:551) | 补齐原名和契约，并补成功／失败对照测试 |
| C27 | 行为及签名差异 | P1 | Http Client / sink | 先完成响应体缓冲，再 tokio::fs::write 到路径；非传输中的流式落盘 | 大文件内存、失败中间状态、文件句柄sink输入均不等价；默认body_limit为usize::MAX，不是固定小容量上限 | [vendor/openrust/framework/src/http/client/pending_request.rs:542](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/http/client/pending_request.rs:542) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Http/Client/PendingRequest.php:630](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Http/Client/PendingRequest.php:630) | 沿用现有传输链实现真正流式sink并补大文件／中断验收 |
| C28 | 接口缺口 | P2 | Routing / URL入口 | 缺 secureAsset/assetFrom/getDefaultParameters/useOrigin/useAssetOrigin 原名 | 已有相对签名 SignatureMode，不重开相对签名缺失；这里仅列其他缺口 | [vendor/openrust/framework/src/routing/url_generator.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/routing/url_generator.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Routing/UrlGenerator.php:300](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Routing/UrlGenerator.php:300) | 补齐原名和契约，并补成功／失败对照测试 |
| C29 | 接口缺口 | P2 | Http Request / resolver入口 | 缺 getUserResolver/getRouteResolver/setRouteResolver 原名 | Request::user 已存在；不能把用户读取已支持与可替换resolver等同 | [vendor/openrust/framework/src/http/request.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/http/request.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Http/Request.php:735](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Http/Request.php:735) | 补齐原名和契约，并补成功／失败对照测试 |
| C30 | 接口缺口 | P2 | Events / 扩展入口 | 缺 hasWildcardListeners/setQueueResolver/setTransactionManagerResolver/getRawListeners 原名 | 监听器内省及注入接口不同 | [vendor/openrust/framework/src/events/dispatcher.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/events/dispatcher.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Events/Dispatcher.php:187](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Events/Dispatcher.php:187) | 补齐原名和契约，并补成功／失败对照测试 |
| C31 | 驱动缺口 | P2 | Queue / 失败存储 | 只装配 null/database-uuids，缺数字ID database、file、dynamodb failed provider | 对应 queue.failed.driver 无法直接配置使用 | [vendor/openrust/framework/src/queue/queue_service_provider.rs:63](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/queue/queue_service_provider.rs:63) |  | 补齐原名和契约，并补成功／失败对照测试 |
| C32 | 接口缺口 | P2 | Process / fake断言 | 缺 assertRanInOrder/assertNotRan/assertNothingRan 原名 | 进程模拟断言未完整移植 | [vendor/openrust/framework/src/process/factory.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/process/factory.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Process/Factory.php:226](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Process/Factory.php:226) | 补齐原名和契约，并补成功／失败对照测试 |
| C33 | 接口缺口 | P2 | Mail / Manager | 缺 forgetMailers 原名 | 已有邮件原名修复不代表Manager全部入口完成 | [vendor/openrust/framework/src/mail/mail_manager.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/mail/mail_manager.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Mail/MailManager.php:631](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Mail/MailManager.php:631) | 补齐原名和契约，并补成功／失败对照测试 |
| C34 | 接口缺口 | P2 | Translation / 扩展接口 | 缺 getSelector/setSelector/getLoader 原名 | 自定义复数选择器及Loader读取入口不齐；getLocale/setLocale已修复 | [vendor/openrust/framework/src/translation/translator.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/translation/translator.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Translation/Translator.php:518](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Translation/Translator.php:518) | 补齐原名和契约，并补成功／失败对照测试 |
| C35 | 接口及驱动适配 | P2 | Image / 驱动及转换 | 使用原生 image 驱动和固定 Transformation enum；无 transformUsing/usingGd/usingImagick；水平垂直翻转用 flipHorizontally/Vertically，缺 flop 原名 | 基本图像能力存在；驱动、转换扩展及部分命名不1:1 | [vendor/openrust/framework/src/image/instance.rs:116](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/image/instance.rs:116) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Image/ImageManager.php:142](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Image/ImageManager.php:142) | 补齐原名和契约，并补成功／失败对照测试 |
| C36 | 额外限制与行为适配 | P2 | Image / 输入限制 | 输入32MiB、每边16384、单图2400万像素，动画1024帧／9600万总像素；JXL零延迟归一1ms | 增加固定运行边界；像素／色彩／动画语义需按驱动逐项对比 | [docs/zh_CN/digging-deeper/images.md:29](/Users/Home/Codes/Rust/openrust/docs/zh_CN/digging-deeper/images.md:29) |  | 保留必要防护并明确配置和兼容边界，不能宣称所有编解码逐像素等价 |
| C37 | 已登记协议缺口 | P1 | Reverb / 用户登录 | 完整 pusher:signin 服务端客户端处理尚未实现 | HTTP authenticateUser 签名端点通过不能替代完整登录握手 | [.agents/project/todo.md:12](/Users/Home/Codes/Rust/openrust/.agents/project/todo.md:12) |  | 完成协议状态处理及真实客户端登录／拒绝／重连验收 |
| C38 | 额外行为 | P2 | Translation / 语言标识 | LanguageIdentifier规范化语言，zh-Hans归并zh_CN、zh-Hant归并zh_TW并输出下划线；上游保留传入locale，不做此ICU强制规范化 | 语言目录、返回值、事件、空值／非法标签拒绝行为不同；BCP47迁移尚是并行计划，不能提前写成已完成 | [vendor/openrust/framework/src/translation/locale.rs:25](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/translation/locale.rs:25) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Translation/Translator.php:576](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Translation/Translator.php:576) | 按当前语言标识决策处理，保留上游调用行为映射和目录迁移证据 |
| C39 | 接口缺口 | P2 | Pagination / 静态resolver接口 | 通过PageOptions和当前请求取得分页数据；缺currentPathResolver/currentPageResolver/queryStringResolver等原名 | 可分页不等于兼容全部全局resolver定制接口 | [vendor/openrust/framework/src/pagination/mod.rs:33](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/pagination/mod.rs:33) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Pagination/AbstractPaginator.php:511](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Pagination/AbstractPaginator.php:511) | 补齐原名和契约，并补成功／失败对照测试 |
| C40 | 原名未保留 | P2 | Cache / Lock契约命名 | Lock公开契约为force_release/current_owner；LockProvider为restore_lock，未给出全部对应camelCase入口 | Facade::restoreLock已存在，但拿到Box<dyn Lock>不能调用上游forceRelease；功能存在而名称不齐 | [vendor/openrust/framework/src/contracts/cache/lock.rs:11](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/contracts/cache/lock.rs:11) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Contracts/Cache/Lock.php:45](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Contracts/Cache/Lock.php:45) | 在现有trait统一原名／兼容映射，并回归所有锁后端，不另造锁实现 |
| C41 | 额外限制 | P2 | Hashing / 执行上限 | 默认ExecutionLimits限制bcrypt成本16、Argon内存262144KiB/时间10/线程4 | 可拒绝上游允许生成或校验的高成本哈希；与默认成本相同并不代表可接受输入域相同 | [vendor/openrust/framework/src/hashing/driver.rs:56](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/hashing/driver.rs:56) |  | 核对生成／校验限制是否可配置，保留资源防护并明确高成本旧哈希迁移边界 |
| C42 | 接口缺口 | P2 | Hashing / 参数setter | 缺Bcrypt.setRounds和Argon.setMemory/setTime/setThreads原名；使用配置或HashOptions | 改变哈希成本的调用方式不同 | [vendor/openrust/framework/src/hashing/driver.rs:17](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/hashing/driver.rs:17) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Hashing/BcryptHasher.php:157](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Hashing/BcryptHasher.php:157) | 补齐原名和契约，并补成功／失败对照测试 |
| C43 | 能力及接口缺口 | P2 | Filesystem / File与原生工具类 | 有Storage/FilesystemAdapter，没有对应Filesystem工具类及File Facade完整入口 | sharedGet/replaceInFile/relativeLink/copyDirectory/cleanDirectory等本地文件工具不能按原入口使用，Rust std::fs并非相同框架API | [vendor/openrust/framework/src/filesystem/mod.rs:17](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/filesystem/mod.rs:17) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Filesystem/Filesystem.php:17](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Filesystem/Filesystem.php:17) | 补齐原名和契约，并补成功／失败对照测试 |
| C44 | 接口缺口 | P2 | Filesystem / Adapter断言与跨盘 | 缺assertExists/assertMissing/assertDirectoryEmpty/assertEmpty/fileMissing/directoryMissing/copyToDisk/moveToDisk原名 | Storage主体可用，但常用磁盘测试／便利操作不齐 | [vendor/openrust/framework/src/filesystem/filesystem_adapter.rs:22](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/filesystem/filesystem_adapter.rs:22) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Filesystem/FilesystemAdapter.php:131](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Filesystem/FilesystemAdapter.php:131) | 补齐原名和契约，并补成功／失败对照测试 |
| C45 | 接口缺口 | P2 | Auth / Gate与Response | Gate无getPolicyFor/guessPolicyNamesUsing/resolvePolicy原名；Response无withStatus/asNotFound修改入口 | 已有typed policy注册和denyWithStatus/denyAsNotFound，调用和动态策略发现不等价 | [vendor/openrust/framework/src/auth/access/gate.rs:160](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/auth/access/gate.rs:160) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Auth/Access/Gate.php:653](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Auth/Access/Gate.php:653) | 补齐原名和契约，并补成功／失败对照测试 |
| C46 | 接口缺口 | P2 | Auth / SessionGuard其余API | 缺attemptWhen/getLastAttempted/setRememberDuration/getSession/getRequest/getTimebox原名 | 已有attempt与登录主流程不能代表全部条件登录／内省／运行期策略接口 | [vendor/openrust/framework/src/auth/session_guard.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/auth/session_guard.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Auth/SessionGuard.php:470](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Auth/SessionGuard.php:470) | 补齐原名和契约，并补成功／失败对照测试 |
| C47 | 接口缺口 | P2 | Auth / Manager其余API | 缺resolveUsersUsing/hasResolvedGuards/forgetGuards原名 | 请求隔离缓存存在，但不能按上游接口配置用户解析器及清除守卫缓存 | [vendor/openrust/framework/src/auth/auth_manager.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/auth/auth_manager.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Auth/AuthManager.php:260](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Auth/AuthManager.php:260) | 补齐原名和契约，并补成功／失败对照测试 |
| C48 | 接口缺口 | P2 | Notifications / Manager与关系入口 | 缺ChannelManager.deliversVia/deliverVia和readNotifications/unreadNotifications等原名 | 已有通知发送、数据库通知及typed关系不等于原API全集 | [vendor/openrust/framework/src/notifications/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/notifications/mod.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Notifications/ChannelManager.php:168](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Notifications/ChannelManager.php:168) | 补齐原名和契约，并补成功／失败对照测试 |
| C49 | 接口及后端适配 | P2 | Log / 原生日志访问 | 缺Logger.getLogger及LogManager.getChannels原名 | 底层非Monolog，无法直接返回PHP对象；已解析channel的内省接口仍需明确映射 | [vendor/openrust/framework/src/log/log_manager.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/log/log_manager.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Log/LogManager.php:691](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Log/LogManager.php:691) | 区分PHP后端不适用与可提供的channel内省，记录typed对应接口 |
| C50 | 接口缺口 | P2 | Session / Manager与handler入口 | 缺shouldBlock/blockDriver/defaultRouteBlockLockSeconds/defaultRouteBlockWaitSeconds/getSessionConfig原名 | Session阻塞能力已实现；这里是其公开配置读取接口缺口，不能误报为不支持锁 | [vendor/openrust/framework/src/session/session_manager.rs:97](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/session/session_manager.rs:97) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Session/SessionManager.php:230](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Session/SessionManager.php:230) | 补齐原名和契约，并补成功／失败对照测试 |
| C51 | 接口缺口 | P2 | Bus / Queueable入口 | 缺allOnConnection/allOnQueue/withoutDelay/beforeCommit/withDeduplicator原名 | 现有typed Job/Options/Chain有部分对应职责；链级传播和调用兼容仍须建立映射 | [vendor/openrust/framework/src/bus/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/bus/mod.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Bus/Queueable.php:172](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Bus/Queueable.php:172) | 补齐原名和契约，并补成功／失败对照测试 |
| C52 | 额外限制 | P2 | Http Request / 固定解析限制 | 请求正文8MiB、文本字段1MiB、字段1024的原生固定限制 | 独立于PHP.ini post_max_size/upload_max_filesize/max_input_vars和前置服务器；分块正文也受原生读取上限限制 | [vendor/openrust/framework/src/http/request/input.rs:15](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/http/request/input.rs:15) |  | 建立可配置请求限制与反向代理对应关系；逐项记录同上游输入域的差异 |
| C53 | 签名及生命周期差异 | P2 | Pipeline / 调用方式 | 没有via(method)与setContainer；只接收typed回调；withinTransaction接受connection和destination并立即执行，上游则设置事务选项后继续链式then | 即使方法同名，参数、返回类型与执行时机仍不同；全局同名方法扫描会漏掉这一类 | [vendor/openrust/framework/src/pipeline.rs:117](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/pipeline.rs:117) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Pipeline/Pipeline.php:265](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Pipeline/Pipeline.php:265) | 明确区分编译型pipe表达方式与可保留的链式事务装配职责，并补对照消费示例 |
| C54 | 接口缺口 | P2 | Cache / RateLimiter便利接口 | 缺resetAttempts/retriesLeft/cleanRateLimiterKey原名 | 已有清理／剩余次数逻辑不能替代所有原名入口 | [vendor/openrust/framework/src/cache/rate_limiter.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/cache/rate_limiter.rs:1) | [target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Cache/RateLimiter.php:216](/Users/Home/Codes/Rust/openrust/target/tmp/laravel-code-audit-20261121/laravel-framework/src/Illuminate/Cache/RateLimiter.php:216) | 补齐原名和契约，并补成功／失败对照测试 |

#### 缺失的内置字符串验证规则（逐项）

`accepted_if`、`declined`、`declined_if`、`active_url`、`ascii`、`before`、`before_or_equal`、`after`、`after_or_equal`、`alpha`、`alpha_dash`、`alpha_num`、`array_keys`、`list`、`required_array_keys`、`contains`、`doesnt_contain`、`date_format`、`date_equals`、`decimal`、`different`、`digits`、`digits_between`、`distinct`、`filled`、`gt`、`lt`、`gte`、`lte`、`lowercase`、`uppercase`、`hex_color`、`in_array`、`in_array_keys`、`ip`、`ipv4`、`ipv6`、`mac_address`、`json`、`max_digits`、`min_digits`、`missing`、`missing_if`、`missing_unless`、`missing_with`、`missing_with_all`、`multiple_of`、`not_in`、`present_if`、`present_unless`、`present_with`、`present_with_all`、`not_regex`、`required_if_accepted`、`required_if_declined`、`prohibited`、`prohibited_if`、`prohibited_if_accepted`、`prohibited_if_declined`、`prohibited_unless`、`prohibits`、`exclude`、`exclude_unless`、`required_unless`、`exclude_with`、`exclude_without`、`required_with`、`required_with_all`、`required_without`、`required_without_all`、`same`、`starts_with`、`doesnt_start_with`、`ends_with`、`doesnt_end_with`、`timezone`、`ulid`。

本表只覆盖固定 `ValidatesAttributes` 的内置字符串规则；`current_password`、Password／Enum等对象规则需另查对应对象实现，不能因不在该trait里判缺失。`integer`浮点转换、`regex`布尔／null等已修复项不重新列缺陷；完整选项、PCRE修饰符、日期自然语言和PHP类型转换仍须差分验证。

### 语言与运行时适配（同样不是字面1:1）

| 编号 | 范围 | 差异 | 证据 |
|---|---|---|---|
| R01 | 运行时与目录 | Cargo/模块/可执行文件替代 PHP/Composer/autoload；文件 .rs 与模块 mod.rs 不是PHP文件1:1文本翻译 | [Cargo.toml:1](/Users/Home/Codes/Rust/openrust/Cargo.toml:1) |
| R02 | 启动装配 | 显式 withConfig/withPackages/withMigrations/withSeeders/withCommands 注册；编译型入口替代PHP运行时发现 | [bootstrap/app.rs:13](/Users/Home/Codes/Rust/openrust/bootstrap/app.rs:13) |
| R03 | 容器与反射 | 类型化 binding/Arc/descriptor/异步解析，非PHP Reflection 任意类型和构造器自动注入 | [vendor/openrust/framework/src/container/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/container/mod.rs:1) |
| R04 | 包发现 | Cargo编译期发现和features替代Composer动态包扫描 | [vendor/openrust/package-discovery/src/lib.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/package-discovery/src/lib.rs:1) |
| R05 | 异步与异常 | I/O为Future/Result，所有权借用、泛型及Option表达可空与失败；签名不逐字1:1 | [vendor/openrust/framework/src/contracts/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/contracts/mod.rs:1) |
| R06 | ORM模型 | static ModelMetadata/ModelState/显式trait关系取代PHP动态属性、trait引导、任意方法与可变模型配置 | [vendor/openrust/framework/src/database/eloquent/model.rs:33](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/eloquent/model.rs:33) |
| R07 | 数据库驱动 | SQLx/TDS取代PDO，底层连接及事务句柄不同；PDO入口列候选而非机械补壳 | [vendor/openrust/framework/src/database/connection.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/connection.rs:1) |
| R08 | Blade表达式 | 仅受支持的表达式树；任意函数/静态方法/对象方法、引用别名、未注册@php/@use不支持 | [docs/zh_CN/the-basics/blade.md:585](/Users/Home/Codes/Rust/openrust/docs/zh_CN/the-basics/blade.md:585) |
| R09 | Blade扩展与缓存 | directive回调接收求值后的数据而非生成PHP源码；view:cache解析所有分支；表达式深度48，块嵌套有独立预算 | [vendor/openrust/framework/src/view/compilers/template/expressions.rs:58](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/view/compilers/template/expressions.rs:58) |
| R10 | Redis | 单Rust客户端替代 Predis/phpredis 双PHP连接器，扩展/清除原名已存在 | [vendor/openrust/framework/src/redis/redis_manager.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/redis/redis_manager.rs:1) |
| R11 | 作用域 | 常驻进程按请求／任务隔离Locale、Facade等状态，不能套用PHP进程级全局可变状态 | [vendor/openrust/framework/src/config/repository.rs:28](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/config/repository.rs:28) |
| R12 | Worker超时 | 超时取消 Rust future；future外自行启动的工作须自行管理，不等同杀死PHP Worker | [docs/zh_CN/digging-deeper/queues.md:922](/Users/Home/Codes/Rust/openrust/docs/zh_CN/digging-deeper/queues.md:922) |
| R13 | 日期时区 | Jiff原生值/内置IANA2025b；原生年份−9999至9999，不能认定任意PHP/Carbon值及系统时区更新等价 | [docs/zh_CN/digging-deeper/helpers.md:127](/Users/Home/Codes/Rust/openrust/docs/zh_CN/digging-deeper/helpers.md:127) |
| R14 | HTTP测试 | Cargo TestApp、typed HeaderMap/StatusCode/JSON结构替代PHP TestCase/动态断言DSL | [vendor/openrust/framework/src/testing/response.rs:6](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/testing/response.rs:6) |

### 项目额外改动（包含用户明确要求）

“本对话用户明确要求”仅用于当前对话可以追溯的要求；没有证据的扩展不擅自归因给用户。已接受不等于相对上游不存在差异。

| 编号 | 范围 | 额外设计 | 来源 | 证据 | 建议 |
|---|---|---|---|---|---|
| X01 | 数据库注释国际化 | 默认迁移新增表／字段／枚举值说明，经 schema 翻译键解析，随配置语言执行；固定骨架无此默认行为 | 本对话用户明确要求 | [database/migrations/2026_11_21_000000_create_sms_deliveries_table.rs:18](/Users/Home/Codes/Rust/openrust/database/migrations/2026_11_21_000000_create_sms_deliveries_table.rs:18) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X02 | 三语与可扩展语言 | 随项目提供 en/zh_CN/zh_TW 的schema等资源及三语文档，非上游默认资源集合 | 本对话用户明确要求 | [lang/zh_CN/schema.json:1](/Users/Home/Codes/Rust/openrust/lang/zh_CN/schema.json:1) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X03 | 微信网站扫码 | 增加 weixinweb，配置归 services.wechat.web | 本对话用户明确要求 | [vendor/openrust/socialite/src/manager.rs:60](/Users/Home/Codes/Rust/openrust/vendor/openrust/socialite/src/manager.rs:60) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X04 | 微信公众号网页授权 | 增加 weixin，配置归 services.wechat.official_account | 本对话用户明确要求 | [vendor/openrust/socialite/src/manager.rs:59](/Users/Home/Codes/Rust/openrust/vendor/openrust/socialite/src/manager.rs:59) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X05 | 微信小程序 | 独立 miniApp/code2session 登录，配置归 services.wechat.mini_app；不是浏览器OAuth驱动 | 本对话用户明确要求 | [vendor/openrust/socialite/src/manager.rs:36](/Users/Home/Codes/Rust/openrust/vendor/openrust/socialite/src/manager.rs:36) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X06 | QQ登录 | 增加 qq 驱动；原生Socialite默认驱动集合之外 | 本对话用户明确要求 | [vendor/openrust/socialite/src/two/qq_provider.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/socialite/src/two/qq_provider.rs:1) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X07 | 品牌与中文规范 | 公开代码／文档以OpenRust为主体；简体中文提示／注释／配置组块说明，不逐字保留上游品牌及英文文案 | 本对话用户明确要求 | [.agents/project/conventions.md:71](/Users/Home/Codes/Rust/openrust/.agents/project/conventions.md:71) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X08 | 工具链管理 | 删除rust-toolchain.toml，仅保留Cargo rust-version；本项目日常Rust1.94、数据库／Redis验收及构建清理规范 | 本对话用户明确要求 | [Cargo.toml:5](/Users/Home/Codes/Rust/openrust/Cargo.toml:5) | 工程约定，不作为运行时缺陷；保留配置／文档一致性 |
| X09 | 默认数据库名 | 未配置DB_DATABASE时由APP_NAME归一、去前导数字并限制63字节；上游骨架各SQL连接为固定默认名 | 项目现有扩展；本次不推定由用户明确要求 | [config/database.rs:154](/Users/Home/Codes/Rust/openrust/config/database.rs:154) | 明确默认值差异；是否恢复上游语义由后续任务决定 |
| X10 | 应用配置额外键 | version/host/port/url_explicit/frontend_url/disabled_runners/public_path；增加APP_VERSION/HOST/PORT/PUBLIC_PATH及FRONTEND_URL | 项目现有扩展；本次不推定由用户明确要求 | [config/app.rs:44](/Users/Home/Codes/Rust/openrust/config/app.rs:44) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X11 | 可配置时区与算法 | APP_TIMEZONE/APP_CIPHER；固定应用骨架timeZone=UTC、cipher=AES-256-CBC是字面默认，不读取这两个环境变量 | 项目现有扩展；本次不推定由用户明确要求 | [config/app.rs:82](/Users/Home/Codes/Rust/openrust/config/app.rs:82) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X12 | 公开URL缺省值 | 缺APP_URL时按监听地址／8000推导http://localhost:8000；上游骨架默认http://localhost | 项目现有扩展；本次不推定由用户明确要求 | [config/app.rs:148](/Users/Home/Codes/Rust/openrust/config/app.rs:148) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X13 | 默认首页语言策略 | 默认全局SetLocale协商浏览器语言，并由WelcomeController输出三语首页；非上游骨架默认welcome模板路由 | 项目现有扩展；本次不推定由用户明确要求 | [bootstrap/app.rs:26](/Users/Home/Codes/Rust/openrust/bootstrap/app.rs:26) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X14 | 静态网站磁盘 | 根web路由默认staticDisk(web/public)，原生静态服务和磁盘配置替代前置Web服务器部分职责 | 项目现有扩展；本次不推定由用户明确要求 | [routes/web.rs:7](/Users/Home/Codes/Rust/openrust/routes/web.rs:7) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X15 | SMS Manager | 统一多厂商短信Manager/Outbox/幂等提交、状态恢复和worker处理；非Laravel核心统一SmsManager | 项目现有扩展；本次不推定由用户明确要求 | [vendor/openrust/framework/src/notifications/sms/sms_manager.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/notifications/sms/sms_manager.rs:1) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X16 | 默认sms_deliveries表 | 根应用自动扫描的迁移新增sms_deliveries，启用迁移即出现；上游默认应用无此表 | 项目现有扩展；本次不推定由用户明确要求 | [build.rs:19](/Users/Home/Codes/Rust/openrust/build.rs:19) | 应评估把可选扩展的表迁移移到其Provider；不直接删除用户功能 |
| X17 | gRPC | 独立tonic服务／客户端、config/grpc.rs和路由装配，非Laravel核心模块 | 项目现有扩展；本次不推定由用户明确要求 | [config/grpc.rs:1](/Users/Home/Codes/Rust/openrust/config/grpc.rs:1) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X18 | 微信／支付宝支付 | 原生pay包的直连商户支付，不是Laravel核心能力，亦非完整Yansongda SDK | 项目现有扩展；本次不推定由用户明确要求 | [vendor/openrust/pay/src/lib.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/pay/src/lib.rs:1) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X19 | OIDC RP/OP/SSO | 独立oidc包，职责超出Socialite与Passport核心移植范围 | 项目现有扩展；本次不推定由用户明确要求 | [vendor/openrust/oidc/src/lib.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/oidc/src/lib.rs:1) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X20 | 动态设置 | Spatie启发的typed group数据库设置；每组256属性/1MiB、请求任务缓存，不是Laravel核心Config | 项目现有扩展；本次不推定由用户明确要求 | [vendor/openrust/settings/src/lib.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/settings/src/lib.rs:1) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X21 | 数据库翻译 | Spatie translation-loader启发的language_lines及显式DatabaseLoader；不是核心默认文件Loader | 项目现有扩展；本次不推定由用户明确要求 | [vendor/openrust/translation-database/src/lib.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/translation-database/src/lib.rs:1) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X22 | 操作审计 | Spatie activitylog启发的activitylog包及可选属性脱敏，不是Laravel核心组件 | 项目现有扩展；本次不推定由用户明确要求 | [vendor/openrust/activitylog/src/lib.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/activitylog/src/lib.rs:1) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X23 | 额外搜索入口 | 框架原生数据库全文搜索接口，与Scout CollectionEngine及官方外部驱动集合分别存在 | 项目现有扩展；本次不推定由用户明确要求 | [docs/zh_CN/digging-deeper/search.md:1](/Users/Home/Codes/Rust/openrust/docs/zh_CN/digging-deeper/search.md:1) | 保留并在扩展边界标明，不计入核心1:1完成度 |
| X24 | 本地依赖补丁 | pcre2、sqlx及图像等本地patch仅为Rust依赖兼容／能力补足；与PHP供应链不同 | 项目现有扩展；本次不推定由用户明确要求 | [Cargo.toml:21](/Users/Home/Codes/Rust/openrust/Cargo.toml:21) | 保留并在扩展边界标明，不计入核心1:1完成度 |

### 生态缺失／部分支持（不是全部属于框架核心）

如Pint、Octane等依赖PHP运行时的工具，可以采用Rust替代；这仍然需要列入差异清单，但不能统称为核心代码缺陷。

| 编号 | 范围 | 现状 | 文档证据 |
|---|---|---|---|
| E01 | MongoDB | **当前源码未提供。** 当前数据库层没有 MongoDB 驱动、BSON 查询构造器或 MongoDB Eloquent 适配器。SQL Eloquent API 不代表支持 MongoDB。 | [docs/zh_CN/database/mongodb.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/database/mongodb.md:5) |
| E02 | 资源构建／Vite | **部分原生支持。** Blade 支持文档所述的 @fonts 消费 version 1 Vite 字体 manifest。尚未提供完整 Vite 集成、JavaScript／CSS 入口 manifest 解析器或前端开发服务器生命周期。 | [docs/zh_CN/the-basics/vite.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/the-basics/vite.md:5) |
| E03 | 入门套件 | **当前源码未提供。** 尚未提供现成套件 UI、控制台页面或 React／Vue／Livewire 套件。原生 init 向导与 Fortify 提供应用装配和无界面认证，页面由应用自行实现。 | [docs/zh_CN/getting-started/starter-kits.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/getting-started/starter-kits.md:5) |
| E04 | 前端 | **部分原生支持。** 已有服务端 Blade 视图、公共文件与 JSON 响应。当前源码没有 Vue／React 应用脚手架、Inertia 适配器或前端包管理工作流。 | [docs/zh_CN/getting-started/frontend.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/getting-started/frontend.md:5) |
| E05 | Agent 开发 | **未提供。** OpenRust 没有框架 AI agent 运行时或 AI 服务安装器。应用可通过自身代码集成外部 AI 服务。 | [docs/zh_CN/getting-started/ai.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/getting-started/ai.md:5) |
| E06 | MCP | **当前源码未提供。** 尚未提供 MCP 服务端／客户端传输、工具发现或 MCP 认证层。 | [docs/zh_CN/ai/mcp.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/ai/mcp.md:5) |
| E07 | AI SDK | **当前源码未提供。** 尚未提供 AI SDK 包、模型供应商注册、agent API 或 embedding／向量存储抽象。 | [docs/zh_CN/ai/ai-sdk.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/ai/ai-sdk.md:5) |
| E08 | Boost | **当前源码未提供。** 尚未提供 Boost 集成、自动生成 agent 指引或工具安装命令。 | [docs/zh_CN/ai/boost.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/ai/boost.md:5) |
| E09 | Head | **当前源码未提供。** 尚未提供独立 Head 包或 Head 服务 API。页面 head 标记可在 Blade 布局与组件中编写。 | [docs/zh_CN/packages/head.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/packages/head.md:5) |
| E10 | 浏览器测试／Dusk | **未提供。** OpenRust 没有 Dusk 包、WebDriver 管理器或 `dusk` Artisan 命令。可使用外部浏览器测试工具测试正在运行的应用。 | [docs/zh_CN/packages/dusk.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/packages/dusk.md:5) |
| E11 | Envoy | **当前源码未提供。** 尚未提供 Envoy 任务文件解析器或远程部署命令。原生进程与并发服务可执行应用自己的命令，不是 Envoy 任务运行时。 | [docs/zh_CN/packages/envoy.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/packages/envoy.md:5) |
| E12 | Pint | **当前源码未提供。** Rust 源码使用 rustfmt 格式化并通过 Clippy 检查。尚未提供 Pint 可执行文件、PHP 格式化器或 pint Artisan 命令。 | [docs/zh_CN/packages/pint.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/packages/pint.md:5) |
| E13 | Mix | **当前源码未提供。** 尚未提供 Mix／Webpack 集成、mix helper 或 Mix manifest 读取器。应用可使用自己的工具构建资源并从 public 目录提供。 | [docs/zh_CN/packages/mix.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/packages/mix.md:5) |
| E14 | Folio | **当前源码未提供。** 尚未提供 Folio 文件页面路由器。使用原生路由层显式注册路由并渲染 Blade 视图。 | [docs/zh_CN/packages/folio.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/packages/folio.md:5) |
| E15 | Octane | **当前源码未提供。** OpenRust 通过主应用进程启动原生 Rust HTTP 服务。尚未提供 Octane 包、Swoole／RoadRunner 适配器或 octane 命令。 | [docs/zh_CN/packages/octane.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/packages/octane.md:5) |
| E16 | Sail | **当前源码未提供。** 仓库提供 Docker／Compose 部署文件，没有 Sail 包装器或 sail 命令。请按实际容器装配与持久化存储说明使用。 | [docs/zh_CN/packages/sail.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/packages/sail.md:5) |
| E17 | Prompts | **部分原生支持。** 原生控制台输入、确认、选项与秘密输入见 Artisan。尚未提供完整 Laravel Prompts 移植或独立 Prompts 包。 | [docs/zh_CN/packages/prompts.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/packages/prompts.md:5) |
| E18 | Valet | **当前源码未提供。** 尚未提供 Valet DNS／TLS 守护进程、站点 parking 集成或 valet 命令。使用原生应用监听器或自行配置反向代理。 | [docs/zh_CN/packages/valet.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/packages/valet.md:5) |
| E19 | Homestead | **当前源码未提供。** 尚未提供 Homestead 虚拟机镜像、Vagrant 配置或虚拟机供应命令。 | [docs/zh_CN/packages/homestead.md:5](/Users/Home/Codes/Rust/openrust/docs/zh_CN/packages/homestead.md:5) |

### 全部核心模块覆盖表

每行均进入文件／类型／方法声明清点。候选为0也不能证明同名API、签名、状态生命周期和错误行为已经全部等价。

| 模块 | 方法声明数 | 原名缺失候选 | 裁决边界 | 本地入口 |
|---|---|---|---|---|
| Auth | 257 | 49 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/auth/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/auth/mod.rs:1) |
| Broadcasting | 108 | 24 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/broadcasting/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/broadcasting/mod.rs:1) |
| Bus | 158 | 36 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/bus/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/bus/mod.rs:1) |
| Cache | 448 | 38 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/cache/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/cache/mod.rs:1) |
| Collections | 494 | 1 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/support/collection.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/support/collection.rs:1) |
| Concurrency | 17 | 6 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/concurrency/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/concurrency/mod.rs:1) |
| Conditionable | 7 | 1 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/support/collection.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/support/collection.rs:1) |
| Config | 18 | 0 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/config/repository.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/config/repository.rs:1) |
| Console | 301 | 94 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/console/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/console/mod.rs:1) |
| Container | 94 | 4 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/container/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/container/mod.rs:1) |
| Contracts | 569 | 62 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/contracts/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/contracts/mod.rs:1) |
| Cookie | 24 | 0 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/cookie/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/cookie/mod.rs:1) |
| Database | 2277 | 891 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/database/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/mod.rs:1) |
| Encryption | 14 | 0 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/encryption/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/encryption/mod.rs:1) |
| Events | 52 | 12 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/events/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/events/mod.rs:1) |
| Filesystem | 173 | 42 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/filesystem/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/filesystem/mod.rs:1) |
| Foundation | 861 | 331 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/foundation/application.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/foundation/application.rs:1) |
| Hashing | 29 | 4 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/hashing/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/hashing/mod.rs:1) |
| Http | 521 | 107 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/http/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/http/mod.rs:1) |
| Image | 80 | 8 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/image/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/image/mod.rs:1) |
| JsonSchema | 46 | 4 | 确认整模块缺失 | [vendor/openrust/framework/src/lib.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/lib.rs:1) |
| Log | 89 | 4 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/log/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/log/mod.rs:1) |
| Macroable | 6 | 0 | 动态宏覆盖存在机制差异 | [vendor/openrust/framework/src/http/response_factory.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/http/response_factory.rs:1) |
| Mail | 228 | 42 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/mail/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/mail/mod.rs:1) |
| Notifications | 95 | 11 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/notifications/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/notifications/mod.rs:1) |
| Pagination | 142 | 17 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/pagination/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/pagination/mod.rs:1) |
| Pipeline | 18 | 2 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/pipeline.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/pipeline.rs:1) |
| Process | 133 | 15 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/process/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/process/mod.rs:1) |
| Queue | 540 | 146 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/queue/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/queue/mod.rs:1) |
| Redis | 106 | 41 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/redis/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/redis/mod.rs:1) |
| Reflection | 7 | 7 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/container/class_descriptor.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/container/class_descriptor.rs:1) |
| Routing | 415 | 171 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/routing/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/routing/mod.rs:1) |
| Session | 149 | 29 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/session/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/session/mod.rs:1) |
| Support | 901 | 184 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/support/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/support/mod.rs:1) |
| Testing | 281 | 177 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/testing/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/testing/mod.rs:1) |
| Translation | 50 | 9 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/translation/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/translation/mod.rs:1) |
| Validation | 563 | 266 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/validation/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/validation/mod.rs:1) |
| View | 279 | 113 | 已有对应实现；原名及类型全量清点完成，语义全集未认证 | [vendor/openrust/framework/src/view/mod.rs:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/view/mod.rs:1) |

### 全部本地包覆盖表

下表逐包登记边界，不将历史测试日志冒充本轮重新验收；包级全部官方PHP源码／SDK接口尚未逐条映射。

| 包 | 本次登记的差异／待核对范围 | 包入口 |
|---|---|---|
| activitylog | X22；单独生态包。typed身份／事件而非PHP模型反射；见包来源与现有审计。 | [vendor/openrust/activitylog/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/activitylog/Cargo.toml:1) |
| cashier | Paddle客户绑定、厂商JSON返回、无PHP SDK类型／PDF渲染器，参见cashier-api-mapping；属于明确差异。 | [vendor/openrust/cashier/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/cashier/Cargo.toml:1) |
| fortify | 原生动作／路由／View callbacks；Passkeys凭据JSON不能跨PHP/Rust直接复用；真实服务和偶发Session失败仍有验收边界。 | [vendor/openrust/fortify/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/fortify/Cargo.toml:1) |
| framework | 本文38个核心模块及C/R/X清单；含内置可选Sanctum。 | [vendor/openrust/framework/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/Cargo.toml:1) |
| horizon | 原生面板；近期索引1000、元数据retention及统计定义见文档，未证明官方面板/指标API全集等价。Redis Cluster限制本身不自动算与上游不同。 | [vendor/openrust/horizon/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/horizon/Cargo.toml:1) |
| oidc | X19；独立扩展，不冒充Laravel核心模块。 | [vendor/openrust/oidc/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/oidc/Cargo.toml:1) |
| package-discovery | R04；Cargo编译期发现，不是Composer动态加载。 | [vendor/openrust/package-discovery/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/package-discovery/Cargo.toml:1) |
| pail | 原生Rust日志／Artisan入口；未做官方全部终端输出及平台适配逐项对照。 | [vendor/openrust/pail/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/pail/Cargo.toml:1) |
| passkeys | 成熟WebAuthn库；凭据存储格式与PHP不同，真机验收未完成。 | [vendor/openrust/passkeys/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/passkeys/Cargo.toml:1) |
| passport | 原生OAuth服务端／typed模型与Result；未重验所有授权流程、模型扩展和官方接口全集。 | [vendor/openrust/passport/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/passport/Cargo.toml:1) |
| pay | X18；微信APIv3/支付宝RSA2直连范围；不等同全厂商SDK。 | [vendor/openrust/pay/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/pay/Cargo.toml:1) |
| pennant | typed scope与Cargo服务发现；已有参考夹具不等于完整官方API全集对照。 | [vendor/openrust/pennant/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/pennant/Cargo.toml:1) |
| pulse | 原生HTML/JS面板替代Livewire组件，自定义卡片/布局接口不同。 | [vendor/openrust/pulse/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/pulse/Cargo.toml:1) |
| reverb | C37完整signin缺口；原生服务实现，TLS verify子集不等同Guzzle所有选项。 | [vendor/openrust/reverb/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/reverb/Cargo.toml:1) |
| scout | 内置CollectionEngine；没有内置Algolia/Meilisearch/Typesense官方驱动等价全集，允许自定义Engine。 | [vendor/openrust/scout/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/scout/Cargo.toml:1) |
| settings | X20；Spatie参考范围，非核心Config。 | [vendor/openrust/settings/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/settings/Cargo.toml:1) |
| socialite | X03–X06；请求借用、OAuth1/2显式enum分派、统一异步redirect，签名不同。 | [vendor/openrust/socialite/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/socialite/Cargo.toml:1) |
| telescope | 原生面板；source/callstack可为空、Query/Gate源码过滤缺失、multipart元数据和部分Redis操作不捕获。 | [vendor/openrust/telescope/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/telescope/Cargo.toml:1) |
| translation-database | X21；缺表明确失败和显式reload，非完整翻译后台或模型字段翻译方案。 | [vendor/openrust/translation-database/Cargo.toml:1](/Users/Home/Codes/Rust/openrust/vendor/openrust/translation-database/Cargo.toml:1) |

### 不能误报为差异的项目

- `jobs`、`job_batches`、`failed_jobs`三张表本身就是固定应用骨架的队列机制；它们不是额外设计。本次新增默认表关注的是`sms_deliveries`。
- 默认数据库配置仍是`sqlite`，不把日常PostgreSQL／MySQL验收环境误写成框架缺省驱动。应用显式环境变量会覆盖缺省值。
- API路径或`expectsJson`决定异常JSON渲染，在固定应用bootstrap也存在，不能误报为本项目独有。
- L01–L22旧缺口已经有修复／复核记录；本次列的是剩余全集缺口。Cookie／slug、邮件sendNow、认证契约、runNextJob、URL forceScheme、翻译getLocale等不凭旧报告重开。
- `Image`已存在于固定Framework13.40.0中，不能把整个图像功能算作项目独有扩展。
- Horizon不支持Redis Cluster不能仅据本地文档判定与上游不同，需比较相同上游版本和部署要求。
- 三语言资源不是语言数量上限；新增资源仍可扩展。数据库注释是在迁移执行时写入数据库，不会因为运行中的APP_LOCALE改变而自动多语切换既有物理注释。

### 尚未完成的认证范围与后续Agent闭合标准

1. **API逐项裁决仍未完成。** 全部原名缺失候选需结合最新CSV统计处理；947处PHP魔术方法需要映射；其余“名称出现”也不能直接判通过。此轮给出完整队列，没有把全部候选伪装成已经人工证实的错误。
2. **签名和默认值全集未完成。** 对每个公开API继续核对所属类／trait／Facade、重载／参数／返回值／同步异步／默认值／异常，不限于上面选定组；同时核对框架helpers、配置全集、全部Artisan参数与输出、生成文件和默认应用装配。
3. **行为全集未完成。** Query／Schema全部方言、ORM全部关系和生命周期、事务并发、缓存锁、Session、请求解析、Blade语义、日期/时区、验证消息、加密与序列化、集合比较、邮件MIME、队列故障恢复等没有在本轮全部差分运行。已有测试通过只能作为对应输入证据。
4. **生态包全集未完成。** 19个本地包须各自固定对应版本并做类／API／配置／协议／面板／命令映射；Cashier的既有API映射不能代替全部Stripe/Paddle SDK兼容。
5. **外部目标和平台。** 真实支付／OAuth与微信QQ小程序／设备Passkeys／SMTP短信／AWS商业服务、正式HTTPS及Windows最终矩阵，仍按当前todo记录等待条件；这是未验范围，不直接等同源码实现错误。Fortify偶发Session失败没有可靠根因，不写成已修复。
6. **交付与复核。** 每条候选裁决为“同名已有”“等价Rust映射”“确认缺口”“用户额外要求”“不适用”或“待证据”，给源码和正反测试。对额外功能先隔离可选装配边界，不擅自删除。修复时遵守项目TMPDIR／独占Cargo target／正常增量并发及32GB停线清理规则。

因此，本报告可以用于安排下一轮Agent核对和修复，**不能作为“所有偏差均已找齐／全框架已1:1验收”的认证**。本轮没有运行Cargo测试，没有提交或推送。
