# 当前交付与验证

核心功能验收按[逐项清单](memory/core-functional-acceptance.md)单独统计：2026-11-06已提交基线501e4a25，188项中72项已验收、86项部分完成、30项待开发，等权验收完成率**38.3%**。覆盖71个核心相关章节，扩展另列不计分母；这是固定功能单元清单的验收比例，不是代码量、工时或全部API方法覆盖率。未提交AR-N01不计交付；复算入口为`scripts/summarize_core_acceptance.py`。

模型属性与认证前置MB-A01已由`06b4d50`限定提交并核远端；主线、实库、最低SQLite与三后端生成/适用consumer验证通过，原失败和未闭合边界保留。C07-W01/W02已由ed3f608限定验收提交推送；AR-P01已由4463238完成根应用/框架独立包迁移和日常适用验收，见[根应用与框架独立包限定验收](memory/testing.md#根应用与框架独立包限定验收)；AR-M01已由6c21bab0限定验收提交推送，当前主线接续AR-N01，J01代码/配套静审完成但尚未接线或运行；不报告完整Query/Model/框架已完成。

Unix网络测试库生命周期与持久归属回收已收齐[限定验收](memory/testing.md#网络测试数据库生命周期与持久恢复限定验收)，本地提交以Git为准；非Unix ACL与专项拓扑边界保留。统一Hashing前置、认证与生成接线已收齐[限定验收](memory/testing.md#统一-hashing-与认证接线限定验收)；通用Manager表面与Model hashed cast仍开放。下一步按唯一todo由主线推进AR-N01，Passport Client/签发/持久接线并行，不重跑已关闭范围或扩张本簇。

当前Fortify官方Web默认/显式JWT组合已通过main12门禁、五类实际生成、正式Provider受保护gRPC TLS、Redis父凭据回滚、真实双库CI、Keycloak/独立API故障恢复、最低SQLite3.26 probe及受影响consumer。限定本地提交以Git为准，具体范围见[当前验收](memory/testing.md#fortify-官方默认与显式-jwt-组合验收)；Google真实客户端、UUID/字符串JWT持久链与完整R7/官方生态仍未闭合。

Query Builder列边界谓词与AND/OR条件链已完成限定验收，唯一模块导入时间线覆盖MySQL/PG及显式JWT模式，生成应用公开API验证通过；完整Query/ORM仍开放。终结存在性已完成限定12门禁、真实双库与生成验收；存在性条件回调已完成8案及限定验收；before-query共享子节点/原query修改与消费已通过限定main12、双库各42及示例/生成验收；MySQL冷prepare已通过单连接淘汰/替换与写回滚、锁释放回归，临时Cargo补丁约束保留；完整Query/ORM仍开放，后继只在唯一todo领取。

🟡 R7具名认证核心编码及适用验证收齐，最终证据独审与资源收尾通过，本地提交以Git为准；主11门禁、当前默认workspace、7consumer、四fresh、真实双库各33及最低SQLite3.26迁移30断言通过。完整Eloquent/Fortify及其余认证范围保留，见[限定验收](memory/testing.md#r7-具名认证核心限定验收)。

✅ SQLite本地认证/MFA限定验收已通过，本地提交以Git为准；实际门禁与复用边界见[测试基线](memory/testing.md#sqlite-本地认证与-mfa-限定验收)。完整ADR-001和Fortify/Google客户端互操作仍开放。

✅ SQLite S3a原生数据库Session与显式Guarded已按[限定范围](memory/testing.md#sqlite-数据库-session-对应限定验收)完成主/精简门禁、固定最低3.26、23项真实双库、四类fresh、不同模型独审与资源收尾，本地提交以Git为准。完整Guard.id、SQLite认证/MFA/缓存锁/队列与ADR-001继续开放；后继回唯一todo，不以当前输入覆盖所有历史服务矩阵。

✅ SQLite配置与迁移子范围已完成13主组合、最低3.26/当前引擎、真实生成/根宿主、12consumer及限定双库验收，最后能力前置/文档4项补验与资源收尾也已完成，见[SQLite配置与迁移限定验收](memory/testing.md#sqlite-配置与迁移限定验收)。本地提交以Git为准；SQLite整体仍🟡，S3、完整Migrator/Schema/故障原子性及ADR-001继续开放。

✅ SQLite连接基础已完成[限定验收](memory/testing.md#sqlite-连接基础限定验收)：主10门禁/deny、单driver严格Clippy/doc、fresh11、真实MySQL3及根进程HTTP通过；SQLite整体仍🟡；S1当时的最低/配置/迁移缺口由上述S2分范围补证，剩余边界见唯一todo。最后导出补丁按静态影响复用已验组合，不删断言；本地提交以Git为准。

✅ 显式模块注册与宿主API前缀已完成[限定验收](memory/testing.md#显式模块注册与宿主-api-前缀限定验收)：10项主门禁、6个真实server进程、fresh minimal9/MFA真实MySQL3、生成模块持久启停1及只读空目录负例通过；首批8页mdBook构建/链接、112生成源码归档、不同模型独审和target清零完成，本地提交以Git为准。registered为应用约定，库file默认保留；完整Modules/Routing与全量文档未关闭，下一SQLite及CLI后继按唯一todo。

✅ 默认production＋log通信启动与安全诊断已完成[限定验收](memory/testing.md#默认生产日志驱动启动与安全诊断限定验收)：原始cargo run三场景、10项主门禁、通信consumer、fresh minimal9/MFA真实MySQL3与实际help通过，113生成源码及失败证据保全，最终target正常清零。完整MailManager与ADR-001未闭合，本地提交以Git为准；随后按唯一todo依次完成模块默认注册/API前缀及首批正式文档，再立即SQLite，CLI后继保留。

> 受众：AI Agent
> 更新：2026-11-02
> 摘要：R0 已固定上游；R1-S、R2-A/A2/A3、R2-B/C/D/M/N、R1-E 与 R3-A/B/C/D 均已按子簇完成适用门禁；R3-D 真实 HTTP、生成应用和安全复审证据见 `memory/testing.md`。完整 Laravel/laravel-modules 行为矩阵继续推进。

## 当前功能簇

配置d952199与应用职责161bfcf已限定提交。模块持久启停、独立状态路径、同容器管理/事件已完成限定验收及产物清理，提交事实见Git；默认1081/35 ignored、全特性1923/112，fresh普通12/MySQL3/MFA2、根真实MySQL1与三进程启停/缓存通过，见[模块管理证据](memory/testing.md#模块持久启停与同容器管理限定验收)。同内核嵌套Provider注册前置已通过限定验收和资源收尾，本地提交39086cf，见[嵌套注册证据](memory/testing.md#同内核嵌套-provider-注册限定验收)；module.json运行发现与Provider/files编译绑定已通过[限定验收](memory/testing.md#模块运行发现与编译文件限定验收)及资源收尾，本地提交d32162d；B003/B004默认实例缓存、失败内存语义与PHP顶层列表形状已通过[限定验收](memory/testing.md#fileactivator-实例缓存限定验收)并完成资源收尾，本地提交以Git为准；完整Modules/ProviderRepository/aliases、DB能力门控及ADR-001继续开放。DB通用能力/驱动/默认连接选择已完成[限定验收](memory/testing.md#db-通用能力与驱动分层限定验收)、独审与资源收尾，本地提交以Git为准；标准MySQL+Redis依赖与队列开箱已通过主门禁、根真实DB/Redis独立worker、四种fresh、外部SQLite及缓存重启的限定验收；不同模型独审、源码归档及资源收尾完成，本地提交以Git为准，详见[队列开箱证据](memory/testing.md#标准依赖与队列开箱限定验收)。默认连接错误时点与完整deferred Provider仍为R2/R10明确缺口。完整懒连接/默认名切换、五后端与ADR-001仍开放。

当前R2共享生命周期的命令所有权前置已完成[限定验收](memory/testing.md#共享命令所有权前置限定验收)：12项主门禁、真实MySQL模块维护1项、标准server/CLI续作构建、fresh精简9/MFA3及实际help通过；用户换号暂停/恢复与原始中断证据保留，原Goal active、原预算沿用。独审、113份生成源码归档及最终target清零完成，本地提交以Git为准；不关闭deferred/manifest/when或完整ProviderRepository。具体owner、配套和下一步以唯一todo顶部为准。

本批 Env/Request-MIME、TrustedHosts、应用/代码配置/Provider、异常、Web/CSRF与部分Query已完成限定组合验收，本地集成提交 `9035a00`。真实来源保全、双workspace门禁、生成、MySQL、TCP/TLS及开放差异见[本批证据](memory/testing.md#当前-main-组合限定验收)。下一步按唯一todo优先配置默认值/单一来源等整改，不将本批通过等同完整1:1。

✅ C04-G TrustProxies已完成main限定子范围验收：默认/全特性workspace963/1775通过、34/111忽略，双严格Clippy/docs、精确MSRV/no-default/deny、公开consumer/唯一模块、fresh普通8项/MySQL认证2项及真实TCP补充3案均通过；三项代理差异经不同模型复审关闭，专属缓存回收后仅保留下一环境簇所需主缓存。PHP8.2参考低于Symfony官方最低版本、MySQL无事前schema基线、逐案Rust映射与真实Cloud部署缺口明确保留；完整能力不关闭。见[本簇验收](memory/testing.md#c04-g-trustproxies-main-限定验收)，提交号以Git为准。

C04-F Request文件家族及MIME续作完成任务分支限定验收：MIME续作默认955/34忽略、全特性1770/111忽略，公开consumer、生成应用、真实MySQL与TCP通过；本次任务分支限定交付，尚未集成main，原生Fileinfo、Windows与完整Files仍开放。见[本簇证据](memory/testing.md#c04-f-request文件家族分支限定验收)。

✅ R03/typed/CC/Signed已完成同一main限定组合；默认/全特性严格Clippy与workspace tests已通过（907/0/34 ignored、1698/0/108 ignored），双docs/MSRV、no-default、deny及生成/MySQL限定证据见最终receipt；五父本地提交为 `e105dbfc58afc330b50e53b75f0b9e1424fa387a`；S-DV/ST-V/P及两guard工作区不纳入。见[组合记录](memory/testing.md#r2-四簇-main-组合限定验收)。

✅ R2-P Provider注册生命周期与S-DV Session对象值已完成同一main限定组合：三父提交 `9e5a94198b421987851b3580b27fa90370feeb77` 保留两个来源祖先；默认workspace943通过/34忽略，全特性1748通过/108忽略，双严格Clippy/docs、MSRV、no-default、deny、4个公开consumer测试与3项额外check、普通生成7及公开fixture1、真实MySQL根1和生成认证2、缓存命令/重启、根TCP六场景和清理均按[最终记录](memory/testing.md#r2-p--s-dv-main-限定组合验收)通过。分支阶段三次Clippy失败后用户授权的单次重验确已通过，旧失败保留；S-DV与P源分支均已安全退役；P两份root文档事实已迁入main，原稿保全。完整Provider发现/Facade、已服务Router热更新、Container及Session/Web仍开放。

✅ ST-V Storage visibility限定子范围已由来源`2262df9`两父合入main `8d18021b630ea10652316e025ef6019c7772bbb0`：805输入/33原日志SHA核对，默认945通过/34忽略、全特性1756通过/111忽略，双严格Clippy/docs、MSRV、no-default、当前锁deny、公开consumer1、根MySQL精确3、普通生成8/MySQL生成2与认证2、根TCP6及正常退出/端口重绑均有[本簇证据](memory/testing.md#st-v-main-限定组合验收)。🔵完整Storage/Filesystem仍开放；真实S3 ACL、迟到CopyObject和`throw=false`差异未实测或未收敛，ignored不算真实服务。

✅ R2-CC配置缓存家族已在隔离分支限定提交 `7be446d`：修后默认851/全feature1638（68ignored）、双Clippy/docs、doctest22/24、普通生成8与真实模块/生成应用缓存复启通过；两task temp已回收，混合归属同名包缓存保留待精确核对。分支证据不外推完整Configuration；main组合另见本页当前进度。
✅ R2-CC配置缓存限定分支验收：官方命令/fresh配置/命中屏障/原子文件/失败资源边界已实现；修后851/1638（all68ignored）、双Clippy/docs、doctest22/24、生成8与真实缓存复启通过，提交见Git。两任务temp已退役，混合归属同名包缓存保留待精确核对；main组合另见本页当前进度，不关闭完整Configuration。

✅ C04-D10-E scheme/secure与guest/intended已验收并提交6b143ca：定向10/main11、新构建、fresh普通7/MySQL认证2+公开helper1、无Session/MySQL真实TCP4/9场景通过；previous原值与先保存/先消费顺序已修复，唯一模块真实确认回跳不替代认证授权。完整Redirector/L020.02仍开放，见[本簇证据](memory/testing.md#c04-d10-e-scheme-与-guestintended限定子范围已验收)。

C04-D10-A公共错误包限定子范围已提交 `27c006a`。[证据及限制](memory/testing.md#c04-d10-a-错误包限定子范围已验收)。C04-D10-B请求绑定重定向、Session命名错误包与空库迁移启动修复已通过当前源码门禁、无Session及MySQL8.4真实TCP、普通生成7项/MySQL认证生成2项，已由 `a2befef` 限定提交；纯 Session 测试入口整合限定试点已验收并由 `4023e05` 提交：48→46 入口，26 项全保留；gRPC 联合监听测试的端口所有权空窗已修并通过定向/全特性门禁；PG与全部后端发布矩阵及完整Redirector/Validation仍开放。[B证据及限制](memory/testing.md#c04-d10-b-请求绑定重定向与-session-错误包限定子范围已验收)。

✅ D10-C请求局部视图错误共享、Session对象身份及模板参数桥接已按限定子范围验收并提交 `784045c`；根目标/fallback、内部forward与HEAD站点边界修复已通过不同模型审查及当前输入门禁、fresh生成与真实TCP。默认workspace798/31 ignored、全特性1571/105 ignored，严格双Clippy/docs、features/deny/MSRV、新server/CLI、普通生成7项、MySQL认证2项与独立公开consumer1项通过；无Session/MySQL8.4.13真实TCP通过并清理自有库/端口。ignored不计真实服务，完整Redirector、Validation/CSRF、HTTP/Session及PG/全部后端发布矩阵仍开放。证据见[最终测试记录](memory/testing.md#c04-d10-c-请求局部视图错误共享已验收已提交784045c)。旧输入失败保留，原resource-blocked回执不回填；独立public续作与原auth2分别记证。

C04-D8 previous与显式Precognition限定子范围已完成适用验收：稳定主线门禁、全特性1439 passed/104 ignored、Rust1.97 check、真实PG18/MySQL8.4各67项模块HTTP及各1项previous、所有权PG3/My2、新生成应用5/2/2、真实TCP6项与最终CLI快照1项均有证据。默认workspace以709项有效结果与修后20项doctest按影响复用，非虚报原失败命令exit0。完整Old Input/CSRF/FormRequest、规则集与整个HTTP/Session继续开放。 [证据与限制](memory/testing.md#c04-d8-previous-与-precognition限定子范围已验收)。C04-D9旧输入限定子范围已完成main组合验收：唯一模块与生成helper通过公开API运行，默认750/31 ignored、全特性1491/105 ignored，根双库各72+1、真实TCP六场景与fresh5/2/2具备[证据](memory/testing.md#c04-d9-main-组合限定子范围验收通过)；RedirectResponse、文件输入合并、CSRF/FormRequest仍开放。R2-X d299ca4与R2-Q 850e1a4已获用户main集成授权，已完成main组合真实验收，本地集成提交见Git与[testing](memory/testing.md#r2-qx-main-组合验收通过)；不以此关闭完整配置/Container。

✅ C04-D10-F refresh/away已验收并提交ba538ea，当前适用门禁、fresh普通7/MySQL认证2+公开helper1及真实TCP7/12与资源收尾通过；完整Redirector仍开放。

✅ C04-D10-G动作路由身份与action URL已限定验收：定向、双严格Clippy、workspace默认824/34ignored与全特性1611/108ignored、docs/features/deny/MSRV、新构建、fresh普通7/MySQL2+1和真实TCP10/15通过，3专属package清理完成，限定提交见Git。gRPC fixture启动空窗已健壮化并严格重验，原停机失败原因未确证、旧失败保留。完整Routing/Controller/URLGenerator/Redirector仍开放，见[本簇证据](memory/testing.md#c04-d10-g-动作身份与-action-url限定子范围已验收提交见git)。

✅ C04-D10-H最终声明集合与真实分派同源化已限定验收：双严格Clippy、完整router124/120、主矩阵默认838/34ignored与全特性1625/108ignored、docs/features/deny/MSRV、新构建、fresh普通7/MySQL2+1、应用真实TCP11/16及3专属package精确清理通过。主控复核74份关联SHA/580运行输入；原失败及复用边界保留，限定提交见Git，不关闭完整Routing。

✅ T01 Session测试入口整合试点已验收：全仓显式test入口46→45、Session family8→7、源文件10不变；两feature各41项与原26+15逐名对应。双严格Clippy、workspace默认838/34ignored与全特性1625/108ignored、双docs/deny/MSRV通过，生产/模板/锁未改，未重跑真实服务；未证明整体开发显著提速，限定提交见Git。

🟡 R2-R03 递归配置发现分支限定验收已收：配置76、CLI21、模块2、公开consumer6、严格Clippy/docs、普通fresh生成7+安装consumer1、MySQL8.4认证2无skip通过；无auth MySQL测试0项仅编译。根config-feature HTTP 200/enabled=true/exit0，URL/TLS runner另10/10，证据不混用。owner报告3唯一生成包清理，真实清理命令 exit0 由执行者终端回报，原始 stdout 未留存，登记54.48GB/磁盘8.782%触软线。分支已提交6482a38，四簇同树组合的限定验收见当前进度；完整Config、typed/cache后继扩展、动态Settings及发布全后端未关闭，见[分支证据](memory/testing.md#r2-r03-递归配置发现任务分支限定验收记录)。

🟡 Signed URL 家族任务分支限定验收已通过：库7、router4（真实 loopback TCP）、唯一模块1、CLI20、普通生成7；补验严格 Clippy、公开 TestApp1、gRPC宿主check、MySQL8.4认证生成2项无skip通过。TCP异常清理增量有不同模型静态复审；本次生成包已精确回收 316,715,008 B，清后仍超过 50GB 软线；分支限定提交及main同树组合已完成，细项见上方组合记录。完整 URLGenerator/Redirector、Vapor真实代理与复杂 PHP 参数未关闭。见[分支证据](memory/testing.md#signed-url-家族任务分支定向验收未合入-main)。

## Laravel 1:1 功能与进度概览

> 更新：2026-11-02；C04-B2本轮适用验收已完成，本簇限定本地提交见Git；不将子簇完成外推整能力。本页是唯一概览入口，详细行为/差异以矩阵为准，执行队列仅在[todo](todo.md)。

**总体仍未完成严格1:1。** 已验收的是明确子范围，不是整项能力全完成。R1-S、R2-A/A2/A3/B/C/D/M/N、R1-E及R3-A–F保留各簇证据；旧专项实现也须重新对应固定Laravel生产行为。C04-B1 `8846a6a`已交付，B2类型便捷方法按子簇验收；APP_PORT `591142f`只关闭端口配置子项，分页`4573234`仅整改计划，历史决策 080仅规划。

图例：✅ **已验收子范围**（只限边界列）；🟡 **开发中**（未收口）；🔵 **已有实现待1:1对齐**；📝 **仅规划**；⬜ **待开发**；⛔ **阻塞**（须写具体原因）。同一行以主状态概括，已验收基础不会把整体涂成✅；阻塞只是具体交付受限，不隐去其他可做部分。

固定来源与细项：[R0基线](plans/laravel-rebuild-baseline.md)；[L001–L034](plans/laravel-rebuild-matrix-1.md)、[L035–L068](plans/laravel-rebuild-matrix-2.md)、[L069–L102](plans/laravel-rebuild-matrix-3.md)。表内L编号可在对应矩阵定位，最终验收仍须上游源码/测试→Rust公开API→生产调用→唯一模块/生成项目/文档→适用真实服务。

### Laravel 能力族

| 能力族 | 1:1目标 | 主状态 | 当前可证边界 | 待补关键项 | 稳定章节 / 阶段 / 证据 |
| --- | --- | --- | --- | --- | --- |
| 启动、应用布局 | Laravel应用职责/生命周期与开箱运行 | ✅ 已验收子范围 | R1-S 34dfb5e、R1-E bc3fa01：根lib/server、唯一模块与真实CRUD | 完整安装/升级/部署流程仍须逐项对照 | L004/L007/L010/L011；R1/R13/R14；[证据](memory/testing.md) |
| Container / Provider / Facade | 统一DI、register→boot、作用域与扩展 | ✅ 已验收子范围 | R2-A/A2/A3、B/C/D已验收，唯一生产Container/Provider、入口scope/TestApp | R2-X保留原factory的extend及公开Session装饰器已完成main组合验收；阶段一Provider编译期发现仅规划，现显式descriptor不是包发现；完整Facade/Container/扩展对应待闭合 | L012–L014/L035；R2；[core](core/core.md)/[facade](core/facade.md) |
| Configuration | env/领域配置、typed读取/校验与包配置合并 | ✅ 已验收子范围 | R2来源失败/Provider配置；历史APP_PORT 591142f已验收，R2-ENV按新授权完成SERVER_PORT与CACHE_STORE/cache.default限定验收（工作区、生成/MySQL、真实HTTP/Redis与收尾） | R2-Q原始/批量读取已完成main组合验收；R2-R01运行期写入限定交付（仓储/App/模块/consumer、工作区、fresh生成/MySQL/真实TCP及资源收尾），本地提交见Git；R2-R02有序来源限定验收通过：适用Rust门禁、consumer、普通/MySQL生成、生产TOML真实TCP与资源收尾，本地提交见Git；R2-R03递归发现/文件名命名空间已在四簇main限定组合验收；完整L005及缓存/来源/扩展未闭合；DB业务设置见增强表 | L005；R2；[core](core/core.md)/[todo](todo.md) |
| Routing / Middleware / URL | 路由组/域/命名URL/绑定/中间件默认与错误 | ✅ 已验收子范围 | R3-A–F按簇交付；F 163bb49域路由；D7请求匹配/方法声明与fallback纠偏、D10-G动作身份/namespace/共享action URL、H真实声明与分派同源化已限定验收 | 模型绑定及其余C03/URL参数行为仍开放，不能由子范围整体外推 | L015/L016/L024；R3；[http](core/http.md)/[todo](todo.md) |
| Request | 输入来源、typed便捷方法与生命周期 | 🟡 开发中 | C04-A/B1/B2/C及D6/D7/D9各子范围已验收；D10-B真实Request借用的输入闪存与重定向已限定验收并提交 `a2befef`；D10-D捕获同应用URL生成器的请求快照已限定验收，提交见Git；C04-F文件输入及MIME续作完成任务分支门禁、生成应用、真实MySQL与TCP，本次任务分支限定交付，尚未集成main | 原生Fileinfo/Windows与完整MIME/文件验证、Session/CSRF/FormRequest及跨平台取消清理仍开放 | L019；R3 C04；[F分支证据](memory/testing.md#c04-f-request文件家族分支限定验收)/[todo](todo.md) |
| Response / API Resource / 异常 | 原生响应、资源序列化/条件字段、错误渲染 | 🔵 已有实现待1:1对齐 | 类型化响应/envelope与语义状态已交付；D10-B普通与请求绑定重定向已限定验收并提交 `a2befef`；D10-D命名重定向已提交018b778；D10-E scheme/guest/intended通过当前适用验收，限定提交见Git | 完整Resource、命名重定向混合/可选参数缺口、refresh/away本簇通过适用验收，signed已在四簇main限定组合验收；动作其余参数形态、missing resolver、stream及异常自动重定向仍待 | L018/L020/L027/L068/L069；R3/R5；[http](core/http.md) |
| Pagination | Query/Model paginate、simple/cursor、链接与Resource | 🔵 已有实现待1:1对齐 | 现PageParams/手动controller分页；4573234仅登记整改 | 默认15/page解析、直接JSON与Resource links/meta、查询/模型入口及边界 | L059/L068.04；R3→R4/R5；[Spec](todo.md#c04c05-分页整改-spec已授权按依赖领取代码未改) |
| Validation / FormRequest | 验证规则、授权、错误与文件验证 | 🔵 已有实现待1:1对齐 | Validate/Validated与模块输入校验已有测试；D10-A消息包已提交，D10-B手动命名错误包闪存已提交；D10-C请求错误视图共享限定验收通过，已提交 `784045c` | 自动验证失败重定向、`dontFlash`、完整FormRequest授权/规则/文件行为 | L026/L019；R3/R7；[http](core/http.md)/[todo](todo.md) |
| DB / 事务 / Migrations | 连接、事务/savepoint、Schema与迁移生命周期 | 🔵 已有实现待1:1对齐 | SeaORM双库等专项资产；D10-B MySQL8.4空库迁移与MFA启动顺序限定验收通过、待提交 | 失败迁移恢复本轮未实跑；R4完整公开契约与发布全后端边界仍待 | L057/L060/L062；R4；[core](core/core.md)/[todo](todo.md) |
| Query Builder | Laravel式组合查询、锁/聚合/chunk与分页 | 🔵 已有实现待1:1对齐 | cd0ac0f/9f51471/73a845d/a9103f9/960aca4/ed3f608各有限行为已验收提交 | 完整JSON/FROM/raw/join/写目标、聚合/chunk和自动分页仍未闭合 | L058/L059；R4；[核心清单](memory/core-functional-acceptance.md)/[db](core/db.md) |
| Eloquent / 关系 / Cast / 集合 | Model CRUD、关系、scope、事件、序列化 | 🔵 已有实现待1:1对齐 | 06b4d50模型原始行、metadata、基础getter/cast与可逆setter已有限验收 | save/dirty/事件完整写链、关系/日期/完整cast与集合仍开放，SeaORM Entity不等于Eloquent | L064–L069；R5；[证据](memory/testing.md#模型属性与认证前置限定验收) |
| Factories / Seeder | 工厂states/关系fixtures及种子工作流 | 🔵 已有实现待1:1对齐 | 已有seeder/TestDatabase资产，不等于官方Factory或完整db:seed | 完整Factory状态/关系/回调、Seeder命令/模型事件仍未闭合 | L061/L070/L078；R6；[核心清单](memory/core-functional-acceptance.md) |
| SQLite | 轻量后端连接、SQL/迁移/事务及驱动生命周期 | 🔵 已有实现待1:1对齐 | 连接、配置/迁移、数据库Session、本地认证、同机租约已有main限定验收 | UPDATE/DELETE写目标、完整Schema/Queue/CacheLock及发布矩阵仍开放 | L057/L060/L078；[证据](memory/testing.md)/[todo](todo.md) |
| Authentication / Authorization | guard/provider、用户流程、Gate/Policy | 🔵 已有实现待1:1对齐 | 5ad34b9具名Guard/UserProvider和SessionGuard限定验收；06b4d50属性接线已交付 | 完整Guard driver、Gate/Policy、PasswordBroker、邮箱验证及所有标识持久链仍开放；Fortify/JWT/OIDC另列扩展 | L051–L053/L056；R7；[auth](core/auth.md)/[核心清单](memory/core-functional-acceptance.md) |
| Session / CSRF / 限流 | 共享会话、请求伪造防护与限流 | 🟡 开发中 | D1–D3及D5各子簇已验收；D4 47cd7b8内置双Cookie、实际保护认证、宿主/生成接线已通过最终12门禁、根双库各62+1、生成5/2/2与真实网络/浏览器，限定提交见Git | D5 GC/索引迁移已按限定子范围验收，final4/根双库62/专项2/Redis1+10/生成5+2+2通过；D8 previous/Precognition与D9旧输入各自限定子范围已验收；D10-B命名错误包持久恢复与手动闪存已提交；D10-C请求局部错误视图共享已限定验收并提交 `784045c`；完整Redirector、CSRF、blocking/cache、其余driver和PHP异常弱类型差异仍开放 | L017/L025/L047；R3/R7/R14；[session](core/session.md)/[testing](memory/testing.md)/[todo](todo.md) |
| Encryption / Hash / 安全 | 应用加密、轮换、签名及安全边界 | 🔵 已有实现待1:1对齐 | 0c29bd9三Hash驱动/needsRehash/默认与扩展及认证生成共用已有限验收；加密/签名另有专项 | 完整Crypt、HashManager表面及Model hashed cast仍未闭合；DB业务secret激活另列增强 | L054/L055/L024；R7；[security](core/security.md)/[signing](core/signing.md) |
| Cache / 原子锁 | 默认/具名store、TTL/共享锁与失败语义 | 🔵 已有实现待1:1对齐 | 统一store/Redis/DB及共享锁实现和专项证据 | Laravel Cache全方法/默认/锁与生产故障完整对应 | L031；R8/R14；[cache](core/cache.md) |
| Storage / Filesystem | 默认/具名disk、私有对象与文件操作 | 🔵 已有实现待1:1对齐 | 本地/S3、受控staging、MinIO与双库业务证据 | 完整Filesystem对应、迟到发布不确定性/孤儿边界仍公开 | L037；R8/R14；[storage](core/storage.md) |
| Views / 模板 / Assets | view与模板扩展、资产开发/生成体验 | 🔵 已有实现待1:1对齐 | 服务端模板和根公开HTML已有实际应用；D10-C请求局部errors与生成helper限定验收通过，已提交 `784045c` | Blade职责等价、组件/布局/资产完整流程；非PHP字节兼容 | L008/L021–L023；R9/R13；[view](core/view.md) |
| Localization | locale/fallback、翻译格式与作用域 | 🔵 已有实现待1:1对齐 | 请求/job locale与国际化模块/真实流程资产 | 完整Laravel本地化语义与资源生成/错误边界 | L041；R9；[i18n](core/i18n.md) |
| Mail / Notifications | Mailable/通知渠道、同步/排队与模板 | 🔵 已有实现待1:1对齐 | SMTP/log/fake、Mailpit与队列通知专项；SMS见增强 | 完整Mailable/Notification公开契约和适用渠道真实验收 | L042/L043；R9/R10；[mail](core/mail.md)/[queue](core/queue.md) |
| Queue / Bus | 代码Job/connection/queue、事务/重试/编排 | 🔵 已有实现待1:1对齐 | DB默认/Redis可选、lease/token/worker/chain/batch已有专项 | R10 after_commit/outbox等完整对应；default错误时点/完整deferred仍有缺口；新动态管理未实现 | L046；R10/R14；[queue](core/queue.md)/[todo](todo.md) |
| Scheduling | frequency/timezone、单服务器/不重叠、runner | 🔵 已有实现待1:1对齐 | DB slot+job事务、cron/IANA/DST/补跑已有双库专项 | 每任务onOneServer/withoutOverlapping默认与策略；DB动态计划未实现 | L050；R10/R14；[schedule](core/schedule.md)/[todo](todo.md) |
| Events / Broadcasting / WS | 事件/listener、排队监听与跨节点广播 | 🔵 已有实现待1:1对齐 | 进程事件、queued listener、WS/SSE和Redis跨进程专项 | 完整Laravel广播/授权/事件契约与故障拓扑 | L030/L036/L096；R10/R11/R14；[events](core/events.md)/[realtime](core/realtime.md) |
| HTTP Client / Process / Context | 出站请求/进程、并发与上下文边界 | 🔵 已有实现待1:1对齐 | 可选出站HTTP与ExecutionContext已有实现 | 完整Client/Process API、fake、取消/资源与上下游故障 | L033/L034/L039/L045；R11；[http](core/http.md)/[core](core/core.md) |
| Search / Flags / Observability | 搜索/特性与日志/观测的官方职责 | 🔵 已有实现待1:1对齐 | metrics/OTel/日志和Collector专项资产 | 搜索/feature flags未由观测实现替代；完整R12及Driver验收 | L028/L048/L091/L095/L099/L101；R12；[计划](plans/laravel-rebuild.md)/[证据](memory/testing.md) |
| Artisan / Testing / 部署 | CLI生成、fake/assertion、测试与集群运维流程 | 🔵 已有实现待1:1对齐 | CLI new/dev/generate、TestApp/真实生成/双库和根启动子范围已验收 | 完整命令/生成/测试公开体验与R14部署、最低版本/性能门禁 | L029/L074–L079/L010；R13/R14；[cli](core/cli.md)/[test](core/test.md)/[todo](todo.md) |

### laravel-modules（独立来源）

| 能力族 | 1:1目标 | 主状态 | 当前可证边界 | 待补关键项 | 稳定章节 / 阶段 / 证据 |
| --- | --- | --- | --- | --- | --- |
| laravel-modules | v13模块注册/资源/依赖、启停/生成与完整管理 | ✅ 已验收子范围 | R2-M/N：唯一modules/example、编译清单/激活、资源挂载、稳定依赖顺序；48be854等子簇 | 编译期发现/生成清单仅规划；完整模块命令/发布与其余子行为未完成，目录相似不是全包等价 | [M001–M035](plans/laravel-modules-matrix.md)；R2/R13；[todo](todo.md)/[core](core/core.md) |

### 官方生态与授权增强（不冒称Laravel核心内置）

| 能力 | 状态 | 已有边界与下一步 |
| --- | --- | --- |
| 官方可选包与配套工具 | ⬜ 已授权待开发 | [全量范围与分期](plans/laravel-official-coverage.md#全量交付授权与两阶段)已登记749条Laravel行为与35个模块入口；AI SDK/MCP/Boost、官方认证/计费/搜索/实时包及工具UI、前端配套等须逐包冻结源码/测试与可执行Spec，登记不等于实现或验收 |
| DB动态业务设置 / 管理员动态计划 | ⬜ 已授权待开发 | 历史决策 080及[唯一计划](plans/runtime-configuration-scheduling.md)；阶段二V1必交增强（不移动核心Config/Queue/Scheduler），宿主启用可选，尚无新实现/验收；可选快速开发模块表单后置 |
| 统一返回格式 / 业务观测增强 | 🔵 已有实现待1:1对齐 | 保留价值，须经Response/Resource/异常与公开扩展融合；分页独立协议整改仍未完成，不宣称上游原生默认 |
| SMS具体渠道 | 🔵 已有实现待1:1对齐 | 作为Notification/驱动扩展；Twilio adapter与fake/log资产不代表真实短信投递，实发缺授权账号/凭据/目标；[mail](core/mail.md) |
| gRPC | 🔵 已有实现待1:1对齐 | 可选协议扩展、生成与真实TCP专项见[grpc](core/grpc.md)；不是Laravel核心原生gRPC；完整集群/生命周期仍须R11/R14验收 |
| MQTT Broker / Client | ⬜ 已授权待开发 | [总计划R11](plans/laravel-rebuild.md#r11-明确待办mqtt-可选服务端与客户端)；阶段二client/Broker按需独立引入；既有Provider加载不暗启连接，client lazy调用/订阅CLI/Broker CLI显式运行；未实现/未真实验收 |

### 如何读取与维护

- [两阶段框架主线](plans/laravel-rebuild.md#两阶段框架主线)：阶段一Laravel+正式modules基线（含Provider包发现），阶段二交付官方可选生态及已授权通用增强，均属框架main/V1路线；不是未启动可选快速开发模块。表按可证范围显示状态，不因阶段重排抹掉既有实现，也不把阶段一完成说成全部框架交付。
- 本表按本次审阅划分 **27个Laravel能力族**：✅子范围4、🟡开发中2、🔵待对齐21、⬜待开发0；另列modules一个✅子范围。它们是概览行计数，粒度不等、不是权重或完成百分比，也不包含增强表。
- 749个Laravel二级节与35个模块导航入口不是同一计数单位；详细矩阵的旧“待验收”不能解释为代码0%。核心完成百分比只使用[核心功能验收清单](memory/core-functional-acceptance.md)的固定分母、明确已验收分子及计算规则；不从本概览行数、已有代码或测试数量推出1:1完成率。扩展单列，未完成行为保留。
- 总控每次功能簇本地commit后就地更新受影响行的状态、可证范围、缺口、commit/证据，并同步详细矩阵；只有新增实际交付/验收才升级状态，WIP与定向测试不能提前变✅。调整分组时重算上述概览行数，不按测试数充进度。
- 每次框架开发进度汇报附最新能力状态表，汇报前按当前代码/提交/验收复核；表格格式、适用范围与“1:1 的 Laravel 框架 Rust 版”表述统一遵循[汇报规范](style.md#开发进度汇报)，普通问答不强制附表。
- overview只摘要；矩阵维护详细源码/测试对应及差异，core/testing保存可复验证据，todo是唯一可领取队列，不在此复制任务或建立第二计划。未验收的整项不得因已交付子簇关闭。
- [可选快速开发模块/C32/R15冻结](plans/optional-rapid-development-modules.md#开发与使用边界)，R14完成不授权开工；未来分支另定，当前main维护框架及官方可选工具（含自身UI），按依赖推进。AI产品仍未启动，均不计V1框架核心1:1进度。当前不为展示表格切换或中断在途HTTP功能簇。精选扩展的接入/范围与独立验收遵循[生态边界](plans/laravel-rebuild.md#生态边界)，不抵扣核心缺口。

R2-T 严格类型配置读取仅在隔离分支通过仓储43、Collection4、模块1、公开consumer6项及严格Clippy3/Rustdoc1；普通生成7+fixture1、真实HTTP及唯一生成包清理通过，原分支限定交付，现已纳入四簇main组合，不改变上表已验收main子范围或完整L005缺口。

## 子簇验证边界

以下记录限定各自交付范围；不能跨簇累计成完整框架已验收，当前领取与未提交实现以todo/源码和上方概览为准。

C04-B1统一请求输入已完成适用验收，限定提交见Git：定向/独审、相应源码快照的workspace门禁、根默认cargo run真实TCP、生成consumer与PG/MySQL生成认证应用均通过；根具名认证双库各1通过/46 filtered。账号/缓存fixture缺Host及新迁移排序问题已修复，原安全与回滚断言保留；根完整PG/MySQL组均47通过/零失败、忽略或筛选，账号MFA与完整缓存模块的双库专项亦通过。最新workspace全部门禁通过；框架完整范围仍按唯一队列推进。临时探针及隔离数据库已清理，外部示例/守护WIP保留；证据见[testing](memory/testing.md#c04-b1-统一请求输入)。

C04-C已完成Cookie读取、请求队列、值工厂/响应组合和复用现加密内核的保护；最终配置/可信HTTPS、原始重复凭据与空domain覆盖已回归。最终workspace默认668/全特性1241 passed、29/99 ignored，根PG18/MySQL8.4各48、生成plain3/认证双库各2及真实TLS通过；fmt/Clippy/docs/no-default/MSRV/deny和不同模型审查完成。根target已清空、五旧consumer未改，真实证据与未提供外部条件边界见[testing](memory/testing.md)。本簇限定提交见Git，后继Session/flash/CSRF等继续唯一todo；可选快速开发模块暂不开发。

C04-B2类型便捷方法已通过适用门禁：公开Request/Fluent、唯一模块、生成项目与真实TCP均接同一输入内核；默认workspace645/全特性1211项、根PG/MySQL各47项、普通与双库认证生成应用、新工具TCP六案通过，fmt/Clippy/docs/feature/MSRV/deny及不同模型复审完成。日期source分支与代表矩阵、类型表示/资源限制、ignored与非供应商发送边界见[testing](memory/testing.md#c04-b2-类型便捷方法)；完整C04及全Carbon工具箱未据此关闭。本簇限定本地提交见Git，后继仅从todo领取。

C04-A RequestInfo/内容协商及存储staging并发回收修复已完成适用验收：默认523/29、全特性1082/99（passed/ignored），fmt/Clippy/Rustdoc/no-default/MSRV1.97/deny、生成consumer2/2和根默认入口真实TCP均通过；不同模型独审通过。仅完成无正文请求检查与协商，不代表完整C04。 原三轮构建NotFound、可控red/green及非原子base复核边界见 [testing](memory/testing.md#c04-a-请求检查与内容协商)。

R3-F 已实现 `RouteGroup::domain`、独立 `DomainParams<T>`、域具名 URL 与保留原请求的候选分派。独立复审发现的声明顺序、merge、forward authority、域与路径参数冲突、fallback scope 优先级及非法嵌套 scope 均已修正；最终只读复审无 blocker/should-fix，定向 HTTP router 87/87。默认/全特性 workspace Clippy/tests/Rustdoc、no-default、MSRV1.97、deny、生成应用和生产模块专项均通过，根无参数入口真实 TCP 域响应/错误 Host/health 与 SIGTERM exit0 已验收。根/生成 target 分别回收28.4/2.8GiB，仓库target归零、磁盘可用120GiB；本簇限定提交见Git，后继C04。已知域模式及fallback前缀差异见历史决策 077，不宣称完整R3已完成。

当前根 openrust 同 package 提供框架 lib 和唯一默认 server，应用职责回根；modules/example 是普通业务模块，经生产 `Application::modules` 编译清单与 FileModuleActivator 状态接入。根无示例 build.rs 或构建依赖；protobuf 工具和锁仍在模块内。

结构子步骤已实际通过 fmt、默认/full Clippy/tests/docs、deny、no-default；默认 263/22、full 486/32（passed/ignored）。根裸 cargo run 的健康/API/公开 HTML/业务响应与正常退出、根 CLI 转发、PG18.6/MySQL8.4.13 各 12 targets/48 项及 testing 1 项、三类生成项目、最小 orders 模块仅改应用清单的真实 HTTP 闭环通过。CI 双库六组、MSRV1.97、八个独立 consumer 与额外 gRPC 生成项目均通过；不同模型最终源码复审无 blocker/should-fix，独立 CI 复核无 blocker/should-fix，已本地提交34dfb5e；最新事实见 [testing](memory/testing.md#r1-当前工程边界与验收入口)。

R1-E 真实 CRUD/当前公开能力示例已按适用范围验收并以 bc3fa01 提交，附件 deadline/EOF 竞态修复经独立复审和真实 MinIO 双库场景验证；限定的跨系统故障边界仍记录在 [testing](memory/testing.md#r1-e-审查修复与追加验收2026-10-29)。这只关闭当前综合示例子簇，不代表 Laravel/laravel-modules 全行为矩阵完成。后继按 [待办](todo.md) 领取。

R2-N 模块依赖顺序已按 历史决策 075 实现并验收：依赖声明与稳定拓扑排序进入 `ModuleManifest`，`ProviderBuilder` 的 register/boot 同序；缺失、未启用、无效、重复、自依赖与循环均有稳定错误码。真实 PostgreSQL 18/MySQL 8.4 隔离数据库 HTTP、生成应用 `.requires` API check/test、根默认 `cargo run` 的 TCP `/health` 与 `/api/example`、workspace 默认/全特性门禁、MSRV/no-default、Rustdoc、deny 均通过。详细命令和证据见 [R2-N 测试记录](memory/testing.md#r2-n-模块硬依赖与确定性启动顺序)。完整模块管理仍属于后继；Laravel 总体范围继续未完成。

目录目标为 `modules/<模块名>/` 与唯一 `modules/example/`，见 [ADR-001](decisions/adr-001-framework-principles.md)。R2-M 当前实现与真实验收单独记录；完整管理命令、发布流程和 CRUD 未交付。计划修订与文档检查不能代替这些代码验收。

当前规划记录已统一：ADR-001、总计划、活跃队列、执行说明与 README 区分 app/example 过渡事实和 modules/example 目标；此前规划文档检查覆盖 63 个变更 Markdown、1222 个本地链接和 53 个锚点，无断链；749 个 Laravel 稳定 ID 与 35 个模块入口 ID 未改状态。文档检查不是代码门禁，代码门禁已补齐，限定提交仍须完成最终审查与提交树核对。

R2-A 容器基元已实际完成15项回归与默认279/22、full502/32及双Clippy/docs、deny、MSRV/no-default。公开下游consumer1项与根裸run/HTTP通过，不同模型最终复审无blocker/should-fix；已提交26df75c。生产启动仍未切换；Provider、异步资源生命周期、配置合并与模块系统留后继，不把基元当整体R2完成。

R2-A2已实际通过25项配置定向、默认286/22与full509/32及对应双库/生成/consumer/HTTP门禁；来源错误不再退默认，Debug/日志不输出目录原值，已提交2003f0a；独立A3补齐容器发布/构造帧竞态回归，继续Provider/配置契约，不把源加载修复当完整内核完成。

R2-A3已实际修复缓存发布早于构造帧结束导致误报cycle，以及scope/root reset锁内通知导致同步Wake重入死锁。19项定向、默认290/22与full513/32、双Clippy/docs、deny、MSRV1.97/no-default通过；公开下游check/test1项通过，根裸run的health/HTML/API/example200及SIGINT0通过；已提交。旧生产 Plugin 路径已由 R2-B 替换，不把此前基元证据转记新架构验收。

R2-B：生产启动已改为全部 register→基础配置→顺序 boot→最终校验/required 解析；HTTP 请求、命令及领域服务使用唯一 Container，旧 AppBuilder/Plugin 启动循环和领域 Plugin 类型已移除。受管资源覆盖启动失败/取消与 App/HTTP scope 关闭。默认/all-features workspace 测试、Clippy、Rustdoc、deny、MSRV1.97、无默认 feature 均已实际通过；PG18/MySQL8.4 各 12 targets/54 项，独立生成双库应用、八个消费者及根默认 server 真实 HTTP/退出的证据见 [testing](memory/testing.md#r2-b-统一-provider-内核验收证据)。Figment 来源字典顺序和第三方自定义 driver 校验仍有明确差异；不宣称 R2-M 模块管理或 R1-E CRUD。

R2-C：HTTP 请求 scope 延续到 stream/SSE body 结束或断连；命令、队列每次 attempt、调度每次到期 slot 各有独立 scope。受管任务在该 scope/显式 metadata 中构造和执行，关闭先封解析/取消任务，再由 App 持有的单一清理 driver 有界回收；普通 spawn 不继承身份。真实 TCP 三项、PostgreSQL 18/MySQL 8.4 各 queue 3 项和 schedule 7 项、Redis 8 三项、公开自定义 driver binary、双库实际生成应用的 check 与真实 auth HTTP 各一项均通过。默认/全特性 workspace fmt/Clippy/tests/Rustdoc、deny、no-default、MSRV1.97、根实际 HTTP 及退出也通过；完整命令、初次 gRPC 5 秒超时后的单项与全量复跑、忽略项和日志见 [testing](memory/testing.md#r2-c-入口-scope-与受管清理验收证据)。当前仍是 `app/example` 过渡模块，R2-D/M 与 R1-E 待后继。

R2-D：`TestApp<A>` 与生产应用共用 Provider 清单和单一 Container/Router 构建路径，支持注册前移除 Provider、全 register 后覆盖 typed 绑定、有界进程内请求及安全响应断言；完整 reset 先关闭旧 App。Drop 即时封闭新解析，存活运行时才可托管异步兜底清理，显式 shutdown 可等待错误与资源。综合示例、公开 consumer、普通生成项目及真实 PostgreSQL/MySQL 认证生成项目均已运行；共享关闭状态机在双库 queue/schedule 和 Redis 上重新验证。默认/全特性 workspace、MSRV1.97、deny、no-default、根默认 HTTP/退出证据及一次未归因的 all-features 初始失败见 [testing](memory/testing.md#r2-d-testapp-验收证据)。`app/example` 仍是过渡模块，R2-M 与 R1-E 未交付。

R2-M：编译期 `ModuleDescriptor` 与运行时 FileModuleActivator 快照进入生产/测试共用 ProviderBuilder；模块配置在实际 boot 时点 overlay，命名空间 view/lang、模块资源与统一迁移历史均由公开能力贡献。原示例完整迁至 modules/example，根和生成应用从运行根读取状态。PostgreSQL/MySQL 隔离库的全局批次/禁用历史/错库锁/单连接池与失败重启、Redis/队列/调度、真实部署目录 HTTP、普通及双库认证生成应用均执行，完整结果见 [R2-M testing](memory/testing.md#r2-m-模块清单资源与统一迁移验收证据)。MySQL DDL 部分提交及 R13 管理/发布与 R1-E CRUD 保留边界；本簇限定本地提交收口。

R1-E（已验收并提交 bc3fa01）：`modules/example` 已接真实 Article CRUD、模块业务服务与当前公开能力的实际调用；PG18/MySQL8.4 的 Article、tenant/MinIO/Redis、队列与 Scheduler、OIDC/浏览器 WebSocket 等有定向证据。Collector PostgreSQL 活跃日志路径已校正，隔离 sink delivery/unavailable 各 1/1；16 MiB 与隐私检查保留，共享 Collector 未改。CLI 生成器重生十种变体，生成源码格式、基础/认证/MFA真实 HTTP、gRPC TLS Echo/HTTP/停机均实际验收，测试 0 ignored；CLI 20/20。默认/全特性 workspace Clippy/tests/Rustdoc、deny、no-default 与 MSRV 适用门禁均通过；默认测试 454 passed/23 ignored，全特性 990 passed/90 ignored。附件 AtomicU8 仲裁 deadline 与 EOF；PG18/MySQL8.4 全模块附件各 15/15，期限释放锁用例与真实 MinIO CopyObject 延迟响应删除竞态各按双库 1/1。独立复审确认并发 blocker 已消除；真实 Keycloak `AlwaysOk` 错误密码、已消费 state 与成功夹证 PG/MySQL 各 1/1；gRPC TLS 错误标识验证成功 RPC 前后、错误 hostname 与网络故障负例；浏览器拒绝事件结合真实 TCP 精确状态测试分别验证，未声称浏览器暴露状态码。远端结果不确定后的迟到发布、App 关闭期限和进程强杀仍不保证跨系统原子性/无孤儿。提交后任务专属 root 与临时 Cargo targets 均已清理。下一簇为 R2-N 模块硬依赖图与确定性启动顺序，具体契约见 [todo](todo.md#当前-r2-n-spec模块硬依赖图与确定性启动顺序)；R1-E 完成不代表 Laravel/laravel-modules 全体完成。

R3-A：已按 历史决策 076 将 `semantic` 作为框架、配置、请求错误 fallback 与生成应用默认，`always_ok` 仅显式配置启用。默认/显式状态、统一 JSON envelope、request ID、validation 422、创建 201、路由与服务错误、middleware I18n 解析早退均有测试；普通与双库认证生成项目实际运行，根默认 TCP 入口验证 404/health 200/正常 SIGINT。默认/all-features Clippy、测试、warnings-denied Rustdoc、no-default、deny、MSRV1.97 通过；并行 all-features 首次出现的两项运行时错误单项通过且串行全量复验通过，具体边界与命令见 [testing](memory/testing.md#r3-a-laravel-语义-http-状态默认与统一-json-响应)。独立复审 blocker 已修复，最终只读复审无 blocker/should-fix。R3 后续路由、Request、FormRequest、Resource、Session/CSRF 仍未交付。

R3-B：公开 `RouteGroup` builder 与 `Routes::group` 已接入，支持嵌套前缀、组级 Tower middleware 和稳定路由冲突错误；`nest` 复用同一前缀归一逻辑。`modules/example` tenant API 已迁移且认证/成员校验顺序保留。定向 HTTP、Rustdoc、真实 PostgreSQL 18/MySQL 8.4 tenant HTTP、带嵌套分组的 CLI 生成项目 TestApp、default/all-features workspace 门禁、deny、MSRV/no-default 均通过；独立只读复审无 blocker/should-fix。完整命令、计数和证据边界见 [testing](memory/testing.md#r3-b-laravel-路由组前缀与-middleware)。R3-B 只关闭此路由组子簇，命名路由、middleware alias/group、domain、model binding、fallback、signed URL 与 C04 Request/Response 仍未交付。

R3-C（已提交 801ca4e）：`Routes::name` 注册具名路由；嵌套 `RouteGroup::name_prefix` 按 Laravel 原样字符串组合，`RouteUrlGenerator` 以 `app.url` 为可信来源。参数编码、命名冲突、空/通配路径拒绝、生成应用和完整门禁通过，见 [R3-C testing](memory/testing.md#r3-c-laravel-命名路由与具名-url-生成)。

R3-D（本次限定提交）：`BootContext` 注册可复用 Tower alias 与递归 middleware group，`Routes`/`RouteGroup` 按名引用；按声明顺序展开，限制在调用时已登记路由/目标组。无应用注册表时 fail closed，公开 `Routes::into_router()` 返回 `Result`，不静默丢弃中间件错误。别名顺序、组嵌套、外部引用、缺失名称、循环组、重名和综合示例均有行为测试；生成项目、根真实 HTTP、全 workspace 门禁与独立安全复审通过，详见 [R3-D testing](memory/testing.md#r3-d-laravel-middleware-alias-与-group)。

R3-E（已提交 `dab80d4`）：已实现 `Routes::fallback` 与静态组前缀/middleware 作用域；GET/HEAD 先查 public 静态资源（显式 fallback 下包括 `/api`、`/ws`），fallback 收到完整 `OriginalUri` 和原始请求 body，方法不匹配仍为 405。根/嵌套/合并重复 scope 稳定报错；独立审查发现并修复 catch-all、状态映射和嵌套冲突问题。Axum 参数化/通配符组前缀无法安全 nest fallback，改为稳定 `http_fallback_scope_unsupported` 并在 API 文档记录。定向测试 7/7、默认/all-features workspace Clippy/tests/docs、deny、no-default、MSRV 1.97、生成 TestApp 与根真实 TCP HTTP 均通过，忽略项未作为服务验收；命令与边界见 [R3-E testing](memory/testing.md#r3-e-laravel-fallback-routes)。
