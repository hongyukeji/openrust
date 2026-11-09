# 测试基线

## 模型属性状态与无持久预览限定验收

MB-P01-A 在原 ModelAttributes/ModelMetadata 上增加 raw original、dirty、changes、previous 及显式内存同步，公开单值 cast 复用既有读取转换。同步不执行 SQL 或模型事件；未决回执/异步 setter 与并发版本变化保留明确冲突。唯一模块按账户隔离记录修改预览并返回 persisted=false，数据库整行不变；根和生成 User、外部 consumer 使用同一公开管线。

精确最终输入为 `tmp/model-persistence-20261107/runtime-assembly-v10/manifest.json`（1615项、SHA `7fa1a4b72a7401b5dfa3f02e0dbb7458de58f65ad56d360ab5decb4a3ee0736a`），运行基线6395fb81、规范31叶变更树 `a64776f6702a23d4e6e0ec344e5eb304d24627da`。独立完整回执为 `tmp/independent-testing-20261106/MODEL-ATTRIBUTES-RESULT.json`（SHA `9bd5ef3225bf82ce23e648736454a90fab2cff877b5006827255caf0bb94b210`），包含21份原始回执摘要与未变源码映射。最终交付事实文档只经静态检查，不改变该运行树的源码。

实际通过默认/全feature严格Clippy、默认workspace及日常MySQL组合、拒警Rustdoc/适用doctest、文档链接、最小feature与三driver编译、Rust1.97两配置、离线cached-advisory cargo deny。Eloquent94项（含18项新状态）、service3、User1、真实MySQL8.4.13预览综合流程1项及routine三个入口通过；12个外包编译与4组实际consumer通过。三类fresh生成应用minimal19、Web/Fortify50、显式JWT95项及locked metadata实际指向精确候选framework。

固定Laravel d52c3b3/13.39.0、PHP8.5.12/64bit有17个实际观测与1项Rust专属分类；12源与Reflection核对、PDO/SQL为0，不把18行称18个PHP测试或单独推成Rust全等价。最终无SQLite auth-fortify workspace全target严格检查及SQLite测试/OIDC生产辅助函数的最小类型复验通过；两处cfg只按真实调用方收紧，函数体和有效断言不变。原dead-code、fixture数值更新、pending状态期望、格式工具嵌套workspace失败与资源软线回执保留，不以旧失败字段覆盖新精确通过证据。

skip/ignored不算实服务。在线advisory刷新未执行；PG/SQLite/最低版本和Windows真实运行、外部OIDC/SMTP/受控副本及文件系统环境仍按相应矩阵验收。完整转换后getOriginal、fill/save/事件/关系/SQL生命周期、全cast和任意PHP值域仍未闭合；有限提交不代表整个Model、Passport/L090或Goal完成。

核心功能当前逐项验收状态与百分比统一维护于[核心功能验收清单](core-functional-acceptance.md)，扩展单列。该清单回溯本文件的实际限定证据，不将本次记录审计当作新运行测试，不修改以下历史结果或替未提交源码背书。

## 模块扩展独立包限定验收

AR-M01由`6c21bab07a80819cd1a1c6fffd5e8392c43c43cb`提交并核对origin/main，精确树`bde4e7845451739bddb74e366ce88a41ac9d9d45`，1565项输入的规范blob与权限一致。应用→openrust-modules→openrust-framework，模块专属机制不留核心或产生反向依赖，根装配、唯一示例、CLI、模板和consumer同步接入。

v10实际通过全target/all-feature check与严格Clippy、完整默认workspace、真实MySQL8.4 routine及37项纯行为/必需Raw、queued Fortify exact、两项新清理正反回归、原cold-prepare取消exact、生命周期与受限可见性恢复、双严格Rustdoc、no-default和MSRV两配置。模块94项单元、9项集成、core prepare7项/root resolver2项及模块2段Rustdoc均逐名确认通过；ignored不计实服务验收。六精简feature、12consumer、生成MySQL应用及普通模块10项CLI动作、root no-default/deny复用未变输入v6证据，根实际TCP复用v8证据，未声称全部在v10重跑。

仅正常关闭已知自有MySQL池后的首次空闲确认允许最多1秒按观测重查；持续外连接、错误归属或不可见性仍保库，恢复与DROP前最终门禁保持严格。历史失败时占用者身份未捕获，不推断自有；未声称主动lease-loss或instrumentation lost故障注入。完整Modules、命名空间、非MySQL/最低版/Windows及外部服务发布矩阵仍开放。

独立回执为`tmp/independent-testing-20261106/AR-M01-RESULT.json`，23份收据/111份日志摘要及精确提交映射已核。全部历史失败与harness纠正保留。完成簇唯一私有target已原生清理约14.24GB，产物归零、可用空间约106.17GB；源码、其他WIP、生成源码和证据保留。

## 根应用与框架独立包限定验收

AR-P01由`4463238`提交并核对origin/main，精确树`692ed33655857d804f217ca96ddac66052863eca`，1554项输入的物理SHA、规范Git blob和权限一致。根openrust仅含默认server，独立openrust-framework位于vendor/openrust/framework，库名与应用依赖别名均为openrust；默认cargo run、12消费者和新建应用均解析独立包。模块扩展拆分与Laravel命名空间完整对应不属于本簇。

独立适用验收包括完整默认workspace测试、routine-mysql真实MySQL8.4及37项纯行为、queued Fortify精确1项且无忽略、默认/全特性严格Rustdoc、MSRV两配置、deny、六种精简feature严格静态检查、12消费者check、新建MySQL应用new/check/test/locked metadata，以及根默认启动的CLI/API/HTML/404/自定义前缀、SIGINT退出与端口复用。原4段Rustdoc保留，3段执行、1段原有显式ignore；唯一示例用进程内Tower验收，另有根实际TCP证据，不混为同一传输层。

全target/all-feature check与严格Clippy、13份locked metadata复用v2证据；v2至v4仅两个隔离测试fixture变化，迁移及router目标分别严格复验，v4完整默认测试通过。生产默认值未为fixture求绿而更改。非MySQL及最低版本运行留发布前，不记为本次通过。初始APP_ENV故障完整日志曾被同名续跑覆盖，无法恢复；原命令、哈希、工具转录及同二进制A/B复现保留，不冒称原日志完整。收据为`tmp/independent-testing-20261106/AR-P01-RESULT.json`，17份收据/70份日志已核哈希，精确提交映射另存同目录。

本簇已清理可再生产物约7.85GB，保留唯一约5.26GB依赖缓存供已批准AR-M01；完成时可用磁盘约102.2GB。其他SC/J01/Passport/翻译来源WIP保全，不纳入已验收结论。

## 插入标识与事务执行闭环

DB-I01 v2独立验收回执为`tmp/independent-testing-20261106/DB-I01-RESULT.json`及同名md，绑定1528项导出组合输入SHA，最终同源。原13项main门禁全0，test-only v2修正后fmt与受影响严格Clippy复验；MySQL8.4和PG18各native/public Query/唯一example真实CRUD三项无skip通过，正常批次catalog恢复。SQLite/MySQL-auth/PG三类fresh实际new/check/test/locked metadata通过；最低SQLite3.26实际加载证明及7项定向无忽略通过。12个consumer locked metadata与两个直接相关consumer编译通过，根/consumer/fresh路径均解析受验副本。

固定SeaORM2.0.4原包SHA、402项原文件及许可证已核，唯一生产改动为安全执行结果访问器；完整vendor是用户已批准分发方案，Cargo顶层patch不会跨workspace自动传播。尚不声称crates.io分发已验。BYTEA字节值是明确Rust表达；完整Query/ORM、其它后端及发布矩阵仍开放。

旧PG SQL文本期待误引号失败与最低版首次pkg-config选系统SDK stub的验收环境失败均保留；v2保留文本投影、绑定和NUMERIC断言，环境修正不改生产源码。网络归属journal均done，完成生成包原生clean回收约1.11GB；独立target仅为后续明确队列保留，主target已清空。组合回执不自动证明任意部分提交树，精确DB-only候选须单独核依赖闭合和适用复验。

## 统一 Hashing 与认证接线限定验收

固定 Laravel13.39.0 / d52c3b3 的 HashManager、HashServiceProvider、Hasher/三驱动、安装默认及测试为 contract；固定 PHP8.5.12 oracle 验证参数/元信息/算法迁移，并补24条真实 `$2x$` 正反向向量。生产统一 `HashManager` 动态委派与首次解析的 `dyn Hasher` 驱动别名；默认bcrypt12/verify=true，显式Argon安装默认65536/4/1。认证、验证码、标准UserProvider与生成应用共用该服务，删除独立Passwords/PasswordConfig/auth.password成本权威。标准Hash保留72字节/NUL别名；增强认证另有统一前置能力检查，严格Argon混合旧bcrypt及make-only limit分支用固定合法dummy输入统一401，容量/预算错误仍传播。自返回工厂拒绝并释放已捕获强引用，同时保留并发extend后继；不声称任意回调图可自动解环。

证据目录 `tmp/hashing-20261105/`：最终顶层运行测试默认1405/全特性2762通过（9/124 ignored不计服务证据；不含CLI内部子进程与独立Rustdoc汇总），双严格Clippy/Rustdoc、Hashing-only、no-default、Rust1.97全特性/no-default与deny通过。之后仅三处示例/测试假设和Session装饰器漏转发身份元数据修正；最终冻结重验 `final-closure/final-gates-result.json` 的12种门禁全0，顶层计数两轮一致见 `top-level-test-counts.json`；包含CLI嵌套输出的全文正则简单求和会相差1，不作唯一测试总数。未以旧测试代替新输入。配置函数非法成本只返回统一脱敏求值错误，新增测试保留该真实边界；最初错误断言和所有失败日志保留。

真实MySQL8.4与PostgreSQL18各28项认证测试、唯一示例TCP完整业务及模块迁移往返各1通过，无skip；持久注册/验证码实际为bcrypt12，统一服务验证成功。MySQL28项沿未变生产输入的 `real-mysql-auth.log` 复用，其余最终见 `real-auth-result.json` / 对应分项日志；两库清单最终均恢复。首次广域示例运行190通过/2失败，已修旧token在密码重置后继续使用、迁移GC尾段漏框架项及恢复项、请求Session装饰器漏set_database_user_id；修复后的两失败目标均精确复跑。广域运行未配置的外部OIDC与ignored不计通过，不额外重跑无影响的190项。异常panic遗留只经既有公开持久归属入口恢复，保留journal，不按名称前缀删库。

fresh生成SQLite JWT API与MySQL Web应用，new/check/test全0，分别26和13测试，无skip/ignored；MySQL清单恢复，见 `generated-auth-result.json`。12个独立consumer重新离线锁图并全部locked metadata通过；gRPC/auth和query-authorization显式locked check通过。两份旧锁的rustix1.1.6与根精确1.1.5冲突按已有版本约束修正，没有依赖升级；其余锁补必需bcrypt/测试资源依赖边。

组合门禁在保留独立Passport/共享支持WIP的工作树上执行，限定提交只选本簇必要源码与依赖，不将WIP报交付。本簇不包含Passport协议WIP、Client/code/token持久签发、OIDC Provider、通用Manager缓存枚举/清除及Container访问/替换、完整Model hashed cast或全部Laravel对应。Rust异步、UTF-8类型输入、执行预算与增强认证策略均不等于PHP运行环境。外部Google/Keycloak客户端限制及任意网络/进程资源总体保证继续按原待办保留。已完成阶段产物用原生Cargo清理，源、fixture、日志、私有归属与服务保留；不push/tag/release。

## 测试库永久锁释放验收

冻结树 `73122c82912328a53f741cb7095c86e442854527`。macOS 上用真实 `try_clone` 保留同一个打开文件描述，旧实现只关闭原句柄，独立打开仍返回锁忙。新实现显式解锁，同 inode 可立即重锁，活跃 owner 期间仍拒绝独立取锁。Done 状态与 token 继续可读。

真实 MySQL 8.4 验证受控连接关闭后的资源清理，以及仍有外部活动时保留资源和归属回执。17 项归属测试、完整默认并行工作区测试、严格 Clippy、全特性编译、Rustdoc 与依赖门禁通过。非 Unix 私有权限及实际 Windows 仍留在原发布矩阵。

## 网络测试数据库生命周期与持久恢复限定验收

`TestDatabase` 的Unix网络范围使用独立I/O与清理线程、正常退出有界票据屏障、私有持久归属回执/永久文件锁、每资源随机远端见证及固定原生管理会话锁。目标与PG模板分别持责；每条业务池物理连接验证实际服务、库名、marker及PG OID。关闭池失败、权限/活动可见性不足、错配或未知操作均保留，下一次显式create/recover补偿；不存在库只有权威核验后才写Done。管理库名保留服务端原值，生成目标名仍严格受限。

证据目录`tmp/database-cleanup-fix-20261105/`。`durable-public-final-result.json` 为真实MySQL8.4/PG18各10场公开API进程用例：close/drop/runtime退出/panic、SIGKILL→recover、已缺失终态、活跃guard、错marker、假前缀、独立应用池；全部通过且完整库清单恢复。`durable-lifecycle/verified-result.json`另有两库各11退出/取消/失败场零遗留，后续仅错误分类、等价静态整改与管理名放宽，按独立审查限定复用旧ASCII路径，不声称最终同SHA重跑。

`permission-result.json`真实验证MySQL建库后例程权限拒绝仍清理未迁移自有库；`visibility-result.json`验证受限同实例账号不能把权限过滤的现存库记Done，恢复管理员后回收，数据库与角色清单恢复。历史碰撞/迁移失败哨兵实跑MySQL2、PG3（PG模板在MySQL不适用），`example-service-result.json`唯一模块真实双库就绪正反流程各1，无skip且清单恢复。`admin-name-result.json`验证MySQL小写中文/连字符、PG混合大小写中文管理库的原值重读，单元另证大小写及Unicode不归一化。早期探针受CLI latin1、MySQL lower_case_table_names=2大小写目录环境影响，原始诊断和精确自有资源清理留存，不计框架修复成功或任意配置已保证。

最终`main-gates-result.json`默认fmt、workspace all-target严格Clippy、workspace tests与doc均0。补充`remaining-*/result.json`按实际成功输入收齐no-default、MySQL-only/PG-only/SQLite-only严格Clippy、全特性严格Clippy/tests、默认/全特性拒警Rustdoc、Rust1.97全特性/no-default和cargo deny，共15种适用门禁。ignored和未配置服务返回不计实服务通过；当前工作树仍含独立Hashing/Passport WIP，测试库未依赖其源码/feature，分离提交只选测试库所需Cargo与lock边，审查见`final-review.md`。失败日志按attempt保留；最初workspace Clippy两处README格式失败的全文被重跑覆盖，仅有原工具回执尾部，详见`main-first-failure-evidence.json`，不虚称完整失败日志保全。

公开边界：调用方须关闭独立池并停止新借用；MySQL完整活动快照不排斥不合作的新连接或跨库访问。非合作管理员DDL、复制了同名marker的备份/物理克隆同ID、非Unix私有ACL及真实副本/无应答端点专项仍开放，不将静态保护或本机进程故障测试扩大为这些拓扑已验。无push/tag/release，原完整Goal继续。

## MySQL 冷预处理取消与连接淘汰限定验收

SeaORM2.0.4/SQLx0.9.0原生对照排除了QueryBuilder：冷PREPARE首次等待取消，两次复用错帧/rollback超时；warm及指定reader/blocker行锁等待取消正常。固定同版本vendor补丁仅改executor/stream/connection/tls/transaction五个文件，未重建未知协议：不完整PREPARE永久判该物理连接不可复用，后续查询/提交/显式回滚有界失败，close/Drop不冲刷未知缓冲，归还后池淘汰重建。正常完整数据库ERR仍复用；Result只在实际执行命令完整排入缓冲后登记。原许可、源码commit与原始文件SHA随vendor保留，SQLx暂精确固定0.9.0。

`tmp/query-cancel-driver-20261105/main-gates-1/main-result.json`完整12门禁通过，各步输入无漂移；default汇总1407通过/42 ignored，all-feature2667通过/161 ignored。之后只强化公开测试的affected-row/锁定读断言，生产输入未变（`lock-proof-code-delta.json`），最终严格all-feature Clippy及真实双库43项通过。MySQL43均实际适用；PG沿既有42项，新cold-MySQL项明确不适用，SQLite也不作MySQL证据。未重跑不受影响的生产门禁，输入与补强证据分别计证。

最终`public-real-2/result.json`新案证明正常prepare缺列错误同连接ID；旧事务UPDATE实际影响1行→cold首次poll取消→旧查询/rollback有界Err→max1池新物理ID→新事务有界FOR UPDATE取得唯一原行及row-1→rollback，直接证明此前未提交写撤回及旧锁释放。原先仅普通MVCC读不足的证据已补强；独立观察池仍可读。`patched-results-final`原生五案均在TLS实连下验证cold两次fail-fast、warm/late复用及参数数目错误后复用/rollback。初轮TLS字段遗漏导致编译失败，其后误跑旧binary的patched-results明确invalid，不计修复成功；旧失败/原始源码及清理回执保留。

CLI独立化清单复制根patch并规范绝对路径，回归证明源清单不变。根+12独立consumer的metadata均解析到唯一vendor源（`resolved-patch-graphs.json`），12个--locked consumer check全部通过；双库各2HTTP通过。fresh MySQL JWT实际new/check/test27项（26模板+1公开取消fixture），最终强化fixture同样有affected1+有界FOR UPDATE/原值，见`lock-proof/result.json`与`fresh-mysql-jwt-verification`，无skip/ignored，DB/角色集合前后相同。生成图再次核唯一vendor源，不能只凭manifest含patch称生效。

限定不同模型复审覆盖协议边界、TLS状态传递、生成接线、错误路径与锁释放证据。取消瞬间不保证服务端回滚，持有废事务仍可能持锁，已证明到replacement锁定读完成时释放。Cargo只读取最终workspace根patch，根及生成应用自动接线，手写消费者必须同项；后续上游修复/版本升级/发布前须重核。完整网络任意阶段取消、完整Query/ORM和五后端仍开放，本地提交以Git为准，不push/tag/release。


## Query 查询前回调与共享子查询限定验收

固定framework d52c3b3：Builder beforeQuery/applyBeforeQueryCallbacks、toSql/getBindings、get/first/value/aggregate、分页与chunk及三类子查询准备时机。基线73a845d，单一QueryState和双生命周期保留不可变终结入口；普通Clone独立父状态并共享live child。标量立即准备、EXISTS不消费子自身回调、UNION延后准备，父绑定按接入时冻结；绑定数不符明确拒绝。失败保留部分修改/列表、成功清空完整列表；同对象并发租约、图环和来源错误不被包装洗掉。完整FROM/跨连接及Query/ORM仍开放。

`tmp/query-before-kernel-20261105/main-gates-5/main-result.json`完整12门禁通过，受验输入无漂移；default汇总1406通过/42 ignored，all-feature2665通过/161 ignored。公开42项覆盖生命周期、投影/limit/分页/chunk、借用事务、共享子节点与绑定、跨线程重入、析构及图环。默认无真实数据库返回与ignored不计真实服务；SQLite行锁取消不适用。既有ColumnOperand编译示例已保留。初轮Clippy与SQLite单连接池测试顺序失败均保留，修复未删断言或降低门禁。

最终`public-real-3/result.json`：真实MySQL8.4/PG18各42项无skip通过，数据库/角色清单前后相同；`example-real-2`各2项HTTP验证唯一模块before_query账户scope及显式JWT时间线。fresh SQLite JWT实际new/check/test27项（26项模板+1项额外公开API fixture），包括回调借用实际事务子查询；最终复验和query-authorization consumer见`final-services-recheck/result.json`，首次生成来源见`final-services-generation-2/result.json`。不是模板自动附带该额外fixture。

取消专项先以实际事务读取reader/blocker身份，再由MySQL data_lock_waits/PG pg_blocking_pids确认指定行锁等待，之后取消、复用和回滚双库通过。原回调通知早于SQL发送，不能充当等锁证据。MySQL冷prepare早期取消可能留下驱动响应清理等待，旧失败、native sample和精确清库回执保留；此叶原受验不覆盖后续冷prepare淘汰修复，当前限定事实见本文件开头；仍不推导驱动任意阶段可复用。不同模型限定复审确认时序屏障及最后Clippy整改；未提交前保持WIP，本地提交以Git为准，无push/tag/release。



## Query 存在性条件回调限定验收

固定framework d52c3b3 Builder4045/4055及交叉命名的官方Or测试，基线9f51471。exists_or/doesnt_exist_or各执行一次已有存在性查询；选中分支才创建一次FnOnce的Future并保留任意T，ExistsOrResult::True与Callback(false)不混淆，数据库/闭包错误透传。最终&self允许不可变query在fallback中再count，公开类型约束无新增依赖。

`tmp/query-callbacks-20261105/main-gates-3/main-result.json`：12门禁实际exit0、受验输入无漂移；default汇总1363通过/42 ignored，all-feature2623通过/161 ignored。8项SQLite覆盖四分支、Future创建惰性、move任意值、false、两方向精确错误、DB错误不调用、同query复用与回调借用原query。初始expect_err strict lint失败保留，修复未弱化错误断言；临时&mut方案经独审改&self，同借用缺口已闭合；先前main-gates-2的通过不替代最终源码。

`example-real-1/result.json`的MySQL8.4/PG18各2项真实HTTP通过，无skip、DB/角色集合前后相同。时间线在账户内open查询上使用exists_or，命中短路与无记录false回调经真实两库；既有exists三后端解码源未变，不将本次wrapper当完整后端驱动新验。fresh SQLite JWT实际new/check/test27项/0 ignored，含另加公开API fixture的惰性分支及不可变query自借用；外部query-authorization --locked check通过。模板既有26项与另加1项fixture分别计证，非模板自动附带该fixture。完整结果`final-services-generation/result.json`。

本叶原验收不覆盖before-query（当前内核限定证据见本文件开头）：独审发现三类子查询时机、共享child、projection恢复、聚合/分页/chunk与失败残留须完整重构。tmp内before.rs/subqueries.rs只是未编译候选；lifetime-probe为不导入框架的独立Rust类型实验，正例编译/运行、延长static负例失败，不宣称业务或等价验证。后继保留唯一todo。本地提交以Git为准，无push/tag/release。


## Query 存在性终结操作限定验收

固定framework d52c3b3 Builder exists/doesntExist与Grammar compileExists，基线cd0ac0f；借用构造器，不用count/first近似。原SELECT投影、绑定、排序、分页、UNION、NULL行、读副本/primary作用域及显式事务保留，错误不转false。SQLite6案已随全特性执行；`main-gates-3/main-result.json`完整12门禁exit0、受验输入无漂移，default测试1362通过/42 ignored、all-feature2614通过/161 ignored；ignored及未配置服务的返回不计真实服务。早期fixture类型推断、双层Result与构造顺序失败保留，严格门禁不削弱。

`tmp/query-existence-20261105/core-real-1/result.json`记录MySQL8.4/PG18各1核心真实测试；NULL投影、limit0/offset、同列数UNION、同查询复用、事务未提交可见性与不存在表错误通过。`example-real-1/result.json`各2真实HTTP通过：同一逻辑Db表及账户过滤的open导入exists标记，另一账户/仅终态账户为false，认证及无Cookie边界保留。两组数据库与角色集合前后均相同，无服务skip。

fresh SQLite JWT项目实际new/check/test27项、外部query-authorization的--locked check通过；其中1项为另加的公开API consumer fixture，26项为模板测试，非模板自动生成存在性fixture。原附件替代候选被独审识别逻辑映射/物理表不一致后恢复，未留下安全检查fallback。公开API、示例及真实SQL测试不同模型窄审，发现的UNION列数fixture已修，未删断言。完整执行见`final-services-generation/result.json`。

当前仅三个已实现数据库后端的限定范围；existsOr/doesntExistOr、before-query生命周期、SQLServer及完整Query/ORM仍开放，取证不是实现。源码未改模板，无push/tag/release；本地提交以Git为准。


## Query 列边界与布尔条件链限定验收

固定framework d52c3b3、docs 960eedf，基线54f4a087。四种between-columns、ColumnOperand、WhereGroup生命周期与来源检查、首OR/AND优先级已接入同一读写条件链；固定两端点数组是公开Rust类型约束。不是完整Query/ORM或跨连接/别名对应。

`tmp/query-between-columns-20261105/main-gates-6/main-result.json`的12条命令全部exit0且输入无漂移：fmt/diff、default/all-feature严格Clippy与workspace tests、拒警Rustdoc、no-default、Rust1.97两组check及deny。default测试汇总1361通过/42 ignored，all-feature2608通过/161 ignored；ignored不计真实服务。新SQLite10案与Query家族13案先已定向通过。早期Rustdoc/test lint及测试接线失败均保留，不降门禁。

组合测试发现两个独立挑战提前验证同一恢复码时败方409：锁内仅核source/generation遗漏完整快照。修复仅登录挑战返回401，底层管理冲突/DB错误不变。确定性双prepare+verify回归实际red→green，完整组合原HTTP一次签发/审计及回滚断言通过；不同模型窄审无阻断。

`example-real-5/result.json`：MySQL8.4与PostgreSQL18各2项真实HTTP通过，无skip，数据库/角色集合前后相同。覆盖账户隔离、NULL/时间边界、默认认证和显式JWT profile、无Cookie/Session、无scope/裸签名拒绝。生成应用`fresh-sqlite-jwt`实际new/check/test共27项通过/0 ignored，其中1项是另加的公开Query验证fixture，26项为生成模板既有测试；不宣称模板自动带该fixture。`query-authorization`外部consumer `--locked`检查通过。完整结果在`final-services-generation/result.json`。

完整Laravel查询、其他数据库版本/后端、Google真实客户端及整个R7/ADR-001仍开放。本地提交以Git为准，不push/tag/release。

## Fortify 官方默认与显式 JWT 组合验收

当前 main 基线 `5ad34b9` 之上的限定组合，固定官方版本与范围不变，本地提交以Git为准。`tmp/fortify-single-system-20261105/main-gates-6/main-result.json` 绑定981项输入，12项门禁全部通过且各步输入未变：fmt/diff、默认与全特性严格Clippy/workspace tests/拒警Rustdoc、no-default、Rust1.97全特性及no-default、cargo deny。测试日志逐suite汇总默认1362通过/42忽略，全特性2591通过/161忽略；忽略与未配置外部服务的普通测试不作为真实服务证据。

真实生成的Web SQLite/MySQL各13项、显式JWT SQLite26项、JWT+gRPC MySQL27项完成new/fmt/check/test，生成输入未变，两个MySQL数据库/账号清单未变。`generated-acceptance-summary.json`绑定各结果；原fmt/撤销状态断言及runner首次Cargo.lock创建误判的失败均单独保留。生成gRPC真实TLS含正确CA/域名、错误CA/域名、关闭监听器；其公开Echo不冒充受保护认证。

`grpc-profile-production-provider-real-2.log`通过正式Provider工厂的真实SQLite+TLS受保护RPC，覆盖命名Policy、每RPC作用域、撤族、跨Guard/Provider、无scope家族、裸签名及错误issuer/audience。`redis-authorized-read-real-3.log`三项期限/最大合法wire/实际Redis TTL与无TTL拒绝通过；`redis-parent-final-check-real-4.log`真实Redis已保存父Session派生JWT/PAT正例及提交前删除回滚通过。Redis截止保守整秒、不足一秒拒绝，不承诺跨Redis/DB分布式原子性。独立复审未再发现上述修复的明确安全缺陷。

当前受影响consumer均已验证：session-custom-driver、container-extension、test-app执行测试，query-authorization执行locked check，gRPC/MySQL16项与Redis双进程WebSocket/SSE1项通过，见final-consumers-1及单独日志。CI移除失效auth-mfa feature/target后，ci-auth-backends-2实际执行MySQL/PG共8条对应命令：OIDC、根Web confirm两模式、无Session/Cookie根JWT、真实因子门控，库与角色清单未变；原队列管理回滚等每库6项证据仍保留原输入边界。

sqlite326-fortify/receipt.json实载既有SQLite3.26.0动态库，核version/source_id与dyld真实路径，60项迁移/字段/回退断言通过，不外推为全部业务最低版验收。Keycloak-real-8在真实Keycloak26.8.1+专属Redis+MySQL下通过双RP/TOTP、签名backchannel、独立API进程、Redis故障503、恢复同令牌重试/重放拒绝；前后端口映射相同、库/角色清单无差异、自有容器删除。首次测试包装器未转发reader、失败登录pending Session清理及runner标签/动态端口造成的失败逐次保留，精确清理自有失败库，未清历史/共享数据。最终夹具以真实SessionStoreHandle执行；被拒后只保存已flush且Guard非认证的空访客再正常销毁/重建CSRF，原401和503断言保留。四处令牌相等断言仅改脱敏消息，最终Keycloak全特性与无session-redis的严格Clippy通过。

普通无选项fresh-plain-default的new/fmt/check/test10项通过，默认MySQL及原生成输入不变。main-gates-6后生产Rust/manifest/lock没有变化，仅Keycloak测试夹具、CI和事实文档修正，新增Query候选两文件尚未mod引用并排除本次提交；匹配影响已定向重验，不重称完整main在后来元数据上重复运行。Google真实客户端与UUID/字符串JWT持久链仍未达成；完整R7/Fortify/ADR-001及Passport/SSO等后继未关闭。

完成上述在途验收后已核构建进程/target占用与源码归属，先退役生成包产物、再按软保护线正常cargo clean共享target：实际54,535,081,984B→0，磁盘可用94,590,861,312B→148,139,393,024B，1467项源码/生成输入哈希未变。回执`artifact-retirement-after-main/result.json`；源码、依赖来源、服务、原始验收证据保留。


## R7 具名认证核心限定验收

2026-11-04，开发基线0d3b4fd，本地提交以Git为准。固定Laravel framework d52c3b3与docs960eedf，AuthManager/UserProvider/SessionGuard生命周期、默认与错误边界按源码对应；DatabaseUserProvider通过公开QueryBuilder与模型工厂保留数字/字符串ID、同数据库/表账户来源。Eloquent尚未实现；原有MFA属于增强，不将本簇当完整Fortify、Auth或R7完成。

证据根tmp/fortify-totp-alignment-20261104/。r7-status-final-source.json冻结833项；final-gates-6十一项均exit0：fmt、全feature严格Clippy/tests、默认及全feature拒警Rustdoc、deny、CLI构建、SQLite/auth-mfa与PostgreSQL/auth及auth-only精简严格Clippy、Rust1.97精简check。此后仅独立realtime consumer两文件变化，r7-consumer-final-source.json绑定最终输入；未变的main生产路径限定复用，不声称全部在新输入重新构建。default-current的当前默认workspace严格Clippy/tests均0；嵌套子进程测试不重复累计为整体通过数。

7consumer均通过：前6项按未变输入保留原日志；query-authorization仅check，其余按实际test结果；realtime修独立宿主的公开异常envelope接线并保留503断言，真实双进程Redis WebSocket/SSE通过，见consumers-r7-final-2及其reuse记录。fresh-r7-6四类SQLite-auth/SQLite-mfa/MySQL-mfa/PostgreSQL-auth的new/check/test共12命令均0，各3测试、0跳过，包含缺表配置500与恢复正常认证；生成源码及日志SHA保全，双库差异0。不将生成测试当严格Clippy。

real-services-current-final共14命令实际通过，MySQL/PostgreSQL各33测试：账户3、Cookie4、数据库认证8、remember迁移1、浏览器WebSocket4、OIDC8、生命周期5。真实Keycloak26.8.1签名回调/登出夹具；测试前清理本任务旧IdP会话，不伪造签名。两backend-database-delta均无增删。自有服务清理见keycloak/current-service-cleanup.json，trigger终止、container正常stop、admin.env删除、四端口释放，共享数据库/Redis不变。

minimum-current实际加载自建SQLite3.26.0动态库，SQL version/source_id和dyld路径核对，当前标准Provider/迁移up/down/up及数据/索引/外键/完整性30断言通过。依赖/native源码与历史失败保留。Guard等待图、Cookie保存、配置冻结及HTTP误配500回归已纳入匹配门禁；SQLite复制FD正常释放锁有独立真实red→green，未声称定位原偶发失败的具体继承者。独立final-current-evidence-review.json核833生产/consumer输入、全部日志及242生成文件SHA，无阻止限定提交的证据缺口。242份生成源码已私有归档；358相对文件链接无缺失，锚点不在该检查范围。三任务target合计16,237,453,312B正常clean至0，磁盘可用160,493,772,800B；依赖/native源码与证据保留。本地提交状态以Git为准。

## SQLite 本地认证与 MFA 限定验收

2026-11-04，开发基线45a1097，守护独立提交2305ca1；S3b最终源码输入17。固定Laravel framework d52c3b3129e4c8e213c97cdc68f94b0b7d27f181、docs960eedf0911155ad16326ade49c0ea5d18eda0b8；本地用户/密码/PAT/refresh/验证码/Policy/MFA属于既有增强调用链适配，不冒称完整Laravel Guard或Fortify默认。普通Db事务不变，SQLite安全读写同一连接BEGIN IMMEDIATE；UTC精确期限、条件消费、refresh重放家族撤销、审计与限流配套，唯一modules/example真实调用。验证码GC按主键逐页扫描，最多删除512条，不承诺最多扫描512条或常量锁时长。

源9的13项workspace fmt、默认/全feature严格Clippy、tests、docs、三精简组合、MSRV1.97、deny与CLI构建实际通过；最终差异逐项独审核对后限定复用未受影响生产路径，不称最终源17重跑整矩阵。两处旧真实服务fixture显式Guarded/增强迁移并保留强断言；MFA修复cookies/session装配、公开异常envelope及同一已enroll账号。源10MySQL认证8、计数器1，加源12MySQL MFA/模块HTTP与PG计数器/认证8/MFA/模块HTTP共22项实际通过，无服务skip、双库差集0。源12→13仅格式，重建原文SHA和rustfmt逐字节等价经独审确认。

最低SQLite3.26.0：源13实际认证52、限流2（另2显式ignored子/服务入口不计通过）及Root5项通过；限流包含两个独立进程，Root包含本地认证/原生Session与真实TOTP/MFA恢复码/审计回滚。dyld实际加载自建3.26动态库、库SHA/原生CLI版本均已核，不能以当前bundled引擎代替最低版证明。最终私有重导出cfg在该session启用组合不改变逻辑。

生成调用方修复testing/db-sqlite feature及同一TestDatabase重导出；新SQLite应用在自动迁移前为所启用用途生成独立32字节OS随机密钥，仅存自有.env，Unix0600，模板空且不覆盖已有应用。JWT/签名密钥属于增强，Laravel安装应用密钥原则见固定encryption.md:16。CLI严格门禁与39测试通过，testing SQLite-only严格Clippy/docs及新CLI构建通过。四fresh（SQLite auth/MFA、MySQL MFA、PG auth）new/check/test共12命令均0，各3项实际通过，无跳过；两个MFA案在实际HTTP测试内执行完整MFA子流程。生成测试有5个既有unused support警告及汇总，不称生成项目严格Clippy通过。

全部失败/完整日志/退出码/来源/输入哈希和234份生成源码归档见tmp/sqlite-s3b-20261103/；独立不同模型核770输入零漂移、日志/归档SHA、实际执行和复用边界，限定无新增blocker。主服务cache9,067,614,208B、最低cache1,206,366,208B、最终共享cache4,314,501,120B均经锁/占用核对后正常clean至0，最终磁盘可用163,407,933,440B；依赖/native源码、服务与证据保留。本地提交以Git为准。

完整Guard.id/named guard/UserProvider、Fortify官方默认与Google真实客户端互操作、SQLite OIDC/缓存锁/队列/完整Schema及一般驱动取消/故障窗口仍开放；事务前签名/RNG失败不承诺已撤销refresh家族。下一步只在唯一todo领取，不将本簇当完整SQLite、认证或ADR-001完成。

## 显式模块注册与宿主 API 前缀限定验收

2026-11-02，基线85f6e9a。固定Laravel d52c3b3的ApplicationBuilder::withRouting/buildRoutingCallback以apiPrefix装配API组；模块文件机制按固定Modules5971484保留。根/new默认registered为获准应用约定，库ModulesConfig::default仍保留file；不是把上游默认重写成语言必要适配。modules/mod.rs集中注册，bootstrap不再直接依赖example；同一ModuleRepositorySeed/Provider管线和原初始化回调保留，with_application_root锚定外部运行根。只读预检先于事件与选择，空catalog --all亦失败。

707输入冻结；fmt、默认/全特性严格workspace Clippy、两组workspace tests和拒警Rustdoc、no-default适用严格Clippy及lib tests、精确Rust1.97 check共10项均实际exit0。覆盖真实bootstrap无JSON注册/模块移除/初始化先后、默认/自定义/空/普通参数前缀、路径边界和模块错误格式、只读无事件副作用及参数预算。CLI定向35及组合CLI断言/快照通过。初始闭包缺move、新fixture lint、CLI断言与后续格式同步失败均保留，未删有效断言或降门禁；同输入root依赖审计复用31项SHA一致及原exit0日志，不虚报本轮deny重跑。

标准server/CLI构建后，6个真实原生server进程验证：默认注册忽略坏状态JSON、/v1挂载和/api撤销且模块自有路径错误格式保留、配置缓存忽略外部前缀/activator替换、file禁用/enable后新进程加载、file-live启动。逐案健康/HTTP、实际config show白名单、SIGINT exit0、端口释放；根状态文件字节未改，自有配置缓存清除，无新建.env。registered全选及未知activator实际命令拒绝，文件enable/disable实际写入自有状态；这些不等于动态热挂卸或完整Modules。

通信consumer1、fresh minimal check/test9、MFA check及真实MySQL认证2＋迁移目录1、实际help通过。生成项目安装公开module-activation fixture并精确执行持久启停/编译文件include_once/跨App loaded快照测试1，原生成9项证据不因追加独立测试文件而失效；fresh空catalog实际enable --all exit1且不生成状态JSON。无服务skip，MySQL数据库集合0增0删。112生成源码/lock归档；相对旧113，两个默认状态JSON不再生成，补1个模块fixture，不是删除行为断言。

首批8页指南按固定Laravel章节顺序和Modules独立组组织；mdBook0.4.52官方资产digest验证、项目内构建command/exit0/logSHA齐备，189相对文件链接无缺失。仅首批，不关闭全部官方章节/生态文档。不同模型代码及最终证据独审无剩余阻断。无占用后正常cargo clean：29,746,253,824B→0后进入fresh，最终5,535,178,752B→0，磁盘可用160,202,297,344B；源码/WIP、服务和证据保留。完整回执 `tmp/module-provider-activation-20261102/final-integrity.json`，本地提交以Git为准。

底层matchit普通参数上限25现转安全错误，前缀＋子路径合计，catch-all不误计；这是尚待解除的后端能力限制，不称Laravel等价或Rust语言必然限制，归唯一todo R3复杂参数后继。完整Modules、Routing、ProviderRepository与ADR-001仍开放；下一SQLite，不扩当前簇。

## 默认生产日志驱动启动与安全诊断限定验收

2026-11-02，基线46bebfb。固定framework d52c3b3的MailManager::createLogTransport无production禁用；MailLogTransportTest覆盖logger/channel及MIME正文。本簇闭合默认production＋log启动故障和安全初始化诊断，保留既有仅记录接受摘要的隐私增强；不声明完整MailManager、具名mailer/channel或SMTP供应商送达对应。SMS为既有增强，不冒称Laravel内建。

696输入冻结；fmt、默认/全特性严格workspace Clippy、两组workspace tests及拒警Rustdoc、no-default适用严格Clippy和lib tests、Rust1.97 check共10项实际exit0。新增依赖链/静态配置键原因/私有原source/深链/HTTP脱敏、日志敏感哨兵和唯一模块production服务/路由回归均通过；两项compile_fail证明外部不能伪造服务身份。默认/全特性测试中ignored不计真实服务。依赖审计复用上一簇同输入exit0证据，31项清单/锁/规则与日志SHA匹配；独立通信consumer锁补齐既有root依赖的13行单独记录，不改变root清单或版本，未虚报本轮重跑deny。

原始 `/Users/Home/.cargo/bin/cargo run --color=always --profile dev` 在缺省、显式production、自有配置缓存三案均真实监听、health和根API200、SIGINT exit0及端口释放。实际config show仅保存production/mail.log/sms.log/cache.redis等白名单；无development覆盖，无新建.env，无外部邮件短信。未知driver与未编译SMTP两项实际负例分别有固定原因且不回显秘密哨兵；缓存命中忽略非法MAIL_DRIVER，自有缓存正常清除。harness首次误把JSON按TOML解析、doc标记及fixture误用私有API的失败均保留；仅文档/测试差异不重跑未受影响生产HTTP。

通信consumer锁定测试1项、标准server/CLI构建、fresh minimal check/test9、fresh MFA check及真实MySQL认证2＋迁移目录1、生成应用实际help均通过，无服务skip，数据库集合0增0删。消费者初次--locked在编译前发现旧锁遗漏，保全旧锁后离线同步并锁定复验；只该独立workspace锁变化，不使已通过root门禁失效。主门禁输入/生产运行映射及113份生成源码归档均保全，当前Rust输入匹配，59相对文件链接无缺失。

不同模型生产代码/证据独审完成，最终收口以证据目录报告为准。无占用后正常cargo clean：29,881,606,144B→0后进入fresh，最终5,422,854,144B→0，磁盘可用161,157,271,552B；源码/WIP、生成源码/锁及服务保留。完整日志/退出码/SHA/失败/清理与归档索引见 `tmp/default-mail-startup-20261102/final-integrity.json`。本地提交以Git为准，不push/tag/release；完整官方能力及模块/API、正式文档、SQLite、CLI后继仍按唯一todo推进。

## DB 通用能力与驱动分层限定验收

2026-11-02，基线710dd55，失败处理规则另由231b0b8本地提交。固定framework d52c3b3的DefaultProviders/DatabaseServiceProvider/MigrationServiceProvider/ModelMakeCommand及对应测试；只闭合通用能力编译、注册、当前默认连接选择及生成子范围。db/database/facade/prelude常开，driver feature只控制连接器；根/活跃模块迁移与种子注册不因缺URL消失。DatabaseConfig::has_default_connection只指默认url；具名实例独立连接，不误选绑定default的自动Auth/Queue/database Session或示例业务。生产模块register读同仓储，静态nested/possible候选不冒充实际登记。自动清单选择后默认URL有无变化拒绝启动；显式模块register未选业务而boot反向启用也拒绝，保原startup资源回收；有变无可关闭路由而不卸绑定。这是当前装配失配的明确失败边界，不抵扣完整Laravel懒连接/动态默认连接缺口。

752输入，attempt8十四门禁全exit0；补查queue-only仅发现候选Vec构造lint，保持相同条件/顺序改vec!后attempt9重新验证fmt、默认/全特性/无DB/queue-only严格Clippy、无DB Server、双workspace tests及双拒警docs十项，全0。仅modules/example/providers/module.rs与attempt8不同；MySQL/PG单驱动库单元、no-default lib、精确Rust1.97 lib与deny五项按输入/作用域未变复用，逐项SHA在attempt9-evidence-reuse.json。最后workspace原始harness汇总1175 passed/35 ignored、2024/115；含重叠target/子进程，不作唯一行为数。8→9只有独立1项summary输出少1/3，27/51个Cargo Running target相同、其余summary值与频数相同，未删测试，见harness-summary-variation.json。无DB业务组合86/1 ignored，单驱动库单元MySQL44/PG43；ignored、未配置IdP的提前返回不算真实服务。

源码阶段的失败均保全：初始类型复杂度/条件lint、内部结果类型旧解构漏改、fixture借用/方法闭包lint；全特性发现私有AuthService误引及旧Session helper参数，改检查四项公开认证绑定与完整同步调用方。无DB编译定位Keycloak的DB/MFA条件和测试hooks边界，保无DB双客户端/token/SSO路径及原20/56/4断言计数；附件仅收紧实际DB测试接缝，3项纯deadline保留。运行profile误删了既有cache-redis，root仍配置Redis store而合法拒绝；控制探针定位后补回缓存feature，DB驱动仍关闭，未放宽缓存校验，临时诊断已还原。用户已将AGENTS改为Agent自主处理失败，旧次数/问答不再作暂停依据，严格门禁与所有失败证据保留。

根真实MySQL8.4与PG18认证各精确1；具名库而无default的真实应用两库各1，0 ignored，验证实际连接/ping、默认句柄未初始化、四项公开认证服务与Queue未绑定及基础HTTP可用。四TCP进程验证模块真实装配、状态缓存/live与下次启动路由，均正常exit0且端口释放。fresh普通应用实际生成两种模型/UUID后check/test/build及12项、MySQL认证/Job3、MFA2、同二进制SHA的缓存/坏dotenv/clear复启全过。新的local环境migrate负例确实进入命令并因未初始化连接池exit1；旧未设APP_ENV的负例在测试Mailer bootstrap失败，明确排除，不能当数据库证据。用户显式授权的旧定向重验答复晚于新自主规则，作为补充，不撤回新规则。

外部SQLite consumer以公开database/prelude导出在无官方DB driver下locked check/run通过，保连接/事务/表名/查询及应用迁移资产，框架迁移互斥仍明确拒绝SQLite。首次check仅补consumer Cargo.lock的openrust→glob既有依赖边，421包身份/版本/checksum全部不变；冻结检查发现并停止后严格核差异，保存post-consumer-lock-source-snapshot，再locked check/run，不重跑无关主门禁。PG首次runner误用postgres角色，按现容器POSTGRES_USER修正，无角色/服务改动；先前MySQL结果复用，不重复场景。不是单个脚本一路绿，原失败与逐停点恢复均可追溯。MySQL/PG数据库集合最终0增0删，未清他人旧库，共享服务未停。

不同模型最终核752输入零漂移、十项新门禁与五项复用、服务SHA/实际数量及锁差异，无限定范围blocker；报告/tmp/openrust-db-capability-final-evidence-review.md已复制证据目录。生成源码192文件归档SHA f00c9daf51d104aa56d1f0454c387434efcc48beee199bd7026623a67f6e42c3，原目录及缓存重启副本仍保留。主target39,083,229,184B→0后跑consumer/fresh，最后无构建进程或target句柄，正常dry-run+clean 10,721,374,208B→0，磁盘可用197,526,224,896B。完整回执tmp/db-capability-20261102/；本地提交以Git为准，不push/tag/release。

固定官方DatabaseManager/ConnectionFactory测试明确懒连接、database.default/usingConnection恢复与扩展resolver；五SQL后端、完整默认连接切换、purge/reconnect、配置工厂及最低版本矩阵仍归L057/R4–R6，不把当前双库实现当最终授权范围。当前自动选择省略无default URL的database Session，完整上游启用/错误语义仍归Session后继；标准MySQL+Redis开箱/队列仍按唯一todo继续，完整CLI/DB/Modules/ADR-001未关闭。Keycloak本批仅编译/条件覆盖，未宣称真实IdP验收。skill独立owner源码不混入、不部署旧Paseo守护、不重建Goal/预算；原生heartbeat手动交接不算真实调度成功。

## FileActivator 实例缓存限定验收

2026-11-02，d32162d后继；固定Modules5971484的FileActivator及Laravel d52c3b3 Filesystem/ConfigRepository。默认file构造读取、实例快照、自身修改先于写入且失败不回滚、cached null/missing delete无写、外部删除后重建剩余缓存及reset均按实际链整改。Manifest仅保存工厂，实际包register读取当前配置后构造；FileModuleActivator::new返回Result，显式reset_file保手工恢复。file-live/with_live_reads保实时增强，不作为官方默认。

751输入最终冻结、attempt3十门禁全exit0：fmt、默认/全特性严格Clippy、workspace tests（harness汇总1159 passed/35 ignored、2002/112）、双拒警Rustdoc、no-default lib、精确Rust1.97.0、deny。跨target汇总不等于唯一行为数，ignored不计服务。默认cache/foreign隔离、busy失败pending续写、delete/null/missing、numeric list、自写同target且同字节恢复与外来重编码/符号链接改向拒绝均有实际回归；原fresh保护用例保为显式live，不以删断言求绿。

临时固定PHP8.5.0/64位镜像执行真实FileActivator/Filesystem/ConfigRepository，只有Container服务寻址及Module.getName为替身；构造缓存、失败内存/重试、cached noop、缺文件重建、numeric list、新实例拒绝、空路径及reset delete=false均有输出/SHA。不是完整上游包或Laravel应用测试。顶层PHP trim/array_is_list/四空格与列表形状由codec对应；未知嵌套值保原JSON、非有限数、任意PHP字节转义/serialize_precision仍不作等价保证。协作锁内合并、原子替换、严格reset IO错误与file-live属于公开保留增强；PHP reset可能忽略delete=false，Rust明确返回Err。

根真实MySQL8.4精确1，fresh普通check/test/build及12、MySQL认证/Job3、MFA2、相同二进制SHA的config cache/坏dotenv/clear复启均通过。隔离部署四TCP进程证明默认旧App不热读外部状态、显式live读取变化、新App按状态挂路由与宿主API保留；各进程正常exit0且端口释放，数据库集合前后0增0删。唯一example和fresh公开fixture均保三App、自身写/外部隔离、路由/事件/原文件保护断言。

失败原件保全：attempt1三处doc-markdown lint；attempt2旧测试用?将新增fallible构造错误提前抛出，改捕获构造→prepare完整链，保原错误码与坏文件原字节断言后attempt3通过。首次清target检测occupied而exit1，未删除；复查无占用，单独正常retry清28,613,931,008B→0，不重复真实TCP。不同模型最终核751输入漂移0、十门禁/服务日志SHA及计数，限定范围无确定blocker；报告/tmp/openrust-file-activator-cache-final-evidence-review.md并已复制证据目录。

生成源码/证据归档SHA7cab7031ef7196bc4aa32ecd9f3ff1c0ff0750fd8a4cc5bee2eefc64f6831cd6，原目录保留；完成fresh后确认无构建进程/target句柄，再正常dry-run+clean 9,007,976,448B→0、磁盘可用200,362,778,624B。独占PHP镜像772,442,516B无容器占用后正常移除，共享MySQL/PostgreSQL/Redis未动。完整回执tmp/file-activator-cache-20261102/；仅本地提交，skill独立owner WIP未混入或部署，不操作其Goal/账本。Windows/硬链接、完整B003/B004、Modules/ProviderRepository/aliases与ADR-001全量仍开放；本机原非UTF8 FS能力缺口不因本簇变为通过。

## 模块运行发现与编译文件限定验收

2026-11-02，39086cf后继；固定Modules5971484、Laravel d52c3b3及PHP8.5.0。源码证据`tmp/module-discovery-upstream-evidence/`、`tmp/module-discovery-20261102/upstream-field-boundaries/`与同目录优先级取证；主路径/额外scan、原始metadata、PHP标量名称、providers flatten/falsy、同名仓储赢家均进入真实包register。命名编译工厂通过唯一内核注册Provider，编译ModuleFile在eager register后、boot前执行；RegisterContext::resolve沿同一容器，已有资源identity及失败/取消回收断言保留。Explicit ModuleDescriptor增强独立保留，不混入发现清单或称上游拓扑。

仓储与Provider普通module_path资源取真实manifest目录，只有files使用可覆盖module_directory；字段转换推迟到实际包含，disabled/空files不提前拒绝。唯一example业务boot实际解析同ModuleRepository，真实宿主使用原目录语言值与包含目录诱饵、仅override位置存在的初始化文件，分别证明资源根/文件根；两次App独立验证source与application-root发布目录，不强迫现I18n未承诺的逐键合并。生成应用新增真实metadata与命名Provider/编译文件，重复声明/路径保留，HTTP与同Container.resolve/Arc计数验证实际初始化一次。

749输入最终冻结，attempt6十门禁全exit0：fmt、默认/全特性严格Clippy、workspace tests（harness汇总1141 passed/35 ignored、1983/112）、双拒警Rustdoc、no-default lib、精确Rust1.97.0、deny。各target/CLI子进程存在重叠，不作唯一行为数。两项非UTF8文件系统用例仅通过能力预检：本机create_dir原始字节名被errno92拒绝，没有执行scanner分支；保原断言并明确诊断，Linux文件系统仍待验，不能将两条计成字节路径已通过。纯OsStr实际匹配器回归已执行，不替代FS证据。

固定PHP8.5.0/64位/precision14的临时Docker镜像实际生成2068组Manifest/Repository排序及显示向量，源码fixture被默认/全features实际对照通过；另跑标量名称与目录拼接探针。镜像digest、脚本/输入/输出SHA见priority-oracle-provenance及metadata-scalar-oracle。不是完整上游包测试或任意PHP ini/全部数值语义。非有限JSON如1e999当前Value表示拒绝是未实现缺口，不归为不适用；完整POSIX glob/非UTF8通配、aliases、完整ProviderRepository eager构造/编译缓存与deferred仍开放。

失败与自纠均保留：attempt1/2新增lint与测试引用错误；attempt3实际PHP发现数组`a>b`须交换操作数，修Repository方向而不改oracle；Mac两FS失败定位为fixture在scan前已被OS拒绝。attempt4新宿主测试错误假设published与source逐键合并，改为两次真实App保所有行为检查后attempt5通过。随后主动复核官方链发现双路径混用/名称过度校验，拒绝用已绿旧输入背书，修为实际Repo目录及使用点转换，attempt6重新完整通过。attempt3/4/5/6的SHA、可匹配源码包及原日志均保全，不回填旧失败。

最终root真实MySQL8.4认证精确1；fresh普通check/test/build及12项、MySQL认证/Job3、MFA2全部通过。相同fresh源码/二进制SHA的config cache/坏dotenv/clear重启通过。隔离根部署三进程实际Provider/files、宿主API保留与下次路由变化通过，三实例正常退出、端口释放；MySQL数据库集合前后0增0删。现App热读外部状态仅为既有Rust行为事实，**不当上游缓存证据**：已确认FileActivator固定实例快照与当前fresh读盘不符，唯一todo B003为本批提交后优先项，未混入冻结输入。

不同模型最终只读报告`/tmp/openrust-discovery-final-evidence-review.md`核749输入漂移0、所有适用receipt/log SHA和实际计数一致，限定范围无确定blocker。显式ModuleExecution共享Manifest空快照及文件完成状态，异步等待/失败/取消/同链cycle已运行；失败封闭与PHP caught-throw后skip不同，属于公开异步安全适配；tokio::spawn不继承task-local，跨任务自等待不作已解主张。此子范围不关闭整个B001/B007/B012、Windows/全后端或ADR-001。

源码owner停写，完整生成源码/回执包SHA eca38aa68311a31890b41b7098eb0b1eeb90c09c3664e7db152b89f6e464121e，原目录仍保留。主缓存35,712,675,840B→0后进行fresh，最终确认无进程/target句柄，再正常clean dry-run+clean：9,004,273,664B→0、磁盘可用200,558,010,368B；本任务原先不存在的PHP oracle镜像772,442,516B确认无容器占用后正常移除，共享DB服务未动。完整回执位于tmp/module-discovery-20261102/。最终只更新Agent状态/验收文档，复用未变化运行输入证据；仅本地提交。skill独立owner WIP不覆盖/混入，不部署、不重启旧守护、不操作其Goal/账本，默认80%与原额度基线不变。

## 同内核嵌套 Provider 注册限定验收

2026-11-02，7f60560后继；固定Laravel Application::register/markAsRegistered及ModulesServiceProvider注册调用位置，来源见`tmp/module-discovery-upstream-contract.md`及同名evidence目录。RegisterContext即时注册子Provider与App/启动共用唯一Registry、Container与执行器；startup统一boot屏障，live子boot完成后父继续，首次槽位/force失败/取消/cycle/TestApp mask均保原契约。nested_providers纯身份声明是Rust无法反射异步回调的必要适配，并在父成功后验证真实登记；不是另一执行队列或自动拓扑。唯一example改由普通父Provider真实注册原独立子项，fallback Events不占真实EventsProvider身份。模块JSON发现、class aliases、files及完整Provider发现仍未实现。

771运行输入最终冻结，10项final2门禁全部exit0：fmt、默认/全特性严格workspace Clippy、workspace test（1099/35 ignored、1940/112）、双拒警Rustdoc、no-default lib、精确Rust1.97.0、deny。初轮编译及Clippy新测试规范错误分别保留；初轮all测试揭示原Session late-overlay引用嵌套child被静态预检遗漏，保留原测试与断言，新增纯身份声明、真实登记核验及三回归后重跑全矩阵通过。不以早期all失败回执宣称通过，最终只计*-final2。

根真实MySQL8.4认证精确1通过；fresh普通check/test/build通过，12项包括配置1、公开Container1、模块1、应用9。Container fixture父两次真实注册同child，registrations=1且原factory/extend/boot全部断言保留；fresh MySQL认证/Job3及MFA2通过。相同fresh源码与二进制SHA的配置缓存/坏dotenv/clear复启通过。根真实隔离部署三实例证明当前loaded不热更新、下一启动404/再启200、禁用模块保宿主API、独立状态文件及未知旧值保留；三个进程正常退出且端口释放。MySQL前后数据库集合无增删，不触碰既有库。ignored不作真实服务证据，不外推PG/Windows或整个Modules完成。

最终不同模型只读审查`/tmp/nested-provider-final-evidence-review.md`核771输入漂移0、回执/日志SHA一致且限定范围无确定blocker；此前contract/declaration两份审查同时保留。全部命令/exit/耗时/资源/失败留存于`tmp/nested-providers-20261102/`。最后仅项目验收/状态文档更新，复用未变化运行输入门禁。源码owner停写；skill独立writer WIP完全排除，不部署、不重启旧守护、不操作其Goal/账本。

完成主矩阵后第一轮target已清零，再完成fresh及全部服务后核无构建进程/target句柄，正常cargo clean dry-run及clean均exit0：8,879,177,728B→0，磁盘可用201,574,379,520B。生成源码和回执包SHA a1845bf378b4ba1b1065cf6939b07821fc0d7975f435e2f9e956d4227d90ab90，原生成目录亦保留。仅本地提交，不push/tag/release；下一功能簇按需重建同一target，模块发现、DB门控及完整ADR-001继续唯一todo。

## 模块持久启停与同容器管理限定验收

2026-11-02，161bfcf后继。固定laravel-modules v13.2.0 / 59714840708a2aece19e810764473f53227c35fe的FileActivator、Module、BaseCommand及Enable/Disable/List源码与测试；来源映射见`tmp/module-persistence-20261102/upstream-correspondence.md`。本簇只完成已编译catalog的持久管理，不把它称module.json运行发现，不关闭B001/B007/B012或完整Modules。

同一次ModuleManifest产生全量metadata与所选激活器，ModuleRepository复用同一Container和弱Resolver；每次派发解析当前根Events，支持强制重绑与根scope更新。四阶段具名事件沿原Events，前异常/取消不写，Stop只停传播，后异常保已提交结果，直接set_active不发四事件。当前loaded与持久enabled分别观察，不热挂卸资源；并发shutdown没有全局文件提交原子屏障。根/生成config/modules.rs及application_root分开资源根和状态文件，CLI只在实际绑定管理仓储的应用登记，裸内核原断言保留。多名预检、重复顺序、all覆盖/空all、异常中断及only/direction可选值边界有回归。

文件驱动保留精确键/非布尔值与未知项；锁内重读、原子替换、显式reset及合法配置符号链接纳入本簇。顺序操作曾偶发busy；本机std探针直接证明复制描述符可在原File关闭后留锁，显式unlock能释放，具体spawn/fork窗口仅推断。修为成功取锁即建立RAII、后置校验受同一guard保护、Drop显式unlock；确定性回归同时检查副本存活与独立新owner，生产竞争仍明确busy。模块定向34/0通过，包含真实子进程锁与隔离cwd。硬链接别名等价、Windows原生及其他平台运行证据仍开放。

最终fmt、默认/全特性严格Clippy、workspace tests、拒警Rustdoc均exit0，回执`*-final4.json`；默认1081 passed/35 ignored，全特性1923/112，跨target重复不计唯一行为数。no-default lib、精确Rust1.97.0与cargo-deny复用`*-final3.json`，后续仅CLI锁忽略/快照/断言及consumer fixture变化，框架lib/依赖/Cargo锁未变。三次早期Clippy失败、用户续作后一次严格通过、事件测试误放queue模块、配置顺序旧断言及裸内核命令差异均保留原回执，修复不删断言/加lint豁免。路由helper内联后整文件SHA恢复原冻结字节。

唯一example真实宿主三App完整state/事件JSON、HTTP状态及状态路径断言通过；独审指出失败分支缺少async shutdown，现所有可失败检查返回Result，已建立实例逆序逐一尝试关闭。fresh普通最终check/test/build通过12项，含新模块consumer确实执行1项、配置consumer1、Container1及应用9；原Application全部hooks转发，仅追加已编译模块。前两次fresh因错误fixture输入失败：补app.url没有解决；实际为把serde(flatten)的driver选项误包在options对象，memory明确拒绝未知选项，修正源fixture而不改框架校验。旧失败及各fresh源码均保留。MySQL/生成Job原QueueContributions3项、MFA2项、根真实MySQL综合精确1项通过，无ignored；未受影响的其他数据库领域证据按原范围复用。

根真实三进程使用任务隔离部署目录和独立状态文件：禁用不卸载当前路由，下个实例模块404且宿主API200，再启用不热挂第二实例，第三次启动恢复；canonical写入、未知原值、默认状态文件不变、事件记录仅进程内均验证。三个服务exit0且端口释放，MySQL前后集合无增删。最初缓存runner错配旧包名二进制的`validated-services-config-cache-restart.json`不计通过；首次同源探针临时目录前缀被拒亦不计。仅`source-matched-config-cache-restart-final.json`绑定combined-plain-validated源码与实际二进制SHA，验证cache创建/重启/坏dotenv屏障/clear后重读通过。

767运行输入分阶段冻结，快照版本及原失败均保留；源码与最终证据独审无确定blocker，完整exit/耗时/日志SHA/复用边界在`tmp/module-persistence-20261102/`。生成源码/证据包SHA344479a60b9dae0910818b20faa824794a8aa202b835ff063879f09443c53e54，排除target、非example dotenv及运行缓存。完成main/根服务后正常清理30,420,819,968B→0；fresh/缓存结束再经byte-mode占用检查及cargo clean dry-run/clean清10,211,336,192B→0，磁盘可用201,763,201,024B。源码/WIP、服务及证据保留，下一模块发现按需重建唯一target。

本地提交事实见Git，不push/tag/release。运行发现/Provider/files绑定、TTY交互、通用false批结果、完整Provider、DB能力门控及Laravel核心/官方可选生态仍在唯一todo。独立guard_native_codex_impl的skill WIP未混入本簇，未部署、不重启旧守护，不操作其Goal/账本，也不外推原生支持已生效。

## 应用启动与固定API宿主职责限定验收

2026-11-02，配置提交d952199后继。固定framework d52c3b3129e4c8e213c97cdc68f94b0b7d27f181、docs 960eedf0911155ad16326ade49c0ea5d18eda0b8，对应L007.02/.03、L011.02、L015.01的职责子范围。具体源码与上游测试锚点见`tmp/application-responsibilities-20261102/upstream-correspondence.md`。框架src/routes.rs移除宿主framework_routes/framework_info，响应归根app Controller；AppServiceProvider→公开BootContext::routes→bootstrap::routes→api相对声明→Controller。API前缀与组在bootstrap配置；routes/mod仅Rust模块导出，Web保既有全局fallback与显式web组边界，不以本次关闭完整Web默认工作流。name/version端点为宿主应用增强，不冒称Laravel内置。

bootstrap原两项真实宿主/模块资源根测试迁至tests/server/bootstrap.rs，资源根原断言保留；新增完整动态应用身份envelope、禁用模块后宿主API仍可用/模块404/无Session Cookie。registration fixture改用真实应用Provider，并验证只装通用HTTP宿主的/api404、health200。根/CLI/普通及认证/MFA/Job/两消费者编译入口同步routes/mod.rs；生成API仍保持JWT/PAT/MFA外部路径及layer顺序，fallback全局，未隐式给API启动Session。必要private cfg/path不扩大生产公共API，没有第二启动内核或框架反向引用宿主。

完整编码和配套收齐后，fmt、默认/全特性严格Clippy、workspace tests、拒警Rustdoc、no-default lib、精确Rust1.97.0及cargo-deny均exit0。default各target报告1050 passed/35 ignored，all1887/112；目标间存在重复，不作为唯一行为数。CLI首次仅预期快照失败26/1，其余断言保留；七个变化段逐一等于对应模板渲染、未变化段不动，后续完整workspace含CLI27通过。生产与测试静态独审、CLI独审无blocker。756冻结输入在执行末尾及证据独审逐项SHA匹配，之后仅更新本节及项目状态。

fresh普通应用check/test/build通过11项，无ignored；配置读取与Container消费者各1及应用9均走最终生成入口，真实配置缓存创建/重启/坏dotenv屏障/清除后重读通过。fresh MySQL认证、邮件覆盖及生成Job复用原QueueContributions共3通过；新增fresh MFA应用check和真实MySQL2通过，含模板已生成的MFA流程。根example_auth_flows精确综合真实流程1通过，无ignored；复用前簇未受影响的完整数据库领域证据，不把本次精确范围外推成全后端/集群验收。

根真实TCP直接断言/api完整name/version envelope及无Set-Cookie，另请求/api/example业务消息通过；Request与Cookie TLS通过，输出`/tmp/openrust-c04c-tls-application-routes-20261102.json`。两个自有进程均exit0、监听释放；数据库集合前后added/removed为空。全部命令/exit/耗时/日志SHA/资源与业务回执见`tmp/application-responsibilities-20261102/`。

清理前只读lsof文本解码遇非UTF8文件名而失败，未执行清理；shell编排继续后主控在软线前停止自有编排、让子命令自然结束，不中断共享服务。中止时根测试只有日志无完整退出回执，未计成功，续跑的services-root-mysql-final有完整exit0。占用检查改为原始bytes路径匹配，确认无构建/产物句柄后正常cargo clean dry-run+clean：48,604,307,456B→0、磁盘153,671,860,224B→201,919,569,920B。没有删除源码/WIP或降低门禁；错误与停止回执独立保留。

MFA/根服务验收后再次确认无占用，最终正常清理target12,371,316,736B→0，磁盘189,830,238,208B→201,898,565,632B，见completed-target-retirement.json。唯一生成源码与证据包SHA cc7716cefd88c88c8b132781e29597c6ebec1bae3d3bf1e9918fca1d98c462df，原生成源码也保留。根target仅留空目录，后继模块簇按需重建同一路径。

本地提交以Git为准，不push/tag/release。独立原生守护owner六文件仍WIP且未混入本簇，不部署或重启旧守护；全部旧来源、保护refs/bundle保留。完整应用生成、Provider发现/生命周期、模块持久启停/发现、DB能力门控及Laravel核心/官方可选生态继续唯一todo；本次不声称完整ADR-001交付。

## 配置可编辑默认值与有序依赖限定验收

2026-11-02，基线c52ba8f后继配置簇；固定framework d52c3b3129e4c8e213c97cdc68f94b0b7d27f181、docs 960eedf0911155ad16326ade49c0ea5d18eda0b8。对应L005.01/.03/.04子范围，来源/测试锚点及必要Rust差异见`tmp/config-defaults-20261102/upstream-correspondence.md`。ConfigContext get/require、根键求值前覆盖、单次来源捕获/重放、模块候选/CAS、根/生成配置字段和Job配置接线沿同一ConfigSources/Repository/Provider；无新依赖或第二内核。声明顺序是显式编译manifest，前向/自依赖拒绝；JWT/签名默认身份继承为授权增强，不冒充Laravel核心默认。

配置定向119、模块投影2、公开consumer11、CLI27通过。main集中fmt、默认/全特性严格Clippy、workspace tests、拒警Rustdoc、no-default lib、精确Rust1.97.0及cargo-deny十项全部exit0，最终回执`*-attempt3.json`；默认各target报告1047 passed/35 ignored，全特性1884/112，重叠入口不计唯一行为数。首轮配置测试118案中1个新typed预检fixture因空根规范化失败，修为仅整个空根数组可承载typed路径，保留嵌套/非空冲突断言后119通过。Clippy两轮分别揭示默认及全feature的代码规范/注释问题，提取helper、补标点/借用，不降门禁。快照只接纳六份配置渲染变化，逐段与模板精确一致，其余不变；独立delta审查无新增blocker。

普通fresh check/test/build通过；最终修复后的installer又在另一个从零生成应用check/test通过，共11项（配置consumer1、Container1、应用9），无ignored。旧cache.stores.insert注入点已随模板变为stores表，新增唯一marker预检防静默未注入。初次configuration_error与误用字面点键导致Production/Testing断言失败均保留，恢复根app对象并正确注入memory store后通过；没有删除环境或HTTP断言。最终来源`final-fixture-*`回执，不用手改生成副本替代源installer证明。生成时父目录不存在的runner失败及后续修正也分别保留。

真实同一生成二进制完成config:cache、重启、坏dotenv屏障、clear后重读，见`services-config-cache-restart`。在已保全的普通生成源码中仅修改server.port默认8123与app.name默认，再用实际manifest执行8类探针：代码默认、SERVER_PORT、独立JWTissuer、独立audience/JWTissuer/signedissuer、显式APP_URL、显式空URL、求值前覆盖、非法端口。独立覆盖沿公开根对象source API，值断言完整；首次使用不受支持的APP_AUTH_JWT_ISSUER映射失败保留，后续不声称该env别名已支持。空URL保留到后续统一校验，不把空值视缺失；非法端口在依赖读取前拒绝。晚merge不重算的永久公开consumer回归同样通过。

真实MySQL8.4 fresh认证/邮件覆盖/Job原QueueContributions共3项通过，无ignored。根bin server149/20 ignored、example_auth_flows151/20、server集成164/20分别通过，存在重复模块用例，不相加。数据库集合前后无增删。根真实TCP Request及Cookie TLS通过；TLS首次runner证据路径被拒绝且未进入网络，使用规定/tmp前缀路径后通过，证据`/tmp/openrust-c04c-tls-config-defaults-20261102.json`。成功服务均正常exit0、端口释放，初轮和最终停止回执独立保留。编排脚本缺cli变量的失败发生在后续命令前，修正后只续未执行项，不重写旧回执。

执行时653输入冻结经独审及最终哈希核一致，运行源码此后未改；最后仅更新本节及项目状态/变更说明。所有日志/命令/exit/耗时/资源与SHA位于`tmp/config-defaults-20261102/`。实现/独审owner均停写；原生守护六文件仍属独立owner WIP，未覆盖/提交/部署，旧总控和守护保持停止。首次fixture错误归因和误改点键的判断已撤回，不以错误说明替代事实。

生成源码及证据完整打包`generated-source-and-evidence.tar.gz`，SHA3242fc666b9e87209accc3da1f4cb269342abe41cfe40e0f6f39e6a8f3610cad。确认服务/构建停止后，三组生成package正常cargo clean dry-run及实际clean均成功：根target38,516,436,992B→37,283,803,136B，磁盘可用163,844,558,848B→164,923,183,104B。只保留同一main根target供紧接的bootstrap/app及固定API簇复用，未留额外target；保护全部源码/WIP、依赖源码、共享服务与证据，明细`generated-retirement.json`。

本地提交以Git为准，不push/tag/release。完整Config/env加密/动态Settings、默认MySQL+Redis依赖体验、bootstrap/app、固定API归属、模块持久启停/发现、DB门控、全后端/原生Windows及完整ADR-001仍开放。字段展开、有限服务或上述测试通过不等于Laravel+Modules全部1:1完成。

## 当前 main 组合限定验收

2026-11-02 收齐本批 Env/Request-MIME、TrustedHosts、中性运行命名、应用代码配置与 Provider、Handler、Web/CSRF、部分 Query 的组合证据。基线 `317e2f6`、环境来源 `8d5e635`；Request/MIME 与其他来源的祖先或三方等价映射保留于 `tmp/controller-handoff-20261102/` 及各 integration 目录。本节对应本地集成树，提交以 Git 为准，不代表完整 Laravel、Modules 或 R4/C07 已完成。

最终默认与全 feature workspace 测试、双严格 Clippy、拒警 Rustdoc、no-default lib、精确 Rust 1.97.0 check、cargo-deny 全部 exit0。最终命令、日志 SHA、耗时及资源见 `tmp/takeover-20261102/{final-gates-index,final-gates-resume-index,final-gates-final-index}.json`；冻结生产/测试/模板输入在同目录 `services-final-inputs.json`。Clippy 首次因 SSE 测试行数失败，等价抽出两次相同的请求构造后通过，未增加 allow 或降低警告级别；快照仅三份已审 helper 与源模板一致后更新，旧失败及快照差异保留。

普通 fresh 生成项目 check/test/build 通过：配置读取、Container 扩展各1项，应用9项，无忽略；实际缓存生成、坏 dotenv 屏障、重启、清除后重读通过。MySQL8.4 fresh check 与真实测试3项通过，包括认证 HTTP、TestApp 邮件覆盖和新 Job 对原 QueueContributions 的复用。原生 API 注册严格无 Session Cookie，显式测试 Session 路由另保 Cookie 数量和加密正向断言。证据 `tmp/combined-validation-20261101/services-*-attempt{7,8}.{json,log}`；不将旧 fixture 的 test/testing、字面键、Provider 清单和全局 Session 假设失败回填为通过。

真实 MySQL 根 bin server 147通过/20忽略；独立 server 集成162通过/20忽略；综合认证/CRUD/分页/Session driver fixture 精确测试1通过。不同目标有重叠模块用例，不相加为覆盖率。证据为 `services-root-mysql-attempt9`、`services-server-integration-attempt11`、`services-root-mysql-attempt11`。首轮缺 Host 的8项及无 Session 宿主错误接线已保留：统一分派 authority 诊断确认后补合法请求头；账户 `/session/*` 按实际 Session 能力登记，原 JWT/模块 API 保留，并新增无绑定及404回归。Query 初始空选择列导致分页 count 子查询 SQL 失败，`Db::table` 已默认选择 `*`，显式 select 替换；真实分页总数、行数据、空第二页和所有者隔离通过。扩展 fixture 显式使用精确 Testing，未放宽生产 Mailer 策略。

根真实 TCP Request 通过；Cookie TLS 的可信 HTTPS、可信 HTTP、不可信伪造协议、属性与密文往返全部通过。TLS 首次输出路径不符被参数检查拒绝、未进入网络，修正 runner 后通过，不改安全检查。TrustedHosts 独立真实 TCP 9案通过：development allow/reject、可信 forwarded Host、HEAD 空体，以及精确 local/testing 绕过。证据为 `services-request-input-tcp-attempt11`、`services-cookie-security-tls-attempt12`、`/tmp/openrust-c04c-tls-takeover-20261102-attempt12.json` 与 `tmp/takeover-20261102/trusted-hosts-tcp-attempt3.json`；前两次模式配置输入格式失败保留。成功验收的自有服务进程正常结束、端口释放；前两次 Host 配置输入失败均在监听前退出，失败证据保留。最终数据库集合无增删。忽略、函数内无凭据返回和 PHP/Windows/Fileinfo/全后端/真实集群等外部或发布矩阵不算此次通过。

旧九份来源、六份 WIP、完整历史 bundle 与保护 refs 已核保全，所有非 main worktree 及八个来源 workspace 已正常退役，不重复整合。危险 `cd487bb` 未合入。skill 本批仅接纳与旧142/61测试及独审九文件 SHA 完全匹配的版本；后续 ended-grant/Codex 原生适配由独立 owner 收口，保持 WIP、未部署，旧总控及旧守护停止。预算历史与问答账本未改；本对话无正式剩余额度读数，不从旧账号推定。

本批完成后确认无构建/服务进程及产物句柄占用，`cargo clean --dry-run` 与正常 `cargo clean` 均exit0；根产物48,729,780,224B→0，磁盘可用154,006,482,944B→202,384,363,520B。源码、生成fixture、依赖源码与证据保留；明细 `tmp/takeover-20261102/completed-target-retirement.json`。下一配置簇仅按需重建同一根target。

<a id="environment-identity-branch"></a>
## 环境身份保真分支限定验收

`tasks/environment-identity`从`266e746`派生，限定提交`8d5e635`，15个源码/模板输入diff SHA `24266a44489fbd567d2e2a86f6baeb009ddf071b7289b02f5dc49356f429d9af`；额外CHANGELOG为root纯文档。主工作区ignored `guards/6b56ea51-0bb7-44a7-8547-7073f8f2c7e4/environment-identity-evidence/`保全158文件，含`INPUT.json`、`FINAL.json`、`COMMANDS.json`（27次命令与退出码/阶段输入、25原日志SHA，未记录的精确耗时为null）和3份生成输入归档40/40/44文件；root逐文件复核无漂移。固定Laravel来源与不同模型三项修复、模板增量复审在同任务`environment-identity-review/`。

实际分支定向：配置97、core136、server示例33、公开consumer8、同名值身份回归；严格workspace/all-target Clippy、workspace docs、OIDC/MySQL/SMTP/Twilio受影响feature check通过。fresh普通check/test8与fresh MySQL认证check/test2通过，0 ignored、无skip；MySQL前后65个库、集合SHA `0def270dab2fa099bb8e025513b8f8beca7846fdc6709e5f00955dbcec8b0313`相同。CLI模板与快照各1通过；旧lib示例过滤0项不当通过，后续实际server示例33项单列。首次Clippy测试expect、普通生成默认production保护触发的8失败、旧快照失败保留，修后对应结果独立记录；测试夹具显式Testing，不放宽生产默认或安全保护。

本簇保留精确名称与官方环境谓词；旧生产安全别名使用独立派生策略，不冒称Laravel环境匹配。Loader只读显式覆盖，不把缺省选择当最终文件环境；同名值相等/哈希与serde一致。现有`.env.example`显式development、CLI `--env`/`.env.<env>`、完整启动环境检测仍开放。生成源码脱敏归档保留；共享根target末次约49.28GB留给已安排的main组合验收，不是新target配额。本节只证明分支限定交付；当前与MIME组成同一候选后才执行main门禁，不自动将旧分支测试转记main通过。

<a id="c04-g-trustproxies-main-限定验收"></a>
## C04-G TrustProxies main 限定验收

当前main候选基于`266e746`；452个Rust/TOML/snapshot/lock输入聚合SHA `792d4b783cc6d55423cf98255f8c5d052e280f892c3ef0b6b30cba9709bc125c`，实际命令、退出码、耗时及日志SHA保存在`.agents/project/guards/6b56ea51-0bb7-44a7-8547-7073f8f2c7e4/proxy-evidence-v3/receipt.json`。修后定向、公开consumer/模块、CLI快照、默认/全特性workspace、双严格Clippy/docs、no-default、精确已装Rust1.97.0及deny均已通过。Docker未运行导致的Redis失败、错误test target、v2测试类型推断错误与`+1.97`别名误触工具链下载后中止均保留原记录，不回填为原命令成功。现仅使用已装精确版本且禁自动安装。

不同模型静态复审已关闭quoted-pair解码、IPv6原文本冲突及显式port惰性scheme三项；固定上游来源、修后副本及报告在同任务`evidence-recovery-review/`。新真实PHP40案两轮exit0、输出相同SHA `6203c72a5c5cc32f9d25b7a1c8745ef160d742035f2a217798ed2bc949b072a9`，不等于恢复旧59案；实际PHP8.2低于Symfony8声明最低8.4，属于所用真实方法的有限动态参考，不称官方支持runtime或完整应用验收。逐案Rust映射的partial/no-exact范围明确保留。

根server SHA `fabd7b52890341631bfb2205d1cd76b0be3314a2fb8b8ff7d2328a2b21c6b81c`；旧URL/TLS网络工具10项通过、source_changed为空、binary未变，工具自身明确不证明完整TrustProxies。新增唯一模块公开TCP3案通过：可信quoted-pair与客户端链/Host/Port/Prefix读取、IPv6双头原文本冲突400、显式空代理列表忽略伪造头；服务正常exit0且敏感配置清除，回执`proxy_tcp_report.json`及`proxy_tcp_untrusted_report.json`。807项全Git冻结窗口核无漂移；首次自写TOML夹具放错mask位置导致未就绪的失败保留，修后结果独立记录。

fresh普通生成check/test通过，实际8项、0 ignored；fresh MySQL认证check及真实auth 2项通过、0 ignored且无skip。未采MySQL事前schema集合，不能声称前后集合相等；事后60个匹配schema的命名时间均早于本轮，没有本轮命名残留，检查方法与指纹见`mysql_schema_after.json`。这是清理证据的有限边界，不推导其他会话数据未变。最终`final_handoff.json`绑定36份日志、452项源码零漂移；默认workspace963通过/34忽略，全特性1775通过/111忽略，ignored不算真实服务。

专属生成target 3,896,115,200B及本轮consumer target 1,801,461,760B已dry-run后正常clean归零；采样登记产物44,658,880,512B→38,929,108,992B，最终磁盘可用174,562,840,576B。该登记数不含另行标注保留的未知外置65,982,464B，不能冒称包含它的全项目总数。根target仅保留给已实际安排的环境身份簇Verify；生成源码84文件哈希、日志和上游参考保留，服务/数据卷未清理。根输入和全部日志由root最终审计后限定本地提交，提交号见Git。

本节只关闭当前代理快照、公开读取及其已实测子范围；完整Request/TrustedHosts、原生TLS、真实Cloud部署、发布后端矩阵仍开放。mask0、Cloud非1/伪客户端头、port00及超过i64的逐案Rust回归映射仍不完整，源码/有限PHP参考不冒称逐项测试已闭合；Symfony合法引号转义已覆盖，不外推全部非法语法宽容行为。MIME仍为任务分支交付，未合main。

<a id="c04-f-request文件家族分支限定验收"></a>
## C04-F Request文件家族分支限定验收

任务分支`tasks/request-files-family`基于`a9b057e`，最终冻结树`e8ff0cac0e66bfa04a92ea5aaa165fce307904cf`，808个输入和36个候选文件SHA均无漂移；最终证据索引`/tmp/openrust-request-files-final-evidence-index.json`核39份原始日志SHA。此为分支限定验证，任务分支限定交付，尚未集成main。原失败回执未覆盖；最后相对全特性通过输入仅模块测试失败清理和生成模板/快照三路径变化，生产`src/`、清单和锁不变，受影响server测试严格Clippy及CLI/生成/MySQL已补验。

Request库41、`http_input`38、公开test-app consumer Session2、最终CLI主测试23均通过；CLI嵌套摘要不重复计数。旧最终索引曾把父CLI的23项再次扣除而记922；按原日志逐Running/Doc-tests段最后summary复核，959项原始摘要扣14项嵌套为945。更正证据`/tmp/openrust-request-mime-counter-historical-audit.json`保留每binary行号和日志SHA；旧索引/原日志未改写。默认workspace945通过/34忽略，全特性1760通过/111忽略；双严格Clippy与警告拒绝Rustdoc、no-default、Rust1.97和deny适用门禁按代码审计`/tmp/openrust-request-files-code-gates-root-audit.json`与全特性审计`/tmp/openrust-request-files-all-features-root-audit.json`通过。ignored或无服务环境跳过不算真实后端验收。

最终真实链`/tmp/openrust-request-files-services-third-20261101T042452Z/receipt.json`中，CLI23、全新生成项目check及`test_app`8、真实TCP1、Request精确1/TCP精确1均通过；唯一模块multipart附件真实MySQL精确1项无skip，隔离库集合前后相等且同一server UUID。进程组均无残留，生成应用源码已由driver清理，现不能重算其原字节，保留命令、fixture SHA和日志。正文沿原Axum默认/覆盖/禁用限额读取；原文32 MiB、结构化8 MiB是可调整Rust资源预算，不冒称Laravel默认。任务结束清理回执`/tmp/openrust-request-files-final-cleanup-retry/receipt.json`记录正常Cargo清理exit0，根target实测29,937,872,896B→0，磁盘可用167,465,619,456B→197,155,180,544B；原缺缓存标记中止记录保留，归属核定后恢复标准标记，未动未知外置缓存。源码冻结后的最终变更仅为Agent事实文档与依赖说明，不影响已执行Rust门禁；真实服务细节以driver回执`/tmp/openrust-request-files-services-prepare/run-20261101T042456Z/receipt.json`为准。当时完整Symfony MIME、真实S3 ACL、迟到CopyObject、Storage `throw=false`和Windows原生取消清理仍开放；MIME续作的新限定证据见下，不能关闭整个Request/Files。

MIME续作的任务分支限定证据索引`/tmp/openrust-request-mime-final-evidence-index.json`已核608输入、21候选及24份本轮原始日志SHA无漂移；v7代码回执`/tmp/openrust-request-mime-code-verification-v7/receipt.json`复用v6前8阶段的有效证据，15阶段均exit0。Request库51、`http_input`38、公开MIME consumer1、Session consumer3通过；默认workspace955/0/34忽略，全特性1770/0/111忽略，双严格Clippy、全特性警告拒绝Rustdoc、no-default、Rust1.97和deny通过。真实链`/tmp/openrust-request-mime-services-prepare/run-20261101T080437Z/receipt.json`九阶段exit0：全新生成项目check与`test_app`8、生成multipart与真实TCP精确各1、唯一模块真实MySQL multipart精确1无skip；同一MySQL server UUID及既有60个schema集合前后相等，进程组无残留，生成应用由本任务清理。此轮证明固定Symfony v8.0.0有序扩展表、Unix `FileBinary`与可替换guesser的限定行为；原生Fileinfo、Windows默认检测/清理、完整MIME及S3 ACL、迟到CopyObject、`throw=false`继续开放。MIME续作本次任务分支限定交付，尚未纳入main；v3–v6失败原始证据保留，旧Request默认945/34（原922误扣CLI父23）的更正不被新955/34替换。

本轮资源清理回执`/tmp/openrust-request-mime-final-cleanup-retry/receipt.json`核无构建进程/target打开句柄，`cargo clean --dry-run`与实际`cargo clean`均exit0；根target30,115,557,376B→0，磁盘可用168,778,330,112B（16.968%）→198,646,296,576B（19.971%），未知外置65,982,464B未删。首次lsof解码失败时未执行clean的原证据保留；本次数值为清理前后实测，不与代码/服务阶段的末次采样混算。

<a id="st-v-main-限定组合验收"></a>
## ST-V main 限定组合验收

来源`2262df990e134ca94dc153c180ff1f16be95e945`由两父本地提交`8d18021b630ea10652316e025ef6019c7772bbb0`纳入main，提交tree与冻结候选`2cfbe929714854d6cbaaf1f8508cb8fef6245ac8`相同。冻结805个输入SHA、33份实际日志SHA和当前锁依赖由`/tmp/openrust-stv-main-final-index.json`与`/tmp/openrust-stv-main-final-root-audit.json`绑定；第一次runtime临时回执写入错误及第二次缓存副本目录前置失败原证据保留，未将其改写成业务测试通过。

默认workspace945通过/0失败/34忽略；全特性1756通过/0失败/111忽略。双严格workspace Clippy、双`-D warnings`文档、no-default、Rust1.97默认/全特性check、当前锁`cargo deny`、fmt与锁定metadata均退出0。独立`data-exchange`公开consumer实测1/0/0；全特性workspace中`storage_http`真实loopback上传/下载与未授权拒绝用例通过，S3真实服务相关ignored不计通过。唯一模块attachments与租户cleanup真实MySQL精确各1项、`data_exchange_tasks`真实MySQL精确1项均无skip，schema集合前后一致。

全新普通生成项目check/test实际8项通过，并以其binary完成GET `/api` 200、SIGINT退出0和端口重绑；缓存维护与重启脚本在带匹配标记的独立`/tmp/openrust-config-cache-*/app`副本中通过。全新MySQL+auth项目check/test 2项及认证精确2项均通过。根server新构建回执绑定805输入与binary SHA，独立进程真实TCP六步通过（`/tmp/openrust-stv-main-root-tcp/result.json`），exit0、无残留监听、SO_REUSEADDR一次重绑；此六步验证Session链，Storage上传下载由上述`storage_http`真实TCP集成测试证明。尾链最终已登记target67,485,491,200B，磁盘可用126,617,210,880B（12.73%），仅当次采样；50GB软线超出、100GB/5%硬线安全。完整Storage/Filesystem、真实S3 ACL启用/禁用bucket、迟到CopyObject与Laravel`throw=false`失败值仍开放。

本簇验收后确认无Cargo/rustc/rustdoc/server占用和打开的根target文件，带`CACHEDIR.TAG`的非符号链接根target执行`cargo clean` exit0（日志`/tmp/openrust-stv-main-cleanup/clean.log`，SHA256 `f3746a9e818a2de305281f28b270ca103b04a5ba32a7a177482fe7f5ce45d9aa`）。去重登记3个现存target合计67,485,491,200B→65,982,464B，磁盘可用126,622,199,808B→193,684,373,504B；这是清理前后实测，不与上述尾链末次采样混算。外置65,982,464B归属不足未删，五个历史登记路径已退役，两个guard工作区、源码、数据库卷和验收日志未碰。4个本任务小生成源码目录166文件在`/tmp/openrust-stv-main-final-index.json`留有清理前哈希并保留，可再生编译产物已随root target清零；资源回执`/tmp/openrust-stv-main-cleanup/receipt.json`。

<a id="r2-p-s-dv-main-limited"></a>
## R2-P / S-DV main 限定组合验收

三父本地提交 `9e5a94198b421987851b3580b27fa90370feeb77`（父 `ddda147`、Provider `4ac8099`、Session `6ffb5c8`）的tree `03b7c799fbf2e23608bbebfe3f30bda4913b22ca` 已完成同一输入的主线验收。最终聚合索引 `/tmp/openrust-main-provider-session-final-aggregate.json`、独立核证 `/tmp/openrust-main-provider-session-final-root-audit.json`：805项冻结输入、59份日志哈希已核，旧失败未覆盖。默认workspace943通过/0失败/34忽略；全特性1748通过/0失败/108忽略，忽略项不计实际行为。默认/全特性严格Clippy与Rustdoc、Rust1.97 MSRV、无默认特性检查、deny、fmt/CLI快照均exit 0。四个公开consumer测试与三个额外check、fresh普通生成7项及公开fixture1、真实MySQL根1项和生成认证2项、缓存命令与重启均按绑定输入通过；MySQL隔离库集合前后一致。根TCP六场景通过，SIGINT正常退出0，端口可重绑、进程/临时目录清理完成。当前资源末次采样已登记target44,603,318,272B、磁盘可用149,478,838,272B（15.03%），仅该时间点，不冒充后续窗口现值。

P分支默认严格Clippy曾连续三次失败，按§5暂停并获用户明确授权单次重验；该重验通过，随后main双特性严格Clippy也通过。生成入口旧CLI复用、首次view漏await、组合阶段的借用与函数长度等原失败回执仍保留，不回填成通过；修复后的实际结果以最终索引为准。P分支原97项源码与Cookie快照/生成补验见 `/tmp/openrust-r2p-cookie-matrix-handoff.json`、`/tmp/openrust-r2p-generated-acceptance-attempt3/root-independent-audit.json`；它们是分支证据，不替代本次main命令。S-DV源工作区与本地分支已安全退役（`/tmp/openrust-session-data-values-safe-retirement.json`）；P两份root文档事实已迁入main提交 `53c63c8`，原稿保全于 `/tmp/openrust-provider-root-docs-wip-preserved/manifest.json`；P工作区/本地分支已安全退役，回执 `/tmp/openrust-provider-registration-safe-retirement.json`。已服务Router热更新、Provider自动发现、完整Facade/Container/Session及发布全部后端仍开放。

## R2 四簇 main 组合限定验收

同一main提交整合 R03 `6482a38`、typed `b1f4bf7`、CC `7be446d`、Signed `c77521f`；原main HEAD `1db5ad6`，五父本地提交 `e105dbfc58afc330b50e53b75f0b9e1424fa387a` 已创建。当前已通过默认/全特性严格Clippy及workspace tests：默认907 passed/0 failed/34 ignored，全特性1698 passed/0 failed/108 ignored；ignored不计真实服务。双Clippy/docs/MSRV、no-default、deny与workspace命令、日志和输入树以 `/tmp/openrust-four-family-main-workspace-clippyfix3/receipt.json` 为准。双warnings-denied docs、MSRV、no-default和deny均exit 0。新生成普通应用7项通过；原一体生成回执的TCP runner阶段失败保留，后续直接诊断记录 `/api` 200、SIGINT exit 0、端口可重绑。新生成MySQL认证应用4阶段均exit 0，auth测试2项无skip，schema集合前后相等。最终索引 `/tmp/openrust-four-family-main-final-handoff.json`（SHA256 `6408da77ed2d77c07567944a82b2c6895a247dd7705cab17b3ae5df1d1613526`）绑定组合树 `e3cef3324da3d82d15e8b19426b0c3d2282e42ec`，804项冻结输入无漂移；外部配置consumer7、生成Provider具名测试1、缓存命令及真实重启脚本均通过，根router signed定向4项含真实TCP，普通生成server直连200/正常关闭/重绑单独留证，不把启动探针替代业务断言。MySQL真实认证2无跳过且库集合前后一致；本批未跑外部Keycloak或PostgreSQL，ignored不计真实服务。收口时无Cargo或生成server进程，target55.76GB触50GB软线、disk空闲138.57GB约13.93%，未触100GB/5%硬线；此处为验收收口时样本；其后正常 `cargo clean` exit 0，根target实测55,298,695,168B→0B，已登记target合计55,494,791,168B→65,982,464B，磁盘可用139,102,576,640B→193,909,436,416B，见 `/tmp/openrust-four-family-cache-cleanup/RESULT.json`；外置db-plugin-consumer剩余65,982,464B归属不足未删；完整Config、URL签名复杂参数/代理及发布全后端矩阵仍开放。

## R2-CC 配置缓存（分支限定验收通过）

本簇为 `tasks/config-cache-family` 的分支证据，base `9581a53`，不等同main组合验收。最终18文件输入 `/tmp/openrust-config-cache-command-bootstrap-script-delta2.json` 经不同模型复审，路径误删、dotenv全局污染、模块绕过缓存、晚期命中前环境校验与维护命令bootstrap时点问题均已修；脚本副本保护/独立负例/失败恢复保护经限定复审关闭。静态结论不代替真实运行。

- 当前修后 `/tmp/openrust-config-cache-final-run.json`：config77、lifecycle18、模块1、CLI模板1、双严格Clippy、新server实际Compiling及唯一模块cache/clear/坏TOML/坏缓存命令真实脚本通过。链在生成成功后的错误忽略规则断言停止，保留该失败，不改正确`/bootstrap/cache/*.json`模板。
- `/tmp/openrust-config-cache-final-resume.json` 从CLI实际重编起续跑；新binary SHA、生成cache JSON忽略与非缓存路径正反验证、生成check/test8/build及真实重启通过，配置源恢复SHA一致；默认workspace lib/tests851通过。root核当前原日志SHA无漂移/截断；最终全特性lib/tests1638通过/68ignored、no-default和Rust1.97全特性workspace/all-targets check均exit0。
- 旧 `/tmp/openrust-config-cache-gates2.json`、supplemental/extra回执及早期失败保留。原all-features all-targets额外运行复制benchmark失败，不能改称原命令成功；测试目标补验、doctest/docs、MSRV/no-default/deny各有独立记录，最终按变更影响明确复用或补验，不将旧输入冒称当前通过。
- 修后文档补验 `/tmp/openrust-config-cache-doc-supplement.json`：默认doctest22通过/34ignored，全feature24通过/40ignored，双warnings-denied docs均exit0；旧离线deny仅在manifest/lock/deny.toml SHA一致前提下复用，advisory快照2026-10-30，不声称在线刷新。
- 最终索引 `/tmp/openrust-config-cache-final-manifest.json`（SHA `73da1b7712e35176dff0bd30b63004d07e7f35f5e6e73603c99b6005c3508119`）；root核61项输入/日志与42个生成文件，`/tmp/openrust-config-cache-root-final-audit.json` 保留当前server与历史不一致的事实。共享target后续构建已替换server，原两脚本开始强制核expected SHA、结束原日志均记同一历史SHA，不能声称现在仍可重算旧binary。
- `/tmp/openrust-config-cache-cleanup-receipt.json`：两自有temp目录在42文件hash/恢复hash/无symlink或进程占用核对后退役，实测磁盘回收13,570,048B，共享target变化0。Cargo dry-run忽略包URL/版本限定、会清所有同名sample-app，故未实际clean；约1GiB宽范围缓存保留，责任root，下一资源交接先核精确归属再退役，不为回收而删他人产物。
- 最终cargo fmt --all --check及diff检查通过；限定分支本地提交见Git，尚未合main。缓存命中无数据库路径变化，生成仅普通项目；如复用既有MySQL证据须明确原输入及影响，不声称新缓存全后端验收。完整发布后端、上述混合归属缓存退役及main组合仍开放；文档变化不改冻结Rust输入、不触发无关矩阵重跑。

## 当前数据库运行政策

- 最新真人要求：日常功能簇、main组合及数据库型生成项目的实际数据库验收只使用真实MySQL8.4；普通无数据库生成、HTTP、fmt/Clippy/unit/doc/feature/MSRV/deny等其余适用门禁保持。编译/features不算真实数据库通过。
- PostgreSQL、SQLite（包括最低3.26.0）、MariaDB、SQL Server及其他后端的真实运行矩阵统一排在发布前，不作日常提交/推送门槛；不保留普遍“后端风险当场多库定向”例外。用户以后明确要求的具体诊断按该次授权执行。
- 已在途命令自然结束并收取真实结果、清理自有资源；未启动的非MySQL阶段转发布待验，不删除服务、driver、SQL、测试、断言或支持承诺，不把未执行记为通过。历史多库证据保留原输入与执行事实。
- 日常runner须显式选择MySQL资源scope，不隐式启用非MySQL运行测试；所有feature仍执行适用编译/Clippy/doc检查，非MySQL运行测试保留在发布scope中。scope未落地时明确记录，不能假报完整日常矩阵通过。
- 发布前完整验证全部承诺后端、对应最低/当前版本及生产、Schema、迁移、事务、数据安全、认证、队列、生成等适用矩阵；目标基线见[数据库兼容计划](../plans/database-compatibility-baseline.md)。未实现、未执行或失败项保持发布阻塞，日常MySQL通过不能外推五数据库兼容。
- 最低版动态库证据核实际加载version、source_id与路径，bundled新版不冒充最低版；必要驱动限制须诊断记录，不悄然提高最低版本。CI日常为MySQL，手动release-backends只是发布子矩阵，不代表完整发布验收。发布积压见[todo](../todo.md#发布前全后端兼容性验收)。

## Query JSON 包含与 SQLite 写目标的发布运行入口

C07-J01 的 PostgreSQL `json`、`jsonb`、SQLite 简单列 JSON 包含及 SQLite 写目标专项仍待发布验收。日常保留全特性编译检查和 J01 的 MySQL 8.4 生成项目运行。以下专项需要显式执行忽略测试，普通 workspace 结果不能替代。

```bash
# 从 OpenRust 根目录执行，测试 URL 由验收环境提供。
cargo test -p openrust-framework --features db-postgres --lib \
  postgres_json_contains_preserves_json_types_unicode_empty_null_and_write_ranges -- --ignored

# 为每个实际链接的 SQLite 引擎设置对应版本前缀后分别执行。
cargo test -p openrust-framework --features db-sqlite --lib \
  database::query::sqlite_write_target::tests:: -- --ignored --nocapture
cargo test -p openrust --features 'auth,data-exchange-tasks,storage-local,db-sqlite' --bin app \
  limited_article_import_write_keeps_other_receipts_open -- --ignored
# 连接需提供 JSON1，环境变量为实际目标引擎的版本前缀。
cargo test -p openrust --features 'auth,encryption,db-sqlite' --bin app \
  sqlite_record_json_contains_preserves_owner_pagination_and_scalar_types -- --ignored --nocapture
```

从相应生成项目根目录执行 PostgreSQL 的 `cargo test --test query_json_contains -- --ignored`，以及 SQLite 的 `cargo test --test sqlite_write_targets -- --ignored --nocapture`。确认 PostgreSQL URL 指向专用测试库。SQLite 当前版和 3.26.0 分别记录实际链接来源及版本，设置 `OPENRUST_EXPECT_SQLITE_VERSION_PREFIX` 核对运行时报告。低版 JSON 包含须另外确认 JSON1 可用，区别普通连接器的 3.26.0 最低目标。每条命令记录实际选中并执行的测试数。

## R2-ENV 环境命名（限定验收通过）

输入为main的R2-ENV工作区，不以R02旧证据替代。当前CLI SHA `4c8bbbde8289ddd7d2a24128893e163af073e4daa7131447f7143509e82ae28d`、config/mod.rs SHA `e5354b2220c4d7acc8a18a468e92db7a6c56c3ebe94682e2d0a8fc587a9d366e`。

- `/tmp/openrust-env-verification/logs/`：CLI全22项、配置73项、cache单元20项、lifecycle8项通过；配置重验日志为`config-lib-retry.log`，保留原71通过/2失败记录。dotenv测试未预登记SERVER_PORT导致Jail退出漏恢复，原owner只补测试隔离登记，生产解析和原期望不变。
- CLI driver覆盖新增fixture首次编译误用Table的FromStr，修为DocumentMut后定向及CLI全组通过，原失败`cli-redis-store-targeted-final.log`保留，不用静态检查替代编译。
- 不同模型静态审查先修正具名Redis store错误套全局连接，再补环境覆盖driver；最终窄审`/tmp/openrust-env-independent-review-final.md`读到上述最终CLI SHA，原should-fix关闭。纯静态审查不代表生成、监听或Redis已验收。
- 默认workspace完整日志30组汇总858通过/0失败/34忽略；全feature完整日志54组汇总1647通过/0失败/108忽略，编译链接耗时3分27秒。忽略项不算真实服务通过；后续docs阶段已自然接续，无同矩阵重启。
- 最终默认并发全feature同样1647通过/108忽略，适用strict Clippy/docs/MSRV1.97/no-default/deny及4个consumer通过。fresh plain7+fixture1、MySQLauth2及测试库集合前后一致、root/生成TCP正常退出和端口释放；生成HTTP额外两场景实证SERVER_PORT覆盖冲突旧键、APP_URL插值、默认redis和改选memory的真实读写，独立公开Provider探针真实Redis验证Cache/Queue共用DB0与dispatch/run_next。
- 最终清单`/tmp/openrust-env-verification/FINAL-RESULT.json` SHA `fa7e0dd925c99504cc50143542483fecaf0124e10e3b15ad5b104655ae4c21d7`；root一次复核`ROOT-FINAL-VERIFY.json`校验566源、21主日志、10回执、7+4生成命令与43+44最终生成输入，5服务PID消失、6端口释放、无源码漂移。MySQL保存日志有意脱敏，raw/stored SHA分列，不将存储hash冒称原流hash。生成probe在原生成源码上独立记录3文件变更与新hash，旧生成通过不冒充修改后测试；probe另编译和真实HTTP通过。
- 仅回收task-owned生成/probe package产物，日志与复跑源码保留；未知旧target未动，主缓存保留。已登记target41.666GB、disk余约99.316GB/9.985%，软告警未作硬停机；日常MySQL结果不关闭发布全后端矩阵。限定本地提交见Git。

<a id="r2-r01-config-runtime"></a>

<a id="r2-r02-config-sources"></a>

## R2-R02 配置来源顺序（限定验收通过）

验收输入基线为`2de050f`，限定本地提交见Git。固定Laravel仍为`d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`，来源读取路径沿context长期参考目录。

- 首轮fmt及142项定向测试通过：配置组默认/无默认各70项、模块1项、CLI快照1项；其后仅新增测试非空字符串断言等价改写，生产实现未变，变化用例已精确重验。root输入复用回执`/tmp/openrust-config-source-order-verification/ROOT-INPUT-REUSE.json`保留18项绑定及差异。
- stage2 attempt2共有11条命令：默认/全feature严格Clippy、默认workspace（853通过/34忽略）、默认/全feature严格docs、no-default检查、MSRV1.97两组、deny与精确变化用例通过；全featureworkspace退出101，已运行部分1445通过/1失败/47忽略，不是完整工作区总数。当时失败为Redis队列attempt用例`queue_lease_lost`，该回执始终保留失败状态；后续结果单独记录如下。
- 最终逐命令回执`/tmp/openrust-config-source-order-verification/stage2/attempt2/RESULT.json`，SHA `066a58035af9325692b5d2183a6a295d3068dede902a86e76a77eaaa38324ed6`；root复核11日志哈希、18输入及PHP脚本/输出哈希，结果`ROOT-VERIFY.json`明确overall_passed=false。首轮lint失败及全feature失败原日志保留。
- PHP8.5最小探针只证明固定源码所用的SORT_NATURAL和数组顺序/形态操作，未运行完整LoadConfiguration。输出SHA `8abe00a502dc988129a9a91adc62c40783a425252dccbc0402023661d093e729`。不能用该探针关闭递归配置发现及文件名namespace映射缺口。
- 后续探针单例通过，原三项并发再次失败；具名并发探针实测两测试请求相同namespace，确认时钟命名碰撞，但不单独证明租约失败根因。临时探针已归档并移除；最小fixture保留时间成分并加入PID/原子序号，新增固定时钟回归，不改生产租约或原业务断言，不同模型窄审无发现。
- attempt4的原target默认并发4项、target严格Clippy及原完整all-featureworkspace通过，后者1642通过/0失败/108忽略。回执`/tmp/openrust-config-source-order-verification/stage2/attempt4/RESULT.json`，SHA `c4166e03738c4e506f25c7df9617c371613c6f76551373cb21f051b45e7a267b`；root复核3命令日志及19个最终输入，`ROOT-VERIFY.json`留证。本轮未复现原lease失败，不宣称生产根因已修；前两轮失败与临时探针编译错误均保留。
- 核心及integration的不同模型静态审查无未决确定发现；当前适用Rust门禁、fresh生成/MySQL/真实TCP及资源收尾均已按限定范围闭合。资源驱动v4独审无发现后，普通fresh生成原7项+安装fixture精确1项通过，7命令及42生成输入已核；真实TCP验证生产reader.toml声明序和GET→POST→GET通过，exit0/端口43188释放。回执plain-result SHA `1cfc318371e633428248efe00a1807024f8e7f834e22151005bde4492e0f6c06`、plain-tcp-result SHA `a29731630a2bf6e29e4d5cb486b553daf6f77d0f06d61c24cb3a2468bcc93f38`。MySQL8.4.13 fresh生成4命令及2auth HTTP通过，44生成输入/566源码链与测试库前后56项集合无漂移；保存的脱敏test log SHA `eb6c51d41e022830787d3dec0186f35f38fd98ce54dc9ea1849c112beee1c33d`，原始被过滤输出的SHA不能从保存日志重算。MySQL回执SHA `0ec3e72db5815ca11a7a777a16d8ce3834221ce05cad170094927a314af7bd9d`。root TCP两实例已通过，初始GET→固定写入→更新GET及第二进程恢复初值，正常exit0/43187释放；生成TCP的43188亦释放。最终STAGE3 SHA `887e49d2c4610ce5adec6204ca1debf31e707a18b0424f82cb91ee8af01f08cc`；root独立复核17日志、8清理/核查回执、566源码和42+44生成输入，ROOT-FINAL-VERIFY.json保留。未知历史缓存不删除；约40.406GB已登记target不是未登记缓存的总量，磁盘软线余量约1.13GB，生成重构建尚未排入。

## R2-R01 配置运行期原始写入（限定验收与资源收尾通过，提交见Git）

范围为同一 `ConfigRepository` 的有序 `set/set_many`、原始值与类型化投影原子发布、启动校验屏障及 App/Provider 共享句柄；`App::section` 读取当前仓储，既解析配置与资源保留启动快照。不包含持久设置、跨节点传播、资源自动重建、完整Config或signed。Figment入口此前丢失的来源初始键序保持后继缺口。

- 固定Laravel `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`；root实际重跑PHP8.5探针exit0、stderr为空、45097字节输出与golden逐字一致，SHA `aa358ee3b1bc95b52becd117242a72618af5b34a7e9e1c326af0a581d990a85d`，回执 `/tmp/openrust-config-runtime-core/php-final.json`。Rust fixture对应七案例及批量结果，静态对应不是单独的运行通过证明。
- 首批 `/tmp/openrust-config-runtime-core/verify-stage1/result.json`：fmt、默认仓储39项、App精确1项、无默认仓储39项通过；root复核日志实际数量和四源码SHA。随后App测试仅等价修正布尔断言，caller只提取路由及测试私有步骤，均有定向回归和不同模型窄复审。core生产仓储SHA `923373513d6998357671a798b8972dbabef584985f6a253e93167178d1db94ca` 未变。
- stage2目录 `/tmp/openrust-config-runtime-verification/stage2`：模块exact1、CLI快照exact1、consumer4项通过；默认/全特性严格Clippy均通过。双workspace实际845/1632项通过、0失败、34/108项ignored，root复核30/54组汇总；ignored不算真实服务。双严格docs、no-default、Rust1.97两组检查及deny亦通过。最终索引 `stage2/STAGE2-RESULT.json` 实际含14条最终有效命令，root逐项复核15输入SHA、14日志SHA及exit0；首批定向证据另列，不把条目数误写为16。原Clippy布尔断言及两处函数长度失败日志保留，不追写成首轮通过。
- core与caller独审及提取窄复审在 `/tmp/openrust-config-runtime-core/independent-review.md`、`caller-extraction-review.md` 和 `/tmp/openrust-config-runtime-callers-independent-review.md`。驱动四问题修复后，经不同模型稳定快照复审，stage3实际通过；不以旧H/T01结果代替本簇验收。两处历史缓存缺完整退役证据，未清理，活跃main缓存继续复用。

- stage3 `/tmp/openrust-config-runtime-services/STAGE3-RESULT.json` SHA `38fbf8b6c457f8c72b5fa1829d59514f15c9bf765cb44b10dc0f5adf20109019`：fresh plain7项、公开Provider fixture精确1项、MySQL8.4.13认证2项通过且无skip/ignored；同一固定Docker容器的既有凭据仅进程内注入，同端点版本与56个TestDatabase型库名前后集合一致，不归责/删除原有库。root双进程及generated plain真实GET→固定POST→GET更新通过，root第二进程默认值验证隔离；PID90821/90834/91012均正常SIGINT/join/exit0，43187/43188已释放。
- root独立核565非Markdown输入、13命令日志SHA/exit/前后输入digest、2TCP日志SHA、PID不存在与端口空闲，回执 `ROOT-VERIFY.json`。MySQL日志仅保留闭集测试事件，raw stream SHA不等同可从脱敏日志重算，root复核的是保存日志SHA。首个root只读结果读取器将TCP build误按test读取skip字段而KeyError，按显式命令类别修正后通过，未重跑服务或放宽测试判断。
- stage3复用main target约37.4008→38.2540GB，新增853.2MB、磁盘余10.341%，不新建target；源码及HEAD无漂移。生成plain/mysql及migration包已按实际归属核对回收，生成源目录<1MB暂留本簇复跑证据；不把历史未明归属缓存当作可删目标，不全清活跃main缓存。该簇是纯内存配置变化，日常真实MySQL按当前政策，PG/其余后端发布矩阵仍未关闭。

- 资源收尾 `/tmp/openrust-config-runtime-services/CLEANUP.json` SHA `aa64721e89ca9223896ace5885257fd345fdf5484e39c2a8954a8879487387f7`：三个生成包取得path package ID后dry-run；Cargo明确忽略path/version限定，因此另核同名产物均本任务归属和创建范围，实际仅49路径删除。main target由22617235456→22091071488B，实测回收526163968B；root复核2109保护项SHA、实际日志SHA及删除路径不存在，回执 `ROOT-CLEANUP-VERIFY.json`。565源码、主manifest/lock、server/CLI及框架/consumer产物不变；磁盘可用103264829440B，未触软线。保留生成源码<1MB及全部证据，由root在下次交接复核退役，不删除未知历史缓存。

<a id="c04-d10-d-命名路由重定向当前验收中未提交"></a>

## C04-D10-D 命名路由重定向（限定子范围已验收，提交见Git）

固定 framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181` 的 `Redirector.php:152–155` 对应 `Redirector::route/route_with`：默认302/空头，经同应用生成器再进入既有 `to_with`；RequestInfo捕获共享Arc，现构造器及输入/错误闪存链不变。参数仍限字符串BTreeMap与必需段；action、guest/intended、signed、missing resolver及完整L020.02/Redirector未关闭，无新依赖或ADR。保留旧锚点供已有链接使用。

- 最终证据索引 `/tmp/openrust-c04d10d-verification-v2/FINAL-HANDOFF.json`；v2 freeze SHA256 `b625c8ab433359b01b9cbfa6dbe7546f1a3be299c79f7ad7b7f18c36b1296841`，运行输入未漂移。fmt/diff、唯一模块4项、CLI精确快照1项、双严格Clippy与main11项均通过；workspace默认802 passed/0 failed/32 ignored、全特性1576 passed/0 failed/106 ignored，另含双Rustdoc、feature/deny/MSRV检查。总数按test summary聚合，不以named_passed代替；ignored不算真实服务。
- 最终server/CLI构建通过；fresh普通生成7项、MySQL8.4.13认证2项及独立公开helper1项均通过、无ignored，源码/锁稳定。真实网络无Session为3个场景、MySQL为4个场景，均accepted=true，非独立进程测试数；server正常exit0，无forced/timeout，监听及敏感文件清理完成，MySQL自有库已删除。网络与生成回执均由最终索引绑定；最终索引SHA256 `7789f286420c2e850a85313135fa3a154f413f788292863a6f9c69172b16d64d`。三个本轮generated/consumer包仅按实际package精确clean，各退出0，回收437,678,080B，主二进制/框架依赖及源码锁回执未变；清后已登记缓存36.715GB。首包检查器宽前缀误将自有plain rmeta归入框架哨兵的异常原样保留，归属修正证明及真正保护项不变由索引绑定；不把原检查异常追写成功，不称全部缓存已清理。
- v2定向目录 `/tmp/openrust-c04d10d-verification-v2/runs/d10d-v2-dir1/`；v1 lib7项、无Session2项、router2项仅按生产字节及输入影响等价限定复用，不称v2重跑。首轮模块1 passed/3 failed源于 `app.url=http://localhost:3000` 与旧请求Host `http://localhost` 的fixture预期不一致；v2修复显式fixture配置与README，生产默认及严格断言未改，原失败回执保留。不同模型v2 delta独审无blocker/should-fix/nit，原README问题已关闭。
- 本次仅命名路由重定向限定子范围已验收，提交见Git；不外推完整HTTP/Session、验证失败自动重定向或发布全后端矩阵。

<a id="c04-d10-c-请求局部视图错误共享验收中未提交"></a>

## C04-D10-C 请求局部视图错误共享（已验收，已提交784045c）

**当前限定子范围已验收，已由 `784045c` 本地提交。** 显式保留旧链接锚点，原验收/失败证据不变。范围为请求私有errors、已存Session共享对象身份/替换/老化、模板参数桥接与显式context覆盖，以及fresh生成暴露的根目标/fallback、内部forward和HEAD站点出口。HEAD只置空/drop正文，保留status/headers/extensions，不poll流、不推断或补Content-Length；不覆盖站点外层middleware短路。不关闭完整Redirector、Validator/FormRequest/CSRF、HTTP/Session或发布全后端矩阵。

最终当前输入索引 `/tmp/openrust-c04d10c-public-only-v9/final-current-input-verification.json`，SHA256 `3204fc2fa9d0ff2e238e88d0b294c39410d5c47f5a8cbcb0af6a29973403a66e`；原v9集中索引 `/tmp/openrust-c04d10c-verification-v9/v9-resource-handoff.json`。共同freeze SHA `74597b3b6b9943257bc64311a211f481e09a365ecc53aeacad9386015e29679f`，各命令/日志/源锁/二进制/清理回执由索引逐项绑定。最终源码site SHA `a7eb531a6602f57237a1a4192efc3321f13a21f082c287c2c1617e1311e0ca6e`、router SHA `a0805bbf3aa43ddd55fee5c957033799892f1936e52c86169d7d45f97ab55712`。

| 范围 | 实际通过与边界 |
| --- | --- |
| 固定上游/公开行为 | 固定framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181` 的六案Session/ViewErrorBag身份参考 `/tmp/openrust-c04d10c-php-identity-oracle.final.output.jsonl`；模板参数差分 `/tmp/openrust-c04d10c-php-arguments-correspondence.md`。Store/View对象证据不冒称完整PHP HTTP bootstrap；Rust类型/资源边界差异仍公开 |
| 定向/main | v9实际router106、view-only6、view-session HTTP7；HEADlib2、模块4、CLI快照1按v8等价提取证明限定复用，纯errors对象10按未变输入复用。默认workspace798 passed/31 ignored、全特性1571 passed/105 ignored；fmt、双严格Clippy/Rustdoc、4组feature check、deny、Rust1.97 all/session检查及新build实际通过。ignored不计真实服务 |
| fresh生成 | 普通fmt/check/test7项通过，覆盖运行时configured-application首页名称与两个空错误段；公开helper以显式上下文Generated精确断言title/h1、错误展示与老化；MySQL8.4.13模板fmt/check与认证2项通过。MySQL模板默认auth测试不调用新helper，因此另以独立公开consumer真实编译执行未改生成helper1项，actual0、0 ignored、无source drift |
| 独立public续作 | `/tmp/openrust-c04d10c-public-only-v9/runs/d10c-v9-public2/result.json` accepted=true，test71.606秒、含driver77.590秒。新fixture仅通过公开迁移初始化driver唯一owned anchor；不调用另建随机库的TestDatabase，timeout/panic亦由finally精确DROP并确认不存在。manifest SHA `568fc0142b9965b22631ab53252d46738cdac1996f6738168feb3688fd240a4b`；13项mock与401节点/feature/edge可复算摘要支撑驱动独审，不能代替真实测试。与先前auth2是两次独立隔离验证 |
| 最终真实TCP | `/tmp/openrust-c04d10c-network-v9/no-session-p7gtkos_/result.json` 与 `mysql-tgdpylg1/result.json` accepted=true、worker0、无timeout。无Session独立空包；MySQL默认/命名/custom错误、两Cookie隔离与flash独立老化；RootContext公开值HTML转义另验，不冒称网络用户错误消息注入转义。MySQL8.4.13同TCP迁移0，自有库DROP并核不存在；两服务正常退出、监听端口释放、敏感文件删除 |
| 构建/审查 | server SHA `359d07984f0a80f194c2be756d3a2d26038e87592f802155a423b50e0fcf89a4`，CLI SHA `45cd9d49e480efa5f72f95052a28761343dd6ba2d411f9ec0aa5f79e46745cae`。最终源码和驱动独审包括 `/tmp/openrust-c04d10c-root-views-extract-independent-review.md`、`/tmp/openrust-c04d10c-public-only-independent-review.md`；驱动独审两项should-fix（唯一owned anchor覆盖child终止清理、逐节点feature/依赖边可复算）均已修复并复审关闭，public2实际编译运行通过；静态审不代替行为通过 |

历史v1–v8源码、freeze、实际失败/通过回执保留原目录；v3生成首页、v4–v7路由/fixture/HEAD失败及v8 Clippy失败只证明各旧输入，未覆写或作为最终通过。v9原MySQL整体回执因resource gate仍accepted=false，旧public-test未Popen；新public2独立续作成功，不回填旧结果。v9认证target退役的 `clean_actual_exit=null` 证据限制保留，不伪造0。资源50/100GB、磁盘10/5%未放宽；3GB为同feature/configuration完整target实测支撑的经验增长估值，不是上界。最终验收时登记37.489GB、diskfree106.536GB仅该时点与已登记路径，不代表全机缓存覆盖；自有一次性consumer缓存已在无symlink/进程/句柄/锁占用确认后正常cargo clean，actual0，精确回收1,293,717,504B；源码/锁/回执/freeze/main二进制哨兵不变，回执 `/tmp/openrust-c04d10c-consumer-v9-cache-retired.json`。main与未知缓存保留。

本轮收口仅Agent状态/矩阵文档，无运行源码、模板、manifest/lock或环境命令变化；按已绑定输入复用上述有效证据，不重跑Cargo。D10-C源码与配套文档已由 `784045c` 本地提交；本次仅校准提交后的Agent状态，不改变验收输入。

## Session 测试入口整合小试点（限定验收通过）

基线 `a2befef`：三个纯 `session` feature 入口 `http_session`、`http_session_helpers`、`http_session_source_cases` 聚为 `http_session` 一个显式 target（48→46），三个源文件与全部 26 个具名用例、断言、fixture 均未删除。实际 `--list` 映射 26 项，定向 26/26；默认 workspace 789 passed/31 ignored、全特性 1546 passed/105 ignored，均 0 failed/filtered；全特性 gRPC 全组 8/8。严格 Clippy 默认/全特性、默认/全特性 `-D warnings` Rustdoc、Rust 1.97 全特性 all-target check、fmt/diff 均通过。各阶段命令、时长、源码/锁及有界日志见最终索引 `/tmp/openrust-session-test-pilot-verification-final.json`；独审结论见 `/tmp/openrust-session-test-pilot-grpc-fix-review.md`。资源有归属清理回执 `/tmp/openrust-session-test-pilot-resource-clean.json`；最终总量见索引。不具备相同缓存条件的前测，不能用本轮耗时推断加速倍数。

全特性首次运行在 `tests/server/grpc.rs` 的联合生命周期用例遇到端口占用；原 fixture 放开随机端口后再按数字重绑，存在所有权空窗。修复为两个已绑定 listener 直接移交共用 supervisor 的公开嵌入式入口，原配置绑定入口保留；新入口以传入地址为准，TLS 等配置仍生效。首轮全特性失败、首轮修后 Clippy `too_many_lines` 失败均保留；提取 HTTP ready/trace helper 后未删断言，最终 gRPC 8/8、全特性 workspace 1546/0，不能反推历史 D10-B 偶发失败的确切根因。唯一模块真实 TLS 联合监听用例在本轮全特性测试通过。独立 gRPC consumer 初次 `--locked` 因自身旧锁失败；离线同步其既有依赖闭包后，四个配置绑定联合生命周期用例 4/4 通过，根锁和依赖版本未变。旧失败回执及新 consumer 锁差异同见最终索引。

`http_session_env` 的 `current_exe` 子进程与环境隔离、`http_session_store` 真实数据库 ignored 入口、`http_session_redis_store` 服务入口，以及 HTTP/配置/扩展/Cookie 用例仍保持独立。此试点未改变 Session 生产行为，未新增 Session 专属 DB/Redis/HTTP 或生成项目验收；gRPC 实网络验证仅限上述范围。D10-B 的既有服务结果只按输入影响复用。后续入口整合仍须逐项证明独有缺陷保护，不能按 target 数量删测试。

## C04-D10-B 请求绑定重定向与 Session 错误包（限定子范围已验收）

限定提交 `a2befef`。固定上游对应 `RedirectResponse`、`Redirector`、`MessageBag` 与 Session Store；Rust 公开 `Redirector::from_info/from_request`、`RedirectResponse`、`RedirectErrors`，普通重定向默认 302 且无需 Session。借用真实 `Request` 的 `with_input/only_input/except_input` 与 `with_errors` 将输入和命名包写入唯一 Session；`ErrorBagCodecs` 经 Provider 扩展，内置包零登记，自定义包跨请求按稳定类型与版本恢复。用户编解码在 Session 锁外，锁内仅验证候选并原子发布；手动闪存不自动过滤密码。请求局部视图共享、自动验证失败重定向、完整 Redirector、CSRF/FormRequest 与上传文件输入仍待后继。

- 最终门禁索引 `/tmp/openrust-c04d10b-console-migrate-final-handoff.json` SHA256 `08ae0b6b87c2c7ed7c658c63386bdab8b7ca5cc8014f16a93bdf2ac1bc2539f6`；root 20项审计 `/tmp/openrust-c04d10b-root-final-gates-audit.json` SHA256 `f2219e88de055d93d740b7af6b446f64473855ed3662999394a77e395c5ee752`。最终严格 Clippy 默认/全特性、fmt/diff、默认/全特性 Rustdoc `-D warnings`、no-default/session-only 与 Rust 1.97 两组均 exit 0；workspace 默认 789 passed/31 ignored，全特性 1546 passed/105 ignored，均 0 failed。ignored 不算真实服务。`cargo deny check` 复用依赖与根锁未变的 原 B 回执 `/tmp/openrust-c04d10b-verify-batch2g/deny.json`：exit 0，SHA256 `e8663b39a05f14f972c7d7aeef87d66918e6f9d49d0d50f373f91887ce86a98d`；未冒称在最终源码重复执行。
- 真实生产 server 构建结果 `/tmp/openrust-c04d10b-server-build-post-tests/build-result.json` SHA256 `2804eaaa530bbb1509c13eadc1df4d06e5f81d6ebfafaee0e48cc11c406dd6e4`，二进制 SHA256 `75073a2d6b3125f9734b4c70c5e063c4cfcfacb26f2b83d46e8394ab8a1e0c12`；791文件源码快照与服务前后一致。无 Session 两项真实 TCP 结果 `/tmp/openrust-c04d6-url-network-d10b-iqviivl4/result.json` SHA256 `37aea9be9af5e0d53821703e3ff0bf36f66a1e6dfac43cc9847bd79a19d3eba9`，accepted=true，服务正常停止、端口和敏感配置清理。
- MySQL 8.4 本次随机私有库 结果 `/tmp/openrust-c04d6-url-network-d10b-db-3_fr2g9g/result.json` SHA256 `cb4eeb3a5b7e424ccce70a51fadfe5b73ec30e0b57b703e56efe15ef10dbc208`，外层命令 回执 `/tmp/openrust-c04d10b-service-final2/mysql.outer.json` SHA256 `27b4150076bb5b83f0388582fcdd84c1f38dcab74337702d53a9d7573653369d`，均通过。空库 `status→up 1→down 1→私有命名库 status→全量 migrate` 每步 exit 0、历史和目标表断言成立；TCP 五项全真、持久 Session 计数 5。删除本次库的指定 MFA 表后，同一二进制默认 `serve` 因缺表 exit 1、进程已回收且采样无监听；源码控制流支持启动失败发生在派发前，采样不等于连续系统追踪。最终精确 DROP 本次库并核不存在，配置、端口、进程清理通过。首轮 `/tmp` 与 `/private/tmp` 路径误判的 overall=false 结果原样保留，修包装器后使用新随机库重验，未回填旧结果。
- 新 CLI 模板的普通生成应用 7/7 具名通过，实际执行 redirect helper；MySQL 生成应用 2/2 为认证回归，**未**执行新 Session helper，PG 仅生成未验证。验收索引 `/tmp/openrust-c04d10b-bfinalcandidate3-acceptance-index.json` SHA256 `8ad6d488aa09f86a22f4231214db3a958ea883cca22392ed79fdf260df0ed539`，各子回执及锁/清理见索引。旧 plain 6/7 的空消息首条预期修正有固定上游依据，仅模板两处断言变更，影响分析 `/tmp/openrust-c04d10b-template-fix-source-impact.json` SHA256 `d437ec09b3e50677d48f755de64b469fa738f7212817ce27bd8b0558e24d53a2`；原连续适配器因下一 MySQL 阶段预算拒绝 exit 1，后经受审单阶段命令 exit 0，不能写成原外层成功。
- 早期三次默认 Clippy 失败、两次 gRPC 全组失败和首轮 MySQL wrapper 失败均留证；最终整组通过不等于 gRPC 偶发根因已修，诊断仅增强。真实失败迁移历史/重启恢复测试本轮未配置 `OPENRUST_TEST_DATABASE_URL`，没有实际数据库执行证据，保留后继 MySQL/R14 定向；本次正常 up/down/status 交叉不能代替故障恢复。当前日常 MySQL 通过不代表 PG/SQLite及全部承诺后端通过，发布矩阵仍开放。
- 最终已登记 11 target 与 PHP 辅助目录合计 49,819,197,440 B，磁盘空闲 14.686%；MySQL 生成阶段曾超过 50GB 软线后停止新增构建，只按归属回收已验收的一次性 MySQL 包 276,201,472 B，回执 `/tmp/openrust-c04d10b-cli-fix/mysql3-clean-actual.receipt.json`。根共享 target、源码、锁与验收输出保留。

## C04-D10-A 错误包（限定子范围已验收）

固定 Laravel framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`：MessageBag/ViewErrorBag 及公开 contract、Collection 保键/预算与同实例派发，已接唯一模块和普通生成应用。构造消息采用 String；递归合并派生 `MessageValue::Nested`，格式错误以 Laravel HandleExceptions 模式对应，裸 PHP 警告后继续的差异单列。完整 RedirectResponse、Session `errors` 与请求局部视图共享仍开放。

- 定向 Support 31、模块 TestApp 1、CLI 精确快照 1；默认 workspace 784 passed/31 ignored，全特性1526 passed/105 ignored。忽略项不算真实服务。
- fmt、默认/全特性 workspace all-targets Clippy `-D warnings`、默认/全特性 Rustdoc `-D warnings`、cargo deny、无默认 feature check、Rust1.97全特性与session-only check均实际通过；deny保留既有重复依赖诊断。
- 真实单进程非TLS loopback TCP五场景通过，外层/worker退出0、服务正常退出、端口释放、敏感配置清理及源码/二进制不漂移均已核；不代表TLS或集群验收。
- 全新生成 plain 的fmt/check/locked test通过，6个精确具名用例包含错误包/nested/数值编码；PostgreSQL18、MySQL8.4生成项目各2个认证回归通过，专属库CREATE成功后精确DROP并验证不存在。双库生成不执行D10 helper，不冒称新数据库行为。
- 失败记录保留：泛型序列化递归、预算fixture超限、CLI快照和lint均已修复并重验。每条有效命令前后摘要一致；跨门禁不宣称具备未记录的逐文件密码学差分，已知纯CHANGELOG/Agent文档变化按影响复用。plain外层退出码为正式工具响应的事后记录，缺独立持久raw外层日志；底层fmt/check/test有实际exit与具名事件，PG/My外层日志另存。
- 精确回收已验收一次性生成包，保留源码/锁/证据和活跃共享缓存；最终12个去重target合计48.866GB，磁盘15.833%，未清空根target。

证据：`/tmp/openrust-c04d10-main2-final-handoff.json` SHA256 `9f01195b291ece7952c6c35b8668aa68c078b0d8bc38f9a2b30644f1c4201a41`，root逐一复核37个path/hash；独立归集 `/tmp/openrust-c04d10-tool-evidence-audit.json`，TCP root复核 `/tmp/openrust-c04d10-root-tcp-audit.json`。本簇测试/模板/生产代码均无后续未验修改；最终收口文档不触发无关Rust矩阵重跑。

复跑：`cargo test --locked --lib support_contract_tests`，其余适用完整命令见同目录各门禁JSON。固定参考源码使用 `/Users/Home/Codes/Git/laravel-framework-baseline`，文档使用平级 `laravel-docs-baseline`；历史PHP输出与来源许可在 `tests/support/fixtures/laravel-message-bags/`，实际目录以源码为准。


## C04-D9 main 组合（限定子范围验收通过）

限定范围：Session旧输入与Request公开读写、唯一模块和生成helper；源提交36f48ec按用户授权集成。实际代码验收索引树`e9fd00b8dbb13d2af6f43389626fedb18ca22848`；607文件输入SHA256 `58bd5e81a5be10844ac869f71b9bbe77bbcdd69f58bc9d767a1f15962aad202d`。不关闭整个HTTP/Session、RedirectResponse、上传输入、CSRF或FormRequest。

| 证据 | 实际结果与限制 |
| --- | --- |
| `/tmp/openrust-c04d9-main-verify/total-evidence-index.json` | SHA256 `33ed51dca142c9534517d11dc05a79a2fe25135abd5f9879e053f0dc4af2f964`；含实际命令/退出、结果/日志与来源绑定、失败留存、资源和索引交还。root另核三fresh结果/events/具名计数及最终来源，审计 `/tmp/openrust-c04d9-main-root-final-acceptance-audit.json` SHA `8f618ea5b494906543d9eba03f358e80891d5767b7eccf3b9a700bb7c9a40a73` |
| main构建/测试门禁 | 默认workspace750 passed/31 ignored、30组、46.60秒；全特性1491 passed/105 ignored、57组、190.59秒，均0 failed/filtered。fmt、两档workspace all-targets Clippy `-D warnings`、双Rustdoc、deny、no-default/Session-only及Rust1.97两档check均exit0，源码/根锁/索引前后一致；ignored不算服务 |
| 定向/公开调用 | Session old-input helpers2、无Session Request1、唯一模块old-input1、CLI精确快照1实际通过且非零项；公开自定义Session consumer4、Container Session装饰consumer3均0 failed/ignored；模块与生成helper覆盖confirmed不保存/老化及普通attempting路径 |
| 根PostgreSQL18/MySQL8.4 | main3两库各业务目标72 passed、0 failed/ignored/filtered，old-input具名各1；所有权PG3/My2各精确1项。两库均预建不存在→CREATE成功→finally精确DROP→确认不存在；定向过滤数据实保留。PG result SHA `9d1d83c3127a1fa4ccd3ef11c3b934dc648407fa92b4025747b85c0a516868ae`，My `d84d680e19b2bf5ecd28c44bef0f5f78c7f250653be2e805ca4a74402e5aafd1` |
| 实际构建来源 | 双库后真实server全特性build与CLI build分别26.46秒/0.86秒、exit0；handoff SHA `f28347ed3294f71da0bfef8f38d97888ae4f7f9f3030a04fdd8bac5bea716122`。server SHA `da59634c976900a33e3a5082e4767c6d3dc10bbb113bc46b51d7c1e54af6ef4f`，CLI `4bb672b8b8121bb29d56a2a94890d76782cff7859a6f52caeda4ba5a92a0373b`。未用双库测试覆盖产物或旧分支binary代替 |
| 真实TCP | 修后命令exit0、worker0、六项全真、后端正常退出、端口62085释放、敏感临时文件清除，源码/binary不变。result SHA `86f159c2b134c927aa4fa14a5968f7091b1574abd5d2c767e43487440436633e`；命令退出来源在`tcp-fixed-command-exit.json`，必须同时核命令exit0与accepted。仅array单进程非TLS；handler计数由模块/生成测试补证，不冒称TCP观测了计数或集群 |
| fresh生成应用 | gen2沿本次新CLI生成的plain/PG/My实际fmt check→首check生成并固定lock→锁定test，分别5/2/2 passed，0 failed/ignored/filtered；无自动fmt掩盖模板差异，双库生成锚库均清理确认不存在。三result SHA分别 `fa2bb4fa94bfd33162e37f5bb353bfe9359f9c8cb8a253f8ee9c9e1dace61060`、`5d017fd36c87bd718917736bce08003529b00562f10513f02253383cb864f97b`、`7bf5497a2fb8a9fb54f5830edf4f1bf32849d6c9ae71e0277e0bcc40cbc1811c` |
| 独立审查 | terra对15个组合路径核12个源blob一致和3个自动组合文件；Q配置仓储、X扩展同实例、D8鉴权/previous/预检与D9路径保留。运行器来源、helper加载前SHA、预算/取消/退出、生成结果绑定修复亦有增量独审及离线正反；未以审查代实跑 |

首次TCP六项通过后因worker尾部未定义executing而exit1，result accepted=false原样保留；仅/tmp脚本初始化修正后新plan重验。首次fresh plain在Cargo前因旧prepare路径白名单失败，completed为空；gen-only新plan精确绑定原成功prepare/result/source/CLI并重验，无业务断言放宽。一次清理命令manifest路径拼写错误亦保留，未删除文件。外部邮件仍为测试路径，不增加供应商发送完成主张。

收口只补Agent事实文档与CHANGELOG；Rust/配置/模板/受测README及锁未变，按输入影响复用本轮证据。本地提交见Git。最终样本总缓存49.344GB、磁盘空闲约17.04%，无在途Cargo/rustc/rustdoc。精确回收旧QX五生成包、本轮plain/PG包及四个已完成测试可执行文件，源码/锁/索引/活跃server/CLI与共享框架库哨兵不变；Cargo关联通用测试产物有实际记录，不称仅包名文件。活跃根target保留，不清未知归属缓存。

## 持续开发守护问答与Goal绑定修复

源提交1d0614e、97c6eff已单独集成，六个技能文件与独立审查分支一致；未混入框架或D9改码。main实际运行 `/tmp/openrust-guard-main-integrated-tests.log`：87项中84通过、3原生Windows跳过，149.755秒；`/tmp/openrust-guard-main-integrated-questions.log`：51项全通过，12.112秒，启用ResourceWarning错误。移除人为小时次数等待，保留四条有界缓存；仅允许同一Goal的updated_at_ms漂移，真实身份/状态/内容/创建时间或协议不一致仍拒绝，额度状态仅做精确拼写映射。不把POSIX模拟当Windows原生验收。热重载沿用原Goal/预算/问答账本，实际进程状态在guards忽略目录。

## R2-Q/X main 组合（验收通过）

限定范围：ConfigRepository原始/批量读取和可信诊断路径、Container保留原factory的extend与公开Session装饰器。源提交Q `850e1a4`、X `d299ca4`；实际代码验收树 `4153d5b783aebd70dbf89f1af16a1f2bd208d01a`。不关闭完整配置/Container/Session、Provider发现或集群。

| 证据 | 实际结果与边界 |
| --- | --- |
| `/tmp/openrust-r2qx-main-verify/build-stage-handoff.json` | SHA256 `006e27d41a8332b197f391c3049e8aafb7604607081221766e6e4ee23e5711b7`；25份实际执行记录逐一核exit/日志/前后树及根锁。默认workspace750 passed/31 ignored，全特性1482/105 ignored；两档严格Clippy/Rustdoc、deny、no-default、Session-only、Rust1.97全特性/minimal通过。ignored不算真实服务 |
| `fmt-final-schema-any.json`（同目录） | 最终fixture修改后fmt check实际exit0，前后树4153d5b。配置33、extend24、示例2、公开Q consumer3/X consumer2及Session组3、既有Container32/Provider10、D8 Auth10/Session HTTP14/helper11均有实际结果 |
| `/tmp/openrust-r2qx-main-main-live3-acceptance-index.json` | SHA256 `98ff4ca0c0bd2a1bf02394b7fe8a56eb8296ff9e19f21d6135dbbe1002e7c9e2`；7个双库/生成stage、3个plan与TCP父/子证据已逐一核SHA。运行源码快照 `a2d056b49374ad40fd8e87010b495288be1e1b9c2015f5682a23b55648474be2`，所有结果source_changed为空 |
| 根真实PG18/MySQL8.4 | main-live1两库各完整模块/认证目标70 passed、0 failed/ignored/filtered，另previous1/Q1/X2具名通过；两库均预建不存在→本次CREATE→DROP→确认不存在。定向filter数分别67/67/66，如实保留；不把未调用的外部供应商当完成 |
| 真实TCP | main-live2 Q/X两项与D8六项精确场景通过；Q/X正常退出、敏感配置清除与lsof端口释放已核。现有D8 worker未报告端口，明确 `not_observed_by_existing_D8_worker`，不冒称该段直接端口观测；不代表TLS或集群 |
| fresh生成项目 | main-live3 q/x/plain/PG/My分别6/6/5/2/2 passed，0 failed/ignored/filtered；各项目绑定CLI、源码、manifest和lock，PG/My专属库清理后确认不存在。Q/X仅在fixture安装后显式fmt并冻结baseline，再严格check；原始plain/PG/My模板只fmt check，未自动格式化掩盖模板问题 |
| 实际构建来源 | 最终handoff `/tmp/openrust-r2qx-main-build/build-handoff.json` SHA256 `e3012c4776d16cd6e35e0dce8b7d52fce6c54615dc6bccb564221f84b5d41272`。server-post-db实际all-features build exit0，server SHA `a472a9b19a2717a9670fb9fb171059f17d3cf76f39b5e61e58bbe66cca05fa47`；CLI实际build SHA `8394f62256dcb3ec2fe26a999b36415ef7d420a597a6c0a8827b87021aff396e`。607文件及日志/binary均核验；双库测试覆盖server后先重build才TCP，未借旧副产物绑定 |
| 独立审查 | terra对组合Provider/路由、错误脱敏、fixture、执行器所有权/具名/lock/停机边界复审；相关问题修正。`plugin_contract`最终9/9通过，无schema只报可信父键，显式schema保留可信字段，均不泄漏秘密 |

修前Clippy、旧consumer断言、首次schema fixture错误记录保留；main-live1 TCP runner误捕获正常SystemExit、main-live2 Q fixture未格式化两次失败也保留，不回填成功。后续仅改/tmp执行器并新prepare/fresh目录；未改变产品行为，不重跑无影响的已通过双库。数据库连接或CREATE结果不确定时仍可能留下锚库，绝不猜测所有权删除。

验收后仅补Agent事实文档及CHANGELOG，代码/配置/模板/锁/受测README未改，按输入影响复用本轮证据。本地集成提交见Git。最终缓存48.162GB、磁盘空闲16.823%，无在途构建或服务；活跃缓存保留，生成专属再生产物先精确审查再回收，不清整target。

## C04-D8 previous 与 Precognition（限定子范围已验收）

本簇实现见 历史决策 084 与 [HTTP](../core/http.md)、[Session](../core/session.md)。不关闭整个HTTP/Session、完整FormRequest或Laravel验证规则集。

| 当前证据 | 实际范围与限制 |
| --- | --- |
| `/tmp/openrust-c04d8-evidence-index.json` | 最终索引SHA256 `c28e7a98fe8662c5d7ac42bbf51366dc77482fb745a1fe38db9e2a897319ce46`；主线门禁、双库、TCP、gen2及修后CLI快照引用均逐项核hash，pending为空 |
| `full-main-final3b/3c/3d`各result、gRPC精确result | 稳定runtime快照`679ecddd4fc2628c73412036997d782e44a178ba89561abfd633e10d88a4299e`；fmt、默认/全特性严格Clippy、workspace全特性1439 passed/104 ignored、精确gRPC1项、两档Rustdoc、no-default/session-only、deny、server build均实际通过；ignored或缺URL返回不算实服务 |
| 默认workspace与修后doctest | 原命令709 passed/1 README doctest failed/28 ignored，不能记单次默认门禁exit0；修后doctest20 passed/27 ignored，按实际输入和默认feature未覆盖DB/gRPC的影响分析复用其他已通过测试 |
| `/tmp/openrust-c04d8-msrv-final1.result.json` | Rust1.97全特性全目标check实际exit0，运行6分09秒、源码无漂移；仅工具链check；一次性独立target已安全回收 |
| `/tmp/openrust-c04d8-root-owned-final-owned1-index.json` | PG18/MySQL8.4各模块HTTP67项及server previous精确1项通过；清理所有权PG3/My2适用项各具名通过，无适用skip/ignored；exact过滤如实记录，整业务target0filtered。两个owned anchor均finally精确DROP并回查不存在 |
| `/tmp/openrust-c04d8-tcp-final-net-session1-seal.json` | SHA256 `99bf56001a091abe50cdbb94c7cbcc9c6e8a806c1f3b0cc6d482acdb29d7a936`；启用session的明确构建SHA `2c4c1b014788f4c4fa5946628b160cd47d7097fb5d505456107d6e36f28df6e1`与原runtime快照绑定，真实TCP6/6通过。仅array单进程、非TLS/DB/集群；default feature未含session的首次失败不作通过 |
| 新生成应用 | gen1双库Cookie fixture缺Host的失败及来源保留；仅补Host并审阅快照后，gen2三个全新目录与成功prepare/CLI SHA/应用源码快照绑定，普通5/PG2/My2项fmt/check/test均通过，适用无skip，两生成库finally回查不存在。最终Cookie改后CLI精确快照1项实际通过，结果 `/tmp/openrust-c04d8-cli-cookie-final.result.json`（SHA `c63226746f287480056f9463e6322cf979eedb655694b1fe26d349e9b5e0a67a`），不借用改动前旧结果 |

Cookie模板与受控快照修复相对上述runtime快照只改这两个CLI生成输入；不改变框架Rust行为。生成验收必须用新CLI/全新应用及prepare来源绑定重新取得证据，其他真实双库和TCP结果按输入影响复用。测试建库失败清理只针对本次已成功CREATE的目标；取消、连接中断或DROP失败仍可能留下自有资源。


> 受众：AI Agent
> 更新：2026-10-30
> 摘要：当前目录迁移与原专项测试入口，分开记录真实服务和未验收边界。

## C04-D7 真实路由上下文（限定子范围已验收）

按历史决策 083交付受管理RouteMethods、真实Route/参数/名称通配与可信应用Resolver；全局前后、路由中间件/短路、错误、domain候选、group/nest/merge/forward、GET/HEAD/QUERY/custom/any、OPTIONS/fallback/静态与请求隔离均有真实生产回归。完整Routing、可选参数/声明defaults/约束/模型与枚举绑定、previous/Precognition不由本簇关闭。

| 最终证据 | 实际结果 |
| --- | --- |
| `/tmp/openrust-c04d7-full-final3-*.result.json` | 12/12实际退出0、来源无漂移：fmt、双workspace all-targets Clippy `-D warnings`、双workspace tests、双Rustdoc `-D warnings`、no-default、Session-only、MSRV1.97、deny、server-build。默认695 passed/29 ignored，全特性1403/103；各8个required具名通过。ignored不证明真实服务 |
| `/tmp/openrust-c04d6-url-network-jch137z7/result.json` | D7真实TCP7场景含具名/匿名、参数/default、query/body伪造、业务404、路由403、原始HEAD至EOF零正文及前后标记、PUT无路由middleware；accepted、正常exit0、未强杀、清理无错误。沿D6监管器目录前缀，不沿用D6通过结果；非TLS/DB证明，PUT slot为空由其他回归证明 |
| `/tmp/openrust-c04d7-final3-root-{pg,mysql}-driver.log` | PostgreSQL18/MySQL8.4根模块各64个具名通过，0失败/ignored/filtered/skip，含真实路由上下文；两库precreate_absent、DROP及isolation_check verified_absent完整保留 |
| `/tmp/openrust-c04d7-final3-evidence-index.json` | SHA256 `7122f6fc90d9314176ae4b412462e68c70ae8761e837937a03433a3b6ab27d85`；20结果、105artifacts、40快照、4生成JSONL和8完整driver均独立核对；739受控文件同一freeze |
| 索引中的final3新生成应用结果及JSONL | 三个新项目实际fmt/check/test，strict具名5/2/2、无忽略/过滤/skip；请求helper与模板相同，双库创建前/清理后不存在；不是旧preflight3目录复用 |
| `/tmp/openrust-c04d7-final-cleanup-seal.json` | SHA256 `ad638a0554e35eceba1eb288ab8471d12ffd2522680a2d21fd5041c6681169eb`；根allocated 24,731,361,280 bytes→0；probe与6份任务临时生成目录已核身份/哈希/无.git/无符号链接/无占用后移除；5旧consumer全树hash和大小不变（2,670,026,752 bytes），磁盘可用17.724%，无后台构建 |

网络执行二进制SHA256 `cda383afd3d86fe43c7a2918b4da62b2ceeaa047345f54db186877fbdcced115`，构建before快照摘要`5e90878aa4fa950305e971c91c0d10cd677a54d2786d2f05b0871b60144fa1f1`；网络执行期间不变，随后其他feature构建合法覆盖target/debug/server，索引分开记录，不假称磁盘当前二进制仍相同。清理后不重建，保留全部证据。

实际资源清理区分：先执行字面`cargo clean --dry-run`；在Cargo兼容锁下清根产物，再实际执行字面`cargo clean`移除剩余2文件/352 bytes。不会将25GiB清理量全部归于最后一条命令。6份生成目录的清单为交接时当前快照，结合prepare拒已存在路径、真实创建/验证记录及owner交接确认，无持久创建时全树manifest的事实保持；旧consumer、D6目录、旧B2 PHP runtime与105份证据未动。

失败与修复事实保留：§5三次路由整组失败后，用户明确选择修复精确前缀分派后重验；approved1精确1/1、路由94/94通过。普通路径非法UTF8、fallback空捕获和scope尾斜线已修；final1资源根子路径404回归最小修复后资源16/16/路由94/94通过；final2旧Provider重复路由错误断言改成真实HTTP后注册覆盖，测试返回错误类型编译问题亦修正。final3完整通过。默认Clippy前两轮11项和2项测试lint已修，第三轮严格通过，没有删除业务断言、降低`-D warnings`或跳过目标。

旧部分验收保留在`/tmp/openrust-c04d7-preflight3-independent-evidence-index-v2.json`（`99141a96b89c3df6544e4cf45abab4ff21736d668b51a1db1fae314a18971fda`）；只证明当时源码且根清理尾stdout缺失，不代替final3。最终final3已完整封存根清理检查，旧证据缺口没有通过补造成功行消除。不同模型源码与实证复审均无blocker/should-fix。最终收尾仅改项目文档，不改已验收运行源码；限定本地集成提交 `9035a00`，不push/tag/release。

## C04-D6 Request URL（限定子范围已验收）

公开API与差异见历史决策 078。本簇覆盖Info的请求origin、URL、解码路径/分段/星号匹配/prefetch，以及唯一Request当前query增删；原始OriginalUri与GET/HEAD当前输入分开，JSON输入优先于HTTP方法。未关闭真实路由命中、previous、Precognition、TrustProxies完整转发Host/Port/Prefix。

| 证据 | 实际结果与范围 |
| --- | --- |
| `/tmp/openrust-c04d6-full-final2-{fmt,clippy-default,clippy-all}.result.json` | fmt、默认/全特性workspace all-targets Clippy严格`-D warnings`通过 |
| `/tmp/openrust-c04d6-full-services1-tests-default.result.json` | workspace默认686 passed/29 ignored，0 failed；ignored不算真实服务 |
| `/tmp/openrust-c04d6-full-grpc-recheck1-tests-all.result.json` | workspace全特性1393 passed/103 ignored，0 failed；整套包含新增PHP对照及唯一模块URL用例 |
| `/tmp/openrust-c04d6-full-depwait1-{docs-default,docs-all,no-default,session-only,msrv,deny}.result.json` | 默认/全特性Rustdoc `-D warnings`、无默认feature、Session-only、MSRV1.97及cargo deny通过 |
| `/tmp/openrust-c04d6-full-networkfix1-server-build.result.json` | 当前server成功构建，全Git tracked/untracked构建前后快照相同；binary SHA与真实网络使用一致 |
| `/tmp/openrust-c04d6-url-network-sz4l6j00/result.json` | accepted=true，10场景全通过，source_changed=[]、binary_unchanged=true、两个后端exit0/forced=false、cleanup_errors=[]；显式CA/hostname TLS1.3，仅证明Python代理终止TLS，经受信peer/proto接入Rust |
| `/tmp/openrust-c04d6-root-first-{pg,mysql}.result.json`及safe.log | PostgreSQL18/MySQL8.4各63 passed；精确9个required名含URL用例，0 failed/ignored/filtered/skipped；锚库创建前不存在，最终drop后再次不存在 |
| `/tmp/openrust-c04d6-generated-fresh1-verify-{plain,pg,mysql}.result.json` | 三个全新生成项目实际fmt/check/test；strict具名5/2/2，0 failed/ignored/filtered/skipped、exact guard=true，双库锚库均最终不存在 |
| `/tmp/openrust-c04d6-final-handoff.json` | SHA256 `ec2deccceb981e0a1a085fd13965c13b525d90ad5a797a4239eeb0d29cb5ef60`；500文件guard及结果/日志哈希交接，独立审查无blocker/should-fix |

实际新生成目录为`/tmp/openrust-c04d6-generated-{plain,auth-postgres,auth-mysql}`，不是旧D1/D5项目；三份request_input helper均与当前模板逐字节一致（SHA256 `43074067dcb7161c26b03937114afe2fe0f1ad1d662559956d57d4e7fae91ff9`），真实调用普通GET、JSON GET与JSON POST三类URL变更断言。两线Symfony固定源码与真实PHP8.5各198 canonical+7操作参考，Rust逐ID全对照；非法UTF-8两项单列400适配，raw NUL仅私有helper，不冒充HTTP wire允许。复跑源码与来源在[fixture](../../../vendor/openrust/framework/tests/http/fixtures/laravel-request-url/README.md)。

失败与纠正保留边界：

- 首次模块失败因fixture给GET统一加JSON头；按固定Laravel优先JSON来源，拆三case而非改框架或放宽普通GET page3/replace4断言。
- 真实PHP golden首次揭露fixed-020嵌套Cookie前缀过滤偏差；撤销UrlQuery模式、复用原Input变量树，php_variables.rs对HEAD无diff，两份golden期望未变。修后29项Request定向与完整workspace均通过。
- 库Clippy首次文档/切片lint、完整all-targets测试断言/路由长度lint均已修并按原严格门禁通过；没有跳过测试或CI。
- Redis服务短时消失使默认CLI真实复用测试失败；在既有共享服务恢复后原断言通过，未由本任务新建或重配置容器。
- 全特性首次gRPC readiness在5秒期限失败；精确单例原期限通过后整套1393通过。不宣称根因已修；源码可触发的临时端口释放竞争/启动任务错误被就绪轮询掩盖仍需后继诊断整改。
- 网络首轮仅到业务ready后，独占bind未listen负例在macOS产生TimeoutError；两次真实5.003秒探针确认后，仅明确接受拒绝或超时、保意外连通必失败，并记录类别。最终实录TimeoutError约5.001秒，不冒称精确ECONNREFUSED。TLS/业务断言均未削弱。

12门禁各次actual_exit=0/source_changed=[]；早期门禁后唯一运行文件变化为已审网络负例平台修复，Rust/模板/golden未变，最终serverbuild与network使用新完整快照。验收后仅文档收口另执行diff/链接检查，不冒称重跑Cargo。资源收尾：锁兼容probe与dry-run通过，锁内回收root target 20,511,793,152 bytes；随后实际执行`cargo clean --dry-run`及`cargo clean`均exit0（`/tmp/openrust-c04d6-cargo-clean-cli.result.json`），root target最终0。五个旧consumer缓存文件树hash/bytes保持不变，总target约2.486656GiB；本任务三fresh目录与新PHP源码树已按精确owner清理，旧B2 PHP runtime、旧D5目录、所有D6证据及三共享容器均保留；无构建/服务遗留。最后CLI清理检查磁盘余量约17.774%。批量清理日志`/tmp/openrust-c04d6-final-clean.log`与临时资源结果`/tmp/openrust-c04d6-temp-clean.result.json`实际通过，不把“计划清理”当完成。

## C04-D5 GC lottery（限定子范围已验收）

公开行为与固定源码SHA见历史决策 082 D5。默认`[2,100]`在start后、业务前调用实际请求驱动的GC；DB/Memory按已持久绝对expiry清理，Cookie/Redis保持noop/TTL职责。与Laravel当前lifetime回溯last_activity、Array严格小于的安全差异仍公开，不以本簇关闭完整Session。

| 范围 | 实际结果 | 证据与边界 |
| --- | --- | --- |
| 最终12项门禁 | final4全部actual_exit=0、source_changed为空，根逐项核日志SHA；默认668 passed/29 ignored，全特性1374 passed/103 ignored | `/tmp/openrust-c04d-d5-final4-<stage>.result.json`；fmt、默认/全特性严格Clippy/tests/docs、no-default、session-only、MSRV1.97、deny及server build。忽略和成功输出捕获不证明真实服务 |
| 核心与公开测试 | Session-only库421；config8/extensions15/HTTP5共28，均0失败/忽略/过滤、exit0/sourceguard空 | `/tmp/openrust-c04d5-indexed-core-unit.result.json`、`/tmp/openrust-c04d5-first-gc-public.result.json`；含确定性抽签、实际bound420秒、顺序、错误/取消清队列；后续机械语法修复另由final4验证 |
| 外部driver consumer | unit2/Cookie1/extensions1共4 passed，0失败/忽略/过滤、exit0/sourceguard空 | `/tmp/openrust-c04d5-first-consumer-test.result.json`；真实Memory GC委托/长Expiry保留/Update Missing及Cookie noop，不代替网络或数据库 |
| CLI模板快照 | 精确1 passed、19 filtered、0失败/忽略、exit0/sourceguard空 | `/tmp/openrust-c04d5-second-cli-snapshot.result.json`；11hunk逐项核模板后更新。最终gc生命周期改写的单行同步随后由final4 workspace tests与新生成实际验证，不伪称旧快照为新实跑 |
| 双库Store/GC专项 | PG18/MySQL8.4各严格具名2/2，accepted/exit0，无失败/忽略/过滤/跳过，sourceguard空；自有锚库均已核absent | `/tmp/openrust-c04d5-backend-{pg,mysql}.result.json`及safe.log；旧原子精度/撤销、新GC绝对期限/同秒微秒、旧表索引升级/同名异列冲突/down-up/存活数据、事务持锁续期竞争。stage外层3600秒、SQL/清理30秒；单个commit/close无额外独立deadline，不声称库API提供该超时 |
| Redis8真实Store/Auth | Store严格具名1/1、Auth10/10，accepted/exit0，无失败/忽略/过滤/跳过，sourceguard空 | `/tmp/openrust-c04d5-redis-{store,auth}.safe.log`；gc返回0后跨连接读取，原TTL/撤销/防复活链保留；不扫描全库或改变共享服务，不代表集群 |
| 根双库完整目标 | root3各62/62、八项required具名齐全，accepted/exit0，无失败/忽略/过滤/跳过，sourceguard空；锚库均已核absent | `/tmp/openrust-c04d5-root3-{pg,mysql}.result.json`及safe.log；原Auth真实TCP内含满中GC与解密回执/双client非空业务，另核精确索引down-up/完整迁移历史。测试Mailer不是供应商发送，组合进程不是集群 |
| 三类全新生成项目 | plain5/PG2/MySQL2，实际fmt/check/test均通过，exact guard/无失败忽略过滤跳过/sourceguard空，双库锚库均核absent | `/tmp/openrust-c04d1-generated-d5-fresh-verify-{plain,pg,mysql}.result.json`及各jsonl；D1是复用脚本前缀，实际新D5项目。覆盖最终helper、原宿主接线与双库迁移；摘要内容相同不等于复用旧执行，项目目录与源快照另核 |
| 独立审查与源码 | 不同模型审核心12文件、叶/真实backend/索引回滚/TCP夹具；发现的加密回执/索引catalog误报已修并复审。最终489文件guard及28个变更运行文件SHA与磁盘相符 | `/tmp/openrust-c04d5-final-handoff.json` SHA93f719830014a007153330e4f41f055eec0d6d479c7012691d0741f7d7202084；根另核33个artifact哈希。私有helper参数、生命周期/unit模式与类型化空Vec仅机械改写，final4与新生成使用最终快照 |

资源收尾：`/tmp/openrust-c04d5-final-clean.log`。持已验证锁并核无其他使用者后，root可再生产物23,677,386,752→0 bytes，保锁inode；五旧consumer逐项不变，总target2.486656GiB，剩余118.634468GiB/12.806633%，post_users为空。根恢复时另查无Cargo/rustc/rustdoc/server，并实际du核root0及五旧consumer相符。生成源码与通过/失败证据保留，不清他人资源。本簇无新增依赖；限定提交见Git。

失败与修正边界：
- 独立审查发现raw Set-Cookie GC回执在加密宿主不可明文断言，改为下一请求的公开解密视图；保真实Cookie/GC委托、完整envelope和非空双client断言，不使用except/plain绕过。索引catalog收紧为有效、非部分、非唯一BTREE且恰好单列expires_at，避免复合索引假绿。
- CLI首轮101/source_changed仅自有.snap.new；完整核11hunk后限定更新。根PG首轮编译E0308/0tests，registration两helper调用只补10处借用，根逆替换SHA回到独审版本。root2实际62passed但runner写错`registration`模块名而accepted=false；实际`mod tests`经test list与7个离线pump样本确认，仅修runner后root3双库严格通过。原`/tmp/openrust-c04d5-root{,2}-pg.result.json`保持false，不回填。
- final1全特性Clippy报私有helper unused参数；final2报可省略生命周期/unit模式/空集合断言；final3改写`[]`后E0282，三轮均原exit101/sourceguard空记录保留。按§5暂停并获用户明确“明确类型后定向重验”，仅两处改`Vec::<String>::new()`，根逆替换SHA精确回到826deaf4…；`/tmp/openrust-c04d5-approved-clippy-all.result.json`单次获准重验实际0/sourceguard空，之后final4完整通过。未加allow或降低`-D warnings`。

本簇新增的是请求级GC，不新增TLS/浏览器验收主张；真实TCP来自正式根应用fixture，D4 TLS/浏览器只保其原证据。previous URL/route、真实Precognition、Old Input/CSRF、blocking、Session cache与其余driver仍按依赖推进。

## C04-D4 内置Cookie Session driver（限定子范围已验收）

契约见历史决策 082 D4。以下均为本簇实际运行；忽略项不算真实服务，代理TLS不算Rust原生TLS或集群。本簇完成限定子范围，不关闭整个Session或已公开差异。

| 范围 | 实际结果 | 证据与边界 |
| --- | --- | --- |
| 最终完整12项门禁 | final3全部actual_exit=0、source_changed为空、日志SHA经根核对；默认668 passed/29 ignored，全特性1367 passed/102 ignored | `/tmp/openrust-c04d-d4-final3-<stage>.result.json`；fmt、默认/全特性严格Clippy/tests/docs、no-default、session-only、MSRV1.97、deny及server build。涵最终认证模板9ff0d16与仓内浏览器脚本7531ee4；final2同计数旧快照保留，不混用 |
| 当前公开行为与consumer | session-only五target共43 passed；auth,encryption完整Cookie target9 passed；独立consumer3 passed，各退出0、无失败/忽略/过滤/源码漂移，根核日志SHA | `/tmp/openrust-c04d4-current-cookie-{public,all}.result.json`及`/tmp/openrust-c04d4-current-consumer-test.result.json`；公开wrapper实际调用driver，不是网络或浏览器证据 |
| 核心安全定向 | Auth34 passed/438 filtered；摘要修后Cookie3 passed/412 filtered，均退出0、无失败/忽略/源码漂移，根核日志SHA | `/tmp/openrust-c04d4-protection2-auth-unit.result.json`及`/tmp/openrust-c04d4-proof-cookie-unit.result.json`；真实绑定handler、保护降级拒绝、预排队列隔离、写后覆盖拒绝/恢复/forget |
| 敏感错误缓存红绿 | 原精确用例0 passed/1 failed、exit101；修后同一用例1 passed/0 failed、exit0，各8 filtered、源码无漂移，根核日志SHA | `/tmp/openrust-c04d4-{red,green}-cookie-cache.result.json`；原实现缺no-store，修后Cookie/Session自身错误不继承旧正文/Cookie/长度，保原错误与脱敏语义；不倒推PG首失败动态归因 |
| 根双库无加密安全负例 | PG18/MySQL8.4各accepted=true、exit0、严格具名1 passed、0 failed/ignored/skipped、71 filtered、源码无漂移；两锚库清理已核absent | `/tmp/openrust-c04d4-root-plain4-{pg,mysql}.result.json`及safe.log；匿名业务200，登录与伪造read/write均500、正确code/no-store/消息脱敏。PG为用户获准的读取器修后单次重验，旧accepted=false不回填 |
| 根双库完整加密模块 | 两库各accepted=true、exit0、62 passed、0 failed/ignored/filtered/skipped、源码无漂移；锚库均清理并核absent | `/tmp/openrust-c04d4-root2-{pg,mysql}.result.json`及safe.log；含受保护Auth、plain/ID排除/数据ID排除四具名场景及既有链，MODE3通过真实Cookie层签发合法ID，不放宽严格拒绝断言 |
| 正式原宿主接线 | 5 passed、45 filtered、0 failed/ignored，exit0、源码无漂移、根核日志SHA | `/tmp/openrust-c04d4-exact-host-session.result.json`；默认无DB404、原Provider显式Cookie非空双Cookie往返、late overlay，不由测试helper假装安装Provider |
| CLI快照 | 精确1 passed/19 filtered、0 failed/ignored、exit0、源码无漂移 | `/tmp/openrust-c04d4-second-cli-snapshot.result.json`；旧snapshot差异完整核对后限定更新，.snap.new移除；后改认证模板由最终workspace与新生成项目另验 |
| 三类全新生成应用 | plain5/PG2/MySQL2，实际fmt/check及test均通过，无失败/忽略/过滤/跳过、源码无漂移；双库锚库清理并核absent | `/tmp/openrust-c04d1-generated-d4-fresh-verify-plain.result.json`、`/tmp/openrust-c04d1-generated-d4-fresh2-verify-{pg,mysql}.result.json`与`/tmp/openrust-c04d4-generated-plain.safe.log`、`/tmp/openrust-c04d4-generated2-{pg,mysql}.safe.log`；D1为复用脚本路径，确为本簇新应用；测试Mailer不算供应商发送 |
| 真实TLS网络首轮 | accepted=true、源码无漂移、cleanup_errors空、三服务均exit0 | `/tmp/openrust-c04d4-cookie-network-1qea_dos/result.json`；显式CA/hostname验证TLS1.3终止代理链、Secure/不可信forwarded、旧key读/当前key写，不是原生TLS listener或集群 |
| 最终网络与浏览器联跑 | accepted=true、源码无漂移、cleanup_errors空、三后端退出码均0；根实际执行仓内浏览器CLI后发完成信号，原network句柄终态0 | `/tmp/openrust-c04d4-cookie-network-d8opmsxc/result.json`与`/tmp/openrust-c04d4-cookie-network2.safe.log`；最终源码再次建真实HTTP/TLS窗口，浏览器结果独立列下行，不用原首轮代替 |
| 真实HTTP浏览器 | 仓内CLI实际exit0、4/4检查accepted=true，Chromium153.0.8010.12，无手动Cookie注入，浏览器与上下文有界关闭 | `/tmp/openrust-c04d4-browser-repository-final/result.json`；脚本`modules/example/tests/browser_cookie_session.py` SHA7531ee453ce998b0b733b2c5b34491857ee7954355ccc7c550060120ea83ce7e；自动双Cookie往返、两context三轮非空隔离/新page、opaque小载荷、HttpOnly/JS隐藏SetCookie。首次独立stdin探针`browser-ves4_s9c`也通过，但不替代仓内CLI实跑；浏览器仅HTTP，不声称浏览器TLS |
| 不同模型独立审查 | 读取顺序、队列覆盖、Auth保护和宿主/生成器接线缺陷均修正后复审通过，最终认证模板两行窄审通过 | gpt-6-sol只读审查不代替实跑。对应最终模板SHA9ff0d16f57703ee2dcc8aea8457505b5879278f99fe5a6738ab7b7d02b9bfb67；源码哈希及各阶段guard在各结果中保留 |

资源收尾：`/tmp/openrust-c04d4-final-clean.safe.log`，锁/probe/lsof确认无其他使用者后清本簇root可再生产物并保锁inode；root target 24,429,355,008→0 bytes，五旧consumer逐项不变，总target2.486656GiB，磁盘剩余118.751392GiB/12.819255%，post_users为空。最终验收交接时489文件sourceguard与磁盘逐SHA一致；guard释放后仅更新来源JSON的验收状态/证据元数据与Agent文档，未修改运行源码/断言/模板，无遗留Cargo/rustc/rustdoc/server或本任务runner；handoff `/tmp/openrust-c04d4-final-handoff.json` SHA81701c22e912610f756592aed6eb1b96b7d1deee4da81d537771d78b40c204a5。只清自有可再生产物，保留生成源码、失败和通过证据。

公开测试首轮实际退出101、源码无漂移，根已核日志SHA：旧Session 10 passed，新Cookie 5 passed/2 failed，后续target因Cargo提前退出尚未执行。证据`/tmp/openrust-c04d4-initial-cookie-public.result.json`。两项fixture错误分别是直接解析`Cookie::from_header`保留的百分编码值，以及将Symfony删除wire错误断为None/空串；修正须经公开读取解码，并严格核对同名、Max-Age=0及过去Expires，不放宽原删除/载荷意图。该轮不能称公开测试全通过。

独立模块审查发现Cookie Auth保护配置阻断：`auth`不蕴含`encryption`，宿主无该feature时选择plain Cookie，客户端快照的身份属性不能因此被信任。实现已在实际Session认证入口与最终save加入可信保护校验，18文件独立审查及修后核心定向通过；普通plain Cookie业务不等于可信认证，不能仅在fixture加cfg隐藏生产组合。修复、伪造负例与受保护真实双库正例通过前，Cookie Auth保持未验收。

Auth保护首轮定向因新增测试把owned Option<Value>误写为引用而编译失败，原断言值保持修正后第二轮通过。无加密PG首次runner选择了required-features包含encryption的旧target，Cargo退出101且0项测试；该次不是安全负例通过，精确隔离锚库已清并核不存在。证据`/tmp/openrust-c04d4-root-plain-pg.result.json`，后续改用公开server集成target真实重验，不修改manifest放宽门禁。

plain3-PG修后真实命令退出0，具名负例实际1 passed、0 failed/ignored、69 filtered、无skip/源码漂移；匿名put/read为200，login与伪造read/write均500、session_cookie_protection_required、no-store且消息脱敏，精确隔离锚库已清并核不存在。但nocapture诊断打断测试具名结果行，读取器named_passed为空，`/tmp/openrust-c04d4-root-plain3-pg.result.json`仍accepted=false，不伪补通过。按§5暂停后，用户已明确批准修读取器并离线验证后单次精确重验；修正读取器7个离线样本及不同模型静审已通过，总源码冻结后获准的plain4-PG单次重验已实际通过，具名/拒绝/清理证据如上；旧plain3 accepted=false保留，不回填旧记录。其他适用验收仍按范围实际执行。

完整加密PG首轮61 passed/1 failed（无skip、源码无漂移），唯一data-ID-except场景把仍需加密的ID也按明文发送，被Cookie层先拒为401，未到目标保护校验；正例/plain/ID-except已具名通过，整组仍accepted=false。精确锚库已清并核不存在。MODE3夹具现用专属测试路由经真实Cookie层签发合法ID，仅篡改被排除保护的数据Cookie；保严格500/code/CSRF/no-store，不接受401放宽。fixture修后独立增量审查及root2完整加密双库各62项均已通过；最初失败记录保持原accepted=false，不改写。

认证生成PG首轮实际fmt/check通过、认证测试1 passed/1 failed，严格Cookie计数断言失败；隔离锚库已核absent、源码无漂移。模板夹具用无数据库的默认配置构建Provider，而实际figment随后才给隔离数据库，导致漏装Session。修复仅从同一figment提取`bootstrap::Configuration`并传给`ProviderBuilder::for_application`，原Cookie、HTTP、数据库与清理断言不变；不同模型窄审通过（模板SHA `9ff0d16f57703ee2dcc8aea8457505b5879278f99fe5a6738ab7b7d02b9bfb67`）。旧失败证据保留，修后已由CLI重新生成PG/MySQL应用并完成实跑，不手补生成物，不以静态审查替代实跑。

四份固定官方源码哈希见[来源记录](../../../vendor/openrust/framework/tests/http/fixtures/laravel-session/provenance.json)。PHP读取表达式探针不是完整官方类或Laravel请求链oracle；异常弱类型差异保持开放。客户端Cookie重放/无全局撤销、秒精度与浏览器容量边界分别记录，不声称整个Session能力完成。

## C04-D3 Session扩展生命周期（限定子范围已验收）

契约见历史决策 082 D3。D2的3cabae2不继承为D3通过；完整Cookie driver另列紧邻D4。

| 范围 | 实际结果 | 证据与边界 |
| --- | --- | --- |
| 最低公开行为回归 | 第四轮extensions12/旧session10/helpers9/HTTP5共36项全通过，0 failed/ignored/filtered，退出0、源码无漂移 | `/tmp/openrust-c04d3-fourth-extensions.result.json`；含数字Cookie精确名、hook busy、pending destroy、bind/start失败不发Cookie且保原500/code/脱敏message。该轮guard包含server实际依赖模块 |
| 外部自定义驱动consumer | 独立manifest check/test退出0，旧unit1+新hook集成1，共2 passed，无失败/忽略/过滤、源码无漂移 | `/tmp/openrust-c04d3-first-consumer-{check,test}.result.json`；对应快照，不代替后续middleware变更或完整门禁 |
| 最终外部consumer | 最终源码check/test均退出0、源码无漂移，旧unit1+新hook1均通过 | `/tmp/openrust-c04d3-final-consumer-{check,test}.result.json`；与早期快照单独记录 |
| 唯一模块定向 | 非空双client补测后4具名通过，43 filtered、0 failed/ignored，退出0、源码无漂移 | `/tmp/openrust-c04d3-second-module-session.result.json`；包含explicit_request_driver_consumes_cookie_and_keeps_two_request_queues_isolated。不同preference/memo/count跨3轮、分别更新后GET，不能以Cookie回执单独代替持久化证据；真实双库TCP待执行 |
| CLI模板快照 | 首轮旧snapshot失败；精确核对更新后定向1 passed、19 filtered、退出0、源码无漂移 | `/tmp/openrust-c04d3-second-cli-snapshot.result.json`；不同模型将19段完整拆分，只有Cargo新增time0.3 devdep和已审helper全文变化，其余17段和assertion_line不变 |
| 独立审查 | 核心首稿4项问题修复后增量审查无新blocker/should-fix；middleware失败队列清理、模块/模板及非空持久补测闭环 | state c6655564…/request bada08c1…及middleware 30de372b…曾冻结审查；后续state仅lint修饰/helper位置移动。README返回传入Arc（仅新后端赋配置）措辞已纠正；实际request实例不得长期保存 |
| 暂停all-Clippy期间的独立门禁 | 默认tests 668/29 ignored、全特性tests 1328/102 ignored，均0 failed；双docs、no-default/session-only同样退出0、源码无漂移 | `/tmp/openrust-c04d-d3-independent1-<stage>.result.json`；六项对应未改format函数的冻结快照，MSRV等后继在途；ignored不算真实服务通过 |
| PostgreSQL/MySQL根真实HTTP | 两侧各55 passed、0 failed/ignored/filtered/skipped，exit0/accepted、源码无漂移；两个required具名命中 | `/tmp/openrust-c04d3-root-{pg,mysql}.{safe.log,result.json}`；主Auth流程内实际执行显式Db wrapper双client非空TCP链，原默认Db链独立保留；邮件fake不计真实供应商。两侧精确锚库已DROP并核不存在；生成应用仍待完成 |
| D3全新plain生成应用 | fmt/check/test通过，4具名测试、0 failed/ignored/filtered/skipped，源码与生成lock稳定 | `/tmp/openrust-c04d1-generated-d3-plain-verify-plain.result.json`与`/tmp/openrust-c04d3-generated-plain.driver.log`；复用安全脚本前缀不代表旧结果，Session helper按实际TestApp/Router证据记录，不冒称TCP；认证双库产物仍待完成 |
| 三类最终新生成应用 | plain4/认证PG2/MySQL2均通过fmt/check/test，0 failed/ignored/filtered/skipped，源码与生成lock稳定 | `/tmp/openrust-c04d1-generated-d3-<kind>-verify-<kind>.result.json`及`/tmp/openrust-c04d3-generated-<kind>.driver.log`；Session helper为TestApp Router，认证主流程及根显式fixture另有真实TCP；两侧精确锚库已删并核不存在 |
| 资源收尾 | 根target有效产物0，释放20,480,098,304字节，五旧consumer逐项不变；剩2.486656GiB，磁盘空闲12.835766% | `/tmp/openrust-c04d3-final-clean.log`；实际独占锁/占用核查，保留Cargo锁inode，未改共享服务；仅保留/tmp有界证据/生成源码与既有PHP缓存 |
| 最终修后快照 | final4完整12项均退出0、源码无漂移、日志SHA逐项核对一致；tests默认668/29 ignored、全特性1328/102 ignored，均0 failed | `/tmp/openrust-c04d-d3-final4-<stage>.result.json`；含fmt、双Clippy(-D warnings)、双tests/docs、no-default/session-only、MSRV1.97.0、deny、server构建；all-Clippy为用户批准单次，暂停原因已解除 |
| 完整门禁 | final1 fmt/default Clippy通过，all Clippy退出101；final2叶lint修后final3全特性Clippy仍退出101，仅新提取测试配置函数uninlined_format_args；前三轮错误分别已定位，按AGENTS §5暂停后用户已明确选择修正后定向重验；待当前独立构建终态后仅内联format命名参数、核配置字节不变，保持-D warnings单次重验，其他独立证据保留；尚无完整通过结论 | `/tmp/openrust-c04d-d3-final{1,2}-<stage>.result.json`；保留-D warnings，不假加await、不改运行语义。真实双库/新生成/资源收尾仍待验收 |

修正与失败证据：旧Session首轮8 passed/2 failed，修复系统随机Insert重试耗尽永久拒绝及invalidate已flush/destroy后的合法start（保tombstone），修后旧24通过。新补测第二轮因Result类型和模块半稿Db别名编译退出101；第三轮旧session10与新extensions11通过、1失败，fixture Error::custom默认400却断言500，改显式500后保留原严格断言；第四轮36通过。日志`/tmp/openrust-c04d3-{first,second,third}-<stage>.log`保留。模块首条过滤器仅3旧测试/44过滤，不计新D3场景通过。首稿独立发现pending误删目标、同ID显式意图、hook busy排队、数字Cookie读取四项均已修；模块空属性fixture缺口以非空双client链修正。final1新增工具链unused_async_trait_impl与items_after_test_module由局部有原因allow和helper位置移动修正，未降低全局门禁。

用户批准后，仅将auth_fixture_config三参数改命名捕获，最终文件SHA3ff24648…；根独立将占位符反向规范化，完整format字面量逐字等于3cabae2中的原始配置，未改配置字节。待独立1 runner自然终态后才改，未漂移其快照；最终新快照12项final4已启动，获准的单次all-Clippy已实际通过，不把独立1旧快照冒称最终tests。

本簇已完成上述最终适用验收，限定提交以Git为准。D4完整内置Cookie driver、Container extend与完整Session/Web后继仍开放，不能由本簇关闭L025全项。

## C04-D2 Store公开API（限定子范围已验收）

契约见历史决策 082 D2。D1已由d47162d提交，其完整验收不继承为D2通过。

| 范围 | 实际结果 | 证据与边界 |
| --- | --- | --- |
| 最小session-only | lib check退出0；helpers 9、HTTP 5均通过，0 failed/ignored/filtered | `/tmp/openrust-c04d2-second-{check-session,tests-session}.result.json`；源码无漂移，非完整服务验收 |
| 外部自定义驱动consumer | 独立manifest check/test退出0，具名测试1 passed | `/tmp/openrust-c04d2-first-consumer-{check,test}.result.json`；源码无漂移，不代表完整driver注册替换生命周期 |
| 唯一模块定向 | 显式session feature下3 passed、43 filtered、0 failed/ignored，退出0 | `/tmp/openrust-c04d2-second-module-session.result.json`；含256/257 UTF-8字节与列表8/9边界，真实双库TCP尚待执行 |
| 修后最终workspace门禁 | final3 fmt、默认/全特性Clippy（-D warnings）、默认tests 668/29 ignored、全特性tests 1313/102 ignored、默认/全特性docs、no-default/session-only、MSRV1.97.0、cargo deny及server构建均退出0 | `/tmp/openrust-c04d-d2-final3-<stage>.result.json`；12项源码无漂移且日志SHA逐项核对一致；忽略项不计真实服务通过 |
| 根PostgreSQL/MySQL真实模块HTTP | 两侧完整target各54 passed、0 failed/ignored/filtered/skipped，exit0、accepted=true、源码无漂移 | `/tmp/openrust-c04d2-root-{pg,mysql}.{safe.log,result.json}`；包含D2跨请求持久调用，邮件fake不计供应商验收；两侧精确自有锚库已DROP并查询核不存在，生成项目仍待完成 |
| 三类全新生成应用 | plain 4、认证PG 2、认证MySQL 2具名测试通过，均0 failed/ignored/filtered/skipped；fmt/check/test退出0 | `/tmp/openrust-c04d1-generated-d2-{plain,pg,mysql}-verify-<kind>.result.json`与`/tmp/openrust-c04d2-generated-<kind>.driver.log`；脚本前缀复用不代表旧结果，源码/生成lock稳定，两侧精确锚库已删除并核不存在 |
| 资源收尾 | 根target有效产物0，释放20,610,981,888字节，五旧consumer逐项不变；剩余2.486656GiB，空闲12.847956% | `/tmp/openrust-c04d2-final-clean.log`；实际独占锁、无其他占用，保留Cargo锁inode；不清理其他任务源码或PHP参考缓存 |
| 独立审查与文档 | 不同模型核心、边界测试、文档及模板快照核对无新问题；10个变更Markdown的435个本地路径无缺失 | 快照完整模板段与当前模板逐字一致，仅分段空行；其余差异只有assertion_line。未包含完整锚点检查，不替代运行验收 |

PHP8.5.0缓存实际执行44条结果，22条裸表达式与22条真实固定官方HandleExceptions方法路径分开，持久数据及版本/源哈希见`tests/http/fixtures/laravel-session-d2/{oracle.php,golden.json,provenance.json}`。leading-numeric在裸表达式可返回值并warning，官方handler路径真实抛ErrorException；false追加执行无LogManager的deprecation早退路径。以上不是完整Store/Container/logger启动或全Laravel oracle；未验证真实logger输出。Rust不支持非有限JSON、完整PHP对象类别及隐藏next-free历史等限制保留。

失败与修正证据保留：helpers首轮8 passed/1 failed，测试误把可百分号编码的newline名称当非法，修为公开CookieFactory先确认非法样本，未收紧生产规则；模块首条默认feature仅0 tests/40 filtered，不计行为通过；final1全特性Clippy因新增测试浮点字面量无数字分隔符而退出101；final2 fmt需换行、默认tests因CLI模板快照未同步而失败（CLI 19 passed/1 failed）。对应`/tmp/openrust-c04d2-first-{tests-session,module-session}.log`与`/tmp/openrust-c04d-d2-final{1,2}-<stage>.log`保留。数值和安全断言未放宽，快照按实际模板核对更新后由final3重验。

本簇基础方法与示例/模板已完成上述适用验收，限定提交以Git为准。完整Session和ID/handler替换生命周期仍按唯一todo推进。

## C04-D1 Session Store/flash（限定子范围已验收）

契约见历史决策 082，当前实现见[Session](../core/session.md)。以下定向结果仅证明各自源码快照，不代替后续修改后的最终门禁；本簇限定提交以Git为准。

| 范围 | 实际证据 | 边界与入口 |
| --- | --- | --- |
| Session-only基础 | 18 passed、0 failed/ignored、390 filtered | `/tmp/openrust-c04d-session-provider-first.log`；含名称推导，不是完整workspace |
| 公开生命周期 | 10 passed、0 failed/ignored/filtered | `/tmp/openrust-c04d-session-public-first.log`；包含删除认证记录后的重复start拒绝 |
| 进程内正式Router | 4 passed、0 failed/ignored/filtered | `/tmp/openrust-c04d-session-http-first.log`；未修改/5xx保存、取消、错误与装配；不是TCP，后续超时修正待最终重验 |
| 完整记录预算回归 | 4 passed、0 failed/ignored、408 filtered | `/tmp/openrust-c04d-session-whole-budget-paths-first.log`；启动合并/token、pull/expiry、迁移和flash老化的原子边界 |
| 真实数据库Store | PG18/MySQL8.4各1 passed、0 failed/ignored/filtered；源码无漂移、各自UUID隔离锚库已核不存在 | `/tmp/openrust-c04d-store-pg-run-second.log`、`/tmp/openrust-c04d-store-mysql-run-first.log`；命名fixture `database_session_atomic_writes_precision_and_revocation`，包括文本/NUL/预算/授权截止，不代表完整Auth或模块HTTP |
| 真实Redis Store | Redis8.10.2命名fixture 1 passed、0 failed/ignored/filtered；源码无漂移 | `/tmp/openrust-c04d-redis-store-run-first.log`；`redis_session_shared_atomic_writes_ttl_namespace_and_limits`，仅自有随机命名空间与ID；不算完整Auth Redis或集群 |
| 模块与认证定向 | 模块2 passed（27 filtered）；Auth runtime 9 passed（0 filtered），均0 failed/ignored | `/tmp/openrust-c04d-module-session-first.log`、`/tmp/openrust-c04d-auth-runtime-second.log`；模块显式memory，非真实持久化HTTP |
| PostgreSQL完整Auth target | 修后完整8 passed、0 failed/ignored/filtered，exit0；3个required named通过、源码无漂移、隔离锚库已核清理 | `/tmp/openrust-c04d-auth-db-pg-second.log`；公开Cookie结构断言后整target通过。首轮7 passed/1 failed及旧HttpOnly子串、Secure默认差异保留于`auth-db-pg-first.log`（同前缀）；不等于唯一模块HTTP或完整集群 |
| MySQL完整Auth target | 完整8 passed、0 failed/ignored/filtered，exit0；3个required named通过、源码无漂移、隔离锚库已核清理 | `/tmp/openrust-c04d-auth-db-mysql-first.log`；实际MySQL8.4认证/撤销/迁移target，不替代模块HTTP、Redis或集群 |
| Redis完整Auth runtime | 10 passed、0 failed/ignored/filtered；跨实例共享与撤销required named通过、源码无漂移 | `/tmp/openrust-c04d-auth-redis-first.log`；Redis8.10.2，非负载均衡集群总验收 |
| 正式宿主与模块Session | 无默认feature、session+db-postgres、无auth组合定向4 passed、0 failed/ignored、29 filtered | `/tmp/openrust-c04d-host-module-session-second.log`；含无DB正式宿主404、DB配置Provider清单恰一份、原2模块memory生命周期；不等于无auth真实DB连接验收。首轮测试误用Container.is_bound编译失败，已改公开contains |
| 外部custom store consumer | 独立package check exit0；公开driver行为test 1 passed、0 failed/ignored/filtered，doc0 | `/tmp/openrust-c04d-consumer-check-second.log`、`/tmp/openrust-c04d-consumer-test-first.log`；无默认feature仅session。首轮getrandom错误类型不实现StdError的feature独立性编译缺陷已修，未新增依赖；不等于真实外部服务 |
| 当前workspace默认tests | 668 passed、29 ignored、0 failed/filtered；actual_exit0、source_changed为空 | `/tmp/openrust-c04d-final9-tests-default.result.json`及同名log；忽略项不是实服务验收。final9全特性缺退出结果未计通过；final10实际重验1300 passed/102 ignored、0 failed/filtered，actual_exit0/source_changed为空，见`/tmp/openrust-c04d-final10-tests-all.result.json` |
| 真实CLI生成项目 | plain4/4、认证PG2/2、认证MySQL2/2，均0 failed/ignored/filtered；各fmt/check exit0，source_guard_changed为空 | `/tmp/openrust-c04d1-generated-d1-{plain-second-verify-plain,pg-second-verify-pg,mysql-first-verify-mysql}.{jsonl,result.json}`；三个新项目的正式生成测试，不替代根TCP/TLS；先前失败日志保留 |
| final10其余门禁（精度/回调修复前快照） | docs默认/全特性、no-default、session-only、MSRV、deny及server build均actual_exit0、source_changed为空 | `/tmp/openrust-c04d-final10-{docs-default,docs-all,no-default,session-only,msrv,deny,server-build}.result.json`；随后审查发现MySQL秒精度expiry谓词缺陷并修为CURRENT_TIMESTAMP(6)，Keycloak仓内fixture回调对齐现有client。两处修改后的专项及适用门禁尚待重验，不继承此行成功关闭修复 |
| 过期精度修复双库Store | MySQL8.4/PG18各1 passed、0 failed/ignored/filtered，exit0、源码无漂移、隔离锚库已核清理 | `/tmp/openrust-c04d-store-{mysql,pg}-subsecond.log`；MySQL使用CURRENT_TIMESTAMP(6)，真实DB微秒时钟样本限定同秒，有限重试；最终独立复审及其余修后门禁尚待完成 |
| 根PG完整模块HTTP首轮 | 52 passed、1 failed，源码无漂移、隔离锚库已清理 | `/tmp/openrust-c04d-root-pg-first.safe.log`；新增正式TCP Session生命周期所在主测试已通过，account_security旧fixture配置键失败。修复旧cookie_name/ttl_secs使用处后须整组重验，不记完整target通过；浏览器15秒到期断言不能删除或放宽 |
| 根PG完整模块HTTP修后 | 53/53 passed，0 failed/ignored/filtered/skipped，exit0、源码无漂移 | `/tmp/openrust-c04d-root-pg-second.safe.log`；含正式TCP数据库Session生命周期与账户安全，原首轮失败保留。浏览器专属短TTL wrapper修后仍需独立真实专项，不以本target替代 |
| 根MySQL完整模块HTTP | 53/53 passed，0 failed/ignored/filtered/skipped，exit0、源码无漂移、隔离锚库已清理 | `/tmp/openrust-c04d-root-mysql-first.safe.log`；正式TCP数据库Session与完整既有target；浏览器原生短TTL边界及Keycloak仍另验 |
| 真实Keycloak PostgreSQL | 主流程1 passed、子进程helper1 ignored，0 failed/filtered/skipped，exit0、源码无漂移、隔离锚库已清理 | `/tmp/openrust-c04d-keycloak-pg-third.log`；helper由主流程显式启动，ignored不另算通过。修仓内回调对齐现client，未改IdP；前两轮失败保留；MySQL对应主流程1 passed/helper1 ignored亦已通过（0 failed/filtered/skipped、exit0、源码无漂移、锚库核清），见`/tmp/openrust-c04d-keycloak-mysql-first.log` |
| PostgreSQL浏览器专项首轮 | Cargo exit0、4 passed/0 failed/ignored，101 filtered；runner accepted=false | `/tmp/openrust-c04d-browser-pg-first.safe.log`；原生浏览器输出打断test状态行，required named提取失败。不得用总数替代具名证据；仅修runner输出模式后重验，业务断言不变 |
| PostgreSQL浏览器具名重验 | accepted=true，4 passed、0 failed/ignored/skipped，101 filtered，exit0、源码无漂移、隔离锚库已清理 | `/tmp/openrust-c04d-browser-pg-second.safe.log`；全部四项显式运行，含原生Chromium与持久截止。show-output成功列表曾被误列failed_names，进程失败计数仍0；统计器修正单独离线提取，不冒称新实跑 |
| MySQL浏览器专项 | accepted=true，4 passed、0 failed/ignored/skipped，101 filtered，exit0、failed_names为空、源码无漂移、隔离锚库已清理 | `/tmp/openrust-c04d-browser-mysql-first.safe.log`；PG统计修正见`/tmp/openrust-c04d-browser-pg-second.corrected.json`，标new_cargo_execution=false且带原safe日志SHA，原raw流未留存，不冒称重提取原Cargo全文 |
| 模块OIDC PostgreSQL首轮 | 2 passed/6 failed、97 filtered，exit101，无skip | `/tmp/openrust-c04d-module-oidc-pg-first.safe.log`；HTTP400根因定位中，属于模块mock业务流程，非已通过真实Keycloak主流程。实际target8项，runner原预期7也需纠正；不得放宽错误/安全断言 |
| 模块OIDC PostgreSQL修后完整 | accepted=true，8 passed、97 filtered，0 failed/ignored/skipped，exit0、源码无漂移、隔离锚库已清理 | `/tmp/openrust-c04d-module-oidc-pg-final.safe.log`；缺Host及3处Cookie大小写属性子串已仅在fixture修为合法Host/结构化精确属性断言，真实TCP定向后完整重验；未改生产Cookie绑定。MySQL对应完整8 passed、97 filtered、0 failed/ignored/skipped亦accepted通过，源码无漂移、锚库核清，见`/tmp/openrust-c04d-module-oidc-mysql-final.safe.log` |
| OIDC原始重复凭据修复定向PG | accepted=true，1 passed、104 filtered、0 failed/ignored/skipped，exit0，隔离锚库已清理 | `/tmp/openrust-c04d-module-oidc-pg-duplicate-green.safe.log`；四种原wire重复排列拒绝后原单Cookie继续成功及重放拒绝。controller通过RequestInfo检查原头数量，再读解密值；不同模型静态复核通过；修后完整PG/MySQL各8 passed、97 filtered、0 failed/ignored/skipped均accepted，源码无漂移、全部锚库核清，见`/tmp/openrust-c04d-module-oidc-{pg,mysql}-security-final.safe.log` |
| 新Session真实TLS | accepted=true；真实PG、正式SessionMigration、session/db-postgres/encryption无auth根server，secure=None；TLS1.3显式证书信任和hostname校验 | `/tmp/openrust-c04d-session-tls-first.safe.log`及`/tmp/openrust-c04d-session-tls-final.result.json`；HTTPS Secure/HttpOnly/Lax与opaque往返、HTTP及不可信HTTPS转发非Secure均通过，两后端exit0/代理回收/精确锚库drop并核不存在、源码无漂移/无凭据持久化；不替代无DB默认入口与修后完整门禁 |
| final12修后完整门禁 | fmt、默认/全特性Clippy、默认668/29 ignored及全特性1300/102 ignored tests、默认/全特性docs、no-default/session-only均actual_exit0、source_changed为空 | `/tmp/openrust-c04d-final12-{fmt,clippy-default,clippy-all,tests-default,tests-all,docs-default,docs-all,no-default,session-only}.result.json`；MSRV/deny/server-build随后亦actual_exit0、source_changed为空，完整12stage均通过；默认入口另见下行。ignored不是实服务证据 |
| 默认cargo run真实TLS/无DB入口 | tool_exit0、两个backend退出码均0、graceful_shutdown_verified、源码无漂移 | `/tmp/openrust-c04d-default-tls-final.log`及`/tmp/openrust-c04d-default-tls-final.result.json`；复用工具目录名不等于沿用旧结果，本轮实际运行；无DB的Session示例404/not_found，不静默memory，旧5consumer目录不变 |
| 修后生成项目最终复验 | plain4、PG2、MySQL2均accepted，fmt/check/test exit0，0 failed/ignored/filtered/skipped，source_guard_changed为空 | `/tmp/openrust-c04d-generated-{plain,pg,mysql}-final.safe.log`；最后一轮使用最终源码，双库精确锚库已drop并核不存在 |
| 资源收尾 | root target有效产物0，仅保Cargo锁inode；释放24,564,617,216字节，五旧consumer逐项字节不变，剩2.486656GiB，磁盘空闲12.864122% | `/tmp/openrust-c04d-final-clean.log`、`/tmp/openrust-c04d-temp-clean.log`；已核无Cargo/rustc/rustdoc/server；仅清3自有生成应用和本轮2 TLS临时目录，详细证据已复制平面JSON；自有pyc已移除，不改全局配置 |

最终配置定向8项已实际通过（0 failed/ignored/filtered，`/tmp/openrust-c04d-session-config-second.log`），包括无效中间值被后置配置修复、严格最终错误字段、typed传输和禁用driver不解析Db。首轮仅fixture引用受feature限制的database别名而编译失败（`/tmp/openrust-c04d-session-config-first.log`），改公开db路径后重验，未给target增加数据库依赖；完整workspace fmt/Clippy/tests/docs/deny、适用feature/MSRV、生成plain及认证双库项目、正式宿主TCP/TLS、模块完整双库HTTP及适用OIDC链均已按上表限定范围通过。旧tower运行时及C04-C证据不自动证明新Session接管。完整Session/CSRF、Store其余公开API与跨节点保证仍按唯一todo和矩阵交付。

## C04-C Cookie/响应（已验收）

契约见历史决策 081。以下均为本簇实际证据，按范围记录，不沿用B2门禁；早期定向记录不替代最终快照；空domain修后根双库已按final独立证据重验。

| 范围 | 当前证据 | 限制与入口 |
| --- | --- | --- |
| 首次构建与Cookie内核 | check exit0；默认13/13、加密15/15；B1 form回归7/7 | `/tmp/openrust-c04c-{core-check-first,cookie-unit-second,cookie-encrypted-first,form-regression-first}.log`；早期定向快照，不代替完整门禁 |
| 公开输入与唯一模块 | Cookie4、input34、request10、模块Cookie1（26 filtered）通过 | `/tmp/openrust-c04c-public-{first,scope-first}.log`、`module-first.log`；后续可信传输/factory公开6项亦通过，见`transport-factory-first.log`（均同前缀） |
| 最终Session配置 | 延迟装配后5/5，含无显式区段默认；受保护Session另1/1（7 filtered） | `/tmp/openrust-c04c-session-{final-config-second,protected-first}.log`；保原始header与严格重复凭据。source_has_section候选经配置发布链复核撤回，未据此改生产 |
| SameSite null/缺键 | 独立Cookie路径1/1（6 filtered） | `/tmp/openrust-c04c-session-samesite-null.log`；未注册Session schema，不能外推现SessionConfig字符串schema已支持null |
| 加密膨胀预算 | 显式Semantic下两个case均精确HTTP413、原机器码且无Set-Cookie，定向1/1；不同模型审查关闭测试假绿 | `/tmp/openrust-c04c-cookie-expansion-semantic.log`；60KiB值、40KiB值+20KiB path；公开非法参数仍500。原`cookie-expansion-{red,green,final}.log`保留失败与补断言前证据，不将AlwaysOk旧fixture说成HTTP413 |
| CLI布局 | 首次全组19/20；限定更新快照后layout定向1/1（19 filtered） | `/tmp/openrust-c04c-cli-{first,layout-second}.log`；新增helper正文/session.path覆盖，不等于全组20项已重验 |
| 默认workspace Clippy | all-targets第三轮exit0，`-D warnings` | `/tmp/openrust-c04c-final-clippy-default-third.log`；首两轮lint日志保留，全特性门禁待验 |
| 最终部分workspace门禁 | fmt、默认/全特性all-targets Clippy与默认server build实际exit0，source_changed均空 | `/tmp/openrust-c04c-final-{fmt,clippy-default,clippy-all,server-build}.result.json`；默认tests首轮因runner注APP_ENV=test失败101；仅修runner环境后完整668 passed/29 ignored、0 failed/filtered，actual_exit0/sourceguard空（`/tmp/openrust-c04c-final2-tests-default.result.json`）。忽略项与未提供外部条件的早返回不算真实服务证据，旧失败保留，不以exact单测代替全组 |
| 最终tests/docs/无默认feature | default 668/29ignored、all-features1241/99ignored，均0 failed/filtered；默认与全特性docs、no-default check均actual_exit0/sourceguard空 | `/tmp/openrust-c04c-final2-{tests-default,tests-all,docs-default,docs-all,no-default}.result.json`；ignored与未提供外部条件早返回均不算真实服务，分类待核；阶段资源约21.415GiB/free10.787% |
| 最终MSRV/依赖审计 | Rust1.97全特性all-targets check及cargo deny均actual_exit0/sourceguard空 | `/tmp/openrust-c04c-final2-{msrv,deny}.result.json`；最终10项矩阵均实际通过，target22.108GiB/free10.713%，正在修后根双库完整重验 |
| 根PostgreSQL 18/MySQL 8.4 | 空domain修后final完整example_auth_flows各48/48；0 failed/ignored/filtered/skipped，3named含Session安全与模块Cookie；两个UUID anchor均drop，sourceguard空 | `/tmp/openrust-c04c-root-{pg,mysql}-final.safe.log`及`/tmp/openrust-c04c-generated-root-final-root-{pg,mysql}.result.json`；旧first日志保留，不替代本行final证据 |
| 真实CLI生成 | 当前CLI实际build，plain/auth-PG/auth-MySQL全新生成，helper/owned空dotenv检查通过，sourceguard空 | `/tmp/openrust-c04c-generated-prepare-first.safe.log`；仅prepare；生成plain首轮fmt失败（`/tmp/openrust-c04c-generated-plain-fmt-red.log`），正在修模板源及owned生成文件，不代表check/test或HTTP通过 |
| plain生成项目 | 修后fmt/check exit0、完整3/3，0 failed/ignored/filtered/skipped，API+health/Input/Cookie具名均中，sourceguard空 | `/tmp/openrust-c04c-generated-plain-third.safe.log`；模板源头与owned生成文件格式同步，前两轮fmt红日志保留；阶段后target11.484GiB/free11.845% |
| PostgreSQL认证生成项目 | fmt/check exit0、完整2/2，0 failed/ignored/filtered/skipped；主认证用例调用同库Input/Cookie helper；anchor已drop、sourceguard空 | `/tmp/openrust-c04c-generated-pg-first.safe.log`；阶段后target13.288GiB/free11.660%，使用测试Mailer，不算真实供应商发送 |
| MySQL认证生成项目 | fmt/check exit0、完整2/2，0 failed/ignored/filtered/skipped；同库Input/Cookie helper通过；anchor已drop、sourceguard空 | `/tmp/openrust-c04c-generated-mysql-first.safe.log`；阶段后target14.069GiB/free11.585%，测试Mailer不作真实供应商发送证据 |
| 真实TLS与默认入口 | 修后tool exit0；TLS1.3显式证书信任/hostname校验；trusted HTTPS Secure+opaque往返、untrusted转发HTTPS不Secure、trusted HTTP不Secure，完整envelope/属性均通过 | `/tmp/openrust-c04c-real-tls-second.log`；`/tmp/openrust-c04c-tls-backends-fyx268pf/launch-result.json`、`/tmp/openrust-c04c-tls-backends-fyx268pf-cases.json`；两个默认cargo run后端均0退出、cleanup空、sourceguard空、旧consumer不变；仅Cookie传输证据，不算集群验收 |
| TLS工具与启动器 | 超时/回收离线回归及不同模型静态复核通过 | 工具SHA `02f89c887a4bd5d8c044f63f4410eadc24614a807b3e2c4c5b5171b709780a93`；启动器修后SHA `ade1ce934c170987681d78f84bbd3916f25d7e39cb00bc43009ecc5eea5fdd41`；DB/.env隔离、模块根层级、listener归属、子代回收和总期限已修，真实TLS首轮tool参数检查exit2，未运行case（输出路径不符工具白名单）；后端listener归属已证、均已清理、sourceguard空，见`/tmp/openrust-c04c-real-tls-first.log`与`/tmp/openrust-c04c-tls-backends-gpskbhne/launch-result.json`。输出路径修复后真实重验已通过，见上一行；首红不覆盖 |

PHP8.5真实SAPI参考采集22个原始HTTP请求，均200：20个可表达单header、1个非法UTF-8 typed-base64、1个多header transport适配分列。fixture SHA `8cfa3f571ad976f0035b3f34b6a39a3da78c32f471b29346749a00fc964865ed`；来源与生成器见`tests/http/fixtures/php85/cookie-*`，owned PHP服务已关闭。采集本身不等于Rust差分通过，`parse_str`不能代替wire解析。

独审收口：公开工厂或`Domain=`解析产生None/空域，两者wire均无Domain，响应Option域比较会重复Set-Cookie；固定Symfony将null域归为空键。两处响应identity已最小修复为null域归空键，公开None/空串/Domain=覆盖及不同名/路径排序回归实际1/1通过（7 filtered，`/tmp/openrust-c04c-domain-identity-first.log`），不同模型窄审关闭发现；最终完整门禁与修后双库已通过。生成helper及生产接线的不同模型审查已有相应证据，但不替代实际运行；except文档已按入站root/出站literal阶段匹配校准。

最终证据索引`/tmp/openrust-c04c-evidence-index.json`保留22份结构结果（包含失败历史）及TLS/ignored引用，最终通过集合按索引gate_selection选取，443个当前源码hash核对无漂移。默认29 ignored均Rustdoc；all99条为35 Rustdoc、60外部集成执行条目和4父验收专用子进程helper，去重59=35文档+22外部集成+2helper，20个模块用例重复编入3个target产生40重复条目。独立逐项分类`/tmp/openrust-c04c-final-ignored.json`（SHA `af0a211ee8df9da2aacf077b3895c58317d6ec87002008cec613c58686c9d33b`），未提供外部条件的早返回不计真实服务。cargo deny四域通过，既有27 duplicate/1 license-not-encountered警告未放宽策略。

资源收尾：持已验证artifact flock，真实Cargo探针确认阻塞后SIGTERM+join，cargo clean dry-run复核35056文件/19.4GiB；确认无其他使用者后清根可再生产物并保锁inode。根allocated 21,123,330,048→0 bytes，五旧consumer逐项不变；总target2.486656GiB，free118.426498GiB/12.784183%，无后继构建/server。日志`/tmp/openrust-c04c-final-clean-root-artifacts.log`与dry-run证据，源码/生成项目及PHP工具链暂留作诊断复跑证据，不删除共享或归属不明资源。本簇限定提交见Git；完整Session/flash/CSRF/FormRequest及完整C04仍开放。

## C04-B2 类型便捷方法

- 已通过本簇适用门禁，限定公开Request/Fluent的标量、Stringable、保键Collection、枚举、clamp、条件分支、日期/区间和上下文；不代表完整Carbon工具箱或完整C04。contract与Rust适配见历史决策 079，固定源/许可证/再生成见[SOURCES](../../../vendor/openrust/framework/src/support/calendar/date/SOURCES.md)。
- 最终冻结快照：`/tmp/openrust-c04b2-final2-*.result.json`共20阶段，全部actual_exit=0、source_changed=[]；driver `/tmp/openrust-c04b2-final2-driver.log`。fmt、default/all-features workspace all-target Clippy（`-D warnings`）、默认/全特性Rustdoc（`-D warnings`）、no-default all-target check、Rust 1.97 all-feature all-target check、cargo deny通过。默认workspace tests 645 passed/29 ignored；全特性1211 passed/99 ignored，均0 failed；default的29项均为Rustdoc示例；all的99项为35个Rustdoc示例与64个显式外部服务验收（20项在三target重复、Mailpit2、Keycloak1、S3版本1）。ignored与未显式提供外部条件的测试不算真实服务证据，分类记录`/tmp/openrust-c04b2-final2-ignored.json`。
- 根真实PG18/MySQL8.4：完整`example_auth_flows`各47 passed，0 ignored/filtered/skipped；普通生成项目check/tests 2项通过；PG/MySQL生成认证项目各2具名测试通过，含同库配置的共用B2公开helper，0 ignored/skipped。四个本轮独占数据库anchor均已DROP，原认证与清理断言保留。邮件采用测试驱动，不将其记作真实供应商发送。
- 真实TCP：根默认`cargo run`，仓内`verify-request-input.py` SHA `028e50883fa8f069d7637dcf304a679e971b9cce4844163ef38410279dc62fea`；6/6（正常/default-null 200，枚举/日期/区间错误400，区间词项超限413），健康200、工具exit0、服务优雅退出0，外层45秒有界。新证据使用final2独立路径，未覆盖第一次运行记录。
- 定向：日期22、公开HTTP34/10/87、模块输入3项（23 filtered）通过。标量617项分为613 serde输入、3私有非有限内核、1 PHP/serde JSON溢出表示差异；enum48分为42 bag、4私有非有限、2不可表示PHP对象；clamp306中304运行时tag/bit匹配，2非法默认参数由Rust静态拒绝。61实际控制字符另验。完整分类与固定PHP8.5/Carbon3.8.4来源保留在fixture；Carbon为声明最低样本，不冒称Laravel lock版本。
- 日期参考包含原119、grammar80、composition30、lexical43、六月clock15、生产无mock18、极值4、named-zone30、全部598 POSIX footer，以及byte18/format19+6输出、remaining53和overflow4。53项保留input_type并分别断言name/e/P/O/offset，17错误逐项检查HTTP状态与机器码。实际PHP异常文本的非法UTF-8只在证据异常字段替换，不修改输入或成功结果。
- 不同模型审查闭合：非有限JSON输出、S后缀false acceptance、i128小数进位panic、错误断言任意Err假绿均已修复并回归；公开Request/Fluent六字符串边界断言400与bag不变。源覆盖审查对既有五类固定分支均找到对应路径，未发现新增缺失production反例；组合样本不是全体字符串形式证明。TCP marker同时检查原始字节与解码值，离线转义泄漏拒绝/正常错误接受通过，最终新工具已真实重验。
- 资源收尾已完成：确认无构建/server后持artifact互斥锁，短Cargo探针实际阻塞并join、clean dry-run后仅清根可再生产物，保锁inode；根allocated 19,947,913,216→0 bytes，旧五consumer合计2.487GiB逐项不变，磁盘free回升至12.782%。源码、证据、临时PHP工具链及共享服务保留，日志`/tmp/openrust-c04b2-final-clean-root-artifacts.log`；本簇限定本地提交见Git。其余Cookie/Session/FormRequest/Response及完整支持工具箱按唯一todo后继，不因本簇通过而关闭。

## 应用端口环境配置

应用端口统一使用 `APP_PORT`，自定义命名空间使用 `SHOP_PORT`，内部仍为 `server.port`；全局 `PORT` 是低优先级回退，旧端口键不再覆盖配置。配置回归 24 项、生命周期 8 项通过，覆盖命名空间隔离、进程端口覆盖与 `.env` 中 `APP_URL="http://localhost:${APP_PORT}"` 展开。

默认 workspace tests 共 570 passed、29 ignored；fmt、workspace all-target Clippy（`-D warnings`）、warnings-denied Rustdoc 与 cargo deny 均通过。实际生成应用的 check/test（HTTP 1/1）、配置读取（进程端口与 URL 均为 4107）、真实 TCP health/API 200 与 SIGINT 0 通过；realtime consumer 双进程 Redis 集群用例 1/1 通过，健康探针使用既有认证主体。首次 workspace 的 localization 重复查询断言失败由其所属功能簇修正后重跑通过；本簇未改该测试或数据库迁移。

日志位于 `/tmp/openrust-app-port-verify.8yXhhA/`。本簇独占 target 已先 dry-run 再清理；并发会话占用的根 target 保留。本项不增加数据库行为，不将默认测试或生成模板编译记作真实 PostgreSQL/MySQL 验收。

## R3-B Laravel 路由组前缀与 middleware

`RouteGroup` 与 `Routes::group` 覆盖嵌套前缀拼接、前缀首尾斜杠归一化、组级 Tower middleware 的范围和声明执行顺序、跨组重复路由启动错误。定向 `http_router` 两项回归及 `cargo test --doc`（15 passed、21 ignored）通过；ignored doctest 不作为验收证据。`modules/example` tenant 路由已改用公开 RouteGroup，原 JWT→membership→trusted layer 请求顺序保持。

真实 PostgreSQL 18 与 MySQL 8.4 各通过生产 `example_auth_flows` 的 tenant HTTP 用例 1/1；这证明组路由在真实双库业务流程中接入，不代表两个服务互相复制。CLI 生成项目加入嵌套路由组后，offline all-target check、test 及生成项目 TestApp HTTP 1/1 通过。真实 TCP 入口由同一根 `server`/Router 生产组合路径承载，生成项目用例通过 TestApp；本簇未声称新增独立 TCP 网络证据。

源码门禁通过：`cargo fmt --all --check`、`git diff --check`、默认与 all-features workspace Clippy（all-targets，`-D warnings`）、默认与 all-features workspace tests、默认与 all-features warnings-denied Rustdoc、`cargo deny check`、`cargo check --workspace --no-default-features --all-targets`、Rust 1.97 all-target/all-features check。默认及全特性测试中 ignored 项不折算为实际服务验收。独立静态复审无 blocker/should-fix；复审不替代上述运行结果。

本簇未新增依赖或改数据库契约。C03 的命名 URL、middleware alias/group、domain、model binding、fallback、signed URL，以及 C04 Request/Response 均仍在队列；完整 Laravel HTTP/Web 行为矩阵未完成。

## R2-N 模块硬依赖与确定性启动顺序

`ModuleDescriptor::requires` 将已编译模块之间的硬依赖放入 `ModuleManifest`；依赖必须同批启用，不自动改写状态文件。依赖边优先于 `priority`，当前就绪模块再按 priority 升序与声明顺序稳定选择；模块 Provider 的 register/boot 使用同一拓扑顺序。缺失、未启用、无效名、重复、自依赖与环的稳定错误码有单测覆盖；Studly 查找保留既有空格别名语义。Provider 集成测试反向声明依赖并制造 priority 冲突，确认生产者 register/boot 都先于消费者（1/1）；模块单测 9/9。

双库模块 HTTP 使用现有真实 PostgreSQL 18 与 MySQL 8.4 服务，以及 `TestDatabase` 创建的隔离数据库。生产 `modules/example` 为生产者；测试临时 `orders` 消费模块声明 `.requires("example")` 且 priority 更早。命令 `cargo test -p openrust --features db-<backend> --bin server dependent_business_module_serves_http_with_real_database -- --nocapture` 每库通过 1/1；测试连接真实服务、执行 `SELECT 1`，再通过 in-process Router HTTP 对 `/api/example`、`/api/orders` 各得 200。独立复审发现正常路径未关闭隔离库，现已在应用优雅关闭后显式执行 `database.close().await`；错误提前返回仍由 `TestDatabase::drop` 在 Tokio runtime 内尽力安排删除。该 HTTP 证据不是 TCP 网络测试，也不改变正式唯一综合示例中只有 `modules/example` 的约定。根无参数 `cargo run` 另通过真实 TCP `/health` 与 `/api/example` 200；`modules_statuses.json` 中 example 启用，SIGINT 后进程以 0 退出。

CLI 生成的普通项目 `/tmp/openrust-r2n-generated-20261029`：`cargo fmt --all --check`、`cargo check --workspace --all-targets --offline`、`cargo test --workspace --offline` 通过；生成应用 API/health 测试与新增公开 `ModuleDescriptor::requires` 调用测试共 2/2。根仓库 `cargo fmt --all --check`、默认/all-features workspace all-target Clippy（`-D warnings`）、默认/all-features workspace tests、warnings-denied 双 Rustdoc、`cargo check --workspace --all-targets --no-default-features`、Rust 1.97 `check --workspace --all-targets --all-features` 与 `check -p openrust --no-default-features`、`cargo deny check` 均退出 0。核心库默认 297 项、all-features 489 项通过；全特性核心另有 2 ignored；外部服务 ignored 用例不计验收。cargo-deny 的 advisories/bans/licenses/sources 通过，保留原有 duplicate base64/winnow 与未匹配 MPL allowance 警告。

Spec、历史决策 075、core/模块/CLI 文档与唯一队列已同步。该簇不改数据库读写契约，真实 DB HTTP 用来验证模块启动与应用装配；完整 Laravel/laravel-modules、模块管理命令、生产集群仍未完成。

## R2-A3 缓存发布与锁外唤醒

安全自定义Wake在Notify同步回调中重新解析已发布singleton保存的Resolver，旧代码确定性返回service_cycle（红测）；修复后缓存工厂先结束active frame并移除已结束出边，再发布结果与通知，完成slot不再创建等待边。取消也经相同顺序清理。scope/root reset锁内仅提交关闭状态和缓存结果，所有通知与用户值释放在锁外执行，避免Waker同步重入State/outcome死锁。

19项容器定向回归通过，新增4项覆盖成功通知立即解析、取消通知观察、已结束父构造的后台等待边回收，以及手动scope/root reset两条status/attempt通知的同步重入。首次Clippy发现测试显式panic及随后single_match_else，已直接修复测试写法，无生产lint豁免；重跑默认/full Clippy通过。默认workspace290 passed/22 ignored，全特性513/32；双warnings-denied docs及deny通过。MSRV1.97全target/all-features与no-default也通过；独立公开consumer locked check/test1项通过，根裸cargo run的health/HTML/API/example均200、SIGINT0。日志前缀/tmp/openrust-r2-publication-。

不同模型gpt-6-sol最终静态复审无blocker/should-fix，审查未替代实际运行。

本簇无DB/auth/queue/配置/生成模板或依赖改动，不将A2真实双库/生成证明转记新容器生产集成。生产仍是旧Plugin唯一路径；Provider切换、异步资源清理、自动HTTP/job scope及模块/CRUD都未完成。

确认无独占target构建/server进程后cargo clean先dry-run再实际清理/tmp/openrust-r2-publication-target，回收47221files/15.3GiB，磁盘余118GiB；未知root/consumer共享缓存未动，/tmp红绿/门禁日志与公开consumer源码保留用于诊断。当前8个变更Markdown的40本地链接/10锚点均可解析，git diff --check与fmt通过。

## R2-A2 配置目录与来源失败语义

ConfigLoader::figment现在返回Result，全部第一方loader调用同步传播；显式new/APP_CONFIG_DIR/自定义namespace目录为空、缺失、非目录、读目录/条目失败即报错，未显式默认config缺失仍可使用内置默认。坏symlink不当作默认目录缺失，已发现领域TOML使用file_exact精确读取并在figment阶段验证来源，不向父目录搜索或静默跳文件。ConfigLoader Debug与bootstrap日志只打印安全来源key。

红测实际复现旧代码对显式缺目录静默退默认；修复后25项config定向测试通过，包含显式/默认/空目录、坏文件、TOML目录、坏symlink与路径哨兵。权限拒绝断言仅在OS实际强制目录权限时执行，root豁免分支不算权限拒绝证明。真实server config show在有效秘密前缀目录+debug日志下不输出路径哨兵，坏目录子进程exit1、含来源key且无原值/无panic。

实际通过fmt、默认及all-features workspace all-target Clippy -D warnings、默认286 passed/22 ignored与full509/32、warnings-denied双doc、deny、MSRV1.97 all-target/all-features与no-default。未把ignored或无URL早退当真实服务通过；日志前缀/tmp/openrust-r2-config-。

复用既有PG18/MySQL8.4，各12 targets/48 passed/0 ignored及testing database1项；新生成普通/PG auth/MySQL auth三工程check/test，双库各1项真实TCP/隔离SQL/fake mail流程通过；普通生成应用裸run/health200/SIGINT0。八个独立consumer locked test全部通过，cache/communication/SQLite/queue/gRPC实际run，gRPC15项/四类RPC、realtime真实Redis跨进程1项通过；query授权consumer仍仅编译，无SQLite完整支持或供应商实发声明。根裸cargo run再次验证health/HTML/API/example与SIGINT0。

不同模型gpt-6-sol最终只读复审无blocker/should-fix，未替代实际运行。Cargo/锁图/配置值/Redis默认/env优先级/模板行为未改；Provider/schema/核心与模块合并操作仍未实现。

构建达到聚合21.06GiB软限后停止扩展，确认无构建进程并仅对独占/tmp/openrust-r2-config-target dry-run/clean，报告移除57698files/18.8GiB；保留状态续跑未验收阶段。最终root启动后再次dry-run/clean，移除40556files/17.4GiB，磁盘余119GiB。未知root/consumer缓存未触碰；/tmp生成源码与日志保留作诊断证据。

独立于A2，随后确认的容器缓存发布竞态归R2-A3；该修复与新增回归见本文件A3节，不以A2配置门禁覆盖该行为。

## 容器清空绑定限定验收

固定 Laravel `d52c3b3` 的 `Container::flush` 与 `ContainerTest` 为基线。PHP 8.5 隔离对照 9 项通过，覆盖绑定、别名、实例和解析记录清除，以及环境、上下文、标签、扩展和回调保留。

框架新回归覆盖根和已有子作用域、原工厂快照、在途共享构造、条件绑定重新发现、旧扩展拒绝发布及用户析构重入。唯一示例在隔离 `TestApp` 复用真实启动装配，清空后重新发现环境绑定并构造新单例。实际生成的 minimal 项目通过 `cargo check`、`cargo test` 及公开 API 探针。工作区格式、严格 Clippy、带隔离 MySQL 8.4.13 的测试、拒警 Rustdoc 和依赖检查均通过。原始日志和源码指纹位于 `tmp/lead-core-next.oA3a9Ud8/`。

## 应用清空注册限定验收

固定 Laravel `d52c3b3` 的 `Application::flush` 使用 PHP 8.5 隔离子类注入状态，对照 6 项通过。实测父容器目录、已加载及延迟 Provider 状态、指定回调被清空，启动标记、路径和持久规则保留。

Rust 验证 Provider 和宿主贡献撤销、同类型 Provider 重新启动、解析和重绑回调清理、多个析构异常后仍完成清空，以及最终关闭回收旧资源。综合示例从实际 Provider 清单启动，清空后重新发现环境绑定并构造新单例。工作区格式、严格 Clippy、带隔离 MySQL 8.4.13 的测试、拒警 Rustdoc、依赖检查、实际生成项目 check/test 和公开 API 探针均通过。源码指纹与日志位于 `tmp/lead-app-flush.ZnbrXy6T/`。

独立 deferred Provider 映射与应用级 `terminating` 仍待对应。Rust 受管资源关闭回调随原资源所有者保留。

## 应用启动回调限定验收

固定 Laravel `d52c3b3` 在 PHP 8.5 的执行序列验证了应用 `booting`、Provider boot、应用 `booted`、阶段内追加、启动后登记及清空后即时执行。`booted` 阶段新登记的回调依固定实现执行两次。

Rust 回归验证动态队列、运行时 Provider 登记、`Application::flush` 释放回调，以及启动回调失败后的资源回收。综合示例在实际应用清单的 `booted` 阶段登记环境绑定的最终解析校验。生成项目验证配置覆盖与晚登记。工作区格式、严格 Clippy、带隔离 MySQL 8.4.13 的测试、拒警 Rustdoc、依赖检查及生成 check/test 均通过。日志和源码指纹见 `tmp/lead-app-lifecycle.z1NIwwzU/`。

应用 `terminating` 的请求和命令接线及 deferred Provider 映射仍待对应。

## Provider 实例回调限定验收

固定 Laravel `d52c3b3` 的 PHP 8.5 对照覆盖依赖注入、两组实例队列、动态追加、强制替换与注册失败。Rust 沿 `BootContext` 显式解析依赖，使用同一容器。

工作区回归验证实例隔离、普通去重、注册失败退役候选回调、强制成功替换旧实例、运行时 `booted` 失败后保留已登记实例，以及应用清空。示例缓存 Provider 在 `boot` 安装驱动，在实例 `booted` 阶段补充缺省 store。严格 Clippy、工作区测试、拒警 Rustdoc、依赖检查及生成项目 check/test 均通过。最终测试单独区分候选和旧实例的析构计数，对受影响检查和两个回归完成复验。证据位于 `tmp/lead-provider-boot.yTtO3qMy/`。

## 应用终止回调限定验收

固定 Laravel `d52c3b3` 在 PHP 8.5.12 验证依赖注入、同轮追加、每轮重放、错误中止和 `flush` 清空。Rust 通过 `ApplicationState` 解析服务，回归覆盖命令成功、帮助、参数错误和未知命令，以及清空时隔离旧遍历。

HTTP/1.1 真实连接覆盖定长、HEAD、204、trailers 和 404。进程内回归覆盖正文报错、流中断，以及 HTTP/2 标记下长度耗尽但尚未结束的流。生成项目实际执行 `serve` 并收到 SIGTERM，HTTP 和控制台回调分别解析到启动及关闭状态，进程退出码为 0。综合示例在 Provider 中登记入口完成 trace。

工作区格式、严格 Clippy、MySQL 8.4.13 工作区测试、拒警 Rustdoc、依赖检查及生成项目检查和测试均通过。工作区 2,843 项通过、54 项忽略，生成项目 24 项通过。证据在 `tmp/lead-terminating.UVmUAi0g/acceptance.json`。完整内核终止事件、终止 middleware 与通用 `Container::call` 继续按待办推进。

## 内核终止事件限定验收

PHP 8.5.12 执行固定 Laravel `d52c3b3` 的 HTTP 与控制台 `Kernel::terminate`，共 6 个场景。两个入口派发同一类事件，停止传播返回后继续终止流程，派发抛错时中止。记录型派发替身用于观察顺序，Rust 则通过现有事件总线验证类型、通配监听和传播边界。

控制台回归覆盖成功、帮助、错误优先级和显式 `Application::terminate`。HTTP/1.1 真实连接验证事件先于应用回调，进程内回归覆盖无应用回调、事件失败保留响应、流中断和已缓存空事件总线的 `before_resolving` 钩子。示例模块登记同一类型监听。生成应用实际执行 `list`，输出内核事件再输出应用回调。

格式、严格 Clippy、拒警 Rustdoc、依赖检查与生成项目检查通过。隔离 MySQL 8.4.13 工作区测试 2,849 项通过、54 项忽略，生成项目 23 项通过。证据在 `tmp/lead-kernel-termination.rTtVv1zt/acceptance.json`。后续的 HTTP 终止中间件与两个内核的生命周期时长处理器各有独立验收。

## HTTP 终止中间件限定验收

PHP 8.5.12 执行固定 Laravel `d52c3b3` 的 HTTP 内核终止流程和容器解析，7 个场景对照了路由与全局次序、参数剥离、短路、未匹配、禁用、实例生命周期及错误中止。PHP 请求和路由选择由替身构造。Rust 真实 TCP 覆盖路由组展开与优先级、相同声明去重、短路、404、错误中止、瞬态重建和单例复用，并核对最终响应头、匹配路由及请求属性。综合示例使用原有响应标记路由，生成应用用公开 API 登记后实际取得 HTTP 200 和终止记录，SIGTERM 退出码为 0。

格式、严格 Clippy、拒警 Rustdoc 和依赖检查通过。隔离 MySQL 8.4.13 工作区测试 2,849 项通过、54 项忽略，生成项目 23 项通过。证据在 `tmp/lead-terminable-middleware.amnKmfCr/acceptance.json`。后续的路由排除和测试夹具禁用见下节。

## 中间件排除与测试禁用限定验收

PHP 8.5.12 执行固定 Laravel `d52c3b3` 的路由中间件解析、Pipeline 与 HTTP 内核终止逻辑，通过 12 个对照场景。覆盖裸别名、精确参数、嵌套组、重复项、短路、全量禁用和保留链上的未知名称。Rust 的类型化别名使用已注册的类型精确匹配。

Rust 回归验证排除项的工厂不执行、保留链的未知名称仍报错、路由排除后全局终止项仍执行。测试夹具禁用应用中间件后仍派发终止事件并运行应用回调。唯一示例对照响应标记的有无。生成应用真实 HTTP 返回 200，分别验证启用与排除时的响应标记和终止记录，收到 SIGTERM 后正常退出。

格式、严格 Clippy、拒警 Rustdoc 和依赖检查通过。使用隔离 MySQL 8.4.13 的工作区测试 2,853 项通过、54 项忽略，生成项目 23 项通过。证据见 `tmp/lead-middleware-exclusions.4BpygBFC/acceptance.json`。后继分别完成[运行时开关](#运行时中间件开关限定验收)和[测试类型替换](#测试中间件类型替换限定验收)。

## 测试中间件类型替换限定验收

PHP 8.5.12 对照固定 Laravel 的按类直通、别名参数、终止、恢复与全局开关独立。Rust 四项回归验证类型跳过时不构造原实例、恢复与跨应用隔离、reset 重用蓝图，以及流式响应按原决定完成终止。唯一示例覆盖命名组的响应标记和路由排除。

隔离工作区格式、严格 Clippy、拒警 Rustdoc 和依赖门禁通过。使用真实 MySQL 8.4.13 的工作区测试 2,907 项通过、54 项忽略。生成应用 24 项通过，独立测试使用公开类型替换 API，普通服务器真实 TCP 执行原中间件并返回 HTTP 200，SIGTERM 正常退出。证据见 `tmp/lead-typed-middleware.VLJnB9pR/independent-result.json`。

## 可空认证账户默认绑定限定验收

固定 Laravel `d52c3b3` 的 AuthServiceProvider 在 PHP 8.5.12 验证直接解析与可空构造依赖、访客、账户同一性、回调替换和错误传播。Rust 使用瞬态 `Option<AuthUser>`，回归核对两作用域、当前回调和原错误。唯一示例在 MySQL 8.4.13 实测 Session 访客、登录、退出及 CSRF，并移除 Session/Cookie Provider 后验证显式 JWT 账户资源、拒绝无效凭据和无 Set-Cookie。

格式、默认及 auth-fortify 严格 Clippy、拒警 Rustdoc 和依赖检查通过。工作区 2,911 项通过、54 项忽略，另两项 MySQL 认证入口均通过。新生成 MySQL 认证应用 54 项通过、2 项忽略，用公开容器 API 验证访客和真实账户对象，普通服务器通过真实 TCP 返回访客和 HTTP 200，SIGTERM 退出 0。证据见 `tmp/lead-auth-account-binding.a9wL52kt/delivery-receipt.json`。

## BindWhen 条件内同键重绑限定验收

PHP 8.5.12 执行固定 Laravel `d52c3b3` 的真实容器，验证条件同键登记后命中覆盖、未命中继续构造原类、抛错保留已登记工厂，以及条件内缓存的共享值由原类替换。Rust 回归覆盖同一行为，另外验证并发解析等待共享槽、独立重绑取消旧选择。唯一示例以 conditional 环境走 HTTP 注入。

隔离工作区通过格式、严格 Clippy、拒警 Rustdoc 和依赖检查，真实 MySQL 8.4.13 工作区测试 2,911 项通过、54 项忽略。生成应用 24 项通过，真实 TCP 返回声明实现和 HTTP 200，SIGTERM 正常退出。验收输入、固定 PHP 对照和门禁日志见 `tmp/lead-bindwhen-write.eLUaFXPj/delivery-receipt.json`。

## 内核生命周期时长处理器限定验收

PHP 8.5.12 运行固定 Laravel `d52c3b3` 的 HTTP 与控制台 Kernel，8 个场景验证严格阈值、结束时间快照、时区、错误中止和动态登记。业务路由与 Artisan 执行体使用受控替身。Rust 采用每入口独立的单调计时，按当前应用时区投影开始时刻。

真实 HTTP 回归在终止事件、路由和全局中间件、应用回调之后观察响应及路由。控制台回归覆盖帮助、参数错误、错误优先级、并行入口隔离、动态登记和无效时区。唯一示例登记慢请求与慢命令处理器。新生成应用经真实 TCP 返回 HTTP 200，收到 SIGTERM 后退出 0，控制台实测成功与失败退出码分别为 0 和 1。

格式、严格 Clippy、拒警 Rustdoc 和依赖检查通过。隔离 MySQL 8.4.13 的工作区测试 2,857 项通过、54 项忽略，生成项目 23 项通过。证据在 `tmp/lead-kernel-duration.ykRF2YC6/acceptance.json`。HTTP 使用无正文快照，控制台状态沿用当前 Rust 命令的结果类型。

## 容器 callable 限定验收

固定 Laravel `d52c3b3` 使用 PHP 8.5.12 通过 9 个 Container 对照场景，覆盖显式空值、属性后置、类名实参、默认值、变参与绑定方法分流。Rust 本批覆盖直接 callable 的参数选择和已取得实例的方法调用入口。后续对象方法绑定及类入口见下节。

Rust 回归验证命名覆盖、属性回调、类实参单次消费、变参类型和剩余实参顺序。调用在原作用域解析，工厂看不到调用实参。并发验收验证 scope 关闭中止 pending callable，并等待显式参数完成析构。唯一示例通过 HTTP 调用配置驱动的模块服务。新生成应用以真实 TCP 返回 HTTP 200，验证命名参数和同一 scoped 实例，SIGTERM 退出 0。

格式、严格 Clippy、拒警 Rustdoc 和依赖检查通过。隔离 MySQL 8.4.13 的工作区测试 2,861 项通过、54 项忽略，生成项目 23 项通过。证据见 `tmp/lead-container-call.fGj6i3D9/acceptance.json`。

## 容器方法绑定与类入口限定验收

固定 Laravel `d52c3b3` 使用 PHP 8.5.12 通过 8 个对照场景，覆盖接收者解析次序、显式对象身份、参数短路、绑定替换、默认入口、上下文恢复与清空后保留绑定。

Rust 用已声明类型与方法名分派。回归验证显式对象及类入口的上下文差异、默认 `__invoke`、闭包式直接方法、嵌套同类型调用和运行中换绑。注册与调用沿同一容器和原入口作用域。唯一模块实际调用已登记的普通方法和替代绑定。新生成应用真实 TCP 返回 HTTP 200，验证声明的命名参数和替代绑定的同一作用域实例，SIGTERM 退出 0。

格式、严格 Clippy、拒警 Rustdoc 和依赖检查通过。隔离 MySQL 8.4.13 的工作区测试 2,868 项通过、54 项忽略，生成项目 23 项通过。证据见 `tmp/lead-bound-method.wSKURNTc/acceptance.json`。后续静态方法类型上下文见下节。

## 容器静态调用上下文限定验收

固定 Laravel `d52c3b3` 使用 PHP 8.5.12 通过 5 个对照场景，覆盖直接静态 callable、命名替代绑定、继承时的声明类型和调用类型、无接收者、异常恢复与嵌套上下文。Rust 调用者显式指定编译类型，调用由 Rust 关联函数适配器执行。

Rust 回归覆盖命名绑定短路、原参数覆盖、嵌套同类型调用、异常恢复及取消后旧解析器不再携带静态上下文。唯一示例通过同一路由观察直接与命名静态调用。新生成应用真实 TCP 返回 HTTP 200，直接入口与命名替代绑定使用同一请求作用域并解析声明类型依赖，SIGTERM 退出 0。

格式、严格 Clippy、拒警 Rustdoc 和依赖检查通过。隔离 MySQL 8.4.13 的工作区测试 2,871 项通过、54 项忽略，生成项目 23 项通过。证据见 `tmp/lead-static-call.BRte8QO5/acceptance.json`。

## 运行时中间件开关限定验收

固定 Laravel `d52c3b31` 使用 PHP 8.5.12 的实际 HTTP Kernel 和 Router 通过 12 项。覆盖严格布尔、三阶段重新解析、全局影响路由和终止事件切换。Rust 回归验证同一 Router 上禁用与恢复，覆盖 CORS、请求 ID、已匹配路由、终止顺序和三阶段求值。唯一示例在根应用装配下验证响应标记与请求 ID。

格式、严格 Clippy、拒警 Rustdoc 和依赖检查通过。隔离 MySQL 8.4.13 的工作区测试 2,871 项通过、54 项忽略。新生成应用 23 项通过，真实 TCP 验证同一监听进程禁用并恢复全局及路由中间件，SIGTERM 正常退出。证据见 `tmp/lead-middleware-runtime.GKMGl9BX/acceptance.json`。

## 延迟 Provider 注册器前置验收

新的注册回归覆盖父注册等待期间另一 Provider 使用已发布子项完成注册，后续取消保留已完成槽位。既有嵌套、强制替换、启动和回收回归随工作区执行。格式、严格 Clippy、拒警 Rustdoc 和依赖检查通过。隔离 MySQL 8.4.13 的工作区测试 2,872 项通过、54 项忽略。新生成应用 23 项通过，真实 TCP 验证嵌套服务可用及启动回调完成，SIGTERM 正常退出。

固定 Laravel `d52c3b31` 的两个 PHP 对照记录各阶段首次解析、事件、显式清载和真实 manifest 缓存。本批验收覆盖共享注册器前置。容器首次解析和事件触发的延迟装载仍待接线。证据见 `tmp/lead-deferred-provider.9M1HjQmY/acceptance.json`。

## 共享 Provider 贡献限定验收

运行期启动句柄沿同一应用共享贡献、命令和资源。类型槽回归验证写入一种贡献期间另一种仍可写。HTTP 装配用同一注册快照取得路由和 URL 命名规则，异常 Handler 与 gRPC 调用在释放借用后使用快照。

格式、严格 Clippy、拒警 Rustdoc 和依赖检查通过。隔离 MySQL 8.4.13 的工作区测试 2,874 项通过、54 项忽略。gRPC 严格 Clippy、服务端 8 项、独立 consumer 全 target check 及 16 个独立锁图通过。新生成应用 23 项通过，真实 TCP 验证共享贡献、启动回调和请求解析，SIGTERM 正常退出。证据见 `tmp/lead-provider-contributions.kYOybDpT/acceptance.json`。

## 延迟 Provider 按服务键装载限定验收

固定 Laravel 在 PHP 8.5.12 下对照首次触发、共享实例优先、别名、显式装载全部、失败及应用启动各阶段。框架回归覆盖这些行为，并验证并发、作用域关闭、取消、注册跨启动边界及清空。

工作区格式、严格 Clippy、拒警 Rustdoc 和依赖检查通过。使用隔离 MySQL 8.4.13 的工作区测试 2,885 项通过、54 项忽略。新生成应用通过检查与 23 项测试，真实 TCP 首次请求通过依赖注入触发 Provider 并读到已完成 boot 的实例。SIGTERM 正常退出。事件触发及持久 manifest 待实现。证据见 `tmp/lead-provider-on-resolution.bBWf0u5v/acceptance.json`。

## 延迟 Provider 事件装载限定验收

固定 Laravel 的 PHP 8.5.12 对照覆盖 eager 注册前监听、精确及通配顺序、`until` 原对象、事件独立装载和失败重试。Rust 回归另覆盖事件与按键解析并发、清空应用后的派发器生命周期及显式移除事件服务。

隔离工作树的格式、严格 Clippy、拒警 Rustdoc 与依赖门禁通过。隔离 MySQL 8.4.13 的工作区测试 2,890 项通过、54 项忽略。新生成应用 23 项测试通过，真实 TCP 首次请求以事件触发装载并验证 boot 完成，SIGTERM 正常退出。证据见 `tmp/lead-deferred-events.VwcR8QUn/v5/independent-result.json`。持久 manifest 仍待实现。

## Provider 编译服务清单限定验收

固定 PHP 8.5.12 对照默认与覆盖路径、冷启动与复用、名单重排失效、缺失字段及缺失父目录。Rust 使用 JSON 和当前编译声明的 TypeId，覆盖具名与类型键、声明变更、文件格式和大小、并发发布及安全清除。

隔离工作树格式、严格 Clippy、拒警 Rustdoc 和依赖检查通过。MySQL 8.4.13 的工作区测试 2,900 项通过、54 项忽略。新生成应用 23 项通过，三次真实 HTTP 覆盖默认冷启动、暖启动复用与路径覆盖，`clear-compiled` 保留配置缓存。包清单的发现与生成保持开放。证据见 `tmp/lead-services-manifest.Zg7LAdza/v2/independent-result.json`。

## R2-A 类型化容器基元

新增 src/core/container.rs，15项回归实际通过：typed dyn接口、覆盖与bind生命周期、singleton并发单次构造/应用隔离、root scoped/reset与子scope、同链/跨根等待环、取消恢复、嵌套缺绑定码/链、source保留与秘密Debug/HTTP脱敏、在途reset、scope Drop缓存释放、保存Resolver延迟解析、关闭singleton owner/waiter、业务析构重入、弱句柄引用环释放。scope/工厂全用显式句柄，不创建全局容器。

冻结源码后实际执行 cargo fmt --all --check；默认及all-features workspace all-target Clippy -D warnings；cargo test --workspace（279 passed/22 ignored）与all-features（502 passed/32 ignored）；warnings-denied默认/full workspace doc；cargo deny check；cargo +1.97.0 check --workspace --all-targets --all-features；root no-default check，全部通过。忽略和无服务skip不记真实外部服务通过。本簇未改DB/queue/auth/配置加载或CLI模板行为，不将R1双库/生成验证转记新Provider通过。

/tmp/openrust-r2-container-consumer-mqyjcnb4 是仅依赖公开openrust的独立下游，check/test通过，1项真实公开dyn绑定/root reset/scope断言；首轮fixture错误使用Tokio重导出属性宏且未声明直接Tokio包，已改为公开Runtime显式block_on后重跑通过，未因此改变框架API。根argv恰为cargo run，/health、HTML、/api、/api/example各200，SIGINT0。启动仍是旧生产Plugin路径，不把它当新Provider已接入。

不同模型gpt-6-sol逐轮静态审查发现秘密Debug、错误码/链和Resolver强引用环，均修复并补测试；最终冻结弱句柄版本无blocker/should-fix，审查没有替代运行。Error保留内部source，Debug仅code/status，HTTP仍按5xx隐藏详情。无新依赖，Cargo.toml/Cargo.lock未变。

资源：独占/tmp/openrust-r2-target在软限阶段确认无构建进程后dry-run/clean回收63458files/19.3GiB；最终再次dry-run/clean回收45539files/14.1GiB，磁盘余119GiB。未知归属root/consumer缓存未触碰，日志和临时consumer源码保留用于诊断。日志前缀/tmp/openrust-r2-。

本次仅容器基元；Provider/Registry切换、异步shutdown/managed任务与HTTP/job自动scope、配置合并/模块overlay、TestApp和modules迁移均未完成。AppBuilder仍使用唯一旧生产引擎，无新旧运行开关；后继按R2合同和todo继续。

## R1 当前工程边界与验收入口

按 历史决策 073，根 openrust package 同时提供框架 src/lib.rs 与默认 server 薄 src/main.rs。default-members=["."]、default-run="server"；Laravel 应用职责直接位于根，唯一 app/example 通过 app/mod.rs 注册薄业务 Plugin。以下是结构迁移子步骤的新执行记录，不表示 R1 综合 CRUD、全能力示例或 Laravel/modules 1:1 目标已完成。

本布局已实际通过 fmt、默认/all-features workspace all-target Clippy、默认/all-features workspace tests、warnings-denied 默认/all-features Rustdoc、deny 和 no-default check。默认 263 passed/22 ignored，全 feature 486 passed/32 ignored；默认不再由独立 example 包隐式强开 auth/grpc 等 feature，不与旧默认测试计数机械比较。根显式 34 个测试 target 保留原框架+demo 的 33 个与原 example auth HTTP 测试；模块注册有/无、普通业务路由、声明错误数据库启动失败回归已执行。底层框架源码、dependencies/features 未改；根锁无新增或升级 registry name/version/checksum，仅移除 20 个不再由根示例 build 使用的工具节点。

根无参数 cargo run 实际验证 /health、公开 HTML、/api 和 /api/example 均 200，业务响应包含 chrono::Utc::now 产生的时间；SIGINT exit 0。现有真实子进程 SIGTERM 退出测试保留。根 CLI config show 转发通过。

PostgreSQL 18.6/MySQL 8.4.13 各执行 cargo test --locked -p openrust --all-features 的 db_connection/db_migration/auth_database/auth_runtime/queue/schedule/auth_mfa/auth_oidc/queue_crash_recovery/schedule_calendar/queue_notifications/example_auth_flows，共 12 targets/48 passed/0 ignored；testing backend database 集成各 1 passed。原 example 的认证/JWT/PAT/入队流程已迁为模块专有测试并通过同一根 Server 装配。第一轮 fixture 未声明 app.name 而沿用默认 OpenRust，预期 openrust 的断言失败；现显式声明 fixture 名称后双库重跑通过，未改框架默认值。使用既有服务和 TestDatabase 隔离库，不修改业务数据或服务配置；测试 helper 的 Drop 清理为 best effort，不能把未能确认归属的历史隔离库批量删除或宣称均已清理。

/tmp 新生成普通/PG auth/MySQL auth 三类应用，均 check/test 通过，双库 auth 各 1 项真实 TCP 流程通过，普通工程公开 resource 宏回归与无参数 cargo run /health 200、SIGINT0 通过。进一步在普通工程仅添加 app/orders、修改 app/mod.rs 注册清单和声明已有 serde/chrono 依赖，src/main.rs 与 bootstrap/app.rs 哈希保持不变；check/test 与真实 /api/orders HTTP、正常退出通过。这是新增模块/第三方业务依赖使用证明，不是完整 CRUD。

根没有示例 build.rs/build-dependencies。模块自有 proto/generator.toml、generator.lock 在 /tmp 临时工具中 cargo run --offline --locked；generate.py --check 实际验证 generated/echo.rs 重生字节一致。生成类型通过 grpc feature 编译，不等于已注册本模块真实 gRPC 服务。

最终 CI 双库命令均通过：每库 cargo test --workspace --features db-<backend> 为 278 passed/22 ignored，auth,db-<backend> 为 324 passed/25 ignored；另显式 cargo test -p openrust --features auth,queue,view,encryption,db-<backend> --test example_auth_flows，各 2 passed/0 ignored。MySQL 使用具备隔离建库权限的测试管理员，未改服务配置。

Rust 1.97.0 的 cargo check --workspace --all-targets --all-features 通过，SQLite/queue consumer 实际 run、gRPC consumer locked check 通过；stable 下八个 consumers locked test/metadata 全通过，metadata 均独立单成员/默认成员。cache/communication/SQLite/queue/gRPC 实际 run 通过，gRPC 15 项与四类 RPC、realtime 真实 Redis 跨进程 1 项通过。SQLite 仍仅是自定义 driver consumer，不等于完整官方支持。

最终 README 更新后重新执行默认/full workspace fmt/Clippy/tests/docs、deny/no-default，全部通过，计数仍为 263/22 与 486/32；根裸 run/HTTP/退出再次通过。额外干净生成 gRPC 应用 check/test 通过（无业务测试，因此仅证明模板/Proto 编译，不代替真实 RPC）；生成项目锁独立解析，未修改源码根锁。模块 proto --locked 离线重生字节再次一致。代码独立审查的 proto 锁定/教程导入修复已落实；不同模型 gpt-6-sol 最终静态源码复审无 blocker/should-fix，未参与CI编辑的代理独立复核也无 blocker/should-fix，限定提交收口中。

最终确认无 Cargo/rustc/server 进程后，对独占 /tmp/openrust-r1-root-target 先 cargo clean --dry-run 再 cargo clean，实际回收 44051 files/14.1 GiB，磁盘剩余 118 GiB（约13%）。本任务在软限时也只于安全阶段清理该独占目录；未知归属根 target 与预存 consumer 缓存不清。/tmp 生成项目和日志保留作本轮证据。后继未挂载 app/example/extensions.rs 与未审 R2 contract 是独立 WIP，不计本次已验证功能。

以上仅为 历史决策 073 过渡路径的 R1-S 结构子交付。最终 modules/example 目录迁移依 ADR-001/R2-M；统一 Provider、完整 CRUD、全能力示例、供应商实发及完整集群尚未交付或不在本子步骤范围，不能转记为这些目标通过。

## R1 历史决策 072 历史验收事实

以下是 14c143f 及其前置布局的实际记录，保留原命令和目录以追溯当时证据；它们不是当前可直接运行的路径或本次 历史决策 073 验收。

按 历史决策 072，根 openrust/src 是框架库，examples/demo 是根 workspace 成员，共用根 Cargo.lock，无嵌套 workspace。根 default-members=["examples/demo"]，根与 demo 目录无参数 cargo run 默认 server；全部原 tests/server 迁入 examples/demo/tests/server。CLI 模板是唯一骨架来源；本轮迁移实现与适用验收已完成。

本轮同一 workspace 已实际通过：cargo fmt --all --check，默认/all-features workspace all-target Clippy，默认/all-features workspace test，默认及 warnings-denied all-features Rustdoc，cargo deny check。默认 361 passed / 26 ignored；all-features 481 passed / 32 ignored。另显式 cargo test -p openrust --lib 为 146 passed / 0 ignored。根锁图仅把原 application 本地节点变为 demo，注册表 name/version/checksum 集合完全不变。

现有 server 进程回归已从 workspace 根启动并验证应用公开 HTML 首页、ready 与 SIGTERM 退出。Cargo 保留调用 cwd，故 demo 薄入口仅在从 workspace 根启动时切换到应用目录；其余 cwd 保持既有部署行为，框架配置 API 不变。

根与 demo 目录均实际执行无参数 cargo run，/health 200、/ 为应用公开 HTML 且 200、SIGINT exit 0。CLI 从 workspace 根按 Cargo metadata 唯一默认成员选择 demo，新增回归测试及实际 config show 转发通过。

真实 PostgreSQL 18.6/MySQL 8.4.13：各以 cargo test --locked -p demo --all-features 执行 db_connection/db_migration/auth_database/auth_runtime/queue/schedule/auth_mfa/auth_oidc/queue_crash_recovery/schedule_calendar/queue_notifications，共 11 targets/46 passed/0 ignored；testing 的 backend database 集成各 1 passed。另按最终 CI 执行 cargo test --workspace --features db-<backend> 与 auth,db-<backend>：各后端分别 373 passed/26 ignored 与 392 passed/26 ignored，包含 demo 与 example 的真实隔离库/HTTP 测试。MySQL 这些入口使用有建库权限的测试管理员；不修改共享服务配置或业务数据。真实复制/完整集群不在本轮验收范围。

/tmp 干净创建普通、PG auth、MySQL auth 三类用户应用，均 check/test 通过；双库 auth 各 1 项真实 TCP/隔离库流程通过，普通工程临时资源宏测试与 cargo run /health 200、SIGINT0 通过。metadata 确认普通工程仅自身、数据库工程仅自身与迁移两个成员，均有自己的 workspace/lock 与单独默认应用，不继承源码 demo 或工具成员；该生成能力不改变框架与 demo 同仓同 workspace 的维护方式。

根 openrust 与 demo 的 no-default check 通过；cargo +1.97.0 check --workspace --all-targets --all-features 通过；同一 MSRV 下独立 SQLite/queue consumer 实际 run 与 gRPC consumer locked check 通过。8 个公开 consumers 均通过 stable locked test/metadata；cache/communication/SQLite/queue/gRPC 实际 run 通过，gRPC 15 项测试与四类真实 RPC 通过，realtime 1 项真实 Redis 跨节点测试通过。SQLite consumer 仅证明明确拒绝框架迁移锁后用自有 Migrator、表名 scope、连接/事务/回滚，不代表 R6 完整支持。

最终不同模型 gpt-6-sol 只读复审无 blocker/should-fix，核对 6+27 tests、28 个 feature 映射、根默认启动/CLI 生成隔离与 CI；审查未代替以上实际执行。所有当前 Markdown 本地链接/锚点、旧路径与锁图核对通过。本任务 root target 与临时生成项目已清理；root target 随后被后台重新生成约 344 MiB，归属未确认，连同预存且非本任务独占的 consumer 缓存保留。

### 中间布局的历史行为证据

以下记录对应根框架与独立 demo 的中间布局，仅保留命令行与真实服务行为证据，不作为同一 workspace 已验收。

中间布局实际通过：框架 fmt、default/all-features all-target Clippy、tests、warnings-denied docs、deny；workspace 默认 344 passed / 26 ignored，all-features 387 passed / 30 ignored。独立 demo 默认与 all-features Clippy/test/doc 通过，分别 16 passed / 0 ignored 与 93 passed / 2 ignored。忽略和因环境缺失提前返回的用例不算真实服务通过。

真实 PostgreSQL 18/MySQL 8.4 各执行 `cargo test --locked --manifest-path examples/demo/Cargo.toml --all-features --test db_connection --test db_migration --test auth_database --test auth_runtime --test queue --test schedule --test auth_mfa --test auth_oidc --test queue_crash_recovery --test schedule_calendar --test queue_notifications -- --nocapture`：各 11 targets / 46 passed / 0 ignored；另 testing package 的 backend database 集成各 1 passed。使用既有服务、TestDatabase 隔离数据，不修改共享容器/配置。复制 lag 分支未提供拓扑，不算真实复制验收。

干净生成普通、PostgreSQL auth、MySQL auth 三工程，均 check/test 通过；认证两工程各有 1 项真实 TCP/隔离库测试。普通工程以临时下游资源宏测试验证公开 openrust 路径，普通 cargo run 后 /health 200、SIGINT exit 0。metadata 确认普通工程仅自身一个成员、数据库工程仅自身与迁移两个成员，workspace 根均为生成目录，不继承开发 exclude；框架本地路径指向仓库根。

独立 demo 应用根无参数 cargo run 后真实 TCP /health 200、SIGINT exit 0。上述命令与 all-features server 子进程测试串行执行，无二进制覆盖。根/独立 demo 的 no-default check、Rust 1.97 all-target/all-features check 与 demo cargo deny 均通过，deny 仅保留既有依赖重复告警。8 个独立 consumers 均 locked metadata/test 通过；cache/communication/SQLite/queue/gRPC 实际 run 通过，gRPC 15 项测试与四类真实 TCP RPC 完成，realtime 1 项 Redis 跨节点测试通过。SQLite 原示例错误期待框架迁移互斥支持 SQLite，现仅修 consumer：断言明确拒绝后执行自有 Migrator/表名 scope；连接/事务/回滚仍通过，不算 R6 官方支持。gRPC 多 bin 的 CI/README 已明确选 grpc-example，应用默认 cargo run 不受影响。

不同模型独立审查发现 MySQL CI 普通用户缺 CREATE DATABASE 权限，新增 demo 整组已改 root；该记录为中间状态；最终同一 workspace 已复验修后双库 feature 命令，结果见本节开头。

## R1 旧布局验收事实（历史决策 071）

以下保留旧 历史决策 071 根 application/server 与 framework/openrust 布局的实际验收；不能作为 历史决策 072 新布局通过。目录与分发依据旧 历史决策 071。根为 application/server，框架库为 framework/openrust，默认成员只有根应用；apps/example 已迁为 examples/example。下文保留的原专项记录是迁移前行为证据，含 `cargo test -p openrust --test ...` 的旧命令；当时应用集成 target 使用 `-p application`，框架单元测试仍用 `-p openrust --lib`。

实际执行：fmt，默认/all-features workspace all-target Clippy，默认/all-features workspace test，默认及 warnings-denied all-features Rustdoc，cargo deny；MSRV `cargo +1.97.0 check --workspace --all-targets --all-features`，no-default-features check。依赖审计仅保留既有重复版本告警。最终默认 workspace 349 passed / 26 ignored，all-features 480 passed / 32 ignored；忽略或缺服务导致 skip 不计真实服务通过。

真实 PostgreSQL 18 与 MySQL 8.4 各执行 `cargo test -p application --all-features --test db_connection --test db_migration --test auth_database --test auth_runtime --test queue --test schedule -- --nocapture`，各 6 targets、31 passed、0 ignored；从现有服务读取授权测试连接，使用 TestDatabase 隔离数据，不改共享容器/配置。数据库连接 target 中依赖真实复制的分支仍有独立环境门禁，本次未提供复制拓扑；完整复制/集群不属于 R1 新验收。

干净 `/tmp` 默认生成应用 check/test 通过，普通 cargo run 后真实 TCP `/health` 为 200、SIGINT 正常退出。PG/MySQL 认证生成应用分别 check/test 通过，`generated_auth_routes_run_over_http_with_isolated_database_and_fake_mail` 各真实执行 1 项；使用当前模板、database/migrations 与 resources/views。发现模板 fake 配置缺 view 段后已同步修复模板/综合示例并重新生成验收。公开 `tests/consumers/cache-custom-driver` 已通过 locked 实际运行，所有 consumer lock 同步本地框架依赖图。

根普通 cargo run（无 --bin/--package）真实 TCP `/health` 返回 200，SIGINT exit 0。

验证隔离：默认 cargo run 与 all-features 子进程测试禁止交叉占用同一 target/debug/server。首次 all-features metrics 子进程超时发生在并行默认 cargo run 覆盖二进制期间；停止交叉运行后整组重跑通过。此项不是新语义验收或放宽超时。原真实供应商/复制/集群、SQLite、最低后端版本与 R2 以后 contract 缺口继续保留。

## M5 WebSocket 与 SSE 跨节点路由

按 历史决策 059 启用 `realtime` / `realtime-redis`。在本机已有 Redis 8.10.2 上通过独立示例执行：

```bash
OPENRUST_REALTIME_TEST_REDIS_URL=redis://127.0.0.1:6379/ \
  cargo test --locked --manifest-path tests/consumers/realtime/Cargo.toml --test cluster -- --nocapture
```

2026-10-27 实际通过：两个独立 API 子进程经 Redis Pub/Sub 路由 WebSocket 和 SSE；真实 TCP 客户端验证 Origin allowlist、跨节点用户撤销、SIGTERM 时两类会话关闭与进程退出。测试随后启动一个 Redis 地址为 `127.0.0.1:1` 的 API 进程，确认在线发布返回 `realtime_transport_unavailable`，不存在本地静默成功。另 `cargo test --manifest-path examples/demo/Cargo.toml --features realtime --test realtime -- --nocapture` 通过 AuthLayer、Origin 与 SSE 响应头聚合 HTTP 测试；实时 service 单元测试 4 项覆盖过期、载荷/Origin 限制、连接投递/撤销、慢消费者上限。

本簇门禁实际通过：workspace fmt、默认/all-features all-target Clippy、默认/all-features workspace tests、warnings-denied all-features workspace Rustdoc、根与独立示例 cargo-deny、独立示例 all-target/all-features Clippy 及 `git diff --check`；真实跨节点命令如上。cargo-deny 的 advisories/bans/licenses/sources 均通过，保留既有重复依赖与未匹配 MPL allowance 警告。整组验证结束时根 `target/` 与 `tests/consumers/realtime/target/` 合计约 13 GiB，所在卷可用 86 GiB（低于 10%）；无构建进程、Cargo dry-run 分别预览根 target 12.5 GiB 与示例 target 2.7 GiB 后，对两处 target 执行 Cargo clean，回收约 15.2 GiB；清理后仓库 target 均为空，卷可用空间为 99 GiB（约 10.7%）。软告警剩余量由仓库外磁盘占用造成，未删除其他文件。

该证据不代表 Redis 断开期间 Pub/Sub 可恢复消息，也不代表 PostgreSQL/MySQL 全栈集群、负载均衡、worker/scheduler、滚动升级或性能门禁通过；M6 仍需复验。用法与边界见 [tests/consumers/realtime/README.md](../../../tests/consumers/realtime/README.md)。

## M5 gRPC G1 基础与生成

按 历史决策 040 增加可选 `grpc` feature、配置校验、服务注册/冲突检测、默认及具名共享 Channel、`grpc serve` 命令、应用自有 proto/build.rs 生成和 loopback TCP 示例。默认 feature 不启用 tonic/prost/protoc；G1 只接受明文 `http://` client endpoint，TLS/认证/health/配额由 G2 交付。

服务端集成测试 3 项通过（注册与真实 TCP 生命周期、重复名称拒绝、未注册服务失败）；独立示例实际执行 unary、server stream、client stream、bidirectional stream、半关闭与断连清理。CLI `--grpc` 模板测试通过；在无 `.git`、无 target 的干净源码副本生成应用后 `cargo check` 和 `cargo test --locked` 通过，无绝对路径或 registry patch。默认 workspace 测试与 `RUST_TEST_THREADS=1 cargo test --workspace --all-features` 通过；all-features 包含真实 Redis targets。all-target/all-features workspace 与独立示例 Clippy、warnings-denied 默认/all-features Rustdoc、workspace 与独立示例 `cargo deny`、fmt/diff check 通过。deny 仅保留既有重复依赖版本及未匹配 MPL allowance 告警。`cargo +1.97.0 check` 的框架 gRPC feature 与独立示例均已通过；默认依赖树不含 tonic/prost/protoc。未覆盖 G2 安全语义和 G3 多节点/联合运行。

## M5 gRPC G2a TLS/mTLS

G2a 已在真实 loopback TCP 上验证服务端 TLS、具名客户端 CA/域名校验、双向 TLS 客户端身份以及错误 CA、错误域名、过期服务端证书、缺失/不受信客户端证书的拒绝。示例测试由 `rcgen` 临时生成证书，不依赖外部凭据。配置单测覆盖明文/HTTPS 与 TLS 材料不匹配、客户端身份证书配对、默认具名端点校验和 Debug 脱敏；`examples/demo/tests/server/grpc.rs` 覆盖未显式 opt-in 时明文 server 拒绝。应用生成模板只在开发配置显式允许 loopback 明文。

本簇已通过 `cargo test --workspace`、`RUST_TEST_THREADS=1 cargo test --workspace --all-features`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、独立示例 Clippy、默认/all-features warnings-denied Rustdoc、workspace 与独立示例 `cargo deny`、Rust 1.97 默认与 gRPC feature check、独立示例 `--locked` check/test/run、干净源码副本生成应用 check/test、`cargo fmt --all --check` 和 `git diff --check`。`cargo deny` 的重复版本与未命中 MPL allowance 为已有提示，advisories/bans/licenses/sources 均通过。未覆盖身份/授权、deadline/health/shutdown 或多节点验收。

## M5 gRPC G2b 身份与错误映射

`tests/consumers/grpc/` 的真实 TCP RPC 测试覆盖已签名 JWT、数据库 PAT 和测试替代 Authenticator 路径，包含缺失/无效/重复 Authorization、Cookie Session 不参与 RPC、JWT/PAT `CurrentUser` 与 PAT abilities、Policy 授权、缺 ability 拒绝及 PAT 撤销后拒绝。PostgreSQL 18 与 MySQL 8.4 的隔离数据库各自真实执行 `database_auth_tests::database_bearer_and_method_policy_work_over_real_tcp`，通过 `OPENRUST_TEST_DATABASE_URL=... cargo test --manifest-path tests/consumers/grpc/Cargo.toml --locked --features db-<backend> database_auth_tests::database_bearer_and_method_policy_work_over_real_tcp -- --nocapture`；测试贯通真实 `AuthPlugin`、JWT 验签、PAT 读写/撤销、Policy 表、gRPC TCP handler 和 Status。错误映射单测覆盖 validation、permission 与内部错误脱敏；网络用例确认状态码为 `UNAUTHENTICATED`/`PERMISSION_DENIED`，不回传消息或凭据。

通过 `cargo fmt --all --check`、`cargo clippy -p openrust --features 'grpc auth' --all-targets -- -D warnings`、独立示例 all-target/all-features Clippy、默认与 all-features workspace tests、独立示例 all-features tests、warnings-denied 默认/all-features Rustdoc、workspace 与独立示例 `cargo deny`、Rust 1.97 gRPC+auth check、独立 consumer locked check/test、PostgreSQL/MySQL 真实 TCP 测试及 `git diff --check`。全功能 workspace 与 cargo-deny 保留既有明确记录的重复 crate 版本和未命中 MPL allowance 提示；advisories/bans/licenses/sources 通过。未覆盖 deadline、全局限额、health/reflection、有界停机或集群。

## F8-A CSV/XLSX 解析与转换

- 定向入口：`cargo test -p openrust --features data-exchange --lib data::exchange::tests`、`cargo test --manifest-path examples/demo/Cargo.toml --features data-exchange --test data_exchange`。
- 独立 consumer：`cargo check --manifest-path tests/consumers/data-exchange/Cargo.toml --locked` 与 `cargo test --manifest-path tests/consumers/data-exchange/Cargo.toml --locked`；示例通过相对 path 依赖使用公开 API，不含 registry patch 或开发者机器绝对路径。
- 核心测试 8 项通过，覆盖 CSV BOM/类型/逐行错误/重复表头/输入上限/行与全局错误上限/callback 失败/公式注入，XLSX 类型保真/公式拒绝/shared string 声明上限/宏拒绝/ZIP 展开上限，以及 根目录集成宿主 feature 接入 1 项通过。
- `cargo fmt --all --check`、默认和 all-features workspace clippy/tests、warnings-denied 默认与 all-features Rustdoc、`cargo check -p openrust --no-default-features`、`cargo deny check` 均通过。cargo-deny 保留既有 duplicate dependency 与未匹配 MPL allowance 警告，advisories/bans/licenses/sources 通过。
- F8-B 的双库任务化验收见下文“F8-B 数据交换任务”。

## F8-B 数据交换任务

`examples/demo/tests/server/data_exchange_tasks.rs` 使用 PostgreSQL 18 和 MySQL 8.4 的隔离测试库、真实 `DatabasePlugin`/`QueuePlugin` 与本地 Storage driver。两个后端均实际执行 `import_batches_are_atomic_idempotent_and_export_is_streamed` 并通过。测试覆盖 CSV 分批事务、重复请求回放、第二批失败后的部分报告与重试恢复、取消后的报告和续跑、字段错误脱敏、tenant 作用域隔离、导出流式落盘、已取消导出不发布文件、attempt 路径隔离、过期代际保护、清理时对象锁忙拒绝删除，以及 QueuePlugin 自动注册并执行报告清理 job。对象上传完整返回后才写保留标记并排期清理；SharedLocks 租约每 5 分钟续期、单次续期有 30 秒超时。续租失败会令任务返回错误，但不会丢弃可能已提交的 Storage future，故不保证数据库长时间不可用时外部 Storage exactly-once。已通过命令：

```bash
cargo test --manifest-path examples/demo/Cargo.toml --features 'data-exchange-tasks storage-local db-postgres' --test data_exchange_tasks -- --nocapture
cargo test --manifest-path examples/demo/Cargo.toml --features 'data-exchange-tasks storage-local db-mysql' --test data_exchange_tasks -- --nocapture
```

独立 consumer `tests/consumers/data-exchange/` 启用可选 feature，通过 `cargo check --manifest-path tests/consumers/data-exchange/Cargo.toml --locked` 与 `cargo test --manifest-path tests/consumers/data-exchange/Cargo.toml --locked`。consumer 演示应用从受保护查询生产行并交给有界导出 sink；框架不代替应用授权，也不提供下载路由。事务只保证单个导入批次内业务写入与幂等完成标记原子；已提交批次在取消或后续失败时保留，导入副作用至少一次，外部副作用仍须由应用自行幂等。`RequestIdempotency` 现有上限为 24 小时；报告保留默认 7 天、最多 30 天。

## M5 可选外部身份：OIDC（F7 子簇）

`cargo test -p openrust --features auth-oidc auth::oidc::tests --lib` 覆盖 ID Token 签名、issuer/audience/nonce/`at_hash`、未知 `kid`、不安全授权 URL 拒绝与 Debug 脱敏。`examples/demo/tests/server/auth/oidc.rs` 的浏览器绑定、回调单次消费、显式绑定/解绑、邮箱冲突与审计事务测试在 PostgreSQL 18 和 MySQL 8.4 各真实通过 1 项。启用 `auth-mfa` 后，同一真实数据库用例还贯通本地账户注册/验证、TOTP enrollment、外部身份绑定、关联 MFA challenge、完成第二因子后自动绑定 refresh family、关联 Session 及 RP logout 撤销；完成 MFA 前确认没有 refresh family。provider fake 的隔离库测试还验证未登记 post-logout redirect 在本地撤销前被拒绝、拒绝后 refresh 仍可轮换；IdP 未提供 logout endpoint 时只完成本地 refresh/Session 撤销并返回无远程 URL。分别运行 `OPENRUST_TEST_DATABASE_URL=... cargo test --manifest-path examples/demo/Cargo.toml --features 'auth-oidc auth-mfa db-postgres' --test auth_oidc -- --nocapture` 与 `OPENRUST_TEST_DATABASE_URL=... cargo test --manifest-path examples/demo/Cargo.toml --features 'auth-oidc auth-mfa db-mysql' --test auth_oidc -- --nocapture`。测试 URL 需要具备创建/删除隔离库权限。

真实 IdP 基础验收使用 Keycloak 26.8.1 镜像和 `examples/demo/tests/server/fixtures/keycloak/openrust-realm.json`，通过同一浏览器 cookie 完成两个独立 client 的 SSO：先交互式登录主 client，再对次 client 执行授权码、S256 PKCE、token exchange、RS256/JWKS、issuer、nonce 与相同 subject/email 验证。启用退出验收时还需设置管理员凭据与可创建/删除隔离库的 `OPENRUST_TEST_DATABASE_URL`。详细多副本强制验收命令与结果见下段；一般 workspace 测试在未设置 issuer 时会显式 skip，不计为真实服务通过。JWKS 最长缓存 5 分钟；过期刷新失败时 fail closed。

OIDC 子簇及并发 storage 回归修复的门禁：`cargo fmt --all --check`、默认与 all-features `cargo clippy --workspace --all-targets --... -D warnings`、`cargo test --workspace`、`cargo test --workspace --all-features`、默认/all-features warnings-denied workspace Rustdoc、`cargo deny check` 均通过。`cargo deny check` 输出仅有重复依赖版本告警，advisories/bans/licenses/sources 均为 ok。all-features 聚合 server 测试 10 项通过；workspace 测试中无 IdP 环境的 Keycloak target 属于显式 skip，不计入上面的真实服务证据。

SSO 强制验收使用真实 Keycloak 26.8.1、PostgreSQL 18/MySQL 8.4、Redis 8 和两个隔离应用（每个应用两个 API 子进程）。callback、MFA 与 logout handler 经真实 loopback TCP round-robin；双库均通过双 client SSO、callback/logout 并发（挂起 callback 在 logout 后返回 401）、MFA completion/logout 竞态（challenge 撤销后返回 401）、refresh/logout 竞争、每个子节点拒绝已撤销 Session/refresh、重启节点后 Token 重放幂等，以及 15 秒 access token 在退出后仍可验签并按 `exp` 到期拒绝。OIDC 双库测试还在真实 Redis Session store 上注入一次 delete error，确认同一通知重放可完成清理；此项与物理 Redis 断连分开记录。真实断连验收与结果见本节 2026-10-27 F7 记录。复跑需 `OPENRUST_REQUIRE_SHARED_SESSION=1`、`OPENRUST_TEST_SESSION_REDIS_URL`、`OPENRUST_REQUIRE_OIDC_IDP=1`、`OPENRUST_OIDC_ISSUER`、Keycloak 管理员变量、双库 `OPENRUST_TEST_DATABASE_URL` 与 `RUST_TEST_THREADS=1`；命令见本节上方。现有 PostgreSQL 18、MySQL 8.4、Redis 8 服务未停止。

## M5 历史决策 054 TOTP MFA 与恢复码（F7 子簇）

`cargo test -p openrust --features auth-mfa auth::mfa::tests --lib` 覆盖 RFC 6238 SHA-1 向量、6 位格式及允许时间步边界。`examples/demo/tests/server/auth/mfa.rs` 的真实 TCP/数据库生命周期测试在 PostgreSQL 18 与 MySQL 8.4 各通过 1 项：`OPENRUST_TEST_DATABASE_URL=... cargo test --manifest-path examples/demo/Cargo.toml --features 'auth-mfa auth-oidc db-<backend>' --test auth_mfa --test auth_oidc -- --nocapture`。覆盖 password 与 OIDC challenge、Session 登录/既有 Session 阻断、挑战来源绑定/过期/五次失败、TOTP replay、恢复码单次消费、enrollment/禁用/重置、密钥轮换、审计与 refresh 撤销；服务端测试还验证此前密码登录 helper 无法绕过 MFA。

CLI 新增 `openrust new --db <backend> --auth --auth-mfa --encryption`。把当前 tracked/untracked 源码复制到干净临时 workspace（无 registry patch、无开发者绝对路径），分别生成 PostgreSQL 与 MySQL 应用；两个生成包均实际通过 `cargo check` 和 `cargo test`，其中生成的完整 auth HTTP/fake-mail 测试各执行 1 项并使用 PostgreSQL 18 / MySQL 8.4 隔离数据库。`cargo test -p cli` 18 项通过。

MFA 子簇门禁：fmt、默认/all-features workspace all-target Clippy、默认/all-features workspace tests、默认/all-features docs（all-features warnings denied）、no-default-features check、`cargo deny check` 均通过。deny 仅有既有重复 crate 版本与未匹配 MPL-2.0 allowance 警告；advisories、bans、licenses、sources 均通过。Twilio 真实发送仍受外部授权凭据/目标约束，与 MFA 验收无关；Keycloak 单 client OIDC 真实验收见上一节，完整 SSO 为后续独立 F7 子簇。

## M5 缓存驱动与命名实例

`cargo test -p openrust cache::tests` 通过 memory namespace/TTL/remove、file key hash/过期/替换/并发、命名 stores 和外部 driver 注册；`cargo test -p openrust --features cache-redis cache::tests::redis_store_roundtrips_ttl_on_live_service -- --exact` 与 `cargo test --manifest-path examples/demo/Cargo.toml --all-features --test server cache_plugin_uses_the_configured_shared_redis_store -- --exact` 均在本机 Redis 8.10.2 真实服务通过。CLI 实测复用可达 loopback Redis，不触碰 Compose；干净 `/tmp` 源码副本生成默认应用后 `cargo check`、`cargo test` 通过，独立 custom-driver 示例 `cargo test/run --manifest-path tests/consumers/cache-custom-driver/Cargo.toml` 通过。默认与 all-features workspace tests、all-target/all-features clippy、fmt、warnings-denied 默认/all-features docs、cargo deny 与 no-default-features check 通过。workspace server 的数据库集成 target 因未设置 `OPENRUST_TEST_DATABASE_URL` 跳过；本缓存簇不修改数据库路径，不以该 skipped target 代替双库验收。Redis 启动 Compose 的冷启动分支尚未在现有可用 Redis 环境下触发；已验证的是复用路径。Redis 最低兼容版本仍待 v1 发布前专项确认。

## M5 F4a 缓存 tag generation

`cargo test -p openrust cache::tests` 覆盖 memory generation 失效、命名空间隔离、旧 entry 写入拒绝、无能力 file driver 拒绝；Redis 真实服务测试使用两个独立 Redis 连接验证共享读取、代际失效和旧 entry 回填被拒绝，`根目录集成宿主` 聚合测试通过真实 Redis 验证应用集成。Redis tag generation 不设置 TTL，tagged value 必须有 TTL；未创建真实跨进程双节点测试，F4c 最终矩阵仍需补足。F4a 的 workspace default/all-features tests、clippy、warnings-denied docs、fmt、cargo deny 和 no-default check 均通过；数据库集成测试因未配置 `OPENRUST_TEST_DATABASE_URL` 跳过，与本簇无关。

## M5 命名数据库连接实例（历史决策 045）

`cargo test -p openrust --features db-postgres named_instance -- --nocapture`、`database_selector` 与 `migration_action` 定向测试通过。`cargo test --manifest-path examples/demo/Cargo.toml --features db-postgres --test db_migration named_database_instances_keep_migrations_and_transactions_isolated -- --exact --nocapture` 和 MySQL 对应命令各通过 1 项，使用 PostgreSQL 18 与 MySQL 8.4 真实隔离库。测试同时配置 default、analytics、reporting 三个独立实例，验证连接身份、默认句柄兼容、具名副本读取、prefix、显式 migration 目标、未迁移表隔离及事务数据不串用。具名副本使用对应实例自己的隔离数据库 URL，验证的是实例绑定边界，不是物理复制延迟；复制语义由读写分离专项另行覆盖。

干净源码副本生成 `apps/named_database_sample` 后直接 `cargo check -p named_database_sample` 与 `cargo test -p named_database_sample` 通过；扫描应用及根 manifest 未发现临时 registry patch 或机器绝对路径。默认/all-features workspace tests、all-target clippy、warnings-denied docs、fmt、cargo deny 与 no-default-features check 通过。`cargo deny` 保留既有重复依赖及未匹配 MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。未覆盖跨数据库事务、自动查询路由或生产集群负载。

## M5 Redis Session driver

启用 `session-redis` feature，通过 `RedisSessionStore::connect(dsn, namespace)` 与 `SessionPlugin::with_store` 显式使用 Redis；数据库 Session store 仍为默认。真实 Redis 8.10.2 测试命令：`cargo test --manifest-path examples/demo/Cargo.toml --features session-redis --test auth_runtime redis_sessions_are_shared_between_apps_and_revoke_across_instances -- --exact --nocapture`。PostgreSQL 18 和 MySQL 8.4 分别运行 `cargo test --manifest-path examples/demo/Cargo.toml --features db-<backend>,session-redis --test auth_database --test auth_runtime`，各 14 项通过（auth_database 7、auth_runtime 7）；两个独立 App 使用两个 Redis connection manager，共享同一 namespace。验证真实 Redis 创建/读取/TTL 过期/删除、重复创建拒绝、删除后 save 不复活、不同 namespace 隔离、ID 轮换清除旧记录、跨 App 登出撤销、CSRF 缺失拒绝与有效 token 放行；namespace 校验、URL/Debug 脱敏及不可达 endpoint 的启动失败也覆盖。用例走访现有 Redis 8 服务，没有修改或停止容器。

`cargo +1.97.0 check -p openrust --no-default-features --features session-redis` 与 server feature check 通过。默认/all-features workspace clippy、test、warnings-denied docs、fmt、no-default check 与 `cargo deny check` 通过；deny 仍有既有重复版本与未匹配 MPL allowance 警告，advisories/bans/licenses/sources 均通过。两个 App 在同一测试进程内，未代表多进程集群门禁；Redis 6.2 最低版本兼容和在线 Redis 故障期间的端到端恢复留待发布/集群验收。

## M5 Redis Queue driver（历史决策 046）

本次使用已运行的真实 Redis 8.10.2 服务。`cargo test --manifest-path examples/demo/Cargo.toml --features queue-redis --test queue_redis_driver` 3 项通过：四个独立连接 worker 对 40 个任务竞争、活动唯一键、失败保留/重试/幂等 retry ID/进度、独立子进程强杀后租约到期恢复、非法 namespace 与不可达服务失败关闭。`cargo test --manifest-path examples/demo/Cargo.toml --features queue-redis --test queue_redis` 2 项通过：Redis QueuePlugin 无需 DatabasePlugin；存在数据库 scheduler 注册时 build 失败。另用 `examples/queue` Redis-only 启动 API，真实 TCP `POST /jobs` 返回任务 ID，再由独立 worker 使用同一 namespace 运行；两个任务进程均正常响应 Ctrl-C。

`cargo fmt --all --check`、默认及 all-features workspace clippy、默认及 all-features `cargo test --workspace`、warnings-denied 默认及 all-features docs、`cargo check -p openrust --no-default-features` 和 `cargo deny check` 通过；all-features tests 运行 Redis targets，workspace 其他 PG/MySQL integration tests 在未提供 URL 时依项目约定 skip。deny 输出有既有重复版本与 unmatched MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。Redis driver 不访问 SQL，故没有把数据库队列既有 PG18/MySQL8.4 验收冒充为 Redis 双库测试；Redis Cluster 分片、Redis 6.2 最低版本、持久化/故障切换及生产多节点负载仍待 v1 总门禁。

## M5 HTTP 有界运行与就绪切换

2026-10-26 定向测试：`cargo test -p openrust serve::tests -- --nocapture` 覆盖未绑定/绑定就绪、drain 中停止接入、在途请求完成、非合作请求到期取消及 bind 失败；`cargo test --manifest-path examples/demo/Cargo.toml --test server serve_process_exits_successfully_after_sigterm -- --nocapture` 通过真实 TCP readiness 和真实子进程 SIGTERM，all-features 下同一测试额外注入仅供测试的加密 key 后通过。全量门禁：fmt、diff check、all-target/all-features clippy、默认/all-features workspace tests、warnings-denied 默认/all-features docs、`cargo deny check` 和 `cargo check -p openrust --no-default-features` 均通过。deny 输出既有重复 crate 版本与未匹配 MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。此项未改数据库/队列协议，不需要双库测试；未覆盖集群依赖 readiness 或跨节点 drain。

## 历史决策 039 源码交付验收

当前 workspace packages 为根 `openrust` 与 `tools/` 下的 `cli`、`macros`、`testing`、`performance`；根 package 内含 `server` bin，所有 package 均显式禁用发布，内部依赖通过 workspace path 解析。仅根 package 是 `default-members`，以保证裸 `cargo run` 选择 `server`；完整验证显式使用 `--workspace`。2026-10-27 目录收纳后，Cargo metadata、CLI fixture 与 workspace 检查确认这五个 package 及 `db_routing` bench target；PostgreSQL 与 MySQL feature 转发的 benchmark 编译检查通过，真实数据库 benchmark 未因目录调整重跑。CLI 保留 `openrust` 命令，并默认生成 `apps/<name>`，应用与 migration 加入现有 workspace。生成器保留根 manifest 其他内容，拒绝重复名、现存目录和不支持的外部目标；dev/serve 按目标应用定位二进制和配置。

2026-10-26 干净源码副本（无 `.git`、临时 registry patch 或开发机绝对路径）生成默认、PostgreSQL、MySQL 与加密认证应用。默认应用 `cargo test` 通过；PostgreSQL 18 和 MySQL 8.4 的加密认证应用各通过 `cargo test`，真实数据库 HTTP auth flow 均执行 1 项通过。两个 workspace 应用以不同配置及端口分别启动，HTTP 根路由返回各自应用名，证明配置和启动目标隔离；CLI 单测覆盖重复/非法/外部目标失败无半成品。

本仓库最终门禁：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`、`cargo deny check`、`git diff --check` 均通过。另在真实 PostgreSQL 18 与 MySQL 8.4 服务分别执行 openrust/testing/server 的数据库、认证、队列、崩溃恢复与调度 feature 集成测试并通过。`cargo deny` 仅有既有重复依赖版本和未使用 MPL allowance 警告；advisories/bans/licenses/sources 均通过。C1–C3 文档整改不属于本簇。

以下 `-p openrust-*` 命令只代表历史验收；当前命令使用新包名。历史临时 patch 不作为 历史决策 039 黄金路径证据。

## 本地 workspace 门禁

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo deny check
```

按 feature 独立检查框架包，避免只依赖 `--all-features`：

```bash
cargo check -p openrust --no-default-features
cargo check -p openrust --no-default-features --features db-postgres
cargo check -p openrust --no-default-features --features db-mysql
cargo check -p openrust --no-default-features --features auth
cargo check -p openrust --no-default-features --features mail-smtp
cargo check -p openrust --no-default-features --features encryption
cargo check -p openrust --no-default-features --features test-util
cargo check --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

应用数据加密专项：`cargo test -p openrust --features encryption security::tests`；生成器/密钥命令：`cargo test -p cli`；`根目录集成宿主` 接线：`cargo check -p openrust --bin server --features encryption`。在干净源码副本中生成的 `openrust new --auth --encryption` PostgreSQL/MySQL 应用均通过直接 `cargo test`，未添加临时 registry patch。

根 package 示例：`cargo run -p openrust --example hello`；应用配置可用 `APP_SERVER__PORT` 指定隔离端口，HTTP smoke 访问 `/` 应返回统一 JSON 200。

## PostgreSQL / MySQL

CI 数据库服务作业分别启用 PostgreSQL/MySQL 的 `queue` feature，并显式运行 `db_migration`、`queue`、`queue_crash_recovery`、`schedule` 和 `schedule_calendar` targets。普通 all-features 作业无数据库 URL 时仍可能跳过真实集成；需以数据库 job 命令实际执行结果作为证据。

暂定最低支持为 PostgreSQL 14+、MySQL 8.0+、Redis 6.2+；日常开发与 Compose 基线为 PostgreSQL 18、MySQL 8.4 LTS、Redis 8；常规 CI 数据库基线为 PostgreSQL 18、MySQL 8.4。每次数据库/缓存集成测试前，先盘点当前可用的 Docker 容器和本机服务，确认版本、健康状态、监听端口及测试所需连通性，然后优先复用合适的现有服务。不要因使用项目 Compose 更方便就重复创建基础服务。

M4 队列集成测试使用 `OPENRUST_TEST_DATABASE_URL`，由 `testing::TestDatabase` 建立隔离数据库并在测试后清理。命令：`cargo test --manifest-path examples/demo/Cargo.toml --features 'queue,db-postgres' --test db_migration --test queue --test queue_crash_recovery`；MySQL 将 `db-postgres` 换为 `db-mysql`。2026-10-26 在现有 PostgreSQL 18 与 MySQL 8.4 容器实测迁移、并发 worker/dispatch、事务派发提交与回滚、进度、失败与幂等重试、单任务超时后继续处理，以及独立子进程强杀后的 lease 到期恢复。`queue_crash_recovery` 当前只验证崩溃恢复；旧 worker 与新 token 并发接管 fencing 的专项竞态断言及完整跨节点验收仍未完成；调度的独立进程证据见下段。凭据由容器环境变量读取，没有输出到日志；既有服务未重建或停止。

2026-10-26 Worker 合作停机修正：先运行真实 PostgreSQL red test，观察到 SIGTERM 后合作 handler 未完成确认；实现后在 PostgreSQL 18 与 MySQL 8.4 分别执行 `OPENRUST_TEST_DATABASE_URL=<service-url> cargo test --manifest-path examples/demo/Cargo.toml --features 'db-<backend>,queue' --test queue_crash_recovery`，各 5 项通过；并执行 `--test queue`，各 1 项通过。真实 worker 子进程 SIGTERM 覆盖合作任务完成并确认、不合作任务有界超时并释放 lease、同队列 pending job 不再领取；测试子进程关闭其 SeaORM pool 模拟连接不可用，worker 返回 `internal_error` 且保留 owner lease；共享数据库并发测试让新 worker 接管过期任务，旧 token 不能确认新 worker 已完成的任务。旧 queue target 的 PG 首次复跑发现连接池关闭与隔离库删除竞争；测试现在显式关闭 SeaORM pool，随后双库 targets 均通过。临时服务 `openrust-m4-auth-tx` 为本任务创建并保留供后续数据库簇复用。

M4 scheduler 集成入口：`cargo test --manifest-path examples/demo/Cargo.toml --features 'queue,db-postgres' --test schedule`，MySQL 使用 `db-mysql`。2026-10-26 两库各通过 4 项测试：两个独立进程经同步屏障竞争同一 UTC interval slot、强制异常退出后保持已提交 slot、`schedule run --once` 命令、常驻命令 SIGTERM 退出；均验证隔离数据库只产生一条 schedule run 和一条 job。另有 interval/daily slot 边界单测。scheduler 使用共享数据库时钟和唯一键；该验收不替代生产负载资源测量或最终集群部署验收。

F1 日历调度：`cargo test -p openrust --features queue schedule::tests` 的 4 项通过，覆盖春季 DST gap、秋季重复 wall minute、闰日、语法/时区拒绝及补跑配置上限；`queue::tests::cron_registration_rejects_invalid_schedule_configuration` 另验证公开注册入口拒绝无效配置。PostgreSQL 18 与 MySQL 8.4 分别运行 `OPENRUST_TEST_DATABASE_URL=<service-url> cargo test --manifest-path examples/demo/Cargo.toml --features 'db-<backend>,queue' --test schedule_calendar`，各 4 项通过：独立进程同刻竞争同一 cron 槽、表达式及 IANA 时区版本变更、实际 DST 数据库槽（春季跳过、秋季重复分钟仅存一条）、跨 26 小时停机后按每 tick 两项上限补齐精确积压数。集成目标显式执行且未跳过；不替代生产负载或完整集群验收。

2026-10-26 队列唯一键改动后，PostgreSQL 18 与 MySQL 8.4 分别重跑 queue migration 和 queue 集成测试通过。额外验证同 key 并发 dispatch 返回同一活动 job ID、完成后可再次创建、以及同一业务事务中重复 dispatch 只创建一个 job。唯一键只抑制活动任务重叠，不提供完成后持久化的请求幂等历史。

2026-10-26 M4 任务链/批次簇：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo test --workspace --all-features`、warnings-denied all-features docs、`cargo deny check` 与 `git diff --check` 通过。PostgreSQL 18 和 MySQL 8.4 各自重跑 `db_migration`、`queue`、`queue_crash_recovery` 集成测试；覆盖 chain 顺序、事务派发、Stop/Continue、batch 并发上限、失败语义与输入边界。cargo-deny 仅报告既有重复依赖版本和未使用 MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。未执行生产资源或多节点集群测试。

2026-10-26 M4 队列化通知簇：PostgreSQL 18 (`queue,db-postgres`) 与 MySQL 8.4 (`queue,db-mysql`) 分别执行 `cargo test --manifest-path examples/demo/Cargo.toml --features 'queue,db-<backend>' --test queue_notifications`，各 1 项通过。真实隔离库验证多渠道业务事务回滚不留任务、提交后两个渠道分别得到稳定活动唯一任务、worker 从应用状态取 Mailer/SmsSender 并分别处理；模拟邮件供应商已接受但确认丢失，验证队列立即不重入、按既有退避重试且外部副作用可能重复。PG18 另执行 SMTP 专项：`cargo test --manifest-path examples/demo/Cargo.toml --features 'queue,db-postgres,mail-smtp' --test queue_notifications queued_notification_is_accepted_by_configured_smtp_sink -- --exact`，Mailpit v1.32.0 接受实际 SMTP 发送；其 API 回读 recipient、subject、text、HTML 均与固定入队内容一致。SMTP sink 由本簇临时容器提供，运行于隔离端口；不是外部邮件供应商验收。Twilio 实发未授权且未执行。默认与 all-features workspace tests、all-target/all-features warnings-denied clippy、warnings-denied all-features docs、fmt 和 `cargo deny check` 通过；deny 仅报告既有重复版本与 unused allowance 警告，advisories/bans/licenses/sources 均通过。all-features workspace run 未设置数据库 URL，其数据库用例不替代上列双库显式运行。未执行生产负载或完整集群验收。

2026-10-26 目录收尾：迁移前后 `cargo metadata --no-deps` 均列出 5 个 workspace packages；all-features `cargo test -- --list` 均为 265 个条目、target/test 名保留。迁移后 workspace test、all-target clippy、warnings-denied docs、`cargo deny check`、fmt 和 `cargo check -p openrust --no-default-features` 通过。PostgreSQL 18 与 MySQL 8.4 各执行 `cargo test -p openrust --all-features`，40 项真实 server 集成测试通过。CLI 生成的 PostgreSQL/MySQL `--db --auth` 项目各自经本地 `openrust`/`testing` path patch 通过 fmt、check 和真实数据库 `auth_flows` HTTP test；默认生成项目 check/test 也通过。hello 示例作为真实 TCP HTTP 服务返回 200 JSON。queue/Scheduler 双库命令已加入 CI；未宣称生产集群或最低版本总验收。

2026-10-26 M3 Mailer/Policy 生成器：CLI 单测 13 项通过，覆盖命令解析、输出结构、CRUD 权限名、模块注册、多模块追加与稳定排序、非法输入、缺少 `auth` 和不覆盖现有文件。PG18/MySQL8.4 `--db --auth` 新项目各连续生成多个 mailer/policy，使用仅限 `/tmp` 的本地 `openrust` / `testing` path patch 分别通过 `cargo fmt --all --check`、`cargo check` 与 `cargo test`；两库 `auth_flows` 各 1 项通过真实隔离数据库和 loopback HTTP。MySQL TestDatabase 从容器内已有 `mysql` 管理库连接，未创建或修改现有服务。Workspace fmt、default/all-features clippy/test、warnings-denied all-features docs 和 `cargo deny check` 通过；deny 仅有既有 duplicate dependency 与 unused MPL allowance 警告。没有外部短信投递或集群验收声明。

2026-10-26 M3 HTTP route rate limit：`cargo test --manifest-path examples/demo/Cargo.toml --test http_router --test http_resource` 通过（35 router、16 resource），all-features route limit 目标测试通过；同 scope/key 超限得到 429 与 `Retry-After`，不同自定义 key 隔离，auth-enabled 缺失主体拒绝。通过 `cargo test --manifest-path examples/demo/Cargo.toml --features queue,db-postgres --test db_migration migration_command_applies_repeats_reports_and_rolls_back -- --exact --nocapture` 与 MySQL 对应命令，在现有 PostgreSQL 18 / MySQL 8.4 容器中的隔离数据库各通过：真实 DatabaseRateLimitStore 多实例并发，以及 `RateLimitLayer::by_client_ip` HTTP 请求遇到伪造 XFF 仍按未信任 socket peer 限额。数据库用户/密码只在运行器进程环境内传递，未输出；容器未被创建、修改或停止。

只有找不到符合版本和测试要求的现有服务，或现有服务无法用于该测试时，才创建隔离的临时服务。使用独立 Compose project/端口，先用 `TestDatabase` 或独立测试库隔离数据；不得擅自停止、删除、重建或改动已有容器、卷、数据和配置。新建资源只清理本任务明确创建且确认无其他使用者的部分。检查命令不得输出连接密码或其他凭据。

```bash
docker ps --format 'table {{.Names}}\t{{.Image}}\t{{.Ports}}\t{{.Status}}'
lsof -nP -iTCP -sTCP:LISTEN
```

确认没有合适服务后，才使用项目 Compose；需要隔离端口时：

```bash
OPENRUST_TEST_POSTGRES_PORT=55432 OPENRUST_TEST_MYSQL_PORT=53306 docker compose -p openrust-ci -f compose.ci.yaml up -d --wait postgres mysql
OPENRUST_TEST_DATABASE_URL=postgres://openrust:openrust@localhost:55432/openrust_test cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-postgres --test auth_database --test db_connection --test db_migration
OPENRUST_TEST_DATABASE_URL=mysql://root:root@localhost:53306/openrust_test cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-mysql --test auth_database --test db_connection --test db_migration
```

MySQL `testing` 隔离测试需具备建库/删库权限的管理 URL；一般应用集成测试使用最低权限 `openrust` 用户。MySQL driver 强制 Rustls TLS，并在新连接设置 `+00:00` 时区。两个独立数据库不能替代真实复制拓扑。

M2 读写路由真实复制测试使用 `OPENRUST_TEST_DATABASE_URL`、`OPENRUST_TEST_READ_REPLICA_URL` 配置主库/副本，并设置 `OPENRUST_TEST_VERIFY_REPLICA_LAG=1` 启用副本暂停/追平断言。测试暂停 PostgreSQL WAL replay 或 MySQL SQL thread，验证副本旧读、两个独立 `App` 的 `with_primary` 主库新读与 scope 隔离，再恢复复制等待追平。2026-10-26 在 PostgreSQL 18.6 和 MySQL 8.4.13 临时主从服务通过。

可运行 `cargo bench -p performance --bench db_routing --features db-routing-bench,db-postgres` 或 `cargo bench -p performance --bench db_routing --features db-routing-bench,db-mysql` 对主库与显式副本分别进行 20 次预热、500 次串行 `SELECT 1` 查询，记录吞吐及 p50/p95/p99；`OPENRUST_DB_ROUTE_BENCH_SAMPLES` 可设为至少 100 的样本数。2026-10-26 Apple arm64、16 核 / 64 GiB、Docker 29.9.2、Release profile 下最新结果：PostgreSQL 18.6 primary 3658.0 req/s (p50 265.875µs, p95 332.292µs, p99 368.708µs)，replica 3452.1 req/s (270.333/424.625/541.75µs)；MySQL 8.4.13 primary 3735.9 req/s (p50 256.958µs, p95 315.333µs, p99 362.041µs)，replica 3709.8 req/s (261.416/303.5/484.333µs)。此 `SELECT 1` 单并发微基准只记录路由和往返基线，不代表业务吞吐或性能提升；完整 CPU/内存/业务负载测试仍按性能计划执行。

2026-10-26 根模块迁移后，上述 DB/auth tests 在 PostgreSQL 16 和 MySQL 8.4 均实际通过（分别 4 项 auth、3 项连接/事务、1 项迁移）。`testing` 数据库隔离测试也分别通过。

## 生成项目

CLI 模板快照测试：`cargo test -p openrust-cli`。生成 PostgreSQL、MySQL `--auth` 项目后运行 `cargo fmt --all --check`、`cargo check`、`cargo test`。旧模板的历史集成验证使用 Cargo patch 指向本地根 `openrust` 与 `testing`。新的源码交付验收按 历史决策 039，在干净源码副本中生成 apps 后直接运行，不继续以 patch 作为默认操作入口。

2026-10-26 根模块迁移后，两种生成项目都以真实 PostgreSQL 16 / MySQL 8.4 服务运行 `generated_auth_routes_run_over_http_with_isolated_database_and_fake_mail` 并通过。测试覆盖实际 HTTP、JWT/PAT 认证、数据库流程、可信代理 client IP 与 fake mail。

2026-10-26 当前日常基线复验：`cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-postgres --test auth_database` 在现有 PostgreSQL 18 容器通过 4 项；`--features auth,db-mysql` 在现有 MySQL 8.4 容器通过 4 项。按当前 CLI 模板新生成的 `--auth` 项目分别以本地 Cargo patch 指向未发布 workspace，并在真实 PostgreSQL 18 / MySQL 8.4 服务上通过 `cargo fmt --all --check`、`cargo check` 和 `cargo test --test auth_flows`（每库 1 项真实 TCP HTTP 流程，使用隔离数据库与 fake mail）。生成目录位于 `/tmp/openrust-auth-pg-resume` 与 `/tmp/openrust-auth-mysql-resume`，没有纳入仓库。

2026-10-26 历史决策 031 出站 HTTP：历史基线为 `cargo test -p openrust --features http-client http::client::tests` 15 项及 `根目录集成宿主` 聚合接入 1 项；CLI `--http-client` 生成项目 check/test 通过，证据保留于原验收记录。

2026-10-27 历史决策 051/052 出站策略与韧性：`cargo test -p openrust --features http-client http::client::tests -- --nocapture` 21 项通过，涵盖目标策略、DNS 固定、重试上限/抖动/Retry-After、单次 body 限制；`cargo test --manifest-path examples/demo/Cargo.toml --all-features --test http_client -- --nocapture` 6 项通过，聚合 HTTP consumer 验证目标拒绝、依赖隔离、容量与等待上限、取消释放、熔断打开/半开恢复。总门禁 `cargo fmt --all --check`、`cargo test --workspace`、`RUST_TEST_THREADS=1 cargo test --workspace --all-features`、default/all-features workspace all-target clippy、warnings-denied default/all-features Rustdoc、`cargo check -p openrust --no-default-features`、`cargo deny check`、`git diff --check` 全部通过。`cargo deny` 仅输出既有未匹配 MPL-2.0 allowance 与重复版本警告，advisories/bans/licenses/sources 均通过。无需外部服务；只访问回环 TCP 测试服务，不访问公网目标、云 metadata 或供应商。

2026-10-26 历史决策 033 邮件/SMS：`cargo test -p openrust --features 'mail-smtp,sms-twilio,test-util' mail::tests:: -- --include-ignored` 通过，含真实本机 SMTP 对 Mailpit v1.32.0 的投递；`curl --noproxy '*' http://127.0.0.1:18025/api/v1/messages` 确认 `user@example.test` 收到 `Welcome`。Twilio transport tests 覆盖 POST URL、Basic auth、form/template fields、2xx receipt、4xx脱敏、timeout及单次调用不重试；测试不访问 Twilio。CLI 9 项测试与 `--sms-twilio` 生成项目 `/tmp/openrust-sms-generated-20261026` 的 check/test 通过；default/all-features workspace test、all-features clippy、warnings-denied docs、cargo deny 与 diff-check 通过。未提供实际 Twilio 账号凭据与授权号码，真实 SMS 投递仍未验收。

2026-10-26 历史决策 032 请求级国际化：6 项 `i18n` 单测与 5 项 `examples/demo/tests/server/i18n/runtime.rs` 通过，覆盖 locale 规范化、Accept-Language q 值、TOML config、JSON resource 合并/参数/fallback、请求并发隔离、错误/validator message、稳定 code/字段路径、未翻译应用错误保留与显式 locale override。新生成项目 `/tmp/openrust-i18n-generated-20261026` 使用本地 Cargo patch 通过 `cargo check`、`cargo test`；其真实 server 进程收到 `Accept-Language: zh-CN` 后对 404 返回中文 message。default/all-features `fmt --check`、workspace clippy/test、warnings-denied docs、`cargo deny check` 通过。无数据库行为，不要求双库集成；邮件/SMS 的实际供应商验收留在 M3 通信簇。

## 迁移验收记录

- `cargo metadata --no-deps --format-version 1` 确认根 package 为 `openrust`，workspace 共四个成员：根 package `openrust`、`cli`、`macros`、`testing`。
- 无默认、默认、关键单独 feature、all-features 编译已检查；workspace default/all-features clippy 与 test、warnings-denied rustdoc、`cargo deny check` 均通过。`cargo deny` 仍会输出重复版本依赖提示，但无 advisory、license、ban 或 source 失败；JWT 使用 `aws_lc_rs`，不再解析到受 RUSTSEC-2023-0071 覆盖的 `rsa 0.9`。
- M2 数据库读写分离的 PostgreSQL/MySQL 真实复制拓扑已单独通过（见上文）；其余尚未验收的范围包括 Redis 最低版本、生产负载下多 worker/旧 token 竞态、M6 全栈集群、S3 bucket-version 专项与完整性能门禁，继续按 `todo.md` 和各专题计划推进。实时在线跨节点单项验收见本文件 M5 历史决策 059。
- PostgreSQL 14 / MySQL 8.0 结果是历史兼容性证据；当前暂定下限已采用 PostgreSQL 14+ / MySQL 8.0+，但这些结果不替代发布前对完整承诺能力的最低版本复核。Redis 6.2+ 的真实兼容性在 M5 driver 落地后验证。

2026-10-26 历史决策 034 应用数据加密/本地轮换：`cargo test -p openrust --features encryption security::tests` 8 项通过，包含 RFC 8452 AES-256-GCM-SIV 向量、二进制/空明文、随机 nonce、context、active key 写入/历史 key 解密、篡改/格式/越界、配置脱敏与多 Encryptor 互解；`cargo test -p openrust-cli` 11 项通过，包含 32-byte Base64 key generation 与 `--encryption` 模板；`cargo check -p openrust --bin server --features encryption` 通过。实际 CLI 生成项目 `/tmp/openrust-encryption-generated-20261026` 使用本地 Cargo path patch 通过 `cargo check`、`cargo test`，并用环境 key 执行 `cargo run -- config show`；输出中的 active key ID 与 key map 值均为 `********`。最终 default/all-features workspace tests、all-features clippy、warnings-denied default/all-features docs、`cargo deny check` 和 `git diff --check` 通过；cargo-deny 保留既有重复依赖与未使用 MPL allowance 警告，advisories/bans/licenses/sources 通过。该簇无数据库读写，不要求新增双库集成验收。测试应用 all-features 时显式注入测试 key ring；生产默认配置不含 key。

2026-10-26 历史决策 035 审计簇：`cargo test -p openrust --features test-util audit::tests` 6 项通过，覆盖 allowlist、敏感详情/Debug 脱敏、错误传播、database/custom plugin 和公开 fake；PostgreSQL 18 与 MySQL 8.4 的 `cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test auth_database` 各 5 项通过。新增测试使用 `TestDatabase` 隔离库和完整 audit 表名映射，验证权限变更与审计同事务提交、外层回滚、审计 insert 失败导致权限回滚，以及最多 N 行的重复清理。CLI 新生成的 PostgreSQL 与 MySQL `--auth` 项目均通过 `cargo fmt --all --check`、本地 Cargo patch `cargo check` 和真实 `auth_flows` HTTP/TCP 数据库流程测试。workspace default/all-features tests/clippy、warnings-denied default/all-features docs、no-default-features check、`cargo deny check` 通过；deny 仅有既有重复依赖与未使用 MPL allowance 警告，advisories/bans/licenses/sources 通过。

2026-10-26 历史决策 036 signed links：`cargo test -p openrust --no-default-features --features signing signing::tests` 4 项通过，覆盖用途/subject/TTL、Debug 脱敏、shared store 撤销与保留旧 key 的轮换；PostgreSQL 18、MySQL 8.4 完整 `cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test auth_database` 各 6 项通过，覆盖两个 signer 实例跨库撤销、自定义完整表名映射、过期记录有界清理及应用插件注册。新生成 `--auth` 项目分别使用本地 Cargo patch，通过 `cargo fmt --all --check`、`cargo check`、真实数据库迁移及 TCP HTTP auth flow。最终 `cargo fmt --all --check`、`git diff --check`、workspace default/all-features clippy/test、warnings-denied default/all-features docs、`cargo check -p openrust --no-default-features`、`cargo deny check` 均通过；deny 仅有已存在的重复依赖与 unused MPL allowance 警告，advisories/bans/licenses/sources 通过。没有新增依赖。

2026-10-26 M3 认证/安全总验收：本机预置 PostgreSQL 18 / MySQL 8.4 容器凭据与仓库 CI 默认值不同，未触碰预置数据库；使用仓库 `compose.ci.yaml` 的无持久卷 PostgreSQL 18 / MySQL 8.4 服务、独立测试库后，`cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test auth_database --test auth_runtime` 各 12 项通过。按当前模板生成的 PG/MySQL 项目分别用真实数据库执行 `cargo test --test auth_flows`，各 1 项真实 TCP HTTP 流程通过。覆盖注册/重复注册、邮箱验证、JWT 与 PAT、refresh 单次轮换/重放撤销、密码重置、refresh logout、Session 登录/退出与 CSRF、共享限流、可信代理、Policy/RBAC 与 PAT ability 交集。生成项目各自还通过 `cargo fmt --all --check` 与 `cargo check`。临时 Compose 服务已停止并移除，既有 PG/MySQL/Redis 服务保持运行。

2026-10-26 M3 数据输出/查询边界：`cargo test -p openrust http::sort::tests` 覆盖排序解析、字段 allowlist、重复/未知/格式错误输入；CLI generator 单测覆盖 `--uuid-id` / `--soft-delete` 和生成 feature 注册。临时生成项目（本地 patch 到当前 workspace）分别在现有 PostgreSQL 18 与 MySQL 8.4 上通过格式检查、`cargo check` 和真实隔离数据库 test：迁移 UUID 主键及 UTC `deleted_at`，插入生成 UUIDv7，显式 `not_deleted()` 隐藏软删除行，普通 `find_by_id` 仍能读取，清空 `deleted_at` 后恢复。HTTP README 补充严格输入 DTO、响应字段 allowlist、显式 SeaORM Condition、关联预加载示例；没有引入 ORM 自动过滤或动态查询 DSL。测试只使用现有服务与临时目录，未输出凭据，未改动容器。

2026-10-26 M4 typed events/queued listener：`cargo test -p openrust --all-features events::tests` 与 `cargo test -p openrust --features queue --lib events::queued_tests` 通过，后者覆盖 mapper 成功、mapper error 与 Queue dispatch error 传播/短路。外部下游 consumer crate 在 `/tmp/openrust-events-consumer` 以 `queue,db-postgres` 通过离线 `cargo check`。真实双库命令均运行成功：`cargo test --manifest-path examples/demo/Cargo.toml --features queue,db-postgres --test queue database_queue_dispatch_worker_concurrency_failure_and_retry -- --exact` 在现有 PostgreSQL 18.6 通过；`cargo test --manifest-path examples/demo/Cargo.toml --features queue,db-mysql --test queue database_queue_dispatch_worker_concurrency_failure_and_retry -- --exact` 在临时 MySQL 8.4 通过。覆盖事件 transaction context、事务回滚不留任务、提交后 worker 消费、普通 dispatch 消费；测试显式释放 Events/Queue/Db/App 后 `TestDatabase::close` 正常删除隔离库。预置 MySQL 8.4 容器因容器内 socket PID lock 错误持续重启，未改动；仅使用 `compose.ci.yaml` 在空闲 53306 端口创建 `openrust-ci` 临时 MySQL，测试后 `docker compose ... down -v` 仅清理本次创建的容器/网络/卷。修复后的最终门禁通过：`cargo fmt --all --check`、`git diff --check`、workspace all-target/all-features clippy、default/all-features workspace tests、warnings-denied default/all-features docs、`cargo deny check`、`cargo check -p openrust --no-default-features`。未触碰预置 PostgreSQL/MySQL/Redis 数据，无凭据输出。

2026-10-26 历史决策 041 事务与 AuthFlows 事件：在任务专属 Compose project `openrust-m4-auth-tx`（PostgreSQL 18、MySQL 8.4，端口 55432/53306）分别执行 `cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test db_connection transaction_events_rollback_before_commit_and_ignore_after_commit_errors -- --exact` 与 `cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test auth_database auth_flow_events_share_account_and_refresh_transactions -- --exact`，四项均实际运行通过。验证提交前拒绝回滚、提交后错误不掩盖已提交结果、注册校验拒绝不建账户、泛化登录失败事件，以及登录成功事件失败无 refresh token 遗留。临时服务由本任务创建；预置服务未改动。

2026-10-26 历史决策 042 共享锁与请求幂等：在任务专属 `openrust-m4-auth-tx` PostgreSQL 18 / MySQL 8.4 服务，对每个 backend 实际执行 `cargo test --manifest-path examples/demo/Cargo.toml --features db-<backend> --test db_coordination -- --exact shared_locks_and_request_idempotency_are_database_scoped`，均 1 项通过。测试使用 `coord_` 自定义表前缀的 TestDatabase 隔离库及两个独立 SeaORM pool，覆盖迁移索引、共享锁 owner 竞争、过期接管/fencing、旧 owner 释放与事务内检查、同键并发仅一 owner、scope 隔离/指纹冲突、事务回滚同时撤销业务写入和完成快照、成功完成后的 JSON replay、过期 lease 接管及每次最多 N 行清理。测试运行时没有临时 services 以外的依赖；没有声称进程强杀、真实 HTTP middleware 或外部副作用 exactly-once。

2026-10-26 F2 国际化：`cargo test -p openrust --lib i18n::tests::` 9 项通过，新增覆盖结构化 plural/select、英文与波兰语 CLDR 类别、无效/缺失参数 fallback、按实际 fallback 资源 locale 选择复数规则、精确小数、英中日期差异及显式时区跨日；`cargo test --manifest-path examples/demo/Cargo.toml --test i18n` 6 项通过，包含并发请求 locale 及按请求语言选复数。干净源码副本 `/tmp/openrust-f2-source-wgP07b/source`（无 `.git`、无 registry patch/开发机路径）使用 `cargo run -p cli -- new i18n_sample --db postgres` 生成到 `apps/i18n_sample`，其 `cargo check` 与 `cargo test` 通过。工作区 `cargo fmt --all --check`、default/all-features workspace tests、default/all-features clippy、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`、`cargo +1.97.0 check -p openrust`、`cargo deny check` 通过；deny 输出为既有重复依赖版本警告，advisories/bans/licenses/sources 均通过。F2 无数据库读写行为，不要求 PostgreSQL/MySQL 集成验收；F1 已有双库证据不由此替代或重做。

2026-10-26 F3 安全邮件模板：新增可选 `mail-templates`（MiniJinja 2.24）、静态 `include_str!` catalog、locale 分层回退/布局继承、HTML 自动转义、严格缺参、100,000 fuel 与 256 KiB 有界输出、主题换行/长度校验及 text/html MIME alternative。`cargo test -p openrust --features 'mail-templates test-util' mail::template::tests` 4 项通过（含 FakeMailer）；`cargo test --manifest-path examples/demo/Cargo.toml --features mail-templates --test mail_templates` 1 项通过；CLI 邮件生成器定向测试通过。干净源码副本 `/tmp/openrust-f3-final-aKylsZ/source`（无 `.git`、无 registry patch/开发机绝对路径）生成 PostgreSQL 应用及 mailer，`cargo fmt --all --check`、`cargo check -p mail_sample`、`cargo test -p mail_sample` 通过。Compose Mailpit v1.32.0 使用 SMTP/UI 11025/18025（默认 1025 端口被占用），显式运行 `smtp_submits_rendered_template_to_mailpit` 成功；Mailpit `/api/v1/message/<id>` 确认 Subject `Greeting`、Text `Hello, Mailpit`、HTML `<p>Hello, Mailpit</p>`，随后停止本次启动的 Mailpit。default/all-features workspace tests、default/all-features clippy、warnings-denied all-features docs、`cargo +1.97.0 check -p openrust --features mail-templates`、`cargo deny check`、fmt 与 diff check 通过；deny 有既有重复依赖及未匹配 MPL allowance 警告，advisories/bans/licenses/sources 通过。本簇不改数据库行为，不需要新增双库测试；队列派发/通知重试边界留给后续功能簇。

2026-10-26 M4 生成与集成收口：CLI `generate job` 单测覆盖数据库前置、重复保护、多任务插件注册、queue feature 与 `QueueMigration` 登记、显式失败 handler。无 `.git` 的干净源码副本中分别生成 PostgreSQL 与 MySQL 应用并生成一个/多个 job；应用及 migration 的直接 `cargo check`、`cargo test` 均通过，manifest 无 registry patch 或开发机绝对路径。`queue-demo` 示例分别以 `queue,db-postgres` 与 `queue,db-mysql` 通过编译；PostgreSQL 18 隔离库中实际执行迁移，真实 TCP `/health` 返回 200，HTTP `POST /jobs` 派发，`schedule run --once` 派发 heartbeat，独立 `queue work demo` 进程消费两项任务。数据库服务为已存在的 `openrust-m4-auth-tx` PostgreSQL 18 容器，仅创建并删除本次隔离数据库；curl 明确绕过本机 SOCKS 代理。PG18/MySQL8.4 的 `queue_notifications` targets 本次各 1 项真实服务测试通过。`cargo fmt --all --check`、all-target/all-features clippy、default/all-features workspace tests、warnings-denied all-features docs、`cargo deny check`、queue-only feature check 与 diff check 通过；deny 只有既有重复依赖与未匹配 MPL allowance 警告，advisories/bans/licenses/sources 通过。M4 功能簇完成；仍未宣称生产负载或完整集群验收。

2026-10-26 M5 通用插件公开契约：新增外部 integration consumer target，只通过 `openrust` 公共 API 组合 Provider/Consumer 插件，验证 section 配置、服务、命令、HTTP 路由与异步 `finish`；缺依赖、重复服务/命令、无效配置值脱敏和重复路由启动错误均有覆盖，兼容 `insert`/`command` 的后注册替换也有测试。配置 section 错误保留完整字段路径且不输出配置值；应用路由组装将 Axum 注册 panic 转为 `http_route_registration_failed`，直接 Axum API 边界写入 Rustdoc/README。`cargo test --manifest-path examples/demo/Cargo.toml --test plugin_contract` 8 项通过；`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo test --workspace`、`cargo test --workspace --all-features`、warnings-denied all-features docs、`cargo deny check` 通过。deny 保留既有重复依赖与未匹配 MPL allowance 警告，其余 advisories/bans/licenses/sources 检查通过。本簇不修改数据库功能，不新增双库集成要求；后续 Db driver contract 需要独立 PostgreSQL/MySQL 验收。

2026-10-26 M5 Db 外部扩展契约：`DatabaseTableNames` 增加 SQLite 双引号引用与通用 `Db::initialize[_with_replicas]` backend 校验；保留 64-byte 通用标识符上限及 PostgreSQL 63/MySQL 64 backend 限制，未知 SeaORM backend 明确以 `unsupported_database_backend` 拒绝。v1 正式后端与 `DatabasePlugin` URL/feature 检查保持 PostgreSQL/MySQL。独立 `tests/consumers/db-custom-sqlite/` 使用相对 path 依赖、独立 workspace 和 `sqlx-sqlite`；`cargo +1.97.0 check` 及 `cargo +1.97.0 run --locked` 实际通过，真实 SQLite 连接、backend、重复初始化错误、表名 quoting、应用自有 SeaORM migration、事务提交/回滚均有运行断言；独立 package 的 stable fmt/clippy 通过，CI MSRV job 已添加运行入口。PostgreSQL 18 与 MySQL 8.4 分别实际执行 `db_connection::plugin_opens_pool_and_executes_a_query` 和 `db_connection::transaction_commits_success_and_rolls_back_callback_errors`，四项均通过。Workspace fmt、all-target/all-feature clippy、默认/全 feature tests、warnings-denied all-feature docs、workspace `cargo deny check` 与 `cargo deny --manifest-path tests/consumers/db-custom-sqlite/Cargo.toml --config deny.toml check` 通过；deny 报告既有重复依赖及未匹配的 BSD-2-Clause/MPL allowance 警告，advisories/bans/licenses/sources 检查通过。SQLite 不进入官方 v1 数据库矩阵，认证/队列/共享锁/限流和 `TestDatabase` 等专属能力不由本簇宣称兼容。

2026-10-26 M5 Queue 外部扩展契约：`JobRegistry` 与 `JobContext` 通过公开 API 支持外部 driver 注册/执行 typed Job、注入取消信号、进度 reporter 和 AppState；未配置 reporter 明确返回 `queue_progress_unsupported`，ownership token 保持内置数据库 driver 私有。`cargo test --manifest-path examples/demo/Cargo.toml --features queue --test queue_driver_contract` 5 项通过，覆盖无需数据库的 await 内直接执行、typed Job/进度 reporter、合作取消、未知任务、重复注册、无 reporter 显式失败及上下文边界；README 与 Rustdoc 明确该入口不派发/持久化、不提供事务、自动重试或任务组。`cargo check -p openrust --no-default-features` 通过。独立 nested workspace `tests/consumers/queue-custom-driver` 通过 `cargo run --locked` 实际派发并运行 typed Job，all-targets clippy 通过；无绝对路径或临时 registry patch。PostgreSQL 18 与 MySQL 8.4 分别实际运行 `db_migration`、`queue`、`queue_crash_recovery`、`schedule`、`schedule_calendar`、`queue_notifications`，各 16 项通过。Workspace fmt、all-target/all-features clippy、default/all-features tests、warnings-denied default/all-features docs、cargo deny 与 diff check 通过；cargo deny 仅报告既有重复依赖版本和未匹配 MPL allowance 警告，advisories/bans/licenses/sources 通过。外部 memory driver 明确不提供持久化/跨节点/事务保证，不代表 Redis driver 或集群验收。

2026-10-26 M5 跨协议验证与调用作用域（验证通过，尚未提交）：新增公开 `validation::Validate` / `validate` 并让 HTTP Validated extractor 复用原字段路径/错误转换；新增 task-local `ExecutionContext`、白名单 `ExecutionMetadata`（safe request ID/Locale）、层级 cooperative cancellation 和不超过父调用的 deadline budget。HTTP middleware 建立 request/locale/deadline/cancel scope，非法 request ID 移除后重新生成；`App::run_command` 建立匿名 scope 与默认 locale；typed Job 与 `JobRegistry::execute` 建立每次执行 scope，JobRegistry 执行入口强制 deadline/cancellation，数据库 worker 保留已有 timeout 与停机宽限控制，缺省 locale 使用应用默认值。数据库 queue 用有版本的 payload envelope 持久化 request ID/locale，legacy raw payload 保持可读、任务组继承 metadata、failed-job retry 原样保留；保留字段 `__openrust_execution` 在含 metadata 新派发中被拒绝，完整且合法的版本 envelope 作为框架格式，畸形旧结构按原 payload 保留；自定义旧 driver 收到非空 metadata 默认显式返回 unsupported。worker span 记录 job id/name/attempt/request_id，不记录 payload；request ID 关联不代表 W3C/Otel trace link。定向 HTTP timeout/cancellation 测试 1 项、queue external driver contract 4 项、JobRegistry deadline/scope 测试 1 项、metadata malformed/roundtrip 单测已实际通过。PostgreSQL 18：`cargo test --manifest-path examples/demo/Cargo.toml --features queue,db-postgres --test db_migration --test queue --test queue_crash_recovery -- --nocapture` 实际运行 migration 1、queue 1、crash recovery 5 项通过；MySQL 8.4 同一命令实际运行 1+1+5 项通过。两库 queue target 含持久化 worker 和任务链中的 request ID/locale round-trip；均使用既有 `openrust-m4-auth-tx` 健康服务及 TestDatabase 隔离库，未改容器或输出凭据。最终门禁已通过：`cargo fmt --all --check`、`git diff --check`、workspace all-target/all-features clippy、默认/all-features workspace tests、warnings-denied 默认/all-features docs、`cargo deny check`、`cargo check -p openrust --no-default-features`。`cargo deny check` 返回成功，仅报告既有重复依赖及未匹配 MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。本地 scoped commit 尚未创建。

2026-10-26 M5 F4b 单进程同键加载合并：新增有界 `CacheStore`/`Cache` `get_or_load_with`，键按 store、namespace、logical key、返回类型和稳定 load ID 隔离；配置校验 key/waiter 上限与 loader timeout。单测覆盖 8 路合并、类型/load ID 隔离、leader 取消重选、容量/等待者拒绝、超时与错误广播回收。`cargo test --workspace` 与 `cargo test --workspace --all-features` 通过；其中 all-features openrust 184 项通过、2 项 ignored；`cargo test --manifest-path examples/demo/Cargo.toml --features cache-redis --test server cache_plugin_uses_the_configured_shared_redis_store -- --nocapture` 在真实 Redis 上通过，覆盖 Redis tag 失效和应用聚合 singleflight。`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、warnings-denied all-features docs、`cargo deny check`、`cargo check -p openrust --no-default-features` 通过；deny 保留既有 MPL allowance 未匹配和重复依赖告警，advisories/bans/licenses/sources 均通过。此单进程功能无数据库写入，不要求双库测试；PostgreSQL/MySQL 集群一致性不由此记录宣称，跨节点 SWR 留待 F4c。干净源码副本中当前 CLI cache 模板生成 check/test 已于同轮执行记录（未使用 registry patch 或机器绝对路径）。

2026-10-26 M5 F4c stale-while-revalidate 与应用后台任务生命周期：定向 SWR 测试覆盖 fresh/stale/最终过期、tag 失效、刷新失败与 stale 上限、并发刷新限制、取消及停机 join；真实 Redis 双连接 server 测试通过。PostgreSQL 18 与 MySQL 8.4 分别以两个独立 App/数据库 pool/Redis 连接实际运行 `db_coordination::f4c_swr_uses_shared_locks_across_apps_and_redis_connections`，各 1 项通过，验证共享锁降低重复刷新并观察共享刷新结果；此为单进程多 App 专项，不代表多节点集群验收。干净源码副本生成应用直接 `cargo check`、`cargo test` 通过，无 registry patch 或开发机绝对路径。`cargo test --workspace`、`cargo test --workspace --all-features`、default/all-features all-target clippy、warnings-denied default/all-features docs、`cargo check -p openrust --no-default-features`、`cargo deny check`、fmt 与 diff check 通过；deny 仅有既有未匹配 MPL allowance 与重复依赖警告，advisories/bans/licenses/sources 均通过。完整集群、多节点缓存负载及外部通知未由本簇覆盖。

2026-10-26 显式同步任务执行：复用 历史决策 043 的 `JobRegistry::execute`，没有增加重复 API。定向 `cargo test --manifest-path examples/demo/Cargo.toml --features queue --test queue_driver_contract` 5 项通过，新增纯进程内 await 行为断言，不配置数据库/Redis；明确进度需 reporter、取消为合作语义，且直接执行不提供事务派发、持久化、自动重试或任务组。workspace fmt、clippy、tests、warnings-denied docs 与 `cargo deny check` 均通过；deny 的重复 crate 版本与未匹配 MPL allowance 为现有告警。

2026-10-26 M5 F5 资源查询授权（历史决策 047）：`cargo test -p openrust --features 'auth db-postgres' auth::query::tests` 验证非空范围构造、空 OR 组与规则错误 fail-closed，以及嵌套 OR 筛选仍位于 owner scope 的 AND 内。真实隔离库 HTTP 测试 `cargo test --manifest-path examples/demo/Cargo.toml --features 'auth db-postgres' --test auth_query_authorization` 与 MySQL `db-mysql` 对应命令均通过 PostgreSQL 18 / MySQL 8.4：owner 只能读取匹配行；越权 ID 返回空；分页只计授权行；混合授权 ID 批量修改/删除只影响本人行；关联查询校验父记录并用子实体规则过滤子集合。独立 consumer `cargo check --manifest-path tests/consumers/query-authorization/Cargo.toml` 通过。裸 SeaORM、直接关联加载、原生 SQL、自定义 driver 不被自动拦截；完整业务 CRUD、导出组合和集群压力仍待相应验收。

## M5 历史决策 048 稳定键有界分批读取

`cargo test -p openrust --features 'auth db-postgres' batch_options_enforce_the_documented_row_bound` 验证默认 100、最小 1、最大 1000 及越界拒绝。PostgreSQL 18 与 MySQL 8.4 分别使用已有 loopback 服务运行 `cargo test --manifest-path examples/demo/Cargo.toml --features 'auth db-<backend>' --test auth_query_authorization -- --nocapture`，各实际执行 1 项通过：owner scope 分页读取、取消后用最后完整主键游标续读、无越权/重复行、复合主键升降序和跨页顺序、callback 错误停止及连接继续可用。测试凭据从既有容器配置在进程内构造，不输出凭据、不创建或更改服务。独立 consumer `cargo check --manifest-path tests/consumers/query-authorization/Cargo.toml --locked` 通过。

## M5 历史决策 049 数据库迁移互斥

在既有 PostgreSQL 18 与 MySQL 8.4 loopback 服务上，分别执行 `OPENRUST_TEST_DATABASE_URL=<service-url> cargo test --manifest-path examples/demo/Cargo.toml --features db-<backend> --test db_migration -- --nocapture`，4 项均通过。真实隔离数据库和独立子进程验证首次迁移竞争只运行一个 migration callback、第二个 up 等待并读到最新状态、短等待预算返回 `migration_lock_timeout`、持锁进程强杀后恢复，以及既有 up/down/status 与具名数据库隔离。凭据从容器配置在进程中构造，不输出、不更改或停止服务。fmt、default/all-features all-target warnings-denied Clippy、default/all-features workspace tests、warnings-denied default/all-features docs、no-default-features check 与 `cargo deny check` 通过；workspace 运行时不配置的 DB 集成 targets 不代替上述显式双库执行。未宣称生产多节点迁移演练。

## M5 历史决策 050 文件存储

`cargo fmt --all --check`、default/all-features all-target Clippy、`cargo test --workspace`、`cargo test --workspace --all-features`、warnings-denied default/all-features Rustdoc、`cargo check -p openrust --no-default-features` 与 `storage-local`/`storage-s3` feature checks 均通过；`cargo deny check` 通过，仍报告既有 duplicate versions 与未匹配 MPL-2.0 allowance。all-features 中 `openrust::storage` 本地文件测试、`server/tests/storage/driver_contract.rs` 独立公开 API consumer、`server/tests/storage/http.rs` 真实 TCP 授权上传/下载均执行并通过。CLI 生成的 `apps/storage_demo` 在无 `.git` 的源码副本中直接 `cargo check -p storage_demo` 与 `cargo test -p storage_demo` 通过；生成 manifest 使用 workspace 相对 path，无临时 registry patch 或开发机绝对路径。

被 workspace 测试忽略的真实服务专项已显式运行：`NO_PROXY=127.0.0.1,localhost OPENRUST_TEST_S3_ENDPOINT=http://127.0.0.1:19000 OPENRUST_TEST_S3_ACCESS_KEY=<本机测试值> OPENRUST_TEST_S3_SECRET_KEY=<本机测试值> OPENRUST_TEST_S3_VERSIONED_BUCKET=openrust-storage-versioned OPENRUST_TEST_S3_PLAIN_BUCKET=openrust-storage-plain cargo test --manifest-path examples/demo/Cargo.toml --features storage-s3 --test storage_s3 -- --ignored --nocapture`。测试连接 MinIO `RELEASE.2025-10-15T17-29-55Z`，versioned/plain buckets 均为真实服务，验证 metadata、12 MiB 多段上传、检查器拒绝、延时上传取消与目标不可见、覆盖后旧 version body 与 presigned URL 仍读取旧内容、短期 URL 过期及未启用版本控制时 fail-closed。结束后通过 MinIO client 核实两个 bucket 均无未完成 multipart，也无本测试遗留对象。服务使用本任务创建的临时容器与 tmpfs 数据盘，非生产多节点集群。

`StorageDisk::put_body` 限制交给 inspector/backend 的数据块并保留背压，但不能限制 Axum 上游构造单个 body frame 时已经发生的分配；直接 `put_stream` 的调用方也须控制输入 chunk。filesystem 仅本机可见；API 节点 A 上传、B 下载仍在 v1 集群门禁验证，不能由单服务 MinIO 测试代替。真实 provider 进程强杀后的 multipart 回收依赖 bucket 生命周期策略。

并发应用启动回归：`cargo test -p openrust --all-features storage::opendal_driver::concurrent_filesystem_driver_builds_reserve_distinct_staging_directories --lib -- --exact` 使用 64 个并发 filesystem driver 初始化同一 root，验证 staging 路径不会因时钟刻度碰撞而失败；`cargo test --manifest-path examples/demo/Cargo.toml --all-features --test server` 聚合测试 10 项并发运行通过。根因是旧 staging 名仅使用 PID 与纳秒时钟，所有同时初始化的 server App 共用 filesystem root；现改为进程内原子序号并对 `AlreadyExists` 做最多 16 次重试。

2026-10-27 F7 SSO 真实 Redis 故障与恢复：PostgreSQL 18、MySQL 8.4 均以 Keycloak 26.8.1、Redis 8 独立容器、两个隔离应用及每应用两个 API 子进程执行 `cargo test --manifest-path examples/demo/Cargo.toml --features 'auth-oidc auth-mfa session-redis db-<backend>' --test auth_oidc_keycloak -- --nocapture`。测试真实停止专用 Redis 容器，确认已验证的 Back-Channel Logout 返回 503；恢复 Redis 后重启两个应用的四个 API 子进程，以相同 Logout Token 重放并获得成功，随后验证共享 Session/refresh 状态。运行结果：PostgreSQL 18 通过 1 项（17.51 秒），MySQL 8.4 通过 1 项（18.11 秒）。另以既有 Redis 8 服务复跑双库正常双应用 SSO/退出路径（各 1 项）及 OIDC 注入一次 Session 删除失败后重试（各 1 项），均通过。真实连接故障后 Redis driver 连接管理器未在存活进程内恢复；当前验收语义要求恢复服务后重启 API 副本，再重放通知。M6 完整集群门禁仍需另行验收。

2026-10-27 F6-A 租户上下文与查询隔离：`cargo test -p openrust tenant::tests --lib` 3 项通过，覆盖 TenantId 校验/Debug 脱敏、task-local 租户隔离、嵌套切租拒绝与 resolver 失败关闭。PostgreSQL 18/MySQL 8.4 分别执行 `OPENRUST_TEST_DATABASE_URL=<service-url> cargo test --manifest-path examples/demo/Cargo.toml --features 'auth auth-mfa db-<backend>' --test auth_query_authorization -- --nocapture`，各 2 项通过：原 F5 授权查询回归 1 项和 F6 tenant HTTP 专项 1 项。新专项在每个 backend 启动两个真实 TCP API listener、共享隔离库，使用 AuthLayer 校验身份后由 TenantLayer resolver 将两个用户映射到不同 tenant；伪造 `x-tenant-id` 不改变 tenant；用户/tenant 双 scope 限制列表、关联、批量更新/删除，交叉 tenant 行保持不变，直接调用受保护规则且缺上下文时拒绝。workspace fmt、default/all-features clippy、default/all-features tests、warnings-denied default/all-features docs、no-default check 与 cargo deny 均通过；cargo deny 仅有既有重复依赖与未匹配 MPL allowance 警告。

F6-B 租户资源传播已完成：Cache key/singleflight/tag generation、跨后台 SWR 任务固定 tenant 物理键、持久 Queue metadata/worker task scope、Storage tenant 物理路径、Audit tenant_id 与损坏 envelope fail-closed。单元测试 133 项通过；PostgreSQL 18/MySQL 8.4 各自 `cargo test --manifest-path examples/demo/Cargo.toml --features 'auth,auth-mfa,queue,db-<backend>' --test auth_database` 7 项、`--test queue` 1 项、`cargo test --manifest-path examples/demo/Cargo.toml --features 'queue,db-<backend>' --test queue_crash_recovery` 7 项均通过。独立 worker 进程首次尝试失败，另一 worker 实例重试后完成并恢复租户上下文。真实 Redis 8 配合两个独立 Cache App/连接运行 `db_coordination`，PostgreSQL/MySQL 各 2 项均通过：验证共享 SWR 锁、跨租户 key/tag 隔离与 tag 失效边界；本地 filesystem 双 App 和 custom Storage driver 使用相同逻辑路径的隔离测试通过。fmt、default/all-features all-target warnings-denied Clippy、default/all-features workspace tests、warnings-denied default/all-features Rustdoc、no-default-features check 与 `cargo deny check` 均通过；deny 仅有既有重复依赖和未匹配 MPL-2.0 allowance 告警。

## M5 gRPC G2c 调用预算、资源限制与健康停机

真实 TCP 示例 `cargo test --manifest-path tests/consumers/grpc/Cargo.toml --locked --all-features` 11 项通过：完整 server-stream deadline、全局并发超限/许可释放、消息大小限制、reflection descriptor 白名单及缺 descriptor 拒绝、health Watch 停机状态、非终止流的 shutdown deadline，以及既有 JWT/TLS/mTLS 用例。`cargo test --manifest-path examples/demo/Cargo.toml --features grpc --test grpc -- --nocapture` 6 项通过，覆盖共享 readiness、明文安全默认和真实子进程 SIGTERM。SIGTERM 子进程测试清除继承的 `RUST_TEST_THREADS`，避免串行 workspace runner 影响子进程测试调度。

通过 `cargo fmt --all --check`、默认与 all-features workspace all-target Clippy（`-D warnings`）、默认 `cargo test --workspace --quiet`、串行 `RUST_TEST_THREADS=1 cargo test --workspace --all-features --quiet`、warnings-denied 默认/all-features workspace Rustdoc、workspace 与独立示例 `cargo deny check`、独立示例 all-target/all-features Clippy、Rust 1.97 根 gRPC feature check、独立示例 Rust 1.97 locked all-target/all-features check、no-default-features gRPC check 与 7 项 gRPC unit tests。独立示例锁定依赖并通过完整 all-features 测试；根与示例 cargo deny 均通过 advisories/bans/licenses/sources，保留既有 duplicate crate versions 与未匹配 MPL allowance 警告。G2c 只变更 gRPC runtime，不新增数据库行为；G2b JWT/PAT/Policy 的 PostgreSQL 18/MySQL 8.4 证据仍见前文，G2c 不重复声称双库认证验收。HTTP+gRPC 联合 supervisor、真实多节点路由、滚动升级和 proto 新旧互通仍未验收，列入 G3。

## M5 gRPC G3a HTTP/RPC 联合生命周期

示例 `cargo test --manifest-path tests/consumers/grpc/Cargo.toml --locked --all-features` 15 项通过，包含联合 HTTP `/ready` 与 gRPC unary 的真实 TCP 调用、双 listener readiness/停机、gRPC 绑定冲突后 HTTP listener 自动释放、`combined_shutdown_timeout_aborts_and_joins_both_listeners` 超时回收，以及插件服务注册前置条件和既有 G2 TLS/auth/deadline/health 运行治理；`cargo test --manifest-path examples/demo/Cargo.toml --features grpc --test grpc -- --nocapture` 7 项通过，其中 `server_host_composes_http_and_grpc_under_one_lifecycle` 通过真实 TCP 调用 HTTP health 与 gRPC health，并验证统一停机关闭两个端口；`cargo test -p openrust --features grpc runtime::tests --lib` 2 项验证早期 listener 退出会报错、故障 listener 会通知并 join sibling。该组合入口先构建 router/TLS、再绑定两个 listener；`Readiness` 只有两个 listener 都 active 才为 ready，supervisor 统一 stop、deadline、abort/join 与 app-owned background task 回收。没有模拟真实节点故障或 HTTP/2 代理，不能作为 G3c/集群证据。

本簇通过 `cargo fmt --all --check`、默认/all-features workspace all-target Clippy（`-D warnings`）、默认 `cargo test --workspace --quiet`、串行 `RUST_TEST_THREADS=1 cargo test --workspace --all-features --quiet`、warnings-denied 默认/all-features workspace Rustdoc、`cargo deny check`、Rust 1.97 根 gRPC feature check 与独立示例 locked all-target/all-features check。示例单独运行 all-features Clippy 与 `cargo deny check`；deny 的 advisories/bans/licenses/sources 通过，仅保留既有重复 crate 版本和未匹配 MPL allowance 警告。无数据库行为变化，不重复 PostgreSQL/MySQL 数据验收；proto/trace 传播已在 G3b 验收；完整双节点路由留待 G3c。

## M5 gRPC G3b Proto 兼容与 trace 传播

当前 Echo schema 保留原有 package、service、RPC 路径和字段 tag 1，在 request 增 `client_hint`、response 增 `api_revision` tag 2；旧 schema 单独生成到示例 build output 的 `legacy/`，不污染根输出。`cargo test --manifest-path tests/consumers/grpc/Cargo.toml --all-features old_and_new_proto_clients_interoperate_over_real_tcp -- --nocapture` 实测旧 client→当前 server（旧响应 decoder 忽略 tag 2）与当前 client（发送未知 request tag 2）→旧 server（旧 server 忽略字段、新 client 收到新 response tag 默认值）均通过。

trace 仅传播经严格校验的 W3C version 00 `traceparent`。`cargo test -p openrust --features grpc --lib` 147 项通过，含 serde round-trip、零 ID/版本/大小写/注入格式拒绝、HTTP middleware 接受有效值并忽略非法值、typed client 只写入 traceparent 的边界测试；`cargo test --manifest-path examples/demo/Cargo.toml --features grpc --test grpc -- --nocapture` 7 项通过，其中联合 HTTP/RPC 的应用集成宿主经真实 TCP `/trace` 读取 `ExecutionContext`。示例 all-features 16 项通过；`combined_runtime_serves_both_protocols_and_drains_together` 发起真实 HTTP 请求并由 handler typed RPC 返回同一 traceparent，证明 HTTP→RPC 传播。没有 OTel feature 依赖，不传播 tracestate、baggage、Authorization、Cookie 或任意 headers。

默认与 all-features workspace all-target Clippy、默认 workspace tests、串行 all-features workspace tests、warnings-denied 默认/all-features Rustdoc、workspace 与示例 cargo deny、示例 all-target/all-features Clippy、Rust 1.97 gRPC feature check、no-default-features check 与 `git diff --check` 均通过；cargo deny 的 advisories/bans/licenses/sources 通过，仅保留既有重复 crate 版本和未匹配 MPL allowance 警告。双库不是本簇行为依赖；all-features 示例里的数据库认证用例因未设置 `OPENRUST_TEST_DATABASE_URL` 按其既有条件跳过，G2b 双库认证证据不在此重复计入。G3c 在该次 G3b 验收时未覆盖，后续真实双节点代理证据见下节。

## M5 gRPC G3c 双节点 HTTP/2 路由与健康摘流

`tests/consumers/grpc/scripts/test-g3c.sh` 使用 Docker Compose 启动两个真实 Linux OpenRust 节点和 Envoy 1.40.1。单一持久 `tonic::Channel` 的 40 次 unary RPC 由同一个 Envoy downstream HTTP/2 connection ID 发出，access log 显示两个 upstream 容器；长流运行时向其所属节点发送 SIGTERM，标准 gRPC Health `Check` 观测到 `NOT_SERVING`，Envoy gRPC active health checker 将节点标记为 unhealthy，10 次后续 unary RPC 全部抵达存活节点。既有长流因节点退出而中断，同 Channel 新调用成功；重启节点后 40 次 RPC 再次覆盖两个 upstream。末次实测 Envoy 约 0.54% CPU、19.78 MiB，单次观测不作为容量基线。Compose 脚本会清理该项目的容器、网络和本地节点镜像。

停机健康状态行为由 `cargo test --manifest-path examples/demo/Cargo.toml --all-features --test grpc` 8 项验证，其中 Health `Watch` 收到 `NOT_SERVING`、宽限期等于 shutdown deadline 被插件拒绝；`cargo test -p openrust --features grpc --lib grpc::tests::health_drain_grace_must_fit_inside_shutdown_deadline` 1 项通过。独立示例 `cargo test --locked --manifest-path tests/consumers/grpc/Cargo.toml --all-features` 16 项通过；本轮没有设置数据库 URL，条件数据库认证路径未构成新的双库证据。

本簇门禁通过：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo test --workspace --quiet`、`RUST_TEST_THREADS=1 cargo test --workspace --all-features --quiet`、warnings-denied workspace/示例 all-features Rustdoc、workspace 与独立示例 `cargo deny check`、独立示例 all-target/all-features Clippy、Rust 1.97 根 `grpc` 和独立示例 check、shell syntax、`git diff --check`，以及上述真实 Envoy 双节点验收。cargo deny 的 advisories/bans/licenses/sources 通过，保留既有 duplicate crate 与未匹配 MPL allowance 提示。G3c 不涉及数据库行为，不以本簇重复数据库验收；完整 API/worker 集群、多 worker/scheduler、共享依赖、跨节点上传和实时通信留在 M6。

## M5 历史决策 060 密码哈希容量

使用 `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test -p openrust --features auth auth::password::tests --lib`，7 项通过；`cargo test -p openrust --features auth --lib`，160 项通过；`cargo test -p openrust --all-features --lib`，258 项通过、2 项忽略。`cargo test --manifest-path examples/demo/Cargo.toml --features auth --test auth_runtime auth_plugin_rejects_invalid_password_capacity_during_startup -- --exact`，1 项通过。PostgreSQL 18 和 MySQL 8.4 复用已运行容器，分别在 `OPENRUST_TEST_DATABASE_URL` 指向容器本地管理库时执行 `cargo test --manifest-path examples/demo/Cargo.toml --features 'auth db-<backend>' --test auth_database auth_flows_run_registration_login_refresh_reset_and_logout_on_the_configured_database -- --exact --nocapture`，各 1 项通过；URL 凭据从容器配置在测试进程内构造，未输出，数据由 `TestDatabase` 创建隔离库并在结束时删除。

门禁：`cargo fmt --all --check`、`git diff --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`、auth feature 下根库/`根目录集成宿主` 定向 Clippy 与 Rustdoc、`cargo check -p openrust --no-default-features`、`cargo deny check` 均通过。首次对仅 `auth` 的根包 all-target Clippy 出现已有的 `CurrentUser::test_user` dead_code 警告，随后匹配范围的库 Clippy 和完整 workspace all-target Clippy 均通过。cargo deny 的 advisories/bans/licenses/sources 通过，保留既有重复 crate 与未匹配 MPL allowance 警告。默认 workspace 未配置 DB URL 的其他集成 targets 按条件跳过；双库 AuthFlows 用例已显式执行。无 all-features workspace 矩阵记录；此子簇匹配 `auth` feature、默认 workspace 及双库用例均已覆盖。

## M5 历史决策 061 依赖就绪探针

`Readiness::register_probe`/`dependencies_ready` 与 HTTP `/ready` 已实现；最多 64 个稳定命名探针，整轮 2 秒、最多 16 并发且同一时刻只运行一轮。探针失败仅在服务日志记录探针名与稳定错误码，HTTP 返回通用 503。探针注册/重复名、失败恢复、并发上限/单飞及 ready 503 脱敏单测通过；根 `cargo test -p openrust --lib` 在新增单飞测试前 139 项通过，随后相关 readiness 5 项与 health 1 项再次定向通过。PostgreSQL 18/MySQL 8.4 均使用现有容器，经本地 TCP 故障代理实测真实 DB pool 断连时探测失败、代理恢复后 pool 重新连通并恢复 readiness；健康 pool 探针各另通过 1 项。Redis 8 现有服务运行 `cargo test --manifest-path examples/demo/Cargo.toml --features cache-redis --test server cache_plugin_uses_the_configured_shared_redis_store -- --exact --nocapture`，真实 CachePlugin store 探针通过。`cargo clippy -p openrust --all-targets --features 'db-postgres,cache-redis' -- -D warnings`、`cargo clippy -p server --test db_connection --features 'db-postgres,db-mysql' -- -D warnings`、`RUSTDOCFLAGS='-D warnings' cargo doc -p openrust --no-deps --features 'db-postgres,cache-redis'`、`cargo deny check`、fmt 与 diff check 通过；deny 的 advisories/bans/licenses/sources 通过，保留既有 duplicate crate 与未匹配 MPL allowance 提示。随后完整 workspace 默认 Clippy 与 tests 已通过；此记录仅描述 历史决策 061 专项证据，不代表当前 M5 全部验收完成。

## M5 历史决策 067 低基数指标与 OpenTelemetry（WIP）

2026-10-27：可选 `metrics` feature 提供显式启用的 Prometheus `/metrics`，HTTP 指标使用固定 method/status 类标签；真实 TCP 子进程测试确认默认服务不提供该路由、配置开启后可抓取，正文无 request ID 或路由标签。HTTP middleware 有界并发超限快速返回 503。可选 `otel` feature 通过 OTLP/HTTP batch exporter 导出 spans；本机临时 Collector 0.120.0 实际收到 HTTP spans，并通过真实跨协议 consumer 观察到 HTTP 与 typed gRPC spans 保留同一个 W3C Trace ID。同步 exporter 在 Tokio blocking pool 中 flush，测试覆盖 server lifecycle drain。HTTP span 不写 URI/request ID/traceparent；OTel 属性只含协议字段。PG18/MySQL8.4 真实隔离库登录突发在 2 connection pool 下运行 16 次请求，两个后端均观察到密码哈希容量拒绝并在突发后成功登录；样本结果见当前任务记录，尚不足以建立正式回归阈值或分离 API CPU/内存基线。

当前轮门禁：`cargo fmt --all --check`、`git diff --check`、默认与 all-features `cargo clippy --workspace --all-targets`、默认与 all-features `cargo test --workspace`、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features` 与 `cargo deny check` 均通过；default workspace 中未设置 `OPENRUST_TEST_DATABASE_URL` 的集成测试按条件跳过。all-features workspace 测试 270 项通过、2 项忽略；默认 230 项通过。deny 的 advisories/bans/licenses/sources 通过，仍有既有 duplicate crate 与未匹配 MPL allowance 警告。PG18/MySQL8.4 突发单次样本分别为：PG pool=2、16 请求、4 成功/12 容量拒绝、623.1 req/s、p50 10.9ms、p95/p99 17.8ms、恢复 7.8ms；MySQL pool=2、16 请求、4 成功/12 拒绝、488.7 req/s、p50 16.8ms、p95/p99 23.9ms、恢复 8.7ms。单次本机小样本不能作为回归阈值；`/usr/bin/time -lp` 包含编译与测试进程，不能分离服务 CPU/内存。仍缺稳定重复压力模型、服务资源分项及过载下真实 API 吞吐/错误率报告，因此 历史决策 067 保持 Proposed，M5 继续 in_progress。

后续对 HTTP response body 持有并发许可的工作区 WIP 做定向复验：`cargo test -p openrust --features 'metrics,otel,grpc' http::middleware::tests --lib` 4 项通过；`cargo clippy -p openrust --all-targets --features 'metrics,otel,grpc' -- -D warnings` 通过。测试验证许可在 body 读完/丢弃后释放，并保持并发超限立即返回 503；这部分 WIP 未包含在前述全 workspace 门禁或已提交实现中。

构建资源：代码门禁结束后 dry-run 再 clean，Cargo 报告主 target 回收 23.5 GiB、consumer 1.2 GiB、默认生成应用 1.8 GiB、双库生成验收 3.9 GiB；磁盘余量由约 101 GiB 恢复至 125 GiB。主 target 实际磁盘占用峰值约 17.95 GiB，合计 consumer 约 19.1 GiB。不清理主仓 target 或他人服务。生成应用源码与 /tmp 验收日志暂留供主控核对。

## R2-B 统一 Provider 内核（验收证据）

2026-10-28：所有本段 Cargo 命令使用 `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/tmp/openrust-r2-provider-target`，URL 由现有 Docker `postgres:18`、`mysql:8.4` 容器环境在子进程内构造，仅传入 `OPENRUST_TEST_DATABASE_URL`，日志不输出凭据。PostgreSQL 与 MySQL 分别以对应 `db-postgres auth` / `db-mysql auth` feature 运行 `auth::provider_tests::early_boot_resolves_connected_database_and_initialized_auth`、`db::provider_tests::failed_provider_second_connection_closes_first_pool`、`auth::provider_tests::default_auth_boots_with_real_custom_database_provider`，每库每项 1/1 通过。覆盖前序 boot 解析后置 Provider 的真实可用 Db/Auth、Provider.start 后续真实连接失败自动关闭首个 pool（保留 clone 的 ping 被拒绝）、自定义 Db Provider 的 Auth 初始化与关闭。DB 另有无 I/O 的后序畸形 URL / 已绑定 Readiness 工厂失败回归，`cargo test --lib --features db-postgres db::provider_tests --quiet` 7/7。启动 future 取消触发已有 ResourceOwner 异步清理的定向回归 `core::provider::tests::cancelled_startup_runs_registered_cleanup` 1/1；仅保证存活的 Tokio runtime 内可派发回收，不证明 runtime 已退出后的异步清理。

生成项目 `/tmp/openrust-provider-cli-check` 以本仓库路径 patch 引用框架；`cargo test --offline --test auth_flows --quiet -- --nocapture` 在 PostgreSQL 18 1/1、MySQL 8.4 1/1，通过真实 TCP HTTP 认证流程与各自隔离 TestDatabase。该项目最初由 CLI 选择 PostgreSQL；第一次 MySQL 验证只把临时项目 Cargo.toml 两处 `db-postgres` feature 改为 `db-mysql`。随后实际执行 `cargo run -p cli --offline -- new /tmp/openrust-provider-cli-mysql --db mysql --auth`，新 MySQL 产物的 `cargo check --tests --offline` 通过，真实 MySQL 8.4 的 `cargo test --test auth_flows --offline` 1/1。两项真实 HTTP 测试均设置数据库 URL，未按缺环境跳过。

Provider 消费者首轮：`tests/consumers/grpc` 的 `cargo test --offline` 15/15，含 TLS、reflection 和 HTTP+gRPC 联合真实监听；`tests/consumers/realtime` 真实 Redis 8 双 API 进程 WebSocket+SSE 测试首跑因旧同步 `AppState::get` 未取 factory 超时，改为关闭时异步 resolve 后重跑 1/1（10.11 秒）。`cache-custom-driver` 与 `communication-drivers` 独立 manifest 的 check/test 各 1/1，binary 各 exit 0；`db-custom-sqlite` check/test 0 项，binary 首跑因示例在 `run_command` 自动关闭连接后继续使用该连接而失败，调整为先完成事务/自有迁移再验证命令拒绝后 binary exit 0。所有 consumer 首次 `--locked` 因各自锁文件需更新而拒绝，后用 `--offline` 更新 task-owned Cargo.lock 后验证。另真实 PG18/MySQL8.4 各运行 `db::provider_tests::cancelled_provider_start_closes_first_real_pool` 1/1，先确认保留的 pool 可 ping，再 abort 启动 future，等待 clone ping 因关闭失败。

根 Server 和已迁领域集成：`cargo test --test server --test storage_http --features storage-local` 分别 15/15、1/1；`storage_driver_contract` 1/1。PostgreSQL 18 与 MySQL 8.4 真实隔离库分别执行 `cargo test --test db_coordination --features 'db-<backend> cache-redis'` 各 2/2，及 `cargo test --test data_exchange_tasks --features 'db-<backend> data-exchange-tasks storage-local'` 各 1/1。`cargo check -p openrust --no-default-features` 通过；根标准配置显式 Redis 时 `cargo run --no-default-features --bin server -- help` 按预期失败，诊断 `cache driver redis requires cache-redis feature`，未静默退回 memory。默认 workspace all-targets check 已通过，all-features check 初轮发现旧测试迁移编译错误，正在修复。
HTTP body/scope/Inject 真实 Router 集成 `cargo test --test http_router --quiet` 45/45；HTTP 内测的 `/ready` 已由 factory-only Readiness 通过 async resolve，`http::health::tests` 1/1；Provider 受管任务的 HTTP shutdown 内测 `http::serve::tests` 3/3；metrics 内测 2/2。这些是定向证据，尚非完整 workspace 门禁。

统一内核替换后的本轮门禁：`cargo fmt --all --check`、默认与 all-features 的 `cargo clippy --workspace --all-targets -- -D warnings`、默认与 all-features `cargo test --workspace`、两套 `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`、`cargo deny check`、无默认 feature 的 workspace all-target check，以及 Rust 1.97 的 all-features 与 no-default-features workspace all-target check 均 exit 0。第一次 all-features test 只有根 README 的旧 QueuePlugin doctest 失败；更新为 `QueueServiceProvider::contributions()` 后 14 passed/24 ignored，完整 all-features workspace 重跑 exit 0。默认 workspace test 也 exit 0；ignored 项不计通过。

真实数据门禁在现有 PostgreSQL 18 与 MySQL 8.4 上各执行 `db_connection`、`db_migration`、`auth_database`、`auth_runtime`、`queue`、`schedule`、`auth_mfa`、`auth_oidc`、`queue_crash_recovery`、`schedule_calendar`、`queue_notifications`、`example_auth_flows` 共 12 target，各 54 passed/0 ignored。初轮 `auth_database` 一项因旧测试只 drop App 就关闭隔离库，未等待新 Provider 持有的连接池关闭而失败；测试在关闭隔离库前显式等待两个 `App::shutdown`，两库复跑均通过。连续迁移命令回归 `db_migration::migration_command_applies_repeats_reports_and_rolls_back` 两库各 1 passed，证明 `App::run_command` 可复用同一连接直至显式关闭。

CLI 实际生成的 PostgreSQL 和独立 `--db mysql` 应用均重新执行 `cargo check --tests --offline` exit 0，真实数据库环境的 `cargo test --test auth_flows --offline` 各 1 passed/0 ignored。八个独立 consumer 的 `cargo test --offline` 均 exit 0；gRPC 15 passed、realtime 使用真实 Redis 8 跨进程 1 passed、query-authorization 与 SQLite consumer 为 0 test。另将验证逻辑放在 `main` 的 cache-custom-driver、communication-drivers、data-exchange、db-custom-sqlite、queue-custom-driver 五个 binary 实际 `cargo run --offline` 均 exit 0，不能把单纯 0 test 当成行为证据。

根默认 feature 真正 `cargo run --offline --bin server -- serve` 启动监听，禁用测试客户端环境代理后 loopback `/health`、`/api`、`/api/example`、`/` 均 HTTP 200，SIGINT 后进程 exit 0。首次 Python urllib 受代理环境影响收到 502，未据此改服务代码；禁用 ProxyHandler 后真实直连复核通过。

不同模型 gpt-6.1-sol 对旧命令过早关闭整 App、最终 shutdown 重新开放 root scope、RateLimit 忽略请求 scope 三项修复做有限只读复核，均无剩余确定 blocker/should-fix；core app 12 项和请求 scoped limiter 1 项定向回归通过。审查不替代实际运行。此段仅证明 R2-B 统一内核簇，不代表 R2-M 模块目录、R1-E CRUD 或完整 Laravel 1:1 已完成。Figment 字典排序导致来源内对象插入顺序无法恢复；第三方自定义 driver 若未提供专属无 I/O validator，其选项仍是公开扩展方责任，保留差异记录。

构建资源：本任务独占 target `/tmp/openrust-r2-provider-target` 一度约 25 GiB，主卷可用约 91/926 GiB 触及 10% 软阈值。门禁中确认无 cargo/rustc 进程后只删除该 target 的 `debug/deps/*.o` 可再生成中间对象，保留 rlib/rmeta、锁文件、共享仓库 target 和服务数据；task target 约 13 GiB，主卷可用回升到约 106 GiB。最终提交后再次确认无构建进程，`cargo clean --dry-run` 报告独占 target 37503 文件/22.4 GiB，随后实际 `cargo clean` 回收同量；未清共享仓库 target、服务数据或生成项目源码。

## R2-C 入口 scope 与受管清理（验收证据）

2026-10-28：本簇在 R2-B 唯一 Container/Provider 内核上为一次命令、一次队列 attempt 和每个到期调度 slot 建立独立 scope；HTTP 请求 scope 延续到流式 body 轮询和结束/断连。`Scope::run` 仅为当前 future 安装同容器 task-local；`Scope::spawn` 在同 scope 和显式 ExecutionMetadata 内构造、执行并受管取消/等待，普通 `tokio::spawn` 不继承请求身份。父 ResourceOwner 使用单一持久清理 driver，最多 4096 个活动/待清理子 scope；超额拒绝新 scope，不丢已登记回调。命令 future 取消时独立 guard 立即封闭 scope，即使业务保留 Arc 也不能继续解析；同一 App 可执行后续命令。队列正常返回、失败、超时和合作取消在确认/重试前等待清理；外层 future 强制中止由 Drop 请求父级回收，进程强杀仍按租约过期接管，不能承诺外部副作用撤销。

定向回归：`cargo test -p openrust --lib core::app::tests` 15/15，包含连续命令、取消和外层 timeout 后各一次异步清理/逃逸 scope 关闭、后续命令新实例、受管 spawn 与普通 spawn 隔离；新取消用例首跑揭示外部 Arc 阻止 Drop 清理，加入 `CommandScopeGuard` 后重跑通过。`global_trigger_temporarily_clears_outer_tenant` 1/1。`cargo test -p openrust --test http_router real_tcp_` 3/3，真实 loopback stream/SSE 首帧可从请求 scope 解析、断连后清理一次、App.shutdown 等待受阻回调、并发请求与 plain spawn 隔离；非仅 Router oneshot。不同模型 gpt-6.1-sol 对持久清理 driver 的 idle 交接、取消边界、CommandScopeGuard、Queue 跨 App 身份和 scheduler 租户/metadata 隔离做有限只读复核，最后无确定 blocker/should-fix；审查本身不算运行证据。

真实服务：用既有 `postgres:18`、`mysql:8.4`、`redis:8` 容器，数据库凭据仅在 Python 子进程环境构造 `OPENRUST_TEST_DATABASE_URL`，不打印 URL、不更改服务。每库执行 `CARGO_TARGET_DIR=/tmp/openrust-r2c-target cargo test -p openrust --features 'queue db-<backend>' --test queue --test schedule --offline --quiet`，分别 3/3 与 7/7，0 ignored；日志 `/tmp/openrust-r2c-pg-queue-schedule.log`、`/tmp/openrust-r2c-mysql-queue-schedule.log`。前者覆盖真实数据库 worker 失败重试、超时/取消及 attempt scoped 资源清理，后者覆盖每 slot 独立 scope、失败/取消与全局 trigger 不继承外层 tenant/request metadata；真实 DB 持久化 tick→worker 的测试验证后续 job 使用不同 attempt scope。触发 scope 精确 ID 由真实 DB 事务加公开 custom driver 观测，数据库队列消费由另一个真实 DB worker 入口验证，两者不冒称同一 driver 路径。真实 Redis `cargo test -p openrust --features queue-redis --test queue_redis --offline --quiet` 3/3，含失败→重试、超时、abort 后 child join、回调恰一次及逃逸 scope 封闭，日志 `/tmp/openrust-r2c-redis-queue.log`。公开 custom driver 独立 consumer `cargo run --locked --manifest-path tests/consumers/queue-custom-driver/Cargo.toml --offline --quiet` exit 0，日志 `/tmp/openrust-r2c-custom-driver-consumer.log` 无测试摘要，仅作 binary 行为证据。

生成/实际入口：CLI 分别实际 `new /tmp/openrust-r2c-generated-{pg,mysql}-20261028 --db postgres/mysql --auth`，在每个产物执行 `generate job scope_probe`；各自 `cargo check --tests --offline --quiet` exit 0，设置对应真实 DB URL 的 `cargo test --offline --quiet` 各 auth HTTP 集成 1/1、0 ignored，日志 `/tmp/openrust-r2c-generated-{pg,mysql}-{check,test}.log`。生成 job 是脚手架编译证据，不把未实现 handler 计作任务执行业务验收；临时产物源码已清理。根 `cargo run --offline --quiet --bin server` 在独立 loopback 端口实际 `/health`、`/api`、`/api/example`、`/` 均 200，SIGINT exit 0，日志 `/tmp/openrust-r2c-root-server.log`；另真正无选项 `cargo run --offline --quiet` 默认入口的 `/health` 与 `/` 均 200、SIGINT exit 0，日志 `/tmp/openrust-r2c-root-default-run.log`。唯一过渡示例 `app/example` 的 heartbeat Provider 注册 scoped 运行记录，公开 `JobContext::service` 在两次尝试中解析不同关联号，内嵌测试 1/1；该号仅本 App 内唯一，不代表跨进程 ID。R2-M 模块目录与 R1-E 完整持久化业务示例仍未完成。

全门禁：所有 Cargo 使用 `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/tmp/openrust-r2c-target`。`cargo fmt --all --check`、默认/全特性 `cargo clippy --workspace --all-targets --offline -- -D warnings`、默认/全特性 `cargo test --workspace --offline --quiet`、默认/全特性 `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps --offline`、`cargo deny check`、稳定版 `cargo check -p openrust --no-default-features --offline`、Rust 1.97 `cargo +1.97.0 check --workspace --all-targets --all-features --offline` 与 `cargo +1.97.0 check -p openrust --no-default-features --offline` 均 exit 0。示例代码落地后又重跑 fmt、双 Clippy、默认/全特性 workspace tests、双 Rustdoc；最终日志 `/tmp/openrust-r2c-final-{clippy-default,clippy-all,test-default,test-all-rerun}.log`。全特性 workspace 首次重跑中既有 gRPC 联合监听用例 5 秒内超时（`/tmp/openrust-r2c-final-test-all.log`）；同配置单独精确重跑 1/1（0.26 秒），完整全特性 workspace 再跑 exit 0。并行负载可能相关，但根因未经证明；未删断言或把初次失败隐去。`cargo deny` exit 0，仅有既有重复依赖及未匹配 MPL allowance 警告；默认/全特性 ignored 项不计通过。

构建资源：确认无 Cargo/rustc/rustdoc 进程后，对本簇独占 `/tmp/openrust-r2c-target` 执行 `cargo clean --dry-run`，报告 32319 文件/16.4 GiB；实际 `cargo clean` 回收相同数量。仓库共享 `target/` 约 2.4 GiB 未清理或改动；主卷可用空间由约 102 GiB 回升到约 118 GiB。

## R2-D TestApp 验收证据

2026-10-28，本簇 Cargo 使用 `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/tmp/openrust-r2d-target`，均在本仓 `main` 串行运行。`core::provider::tests` 8/8 验证全体注册后覆盖与移除依赖检查；`testing::` 11/11 验证可重放蓝图、两应用隔离、生产 Router 一次组装、bounded body/timeout、安全断言、raw stream 与 pending handler 的请求 scope 清理、重置/关闭失败、无运行时 Drop 即时封闭；`core::container::tests::sealed_container_does_not_reissue_registered_instance` 1/1、资源内测 13/13。`app/example` 生产 `Server` 的 `production_testapp_` 两项在默认和 all-features 各 2/2：覆盖业务服务绑定替换、Provider 移除后的 Semantic 404 与保留的 health 200。`TestApp.request` 是生产 Router 上的进程内 Tower 请求，不冒称真实 TCP；调用方已持有的旧服务 Arc 无法被框架撤销，显式 `shutdown` 才能等待异步清理。

公开 consumer `cargo test --manifest-path tests/consumers/test-app/Cargo.toml --offline --quiet` 1/1，使用显式 Semantic 模式验证移除业务 Provider 后的 HTTP 404；框架默认 `always_ok` 未改变。CLI 模板快照 `cargo test -p cli project_template_has_expected_layout --offline --quiet` 1/1。实际 `openrust new /tmp/openrust-r2d-generated-basic-fixed-20261028` 生成普通应用，其 `cargo check --tests` 和 `cargo test --offline --quiet` 均 exit 0，生成 TestApp HTTP 集成 1/1。首轮普通产物 `/tmp/openrust-r2d-generated-basic-20261028` 的测试因模板错误地将完整 API envelope 当裸 `name` 对象而失败（`/tmp/openrust-r2d-basic-test.log`）；修正模板断言 `success/code/message/data.name` 后重新生成并通过，未改框架响应默认。

CLI 分别实际 `new /tmp/openrust-r2d-generated-pg-20261028 --db postgres --auth` 和 `new /tmp/openrust-r2d-generated-mysql-20261028 --db mysql --auth`；两个产物的 `cargo check --tests --offline --quiet` 均 exit 0。用既有 Docker PostgreSQL 18/MySQL 8.4 容器环境仅向测试子进程传 `OPENRUST_TEST_DATABASE_URL`，在隔离数据库中各执行 `cargo test --test auth_flows --offline --quiet`，每库 2/2、0 ignored，包含原真实 TCP 认证流程与新增 TestApp fake Mailer 注册请求；日志分别为 `/tmp/openrust-r2d-generated-pg-auth-test.log`、`/tmp/openrust-r2d-generated-mysql-auth-test.log`，没有输出 URL/凭据。生成项目的其余未运行 scaffold 不计业务验收。

共享 `ResourceOwner::seal`、Container 关闭入口改动后，重新执行 `cargo test -p openrust --features 'queue db-<backend>' --test queue --test schedule --offline --quiet`：PostgreSQL 与 MySQL 各 queue 3/3、schedule 7/7、0 ignored，日志 `/tmp/openrust-r2d-postgres-queue-schedule.log`、`/tmp/openrust-r2d-mysql-queue-schedule.log`；真实 Redis 8 的 `cargo test -p openrust --features queue-redis --test queue_redis --offline --quiet` 3/3，日志 `/tmp/openrust-r2d-redis-queue.log`。这些是本簇对 R2-C 清理/隔离路径的重新验证，不把历史结果借用为本次通过。

`cargo fmt --all --check`、默认与 all-features `cargo clippy --workspace --all-targets --offline -- -D warnings`、默认与 all-features `cargo test --workspace --offline --quiet`、默认及 all-features `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --offline --quiet`、`cargo deny check`、`cargo check -p openrust --no-default-features --offline --quiet` 均 exit 0。最终默认与 all-features 测试日志分别为 `/tmp/openrust-r2d-final-test-default.log`、`/tmp/openrust-r2d-final-test-all.log`；汇总各测试结果组，默认 428 passed、23 ignored、0 failed，all-features 719 passed、33 ignored、0 failed，其中 all-features 库测试 463 passed、2 ignored。Rust 1.97.0 的 `cargo +1.97.0 check --workspace --all-targets --all-features --offline --quiet` 及 `cargo +1.97.0 check -p openrust --no-default-features --offline --quiet` 均 exit 0；这两个是 check 门禁，不当成 MSRV 完整测试。`cargo deny` advisories/bans/licenses/sources 均通过，保留既有 duplicate crate 与未匹配 MPL allowance 警告。默认 workspace 的无环境集成用例和 doctest ignored 项不计通过。all-features workspace 首轮在 `--test server` 的 `example_business_route_is_mounted_through_the_root_application` 返回安全码 `internal_error`，单项精确复跑 1/1、server 整组 25/25、全 workspace 复跑 exit 0（首轮 `/tmp/openrust-r2d-allfeatures-workspace-test.log`、复跑 `/tmp/openrust-r2d-allfeatures-workspace-test-rerun.log`）；原因尚未证实，测试辅助启动/构建路径已增加仅失败时输出阶段与安全错误码的诊断，修改后全 workspace 再次 exit 0，不删除断言或序列化并发。

生产默认入口使用显式隔离随机端口 `APP_SERVER_PORT=<port> cargo run --offline --quiet`（该历史验证使用旧键，当前统一为 `APP_PORT`），本机探针禁用环境 HTTP 代理直连，`/health`、`/api`、`/api/example` 与 `/` 均 HTTP 200；对进程发送 SIGINT 后命令 exit 0，日志 `/tmp/openrust-r2d-root-default-run-success.log`。首两次仅测试脚本分别误用 `APP_SERVER__PORT` 与继承本机 HTTP 代理，第三次四路 HTTP 成功但脚本假设 Cargo 必有子 PID，未正确发 SIGINT；这些探针错误不记作框架启动失败，日志分别保留于 `/tmp/openrust-r2d-root-default-run.log`、`-fixed.log`、`-direct.log`。最终版本没有修改系统代理、数据库或 Redis 服务配置。

确认本簇构建已结束后，`CARGO_TARGET_DIR=/tmp/openrust-r2d-target cargo clean --dry-run` 列出 33,772 个任务专用产物、约 17.0 GiB；随后对同一 target 执行 `cargo clean` 完成清理。本仓共享 `target/` 和其他会话的工作目录未清理，验收日志保留于 `/tmp`。

## R2-M 模块清单、资源与统一迁移验收证据

本簇在 `main` 的唯一 Container/Provider 启动路径接入 `ModuleManifest`：编译描述与运行激活分开，标准应用及生成应用从运行根读取 `modules_statuses.json`；缺键/缺文件禁用、严格 JSON `true` 才启用。模块按 priority 稳定排序，实际 Provider boot 时点逐文件 overlay；示例业务已迁 `modules/example/`，仍不把 R13 管理写命令或 R1-E CRUD 计为本簇完成。核心内嵌测试覆盖 B002–B004、priority/依赖、重复来源、config 顺序/数组与版本、路径和 symlink 边界、启动失败回收；示例/HTTP 测试验证禁用业务后框架健康路由仍可用。

迁移执行器统一按 migration basename 排序、对所选物理库使用单一 `openrust_migrations(version,batch)` 历史；默认 up 同批、`UpStep` 逐条新批、down 全局最新批或精确全局步数，禁用模块的历史不会使较旧启用模块被错回滚。旧非空 `seaql_migrations` 明确拒绝，不猜 batch、不重跑现有业务表。PostgreSQL 单条 DDL 与历史默认事务协同；MySQL DDL 可隐式提交，失败/中断可能留下未记历史的部分 schema，必须检查后修复或使迁移幂等，不能宣称 exactly-once。旧两表 journal 方案曾在独立库正常通过，但写入崩溃窗口会永久不一致，已弃用，不作最终验收。独立锁池最多额外一连接；主库与锁连接用实际数据库/服务器身份比对，同 backend 不同物理库拒绝，获取总截止时间覆盖建连/身份查询/锁探针；存活 Tokio runtime 中取消会有界关闭独立会话。

真实 Docker PostgreSQL 18、MySQL 8.4 各以 `OPENRUST_TEST_DATABASE_URL` 仅传测试子进程，隔离数据库运行：`cargo test --lib --features db-<backend> db::migration::tests` 各 10/10；`cargo test --test db_migration --features db-<backend>` 各 5/5；`cargo test --bin server --features db-<backend> module_migration_runs_with_framework_and_disable_keeps_existing_table` 各 1/1，均 0 ignored。日志 `/tmp/openrust-r2m-after-lock-{postgres,mysql}-{lib,integration}.log`、`/tmp/openrust-r2m-example-migration-{postgres,mysql}.log`。错库锁 URL 各库 1/1，日志 `/tmp/openrust-r2m-lock-{postgres,mysql}-0.log`；MySQL 真实 `MigrationLock::acquire` 在 GET_LOCK 成功、最终 guard 构造前取消，另连接可重新取得锁 1/1；主库唯一连接占用时身份查询等待按 500ms 总截止时间失败，释放后可重试 1/1，日志 `/tmp/openrust-r2m-lock-mysql-{cancelled_acquire_closes_mysql_session_before_guard_exists,identity_query_wait_obeys_absolute_acquire_deadline}.log`。后两项不冒称进程强杀可执行 Rust Drop，也不覆盖任意代理网络黑洞的所有故障形态。

模块/任务与既有入口回归：两库各 `cargo test --features 'queue db-<backend>' --test queue --test schedule --offline --quiet` 分别 queue 3/3、schedule 7/7、0 ignored，日志 `/tmp/openrust-r2m-queue-schedule-{postgres,mysql}.log`；真实 Redis 8 `cargo test --features queue-redis --test queue_redis --offline --quiet` 3/3、0 ignored，日志 `/tmp/openrust-r2m-redis-queue.log`。根无参数 `cargo run --offline --quiet` 从仓库运行，禁用测试客户端环境代理直连 `/health`、`/api`、`/api/example`、`/` 全部 HTTP 200，示例消息为 `Hello from modules/example`，SIGINT 后 exit 0，日志 `/tmp/openrust-r2m-root-default-run.log`。模块 proto `python3 modules/example/proto/generate.py --check` 返回“Protobuf 重生字节一致”；这不是生成应用的 Proto/gRPC 网络验收。

CLI 真实生成普通项目及独立 `--db postgres --auth`、`--db mysql --auth` 项目。普通项目离线 `cargo check` 和 `cargo test` 1/1；随后只在 `/tmp` 项目添加文档 orders 模块和状态启用，TestApp 1/1。已编译的普通应用从与源码不同的 `/tmp/openrust-r2m-deployed-runtime-final` 目录运行，真实 `/api/orders` 为 HTTP 200、`data.module=orders`、SIGINT exit 0，日志 `/tmp/openrust-r2m-generated-relocated-runtime.log`；另在临时测试中显式 `Configuration.modules_root` 指向该部署树，TestApp 1/1。PG/MySQL 生成认证项目各自离线 check exit 0、带真实对应数据库 URL 的 test 各 2/2、0 ignored（含原 TCP auth 与 fake Mailer TestApp），日志 `/tmp/openrust-r2m-generated-auth-{postgres,mysql}-{check,test}.log`；仅本仓 `/tmp` 生成物变更，不把脚手架中未运行的业务代码算验收。CLI 快照首轮因新增 `Configuration.modules_root` 与 serde 依赖失配，更新为真实模板快照后定向 1/1。

工作区门禁：`cargo fmt --all --check`，默认与 `--all-features` 的 `cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` 均通过。默认测试日志 `/tmp/openrust-r2m-default-workspace-test.log` 共 26 组 440 passed/0 failed/23 ignored；全特性 `/tmp/openrust-r2m-all-workspace-test.log` 共 43 组 748 passed/0 failed/33 ignored，组数不是测试总数。首次默认整套唯一失败是 `tests/server/server/aggregate.rs` 旧 fixture 手工只装应用 Providers、遗漏 `Application::modules`；改走生产 `ProviderBuilder::for_application` 后原断言定向 1/1，完整复跑通过。`cargo check --workspace --all-targets --no-default-features`、Rust 1.97.0 的全特性 workspace all-targets 与 openrust no-default check、`cargo deny check` 均 exit 0；deny 的 advisories/bans/licenses/sources 通过，既有 duplicate crate 警告保留。各日志 `/tmp/openrust-r2m-{default,all}-clippy.log`、`-doc.log`、`-no-default-check.log`、`-msrv-{all,no-default}-check.log`、`-deny.log`。无数据库 URL 的旧集成用例及 doctest ignored 项不计通过；本段列出的隔离数据库入口均实传 URL 并为 0 ignored。

最终源码 Rustdoc 微调后重新运行 `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features --offline` exit 0，日志 `/tmp/openrust-r2m-final-doc.log`；`cargo fmt --all --check` 与 `git diff --check` 再次 exit 0。23 个本簇变更 Markdown 的普通相对链接目标检查为 0 缺失，未把锚点逐项校验算入。确认无 Cargo/rustc/rustdoc/服务进程后，仅对本簇独占 `/tmp/openrust-r2m-target` 运行 `cargo clean --dry-run`，报告 34329 文件/19.6 GiB；实际 `cargo clean` 回收同量，主卷可用由约 99 GiB 回升至约 118 GiB，仓库共享 `target/` 约 2.4 GiB 与数据库/Redis 服务数据未清理。

## R1-E 业务示例定向验收证据（整簇进行中）

当前源码在唯一 `modules/example` 接入有所有者的 Article CRUD 及各公开能力。以下将定向结果与最终 workspace/生成项目/consumer 门禁分项记录；定向通过本身不替代完整门禁。PostgreSQL 18、MySQL 8.4 测试通过各自环境 URL 连接隔离库；标明两库的测试没有把缺环境早退计为通过。真实 TCP、oneshot、同进程双 App 与外部服务分别按测试实际范围记录。

| 能力与实际路径 | PostgreSQL / MySQL 或其它结果 | 日志与边界 |
| --- | --- | --- |
| Article 业务 HTTP CRUD、owner 授权、验证、分页和事务 | 各 9/9、0 ignored | `/tmp/openrust-r1e-article-{postgres,mysql}.log`；使用真实持久化实体，不称完整 Eloquent。 |
| 双租户成员解析、主库查询、缓存、本地存储及持久队列；同逻辑 key 的真实 MinIO/Redis 租户分区及附件清理快照 | 两库各 3/3；MinIO/Redis 各 1/1，均 0 ignored | `/tmp/openrust-r1e-tenant-{pg-second,mysql-first}.log`、`/tmp/openrust-r1e-tenant-s3-{pg-second,mysql-first}.log`；两个独立 App 在同一进程，撤权仅约束尚未进入的 IO，不承诺在途外部写立即取消。 |
| S3 私有暂存、完整校验后 copy 发布、旧对象保护/取消及跨 App 真 TCP；版本化 bucket | 两库各 4/4、0 ignored；版本化 1/1 | `/tmp/openrust-r1e-s3-{pg,mysql}-staged-uuid.log`、`/tmp/openrust-r1e-s3-versioned-after-staging.log`；首轮发现待完成流的最终 key 提前可见，修为暂存发布后复跑。copy 开始后响应可能不确定，强杀/远端 hang 不保证同步清理。 |
| 业务派生缓存 named Redis、tag/SWR 与队列失效 | 两库各 6/6、0 ignored | `/tmp/openrust-r1e-cache-swr-business-{pg,mysql}-final.log`；使用已配置 default store，不静默回退本地 memory；普通 named 缓存仍是有界 TTL 的 eventual 展示值。 |
| 显式 seed、真实数据库/Redis worker 失败退避与重试、生产 Scheduler heartbeat slot→持久 job→worker scope 关闭 | seed 各 3/3，queue 各 2/2，scheduler 各 1/1，均 0 ignored | `/tmp/openrust-r1e-seed-{pg-final,mysql-third}.log`、`/tmp/openrust-r1e-queue-retry-{pg,mysql}-final.log`、`/tmp/openrust-r1e-scheduler-{pg,mysql}-first.log`；Scheduler 证据为同进程双 App 与真实数据库时钟，不冒充多进程集群。 |
| 上游 HTTP 超时/取消、视图发布副本优先与回退、i18n 复数/长日期、gRPC TLS 双端口组合 | 上游两库各 2/2；view 2/2；i18n 2/2；gRPC TLS 2/2，均 0 ignored | `/tmp/openrust-r1e-upstream-{pg,mysql}-first.log`、`/tmp/openrust-r1e-view-first.log`、`/tmp/openrust-r1e-localization-first.log`、`/tmp/openrust-r1e-grpc-tls-marker-final.log`；上游是真 TCP、RP 取消为 oneshot；gRPC 为同 App 两监听端口，不称同端口。 |
| OIDC MFA、RP logout、关联 Session/refresh、真实 Keycloak 正向签名 backchannel/重放及第二客户端真实签名 wrong audience | 两库各 5/5、0 ignored | `/tmp/openrust-r1e-oidc-lifecycle-final-{pg,mysql}-second.log`；MySQL 首轮 4/5 假设首个并发 sid 通知必撤销目标，改以 `revoked=1` 且目标 Session 已拒绝选择关联通知后双库复跑。旧 access JWT 仍遵循自身到期。 |
| 浏览器原生 Session WebSocket 与独立真实 TCP 协议状态 | Chromium 153 + PostgreSQL/MySQL 各 1/1、0 ignored | `/tmp/openrust-r1e-native-browser-{pg,mysql}-final-first.log`；浏览器只证明原生 Cookie/Origin/拒绝事件及到期关闭，精确 401/403/404/503 来自另行运行的 TCP 协议测试，未把 CDP 状态或超时当浏览器拒绝。 |
| 真实 OTel Collector OTLP trace 与 Prometheus scrape；不可达出口 | PostgreSQL 18、MySQL 8.4 交付与不可达均各 1/1，0 ignored | MySQL 日志 `/tmp/openrust-r1e-collector-{delivery,unavailable}-mysql-final.log`；PG 隔离实例日志 `/tmp/openrust-r1e-collector-pg-corrected-{delivery,unavailable}.log`，sink `/tmp/openrust-r1e-collector/evidence-pg-isolated.log`（本轮结束 1,384,574 bytes）。PG 实际使用隔离 Collector `otel/opentelemetry-collector:0.120.0`、独立 18773 OTLP 端口、fixture service/run_id 与受限 sink；用例以请求前字节 offset 只接受本次新输出，测试原样执行全文件 16 MiB 上限、私密 sentinel 扫描、同资源 span/metric、HTTP 提交、shutdown flush 与不可达 warning 断言。两种数据库均真实 sink 验证 trace/Prometheus scrape 与不可达路径；不声称入站 W3C parent 已连 OTel，也不把浏览器/进程内证据外推成集群。旧共享 PG Collector 和超出 16 MiB 的原 sink 未改；此前 PG 复验因 runner 指向旧路径、随后 sink 超限而在 HTTP 前失败，采用独立 sink 后两用例通过，不归因生产 OTel。 |

Browser、Keycloak 与 Collector 均是本任务隔离 fixture。Keycloak 的两个临时 client backchannel 属性已与预备快照全值核对恢复，task-only 管理触发监听已关闭，备份已删；未改共享 PostgreSQL/MySQL/Redis 服务。聚焦测试后确认无活跃本任务 Cargo，再删除仅本簇 `/tmp/openrust-r1e-target` 可再生产物约 18 GiB，主卷可用由 99 GiB 回升 117 GiB；原始日志仍保留。R1-E 最终门禁与提交已由 `bc3fa01` 收口。

当前源码在 WebSocket Close 握手和 Redis 测试命名空间修正后，默认 workspace tests 26 组共 454 passed/0 failed/23 ignored（`/tmp/openrust-r1e-gate-test-default-after-ws.log`）；全特性 43 组共 977 passed/0 failed/84 ignored（`/tmp/openrust-r1e-gate-test-all-final-after-ws-cache.log`）。`cargo fmt --all --check`、`git diff --check`、默认/全特性 Clippy 与 warnings-denied Rustdoc、`cargo deny check`、no-default workspace all-target、Rust 1.97 全特性及 no-default 检查均 exit 0；对应日志前缀 `/tmp/openrust-r1e-gate-{clippy,doc,deny,check,msrv}`，具体文件名保留在任务日志。忽略项未算真实服务通过。WebSocket 修改后真双进程 Redis consumer 收到严格 Close 1/1，模块 JWT 与 Session 真实 TCP WebSocket 在 PostgreSQL/MySQL 各 3/3，stalled-send 单测 1/1；旧 Redis 用例由 PID 命名空间造成跨进程残留，改为每次随机 128 位后单项连续两次 1/1，再运行上述全组通过。独立 consumer 的 test-app base/session、gRPC、cache、communication、data-exchange、SQLite、query authorization、queue、realtime 均已按各自公开入口执行，其中 session consumer 只证明公开签名可调用。确认无活跃 Cargo/rustc 后，专属 `/tmp/openrust-r1e-gates-target` 经 dry-run 报 33,153 文件/20.1 GiB，实际 `cargo clean` 回收同量；共享 target、服务数据和日志未动。

修复前生成器的旧产物 `/tmp/openrust-r1e-generated.QL2ZdI`：basic `cargo check` 与真实 TestApp test 1/1；PostgreSQL/MySQL `--auth` 与 `--auth-mfa --encryption` 四项目各自真实隔离库测试 2/2、0 ignored（`/tmp/openrust-r1e-generated-{pg-auth,mysql-auth,pg-mfa,mysql-mfa}-test.log`），但 MFA 项目的现有 2 项只覆盖基础认证，尚不证明 MFA 启用/挑战。`--grpc` 项目编译和 `cargo test` exit 0，测试 0 项，不冒充生成应用真实 TLS。当时生成源码的 `cargo fmt --all --check` 失败；这些旧产物没有计入下方修复后验收。

修复后 CLI 重新生成在 `/tmp/openrust-r1e-generated-complete.WA4aqn/`：`basic-final` 的 `cargo fmt --all --check`、`cargo check --all-targets`、`cargo test --workspace` 分别 exit 0，TestApp 1/1；`pg-auth-final`、`mysql-auth-final`、`pg-mfa-final`、`mysql-mfa-final` 的相同格式、check 与真实隔离数据库测试均通过，每库每项目 auth HTTP 2/2、0 ignored。MFA 测试现实际走启用→登录 challenge 且不预发凭据→恢复码一次性、轮换与撤销；它嵌入现有 HTTP 测试，不把 2/2 误称新增测试数。日志 `/tmp/openrust-r1e-generated-{pg-auth,mysql-auth,pg-mfa,mysql-mfa}-final-{check,test}.log`；普通应用的 CLI 输出及测试结果保留在本轮工具记录。

生成器 `--grpc` 现把 typed Echo 服务与 Provider 和 `HttpGrpcServiceProvider` 装入应用，而非只生成 proto：全新 `grpc-final` 从生成二进制运行 `serve all`，真 HTTP、TLS typed Echo、错误 CA/hostname 拒绝、SIGTERM 双监听收尾 1/1。`pg-auth-grpc-complete`、`mysql-auth-grpc-complete` 各自全新生成的 auth HTTP 2/2 与真实二进制 TLS gRPC 1/1；`pg-mfa-grpc-final`、`mysql-mfa-grpc-final` 的 `cargo check --all-targets` 亦分别 exit 0（`/tmp/openrust-r1e-generated-{pg-mfa-grpc-final,mysql-mfa-grpc-final}-check.log`）；它们除 TLS gRPC 1/1 外还各有真实认证/MFA HTTP 2/2，均 0 ignored，十个最终生成变体 `cargo fmt --all --check` 均 exit 0。组合测试使用显式 `OPENRUST_TEST_DATABASE_URL` 创建隔离库、临时证书与测试配置，缺 DB 环境时测试会早退，不能把无环境运行计通过。证据分别为 `/tmp/openrust-r1e-generated-pg-auth-grpc-complete-{check,test}.log`、`/tmp/openrust-r1e-generated-mysql-auth-grpc-complete-{check,test}.log` 与 `/tmp/openrust-r1e-generated-{pg-mfa-grpc-final,mysql-mfa-grpc-final}.log`，全新纯 gRPC 项目测试日志 `/tmp/openrust-r1e-generated-grpc-only-final-test.log`。旧无测试 `--grpc` 产物及临时手修诊断项目不计入以上结果；生成应用同一端口 HTTP/2 与多进程集群不在此证据范围。

CLI/模板最终改动后再次运行完整仓库门禁：`cargo fmt --all --check`、`git diff --check`、默认及全特性 `cargo clippy --workspace --all-targets -- -D warnings`、warnings-denied Rustdoc、`cargo deny check`、no-default workspace all-target、Rust 1.97 全特性及 no-default 检查均 exit 0。默认 workspace tests 26 组 454 passed/0 failed/23 ignored（`/tmp/openrust-r1e-gate-cli-final-test-default.log`）；全特性 43 组 977 passed/0 failed/84 ignored（`/tmp/openrust-r1e-gate-cli-final-test-all.log`）。文档、no-default、deny 与 MSRV 的新日志为 `/tmp/openrust-r1e-gate-cli-final-{doc-default,doc-all,check-no-default,deny,msrv-all,msrv-no-default}.log`；Clippy 最终结果来自本轮集成会话原始输出，CLI单项20/20（`/tmp/openrust-r1e-gate-cli-final-unit.log`）。忽略项没有计入真实服务验收；R1-E 仍待独立审查、S3 取消/删除竞争验收与限定提交，整簇未关闭。

PostgreSQL Collector 旧 runner 三次因活跃 `evidence-pg-final.log` 超过完整文件 16 MiB 上限，在请求前被严格 `evidence()` 拒绝；该结果不代表业务投递失败。原共享 Collector 和旧 sink 保持不动，改用任务专属 Collector、独立空白 sink 重验。第一次 follow-up fixture 的 scrape `job_name` 未与测试 run ID 对齐，trace 到达但 metric resource 归属不匹配；修正为一致后，`/tmp/openrust-r1e-collector-pg-followup2-delivery.log` 与 `...-unavailable.log` 各 1/1、0 ignored，分别报告业务提交、trace、Prometheus scrape、shutdown flush 成功，以及不可达 warning 而不声称交付。sink `/tmp/openrust-r1e-collector/evidence-pg-followup2.log` 小于 16 MiB，包含匹配的 resource trace/metric 且无私密 sentinel。仅清理本轮创建的两个 Collector 容器，保留日志/配置；共享 Collector、PG/MySQL、原 sink 未改。


## R1-E 审查修复与追加验收（2026-10-29）

附件上传将数据库事务和文章行锁移入后台写入任务；15 秒 HTTP 等待超时或请求 future 取消时，任务继续等待存储调用收敛，文章删除必须等锁释放后执行对象清理。PostgreSQL 18 `modules::example::attachment_flows::cancelled_after_publication_cannot_orphan_object_or_delete_new_upload` 1/1，日志 `/tmp/openrust-r1e-attachment-cancel-pg-review.log`；MySQL 8.4 同项 1/1，日志 `/tmp/openrust-r1e-attachment-cancel-mysql-review.log`。两项使用真实隔离数据库和本地存储；强杀进程/远端操作结果不确定仍不宣称跨系统原子。

真实 PostgreSQL + MinIO 新增 `s3_cancelled_waiter_keeps_delete_blocked_until_publish_finishes` 首轮测试失败后，按 §5 暂停并只读核对 fixture。runner 与测试代码变量名一致；endpoint 为 loopback `18762`，bucket `openrust-r1e-attachments`，region `us-east-1`，凭据仅从权限 0600 的本机 `minio.env` 读取且未输出。用正确参数重跑一次后 1/1 通过，日志 `/tmp/openrust-r1e-s3-cancel-delete-postgres-retry.log`。该用例证明真实 S3 发布完成、数据库事务尚未收束时删除仍等待行锁；不覆盖 copy 在途或远端迟到完成，不能称作该故障的恢复证据。

OIDC 确认页面现要求 HTTP 成功、envelope `success=true` 且 `data.status=bound` 才显示绑定成功；malformed/error JSON fail closed。页面合同单测 `binding_confirmation_requires_http_and_business_success` 1/1；新增默认 `AlwaysOk` 下真实 Keycloak/双库 TCP 负例覆盖错误密码和已消费 state，当时本机测试变量未配置；后续通过现存 loopback Keycloak fixture 注入环境并在双库实测，结果见本节最新记录。

按开发者确认方案重新运行 gRPC TLS 双监听测试 `real_tls_and_http_share_module_binding_and_one_supervised_shutdown` 1/1，日志为本轮工具输出；证书错误分类单测 `certificate_classifier_rejects_other_failures` 1/1，确保只接受 UnknownIssuer/错误 hostname 标识并拒绝网络故障负例。浏览器跨用户拒绝采用 Chromium 原生拒绝事件 + 独立真实 TCP 精确状态测试分别证明，PostgreSQL/MySQL 原生浏览器测试已有日志 `/tmp/openrust-r1e-native-browser-{pg,mysql}-final-first.log`；Collector PostgreSQL delivery/unavailable 已用独立 sink 复验 1/1+1/1，日志 `/tmp/openrust-r1e-collector-pg-isolated-{delivery,unavailable}.log`。


### 2026-10-29 附件租户与资源生命周期复验

附件 worker 使用应用 `ResourceRegistrar` 跟踪，派发时捕获 `TenantContext` 并在操作中恢复；请求取消且请求体尚未读完时，中断 body 读取。操作上限为 45 秒，应用 shutdown 仍受自身 deadline 限制。PostgreSQL 18 与 MySQL 8.4 的 `attachment_flows::` 全量回归各 15 passed、0 failed、0 ignored，日志 `/tmp/openrust-r1e-attachment-managed-{postgres,mysql}-deadline-final.log`。新增 `upload_operation_deadline_releases_article_lock` 将测试期限设为 50 ms，在首个 body chunk 已读后触发稳定 `example_attachment_upload_deadline`，随后真实 DELETE 成功；各数据库定向用例也单独 1/1，日志 `/tmp/openrust-r1e-attachment-managed-deadline-{postgres,mysql}-final.log`。此前独立复审 blocker 已消除；没有测试 App shutdown deadline 超时，也没有真实暂停 S3 copy 后模拟超时/迟到完成，后两者不计通过。

OIDC 控制器使用 `openrust::http::Json`，其 IntoResponse 确认将成功载荷包装为 `ApiResponse::success`，因此确认页要求的 envelope 与后端类型一致。新增 envelope 单测及 AlwaysOk 下真实 IdP 成功闭环断言；当时 IdP 变量缺失，后续通过 loopback Keycloak fixture 双库实际执行，结果见本节最新记录。独立复审撤回先前将其误判为原始 Axum Json 的 blocker。

Collector PostgreSQL 日志路径已校正到 `evidence-pg-final.log` 并加活跃增长检查；该共享 sink 超过 16 MiB 时未放宽完整文件上限，而是使用匹配 run ID 的任务专属 Collector/sink。最终 delivery/unavailable 各 1/1 通过，路径、文件大小、私密 sentinel 与 trace/metric 均有检查，详见上方记录。

R1-E 当前工作区整组门禁复验：`cargo fmt --all --check`、`git diff --check`、默认与全特性 `cargo clippy --workspace --all-targets --all-features -- -D warnings`、默认 `CARGO_INCREMENTAL=0 cargo test --workspace --quiet`、串行全特性 `CARGO_INCREMENTAL=0 RUST_TEST_THREADS=1 cargo test --workspace --all-features --quiet`、warnings-denied 默认/全特性 workspace Rustdoc、`cargo deny check` 与 no-default-features workspace all-target check 均退出 0。默认测试 26 组 454 passed/23 ignored；全特性测试 43 组 990 passed/90 ignored；新增期限回归在 PostgreSQL/MySQL 真实库各通过，MinIO 发布后删除竞争用例及 Collector PG delivery/unavailable 也有独立真实服务证据。deny 的 advisories/bans/licenses/sources 通过，保留既有重复版本与未匹配 MPL allowance 警告。Ignored 项不作真实服务验收；S3 copy 在途请求结果不确定后的远端迟到成功仍属未保证边界。后续已用本机 loopback Keycloak fixture 在 PostgreSQL 18/MySQL 8.4 各执行 `always_ok_http_mode_keeps_oidc_business_failure_in_error_envelope` 1/1，覆盖错误密码、已消费 state 与成功夹证；Collector 活跃日志修正后，真实 PostgreSQL delivery/unavailable 各 1/1。最终门禁后通过 `cargo clean --dry-run` 核对并清理仓库 `target/` 15.3 GiB、任务专属 `/tmp/openrust-r1e-target` 5.7 GiB 与 `/tmp/openrust-r1e-gates-target` 2.9 GiB；三处均归零，磁盘可用空间从 97 GiB 增至 110 GiB。该簇已由 `bc3fa01` 提交；结果不确定后的远端迟到成功仍是已记录的运行边界。


OIDC 确认页增加真实 Chromium JavaScript 回归：`cargo test --bin server --features 'auth-oidc db-postgres storage-s3' browser_confirmation_reports_success_and_failures_from_actual_page -- --ignored --nocapture` 1/1。该用例通过 Python Playwright 启动本机 Chromium，直接装载生产 `confirmation_page` HTML，并用仅绑定 loopback 的 mock 确认 API 检查成功 envelope、HTTP 200 业务失败、HTTP 500 和非法 JSON 的页面提示；未访问 Keycloak，不能替代真实 OIDC/双库验收。


当前源码（含 EOF/deadline CAS 修复）门禁：`cargo fmt --all --check`、`git diff --check`、默认及 all-features Clippy、default workspace tests、串行 all-features workspace tests、warnings-denied default/all-features Rustdoc、`cargo deny check`、no-default all-target check 均通过。deny 的 advisories/bans/licenses/sources 通过，保留已有重复 base64/winnow 与未匹配 MPL allowance 警告。全特性测试逐组输出均 0 failed；Ignored 只列为忽略，不作真实服务证据。

EOF/deadline 并发仲裁经独立只读复审确认 blocker 已消除：共享 AtomicU8 CAS 竞争 pending→EOF 与 pending→deadline；deadline 获胜后迟到 EOF 返回错误，EOF 获胜后上传继续等待并持有文章行锁。新增单测 `completed_body_wins_when_eof_and_deadline_are_both_ready`、`closed_eof_signal_keeps_deadline_active_for_failed_body_cleanup`、`deadline_winner_prevents_a_late_normal_eof_from_starting_publish` 3/3。最终版本 PG18/MySQL8.4 `modules::example::attachment_flows::` 各 15/15，真实 MinIO CopyObject 成功响应暂缓的 S3 删除竞态各 1/1；日志分别位于 `/tmp/openrust-r1e-attachment-flows-atomic-{pg,mysql}.log` 与 `/tmp/openrust-r1e-s3-inflight-atomic-{pg,mysql}.log`。

Collector PostgreSQL path 已明确读 `evidence-pg-final.log` 且检查 sink mtime/size 活跃增长；该共享日志达 36 MiB，未放宽测试 16 MiB 完整文件限制。使用单独 Collector 与 `/tmp/openrust-r1e-collector/evidence-pg-isolated.log` 复验 delivery/unavailable 各 1/1，日志 `/tmp/openrust-r1e-collector-pg-corrected-{delivery,unavailable}.log`；匹配 trace/metric、业务提交/不可达 warning 和无私密 sentinel 均由用例断言，共享 Collector 未改。

独立复审核实 EOF/deadline blocker 与 OIDC confirmation success 判定已修复。真实 Keycloak 双库确认用例已完成；`storage_publication_uncertain` 后远端迟到成功、App shutdown deadline 和进程强杀仍不保证跨系统原子性或无孤儿。R1-E 已按适用验收提交；这些情况保留为未保证的分布式存储边界。

## R3-A Laravel 语义 HTTP 状态默认与统一 JSON 响应

R3-A 按 历史决策 076 将 `HttpStatusMode::Semantic` 设为框架、配置、无请求上下文错误 fallback 与生成应用默认；`always_ok` 保留显式覆盖。默认 HTTP 行为测试覆盖创建 201、404、405、415、422、429、500，显式覆盖覆盖成功、异常、fallback、提取错误和 timeout 的 HTTP 200。另测试故障 I18n Provider 的早退分支：`semantic` 返回 500、`always_ok` 返回 200，响应头和正文 request ID 均与输入 ID 一致。metrics 未启用路由现按语义返回 404；原先把 200 当默认值的断言已改正。历史决策 008 已明确旧 always_ok 默认是历史决策，索引登记 历史决策 076。

源码门禁：`cargo fmt --all --check`、`git diff --check`、默认与 all-features workspace all-target Clippy（warnings denied）、默认 workspace tests、串行 all-features workspace tests、默认与 all-features workspace Rustdoc（`RUSTDOCFLAGS='-D warnings'`）、workspace no-default all-target check、`cargo deny check`、Rust 1.97 all-features workspace check 和 Rust 1.97 `openrust --no-default-features` 均通过。默认 workspace 测试中核心库 298/298；all-features 串行 workspace 所有组汇总 1,011 passed/0 failed/93 ignored。两次默认并行度 all-features 全量运行分别在视图失败场景和模块禁用 TestApp 场景报告了 `internal_error`；两个用例各自单独重跑通过，完整 `RUST_TEST_THREADS=1` all-features workspace 复跑通过。未把 ignored 用例计作通过；该并行运行异常保留为证据，后续若复现再定位。

生成项目：`/tmp/openrust-r3a-generated-20261029/{plain,postgres,mysql}`。普通、PostgreSQL 认证、MySQL 认证项目均通过格式和 workspace all-target offline check；普通生成项目的测试通过，PG18/MySQL8.4 生成认证应用真实 HTTP 测试各 2/2（包含默认语义状态）。同批较早验证已实际启动仓库根无参数 `cargo run`：TCP `/api/r3a-missing-route` 返回 HTTP 404 `not_found` envelope，`/health` 返回 200，SIGINT 后 exit 0。生成构建使用独立 `/tmp/openrust-r3a-generated-target`，不替代根真实服务测试。

独立只读复审发现并促使修复 I18n 早退响应上下文缺口，复审后未发现 blocker/should-fix；其文档 nit 已同步处理。PostgreSQL/MySQL 响应数据库状态的业务路径未改变，既有真实 DB 验收不被重标为本簇新证据。 两次全量验证结束且确认无本任务 Cargo/rustc/rustdoc/server 后，`cargo clean --dry-run` 显示根 target 32646 文件/19.8 GiB，独立生成 target 9245 文件/4.1 GiB；分别清理后两处均归零，磁盘可用空间由 99 GiB 增至 121 GiB。

## R3-C Laravel 命名路由与具名 URL 生成

`Routes::name` 将名称绑定到紧邻注册的路由；嵌套 `RouteGroup::name_prefix` 按 Laravel 行为原样拼接，测试覆盖两层 name/path prefix 以及组调用结束后在外层 `.name(...)`。`RouteUrlGenerator` 以 `app.url` 生成绝对链接，不受请求 `Host` 影响；path 参数按单段编码，剩余项按 BTreeMap 顺序编码为 query。未知名称、缺失参数等生成错误采用 HTTP 500。重复名称、通配符和空路径段（含嵌套组的 `/files//`、`//`）在 router build 时返回错误；末尾 `/` 被保留，生成 URL 后实际请求目标 route 得到 200。

定向 `cargo test --test http_router -- --test-threads=1`：最终 59 passed、0 failed、0 ignored。默认和串行全特性 `CARGO_INCREMENTAL=0 cargo test --workspace --quiet` / `CARGO_INCREMENTAL=0 RUST_TEST_THREADS=1 cargo test --workspace --all-features --quiet` 均 exit 0；各测试组无失败，显式 ignored 仍按 ignored 处理。默认与 all-features workspace Clippy（all targets、warnings denied）、Rustdoc（warnings denied）、no-default-features all-target check、Rust 1.97 all-features workspace check 与 `cargo deny check` 均通过。deny 保留既有 duplicate base64/winnow 与未匹配 MPL allowance 警告；advisories/bans/licenses/sources 均通过。

生成项目 `/tmp/openrust-r3c-generated` 经 `cargo check --tests`，并执行生成 TestApp 的 `generated_application_serves_api_and_health` 1/1；在生成应用的 handler 中调用 `RouteUrlGenerator`，返回 `http://localhost:3000/api`。该项目依赖当前本地源码路径，生成目标与 CLI 工作目录均在 `/tmp`；不构成发布版本兼容证据。PostgreSQL/MySQL 不属于纯路由 URL 行为，本簇不重复数据库验收。

gpt-6.1-terra 独立只读复审最终未发现 blocker/should-fix；审查范围包括组外嵌套命名、原样 name prefix、尾斜杠、通配/空段拒绝和 HTTP 500 错误语义。R3-C 由 `801ca4e` 限定本地提交于 2026-10-29。

## R3-D Laravel middleware alias 与 group

`BootContext::middleware_alias` 将 Tower Layer 登记为应用级名称；`middleware_group` 可按声明顺序组合 alias/其他 group。`RouteGroup` 与 `Routes` 按名称应用；路由级调用只包裹调用时已登记的路由，之后添加的路由不继承。路由组只覆盖组内路由。没有应用 middleware registry 的独立 `Routes` 无法解析命名引用，调用会保留稳定错误，`Routes::into_router()` 返回 `Result` 并检查该错误；嵌套 group/merge 同样传播错误，不会静默生成无保护路由。

行为回归：`tests/http/router.rs` 定向 `named_middleware_aliases_and_nested_groups_keep_declaration_order` 与 `named_middleware_without_registry_fails_closed_on_all_route_paths` 均通过；分别覆盖声明顺序、嵌套 group、Route-level alias 前后作用范围、无 registry 的直用/嵌套/组级引用，以及 fail-closed。既有重复、无效、未知和循环定义测试通过。独立 gpt-6.1-terra 只读复审先发现 pending middleware 扩大/丢失和 `into_router` 静默绕过；修复后复审通过，无 blocker/should-fix。

源代码整组门禁在最新修复后均 exit 0：`cargo fmt --all --check`；`CARGO_INCREMENTAL=0 cargo test --workspace --quiet`；`CARGO_INCREMENTAL=0 RUST_TEST_THREADS=1 cargo test --workspace --all-features --quiet`（核心 all-feature 490 passed、2 ignored，其他测试组均 0 failed；忽略项未计通过）；默认与 all-features workspace all-target Clippy（`-D warnings`）；默认与 all-features workspace Rustdoc；`cargo check --workspace --all-targets --no-default-features`；`cargo +1.97.0 check --workspace --all-targets --all-features`；`cargo deny check`。deny 的 advisories/bans/licenses/sources 均通过，保留仓库既有重复 base64/block-buffer/winnow 与未匹配 MPL allowance 警告。

CLI 生成应用 `/tmp/openrust-r3d-generated-20261029` 使用本地框架路径依赖并在 `modules/example` 模式下登记 alias/group；独立目标 `/tmp/openrust-r3d-generated-target` 执行 `cargo check --tests --offline` 与 TestApp 用例 `generated_application_serves_api_and_health` 1/1 通过。根应用以无参数 `cargo run --offline` 实际监听 `127.0.0.1:3000`：带 `--noproxy '*'` 的真实 HTTP `GET /api/example/middleware` 返回 200、`x-example-middleware: named-group` 和预期 JSON；`GET /api/example` 返回 200 且没有该响应头；SIGINT 后正常退出。该纯 HTTP 路由簇不更改数据库行为，双库验收不适用。

验证时 `cargo clean --dry-run` 显示根 `target/` 39,531 文件 / 19.2 GiB，独立生成目标 5,711 文件 / 2.1 GiB；所在磁盘可用 101 GiB（总容量 926 GiB）。确认无本簇 Cargo/rustc/rustdoc/server 进程后，根 target 回收 19.2 GiB，生成目标回收 2.1 GiB，CLI 临时生成应用源码也从 `/tmp` 移除；结束时根与独立生成 target 均为 0，磁盘可用 121 GiB。仓库内五个公开 consumer 的独立 target（db-custom-sqlite、query-authorization、data-exchange、realtime、queue-custom-driver）归属其他验收，未清理。真实 HTTP 响应文件保留在 `/tmp/openrust-r3d-live-*-final.*`。R3-D 由本地限定提交记录，其他 R3/模块能力仍待实施。

## R3-E Laravel fallback routes

行为回归：`CARGO_INCREMENTAL=0 cargo test -p openrust --test http_router fallback -- --nocapture` 通过 7/7，覆盖自定义/组级 fallback、middleware、已匹配路由优先、方法不匹配 405、GET/HEAD 静态优先、显式 fallback 下 `/api` 与 `/ws` 静态文件、POST 原 URI/query/header/body 保留、默认 `/api` JSON 404、`always_ok` 下仍执行 fallback、同 scope/嵌套组/merge 重复稳定报错。独立 gpt-6.1-terra 复审先发现三个 blocker（保留路径静态资源跳过、嵌套重复可能 panic、依赖渲染后状态误判静态命中），修复后复审未发现确定性 blocker；最终复审发现并已修正文档中动态 scope 错误优先级描述。Axum 在带参数前缀上 nest fallback 会发生 catch-all 路由冲突；当前在构建时以 `http_fallback_scope_unsupported` 拒绝参数化/通配符 fallback 前缀，文档公开说明。

本源码整组门禁实际 exit 0：`cargo fmt --all --check`、默认与全特性 `cargo clippy --workspace --all-targets [--all-features] -- -D warnings`、`CARGO_INCREMENTAL=0 cargo test --workspace --quiet`、`CARGO_INCREMENTAL=0 RUST_TEST_THREADS=1 cargo test --workspace --all-features --quiet`、warnings-denied 默认与全特性 `cargo doc --workspace --no-deps [--all-features]`、`cargo deny check`、`cargo check --workspace --all-targets --no-default-features`、`cargo +1.97.0 check --workspace --all-targets --all-features` 与 `cargo +1.97.0 check -p openrust --no-default-features`。全特性核心库 490 passed/2 ignored；其余用例组均无失败。ignored 用例不计服务验收。deny 的 advisories/bans/licenses/sources 通过，保留既有重复 base64/winnow 与未匹配 MPL allowance 警告。

生成项目 `/tmp/openrust-r3e-generated-final-20261029` 由 CLI 产出；使用独立 `/tmp/openrust-r3e-generated-target` 执行 `cargo test --manifest-path ... --offline`，生成 `TestApp` HTTP 用例 1/1 通过。根项目用无参数 `cargo run --offline` 实际监听 `127.0.0.1:3000`；Python `http.client` 直连 TCP 验证 `GET /pages/not-found` 404 且带 `x-example-middleware: named-group`，`GET /api/example/middleware` 200 且同 middleware header，`GET /api/not-found` 默认 JSON 404，`GET /index.html` 200 静态 HTML，`HEAD /index.html` 200 空 body；SIGINT 后进程 exit 0。一次默认 Python urllib 请求受本机代理环境返回 502，改为不读取代理变量的 `http.client` 原始 TCP 请求后成功；不将代理响应算作应用证据。本簇不改变数据行为，双库门禁不适用。

功能簇收尾检查无活跃 Cargo/rustc/rustdoc/server 进程；`cargo clean --dry-run` 确认根 `target/` 为 26,687 文件 / 12.1 GiB，实际 `cargo clean` 回收同量；任务专属生成 target 为 2,860 文件 / 1.5 GiB，单独清理回收同量。两个本簇临时生成源码目录已移除；磁盘可用从 107 GiB 回升至 120 GiB。未清理其他 consumer target 或共享服务数据。

## R3-F Laravel domain routing

`RouteGroup::domain`、`DomainParams<T>` 和具名域 URL 已接入公开 HTTP API、prelude 与唯一模块示例。候选分派只识别内部 miss，保留原请求 body/metadata，业务 404/405 不会重复执行；方法候选全部 miss 后合并 `Allow`。声明顺序覆盖交错域模式、独立路由表 merge 与域组中合并普通路由；fallback 在同模式内按静态 scope 深度优先，不同重叠模式按首次 fallback 声明顺序。独立 gpt-6.1-terra 只读复审发现的顺序、单标签参数、forward authority、未命名路径参数冲突、嵌套 scope 与根 fallback 问题均已有回归，最终复核无 blocker/should-fix。最终定向 HTTP router 为 87/87（`/tmp/openrust-r3f-root-fallback-router.log`），包含真实 TCP 域与普通路由分派。

当前已完成的最终门禁：`cargo fmt --all --check`、默认/全特性 workspace all-target Clippy（`-D warnings`）、`cargo test --workspace` 与 `RUST_TEST_THREADS=1 cargo test --workspace --all-features`；日志分别位于 `/tmp/openrust-r3f-final-{fmt,clippy-default,clippy-all,test-default,test-all}.log`。默认 26 组 502 passed/28 ignored，全特性 43 组 1052 passed/98 ignored，均无失败；ignored 不作为服务验收。初轮 workspace 暴露 bootstrap、模块与 server aggregate 的正常 oneshot/TestApp 请求未提供 Host，已在这些 fixture 显式加入 `Host: localhost`，保持框架缺失/畸形 authority 返回 400 的断言。

新增生产模块 TestApp 用例 `production_module_domain_route_binds_host_path_and_named_url` 另行 1/1 通过，日志 `/tmp/openrust-r3f-final-example-domain.log`；验证 `Acme.example.test` 捕获为 `acme`、路径百分号解码、URL 不采信 `X-Forwarded-Host`、配置端口保留及错误 Host 404。域参数仅路由输入，不作为已认证租户。数据库行为未改变，不新增无关双库验收。

最终源码（包括新增测试与文档）default/all-features Clippy 和 warnings-denied Rustdoc 均重新退出 0，日志 `/tmp/openrust-r3f-final-{clippy-default-last,clippy-all-last,doc-default-last,doc-all-last}.log`；文档中未标为代码的 `TestApp` 曾触发 Clippy，修正后复验通过。`cargo check --workspace --all-targets --no-default-features`、`cargo +1.97.0 check --workspace --all-features --all-targets` 通过，日志 `...-check-no-default.log` 与 `...-msrv-all.log`。本轮 `cargo deny check` 的 advisories/bans/licenses/sources 通过，保留既有重复依赖和未命中 MPL allowance 警告；该次检查后清单与锁文件未变。

CLI 生成应用 `/tmp/openrust-r3f-generated` 使用当前本地框架依赖；在生成的应用路由中登记 domain group 并用 `DomainParams<T>` 提取参数，生成 TestApp 同时验证 API/health/fallback/domain。`cargo fmt --all --check`、`cargo check --tests`、`cargo test` 均通过，HTTP 测试 1/1、0 ignored，日志 `/tmp/openrust-r3f-final-generated-{fmt,check,test}.log`。这是本地生成 consumer 行为证据，不宣称发布包兼容。

根宿主由根无参数 `cargo run --quiet` 使用任务专属 `/tmp` 配置和独立端口启动，直连 loopback HTTP 验证域接口 200、account=acme、id=42、具名 URL 域及端口正确、不匹配 Host `other.invalid` 为 404、`/health` 为 200；SIGTERM 后 exit 0。脚本 `/tmp/openrust-r3f-root-tcp.py`，结果 `/tmp/openrust-r3f-final-root-tcp.log`，运行日志 `...-root-tcp-attempt4.log`。前两次 readiness 检查误经本机代理；改为禁用代理的直接 HTTP 后进入业务断言。第三次脚本使用了实际匹配 `{account}.example.test` 的 `wrong.example.test`，200 正确，修正负例为不匹配域后通过；未改框架或放宽超时、状态断言。

收尾确认无 Cargo/rustc/rustdoc/server 进程；根和生成 target 均为非 symlink、Git 忽略且未跟踪的本簇产物，由 Cargo 自身构建锁协调 `cargo clean --dry-run` 后执行 `cargo clean`。根回收71,767文件/28.4GiB，生成目标回收8,469文件/2.8GiB（`/tmp/openrust-r3f-final-clean-{root,generated}.log`）；结束时仓库target总量0，磁盘可用120GiB/926GiB。其他consumer、共享服务及守护任务改动未清理或夹带。

成功后的临时生成源码已核无其他WIP/worktree后移除；手工添加的consumer路由与测试逐字节保全到 `/tmp/openrust-r3f-generated-routes-web.txt`、`/tmp/openrust-r3f-generated-test-app.txt`，验收日志、根TCP脚本与运行日志目录继续保留。

## C04-A 请求检查与内容协商

RequestInfo 的 parts-only 快照、严格 authority 复用、文本头/bearer、固定 Laravel 13.39/Symfony 8 内容协商已接入公开 API/prelude、唯一模块和生成应用。定向 Request10/10、router87/87，模块 Request3/3及HTML转义1/1，均0 ignored。独立不同模型审查修复 UTF-8、flag参数规范值、q数值前缀、多后缀匹配和正负零排序；限定最终复审无 blocker/should-fix。完整 Request/Response、输入修改、Session/FormRequest 等仍留后继范围。

最终修后源码实际执行 `cargo fmt --all --check`、默认及全特性 `cargo clippy --workspace --all-targets [--all-features] -- -D warnings`、`cargo test --workspace [--all-features]`、warnings-denied `cargo doc --workspace --no-deps [--all-features]`、`cargo check --workspace --no-default-features --all-targets`、`cargo +1.97.0 check --workspace --all-features --all-targets` 和 `cargo deny check` 全部exit0。默认27组523 passed/29 ignored；全特性44组1082 passed/99 ignored，零失败，保持默认测试并发。99 ignored为64个既有外部服务/子进程用例和35个doctest，不计服务验收。deny四类检查通过，保留既有warning。日志 `/tmp/openrust-c04a-storage-fixed-{fmt-check,clippy-default,clippy-all,tests-default,tests-all,docs-default,docs-all,check-no-default,check-msrv,deny}.log`，阶段命令/退出/资源见 `...-gates.log`。

根无参数 `cargo run --quiet` 在任务专属临时配置及独立端口直连TCP，验证缺/空Accept、HTML/JSON、双向q、vendor JSON七种响应均200，不匹配text/plain为406，各分支核对Vary、Content-Type及正文；POST保留完整UTF-8正文、路径、方法与布尔is_json，health200，SIGTERM exit0。修后预构建和网络复验均exit0，脚本 `/tmp/openrust-c04a-root-tcp.py`、结果 `/tmp/openrust-c04a-root-tcp.log`、日志 `/tmp/openrust-c04a-storage-fixed-{server-build,root-tcp}.log`。隔离HTTP验收显式memory store不代表生产Redis回退。模块进程内测试另覆盖显式AlwaysOk与Semantic的200/406，不手改业务错误状态。

真实CLI创建 `/tmp/openrust-c04a-generated`，核验当前框架path依赖后追加RequestInfo公开consumer，保留原bootstrap/API/health/fallback。产物fmt、check --tests、cargo test全部通过，consumer2/2涵盖正文共存、合法UTF-8首头、非法文本错误、方法/路径/Host与协商。首轮日志 `/tmp/openrust-c04a-final-generated-{prepare,fmt,check,test}.log`；修后复用根target串行check/test亦通过，日志 `/tmp/openrust-c04a-storage-fixed-generated-{check,test}.log`。生成脚本 `/tmp/openrust-c04a-generated-prepare.py`；不宣称发布包兼容。

首轮示例测试fixture误以为helper(false)选择AlwaysOk，已局部显式配置，保持框架Semantic默认。修复前全特性三轮在不同fixture出现internal_error；第三轮安全诊断确认build阶段IO NotFound/errno2，并非HTTP500响应。失败分别保全 `/tmp/openrust-c04a-final-tests-all-{first,second,third}-failure.log`；旧用例定向及aggregate可通过不能证明偶发故障已定位。用户按§5批准限定修复后，以取得DirEntry再删除run目录的可控回归证明原实现失败（`...-storage-red.log`）、修后通过（`...-storage-green.log`）。清理仅在条目NotFound且base当前可读时容错，base消失/非目录、权限和其他IO仍失败；补确定性base负例和build/drop交错压力回归。storage12/HTTP TCP1/Request10/aggregate117定向均通过，日志 `/tmp/openrust-c04a-final-storage-base-gates.log` 及分项。最终不同模型复审无剩余blocker/should-fix。根目录复核非原子，仍要求storage root系统用户独占写；不保证并发目录替换。三轮旧失败未捕获具体文件调用点，不声称每轮已逐一确认同一调用点。临时探针已移除，快照保全 `/tmp/openrust-c04a-registration-diagnostic-preserved.rs`。

最终测试后触资源软停（31.343GiB target、磁盘9.801%空闲），核实本簇独占、无占用、ignored/未跟踪/非symlink后仅回收根incremental，空闲恢复11.05%，续跑余下门禁无跳过。独立生成target先经Cargo锁协调回收2.1GiB，日志保留；功能簇末尾核无构建/server占用后由Cargo锁协调完整清理根target，dry-run和实际回收56,621文件/23.5GiB，日志 `/tmp/openrust-c04a-storage-fixed-clean{,-dry-run}.log`。根及独立生成target均不存在；五个原有consumer target共2.486656GiB不属于本簇，保留未动。磁盘最终119.475739GiB空闲（12.897449%），精确资源 `/tmp/openrust-c04a-storage-fixed-clean-resources.json`；生成源码、验收脚本、日志与诊断快照保留作为证据。

## `modules/example` 资源命名隔离与字段类型 CRUD

`modules/example/tests/example_record_flows.rs` 使用隔离数据库和生产 `TestApp`，覆盖 `/api/example/records` 登录保护、五种资源动作、分页、字段 round-trip 与 owner 隔离；请求检查确保示例认证、任务和视图不占用 `/api/auth`、`/api/me`、`/api/jobs/*`、`/pages/*`。PostgreSQL 18 与 MySQL 8.4 的定向真实集成均为 1/1，包含 string/text/integer/big_integer/boolean/UTC datetime/JSON/references、nullable 值及外键。`cargo fmt --all --check`、`git diff --check`、workspace 默认 Clippy、`cargo test --workspace` 和 `cargo doc --workspace --no-deps` 均通过；服务 URL 未写入日志。


## C04-B1 统一请求输入

状态：B1 实现与下述验收已有实际证据；最新源码的全部适用门禁已通过，最终资源收尾完成，限定本地提交见Git。不得据此宣称已完成全部 C04、Laravel Request 或严格 1:1。契约见 历史决策 079。

范围为唯一正文所有者 `Request` 的 query/json/form bags、顶层左并集、dot/数字/读取通配、存在性、筛选和修改，以及受管理 `Request`/`Json`/`Query` 的 trim/null 规范化和公开策略覆盖。原始正文不重编码；query 16KiB、原始非空 URL 字段对及容器 entry 各1024、子级最大64层、展开完整 dot key 累计20MiB，超限不截断并安全拒绝。typed JSON 保留数字词法，动态 Value 不保证128位整数精度。multipart 仅在显式选择 Request 时保存有界原文，不解析文本/文件 bag；B2 string/integer/date/enum 等便捷方法、Cookie/Session、Old Input/flash、FormRequest 与完整文件/响应语义仍保留未完成范围。

### 已取得的验收证据

| 验收范围 | 实际结果与证据 |
| --- | --- |
| 最终定向 | input19、request10、router87、config24、内部值树/form/词法19、唯一模块 production_input3，全部零 ignored；定向 Clippy 通过。日志 `/tmp/openrust-c04b-tests-final.log`、`/tmp/openrust-c04b-config-final.log`、`/tmp/openrust-c04b-unit-final.log`、`/tmp/openrust-c04b-example-final.log`、`/tmp/openrust-c04b-clippy-final.log`，命令/退出/资源记录 `/tmp/openrust-c04b-directed-gates.log`。不同模型限定复审无明确 blocker/should-fix。 |
| 最新 workspace | 迁移编号修正后默认570通过/29 ignored、全特性1136通过/99 ignored，均 exit0；fmt、默认及全特性 workspace/all-target Clippy `-D warnings` 通过，阶段源码哈希无变化。日志 `/tmp/openrust-c04b-final-migration-fixed-tests-default.log`、`/tmp/openrust-c04b-final-migration-fixed-tests-all.log`、`/tmp/openrust-c04b-final-migration-fixed-fmt-check.log`、`/tmp/openrust-c04b-final-migration-fixed-clippy-default.log`、`/tmp/openrust-c04b-final-migration-fixed-clippy-all.log`；驱动记录 `/tmp/openrust-c04b-final-migration-fixed-gates.log`。ignored 不充当真实服务证据。 |
| 根完整认证 target 双库 | PostgreSQL18/MySQL8.4 的 `example_auth_flows` 各47通过、零失败/ignored/filtered，具名认证用例逐名 ok、Cargo exit0；各阶段源码哈希无变化，owned anchor 删除。日志 `/tmp/openrust-c04b-final-auth-root-complete-migration-fixed.log`、`/tmp/openrust-c04b-auth-root-migration-fixed-driver.log`。 |
| 真实 CLI 认证生成应用双库 | 每库实际 new、fmt、check、完整 tests exit0，各2通过、零失败/ignored/filtered；`generated_auth_routes_run_over_http_with_isolated_database_and_fake_mail` 与 `generated_test_app_overrides_mailer_for_registration` 均执行，覆盖 password 重置字段、首尾空白密码登录与去空白版本拒绝。邮件为显式 fake mail，不宣称供应商发送。两端 owned anchor 删除，阶段源码哈希无变化。日志 `/tmp/openrust-c04b-final-auth-generated.log`、`/tmp/openrust-c04b-auth-generated-driver.log`。 |
| 根真实 TCP | 根默认无参数 `cargo run`，health200；JSON/query优先级、PHP form键、原文保留、typed JSON规范化、独立路由禁用策略通过；超过16KiB query实际返回413/`input_limits_exceeded`，进程优雅退出0。证据 `/tmp/openrust-c04b-root-tcp.log`、`/tmp/openrust-c04b-runtime-scoped.log`，记录 launch_mode=`root_cargo_run_default`。 |
| 普通生成 consumer | 实际 CLI 生成后 fmt、check及test通过，consumer2项通过/0 ignored；阶段源码哈希无变化。证据 `/tmp/openrust-c04b-runtime-consumer-fixed.log`。 |

较早源码快照曾完成 default/all Rustdoc `-D warnings`、no-default workspace/all-target check、Rust1.97.0 all-feature/all-target check 和 cargo deny，见 `/tmp/openrust-c04b-independent-gates.log` 及 `/tmp/openrust-c04b-final-docs-default.log`、`/tmp/openrust-c04b-final-docs-all.log`、`/tmp/openrust-c04b-final-check-no-default.log`、`/tmp/openrust-c04b-final-check-msrv.log`、`/tmp/openrust-c04b-final-deny.log`。这些不能替代最后源码的复验。最新五项组已全部退出0：default/all-features Rustdoc（`-D warnings`）、no-default workspace/all-target check、Rust1.97.0 all-features/all-target check及`cargo deny check`；各阶段完整源码哈希均无变化。驱动记录`/tmp/openrust-c04b-final-migration-fixed-remaining.log`，分项日志`/tmp/openrust-c04b-final-migration-fixed-{docs-default,docs-all,check-no-default,check-msrv,deny}.log`。结束时targets4.070GiB、磁盘可用117.367GiB/12.670%，随后持同一已验证锁清空root编译产物（1660744→0KiB），保留锁inode及父目录，5consumer逐项不变；总targets2.486656GiB、可用118.897743GiB/12.835054%，前后无构建进程。证据`/tmp/openrust-c04b-final-clean-root-artifacts.log`，源码与生成应用及/tmp日志均保留。

### 已纠正的失败与适配边界

- 初次 query 测试构造超过 `http::Uri` 自身上限而 panic，尚未进入框架入口，不能计为413。原始日志被后续定向日志覆盖，未保留独立红日志。现限额16KiB已由有效 URI 的真实 TCP 413证明；初轮 lint/并行 ABI 编译错误也仅是集成修复，不算行为验收。
- form/value 独审修正路径分段前有界、POST/query原始NUL差异、`%00`后续字段及空根数组。整簇复审发现并修正 reset字段 `new_password` 被trim、`has([])`、typed Query嵌套数据枚举和typed JSON大整数精度问题。大整数与枚举红证据为 `/tmp/openrust-c04b-wide-red.log`、`/tmp/openrust-c04b-enum-red.log`。
- 全局 `arbitrary_precision` 虽使精度回归变绿，却破坏应用侧 JSON Value→Figment Serialized 来源，见 `/tmp/openrust-c04b-figment-bridge-red.log`。配置桥无法恢复进入框架前已变形的来源，方案弃用；实验独占 diff 保全 `/tmp/openrust-c04b-config-bridge-preserved.patch` 后精确移除。最终采用值字符串 token 局部重写并保留数字词法，没有开启全局 feature 或修改配置仓储。未知字段 `1e400` 曾出现原生200/受管理400，红日志 `/tmp/openrust-c04b-ignored-number-red.log`；修为 IgnoredAny 语法校验、有界词法扫描、最后由目标类型解析。1024/1025 entry与展开key资源测试初次因引号多转义而输入非法JSON，见 `/tmp/openrust-c04b-lexical-fixture-failure.log`；修正fixture后实际通过。
- 并发 APP_PORT 配置变更使首次 default workspace 出现 lib320通过/1失败，原日志 `/tmp/openrust-c04b-final-tests-default-app-port-first-failure.log`；由原owner修fixture后精确回归通过 `/tmp/openrust-c04b-final-lifecycle-corrected.log`。该配置/CLI修改不属于B1。
- localization 旧重复 `count` 负例期待400、实际200；文档/控制器没有独立拒重复契约。按PHP后值覆盖改成严格校验count=2、翻译及响应字段的真实TCP正例，其他越界/日期/时区/缺参负例保留。红 `/tmp/openrust-c04b-final-tests-default-localization-red.log`，绿 `/tmp/openrust-c04b-localization-green.log`（3/3）。早期全特性1133/99仅对应当时快照，最新1136/99见上表。
- 普通生成consumer首次test因生产路由使用serde_json而清单仅有dev-dependency报E0433；check --tests成功不能替代bin构建。红 `/tmp/openrust-c04b-generated-missing-runtime-dependency-red.log`。仅修本任务临时consumer及prepare脚本的直接依赖，框架/模板未改；随后check/test与2项consumer通过。
- 初次双库脚本捕获具名认证ok但Cargo exit101，原统计仅识别ok summary造成0/0，原始输出未保留，不能事后伪造失败细节，见 `/tmp/openrust-c04b-auth-pg-first-incomplete-diagnostics-red.log`。安全parser补充失败summary/名称/位置后，PG整组实际41通过/6失败，定位account_security:193、cache:296、registration:677；nocapture交错导致其余失败名不完整，不推断，见 `/tmp/openrust-c04b-final-auth-pg-diagnostic.log`。中途双库具名各1通过/46 filtered只证明该用例（`/tmp/openrust-c04b-final-auth-root-exact.log`），不充当整组通过。
- cache/account连续三次同类失败曾按§5停止；探针、原hash/diff与anchor均回收。原响应探针为 `/tmp/openrust-c04b-cache-response-probe.log`、`/tmp/openrust-c04b-account-response-probe.log`，safe parser合成5例为 `/tmp/openrust-c04b-safe-parser-synthetic.log`；用户获准的一次cache诊断仍400且title缺失（`/tmp/openrust-c04b-cache-authorized-once.log`）。随后账号根因实验确认进程内相对URI无Host返回400/`http_domain_authority_invalid`，补Host即403/no-store，见 `/tmp/openrust-c04b-account-rootcause-red.log`。旧safe码白名单遮住已有错误码，不能将 unlisted_or_absent 当作框架无code。仅修fixture Host，不改安全边界、不放宽断言；PG/MySQL session各1、MFA相邻各2通过（`/tmp/openrust-c04b-account-host-green.log`），缓存模块各6通过/41 filtered（`/tmp/openrust-c04b-cache-host-green.log`、`/tmp/openrust-c04b-cache-host-mysql-green.log`）。
- 后续PG整组46通过/1失败，仍是迁移fixture（`/tmp/openrust-c04b-final-auth-root-complete-host-fixed.log`）。框架按迁移名称排序，不能根据注册Vec末尾判断down1；新showcase编号000001早于tenant000008，用户按§5明确授权改未提交新迁移编号000009。保留showcase两表down/up、旧表及history严格断言，不改排序契约或已有数据库。修后完整双库各47通过见上表。外部record局部3lint与迁移fixture适配只为当前集成门禁，不归B1提交；最新fmt曾发现fixture布局未格式化，原证据 `/tmp/openrust-c04b-final-migration-fixed-fmt-registration-first-failure.log`，随后fmt通过。

### 资源与并发源码守卫

运行阶段按实际编译依赖校验源码哈希；曾因新增外部record迁移触发exit92而在启动CLI/TCP前停止，见 `/tmp/openrust-c04b-runtime-gates.log`，不把旧快照结果冒充外部WIP验收。后续阶段重新建立快照；fixture-only变化与生产产物依赖分别审计。

两次中途恢复均先取得`target/debug/.cargo-lock`的flock，用本任务短offline check证明Cargo确实阻塞于artifact-directory锁，仅终止/join自己的探针并核对无其他构建/服务使用者，再清可再生incremental；不碰deps/build/consumer target或他人WIP。首次21.153275→13.803791GiB，实际释放7.352875GiB、可用11.391436%，证据 `/tmp/openrust-c04b-clean-incremental.log`；再次释放1.616997GiB、可用10.666409%，证据 `/tmp/openrust-c04b-clean-incremental-auth-soft.log`。

最新workspace后仓内所有target共31.807236GiB、可用90.010311GiB/9.716645%，触软线暂停扩展。按同一锁与探针协议先dry-run再清本任务根target产物，保留锁inode及consumer缓存；根target归零，仓内所有target余2.486656GiB，可用118.955631GiB/12.841303%，无后续使用者。证据 `/tmp/openrust-c04b-clear-root-dry-run.log`、`/tmp/openrust-c04b-clear-root-artifacts.log`。五项剩余门禁随后会再生成产物，以上只是恢复时点，不是最终收尾。

### 提交与证据边界

仅提交B1内核/公开API及输入测试、模块input演示、密码字段/空白密码回归与模板、PHP重复query契约测试、account/cache Host fixture及ADR/对应文档。外部record实现、迁移/种子、URL/对象key命名空间改动及guard WIP不夹带；混合文件必须按patch暂存，工作树保留原owner内容。record迁移fixture适配完整备份与精确patch位于 `/tmp/openrust-c04b-record-migration-fixture-eqpfjafm`，record纯lint备份位于 `/tmp/openrust-c04b-record-lint-3r_c2szd`。上述迁移编号授权与真实47项验收是当前工作区集成证据，不代表B1限定commit包含record代码。外部record测试记录小节另行逐字保全，本草稿不纳入其内容。


<a id="c04-d10-e-scheme-与-guestintended验收中未提交"></a>
## C04-D10-E scheme 与 guest/intended（限定子范围已验收）

最终索引 `/tmp/openrust-c04d10e-verification-v4/FINAL-HANDOFF.json`，SHA256 `771e8e18b8526b4080ad71ac363ab19ebb443ee46daa97edc8ac586ba0005437`；status为 `verified_limited_C04_D10_E`，执行句柄已全部收齐。最终freeze SHA `8dd1ba1dcff13e4af84aae8baf96b2e22ebbdf6bb4b1c455e8cd6460d553f0f3`，主控已核577项源码、16项辅助输入和综合索引所链证据哈希无漂移。

- v3定向10阶段通过：fmt/diff、模块5、CLI快照1、默认/全特性严格Clippy、Session/无Session重定向lib15/6、完整router110/108。main11阶段通过：默认workspace804 passed/33 ignored、全特性1589 passed/107 ignored、双严格Rustdoc、4种feature、cargo deny、Rust1.97两项MSRV；所有实际0/source_changed=false，ignored不算真实服务。最终server/CLI构建通过。
- v4仅修模板与snapshot的根URL精确期望，同一字面量去尾slash；冻结577项只有这2项变化，运行源码/manifest/lock/核心模块测试均不变。独审核定受限runtime-input-reuse映射后，复用v3运行门禁和单一server回执；CLI重新实际snapshot/build，fresh普通生成fmt/check/test7通过，MySQL8.4.13生成认证2与额外单锚库公开helper1通过，生成源码与锁稳定。
- 最终真实TCP无Session4/MySQL9场景通过：secure仅核https Location不跟随或冒称TLS listener；guest两Cookie意图独立、两次普通GET不老化、真实确认回跳、消费一次与固定回退、既有错误包/flash老化均核验。MySQL自有锚库和网络库已created/dropped，进程正常stop0、非强杀、端口释放及敏感文件删除为true。
- 首轮严格Clippy101（doc标识/unused_self）及v3生成6pass/1fail（fixture根URL多尾slash）原回执保留；修复不降断言。核心源码、v3最小借用修复、驱动与v4字面量/输入复用均经不同模型独审，无剩余问题；静态审查不代替上述执行。
- 3个自有生成package精确clean实际0，未清根target；清理前后源码/锁/serverCLI/框架与8461个非自有依赖哨兵不变，无Cargo/锁占用。登记缓存37,204,033,536→36,765,954,048字节，磁盘空闲107,083,919,360字节（总994,662,584,320），低于50GB软线并高于10%空闲。

固定PHP方法参考39案及previous边界12案见 `/tmp/openrust-c04d10e-url-values.provenance.json`、`/tmp/openrust-c04d10e-previous-values.provenance.json`；后者替代request/root/response构造边界，不是完整PHP HTTP/Symfony验收。JSON关联数组/任意Stringable对象差异明确；完整Redirector、action/signed/混合URL参数、HTTP/Session及PG/全部后端发布矩阵继续开放。限定本地提交见Git。


## 持续开发守护当前验证

skill-only提交 `a3b2ca3` 提供显式 `--rebase-usage-budget` 及正式额度可用后结束旧等待的完整恢复路径。完整测试120项：117通过/3项Windows原生跳过；questions52通过，不同模型复审通过；日志 `/tmp/openrust-guard-upgrade-full.log`、`/tmp/openrust-guard-upgrade-questions.log`。本任务已由所属owner plain重载并核原Goal active、working/task_running及进程/心跳正常，现有baseline `1793413935.123801`、account history与questions摘要前后相同，无full-budget覆盖，默认80%政策保持。当前任务已显式重定，不再次rebase；该纯skill变更不要求重跑D10-F Rust矩阵，未替代任何框架行为验收。


<a id="c04-d10-f-refresh-与-away验收中未提交"></a>
## C04-D10-F refresh 与 away（限定子范围已验收）

最终输入 `/tmp/openrust-c04d10f-verification/freeze.json` SHA `ba5e14b4e8517d9462c9e201e1c77bd20b9f679d54333e32fba43a11aeeedd79`；执行基线a3b2ca3。本簇全部使用新运行门禁与新server/CLI，无旧簇runtime或二进制复用；主控核577项源码、12项辅助输入哈希一致。最终证据索引 `/tmp/openrust-c04d10f-verification/FINAL-HANDOFF.json` SHA `e13bad753fb32457a2086b27a3342322ac978000d1b4816dddab69e560a83016`，全部10个执行句柄终态0，限定提交见Git。

- 定向10阶段全部实际0/source_changed=false：fmt/diff、模块5、CLI快照1、默认/全特性严格Clippy、Session/无Session重定向lib19/9、完整router112/110。具名定向用例无失败或忽略。main11项通过：默认workspace809 passed/34 ignored、全特性1595 passed/108 ignored、双严格Rustdoc、4种feature、cargo deny、Rust1.97两项MSRV；ignored不计真实服务。
- 新server/CLI构建通过；fresh普通应用fmt/check/test7 passed，MySQL8.4.13 fresh认证2与额外单锚公开helper1项通过，源码和锁稳定，不修补生成后源码。最终真实TCP无Session7/MySQL12场景通过，包含刷新丢query、同路径真实GET、输入/错误首GET恢复后老化、固定外链状态/头/HTML与忽略不可信url；保留E导航意图和错误包正例，不跟随外站或冒称HTTPS listener。
- 所有执行句柄收齐；网络worker/stop实际0、非强杀/非超时、监听释放与敏感配置移除true，自有MySQL生成/网络库created/dropped=true。无F运行失败，准备期network dryrun变量错误与修后输出保留，不作运行失败或成功的替代证据。
- 3个F自有package按实际manifest/fingerprint精确clean，actual0；同名test_app/auth_flows/generated_view仅排除本包unit hash，未泛排其他包。源码/锁/serverCLI/框架与非自有依赖哨兵不变，无Cargo/target锁占用，不清根缓存。登记缓存37,209,677,824→36,771,332,096字节，磁盘空闲107,002,572,800/994,662,584,320字节，满足50/100GB及10/5%策略。

固定Laravel为d52c3b3，Symfony8对象14案实际构造只作历史决策 078独立固定比较点；Laravel无已解析composer.lock，不声称锁定该版本或完整PHP HTTP。共享构造器顺序为HTTP范围→空目标→允许状态→头；to先生成目标，与固定上游一致，guest先put/intended先pull未变。核心、调用方、驱动均经不同模型独审，无剩余问题。完整Redirector、action/signed、URL参数其余语义及HTTP/Session/发布全后端矩阵继续开放。


## C04-D10-G 动作身份与 action URL（限定子范围已验收，提交见Git）

固定Laravel framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`，Route/RouteCollection真实PHP探针7+2案对应声明方法顺序、完整无分隔拼接键、域优先union及前导反斜线例外；并非完整PHP HTTP。Rust公开RouteAction、get_action/on_action、组namespace、真实Route.action、Boot与生成器本实例根namespace、共享action URL渲染及Redirector.action/action_with；clone/请求快照隔离，无名动作应用亦提供生成器。参数仍限字符串映射/必需段，实际dispatch原串碰撞差异及完整Controller/URLGenerator/Redirector保持开放。

最终运行冻结 `/tmp/openrust-c04d10g-verification-v4/freeze.json` SHA `572f078577a47cf50fdaedc376dfdb171d0f15c6698d702dd1fda1c335bc5b3a`；总索引 `/tmp/openrust-c04d10g-verification-v4/FINAL-HANDOFF.json` SHA `fe4aa672aba5b465c40798861c9b2489ba9cade0ad480c40e4733768f4876ae3`。主控实际核68份关联path/SHA、579项源码均一致；21个执行句柄全部终态，旧失败仍是非零退出，不改写为成功。不同模型对契约/核心/调用方/修复/驱动与最终输入复审无剩余问题。

- 最终定向通过：重定向Session/无Session lib19/9、动作身份/集合/URL单元全特性及无默认特性各11、完整router117/113，均无ignored与源码漂移；HTTP动作4、模块5、CLI快照1已实际通过。fmt/diff、默认与全特性严格Clippy通过，无allow或降阈值。
- main11项成功证据收齐：workspace默认824 passed/34 ignored，全特性1611 passed/108 ignored，双严格Rustdoc、4种feature、cargo deny、Rust1.97两项MSRV；最终fixture还实际补验all-targets MSRV。忽略项不算真实服务。仅Agent文档提交不影响运行输入；版本间仅文档或cfg测试差异按input-impact精确说明复用，变更unit在最终断言下重新执行，gRPC fixture亦经exact及完整all重验。
- 本轮新server SHA `b978508bd4ee00d2a69f30be415ef5f8819a4ea89ebe78c0e34feb919feac327`、CLI SHA `53116d02af1d18ee35610c7116ccc1b5bcd1fb64f68fdab924c4c995afc9970c`。fresh普通应用fmt/check/test7通过；MySQL8.4.13认证2与公开helper1分别实际通过，无生成后源码补丁。原MySQL合并summary仍false：auth2已过，public-check日志前RuntimeError具体tag未知，不宣称磁盘根因；独立单项补验绑定同生成源码/helper/锁与本轮双binary，新owned锚库，严格check/exact1及finally清理均通过。补验summary SHA `7d51fd1e617eb2bfe954e4769a0630580a5da7b4d0a12240e5a4ca860dfa9987`。
- 真实TCP无Session10/MySQL15通过，新增固定动作跳转忽略不可信url、tuple/invokable匿名目标真实action/id、303自定义头、Session输入/错误恢复及再次老化。正常stop/worker退出0，无强杀/超时、监听释放、敏感临时数据移除，所有本轮自有MySQL库created/dropped均true，不触共享服务。
- 保留五份失败：HTTP缺生成器fixture错预期500（既有Custom契约实际400，修为精确code/status/无Location）；README doc_markdown；unit expect lint；gRPC联合关闭裸connect断言；生成public前置RuntimeError。gRPC仅修已证startup drop/rebind空窗，改公开预绑定listener入口并保留严格stop/join/readiness/关闭一次/双端口拒绝，补角色地址诊断；原停机失败根因未确证，不以通过重验声称已证根因。PHP前三次helper装载失败另保留，不算框架行为失败或通过。
- 3个本轮真实package精确clean均实际0，回收439,365,632字节；579项运行源码、3份生成源码/锁、新server/CLI、50个精确framework与全部nonowned deps哨兵、旧回执保持不变。登记缓存37,729,984,512→37,290,618,880字节，磁盘空闲107,410,919,424/994,662,584,320字节；无Cargo/target锁。额外绝对.d路径归属预检首次过严，尚未clean即停止并保留；核relative first-source后按精确package/FP/unit hash/.d/真实manifest源解析再执行，不宽排同名测试、不清根缓存。清理回执 `/tmp/openrust-c04d10g-verification-v4/cleanup-result.json` SHA `90ed6ab734648ccb325460aa85db418557a78a9d23d414cd55ceb2330f01aaa7`，清后索引 `POST-CLEANUP-HANDOFF.json` SHA `3e29fe2ad0119ed620b6a02873c9c833e9e2dea50bbcd44fda405aa8671f74cb`；全部23执行句柄终态。PG及全部承诺后端发布矩阵继续开放。


D10-G gRPC环境补充：用户报告此前可能在失败期间手动清理Docker并删除全部容器，仅列候选外因，尚无对应时间或连接记录可确认关联。原失败整条all命令时间为2026-10-31 03:38:40–03:41:26 UTC（北京时间11:38:40–11:41:26），没有该用例的独立时间戳或当时listener owner记录。该fixture显式使用memory cache、默认数据库URL为空且无具名库/副本，访问本机HTTP/gRPC与临时生成TLS材料；验证驱动亦剔除继承的DB_/DATABASE等变量，未发现该用例直接依赖容器DB/Redis的证据。因此不能仅凭Docker操作解释本机停机后connect成功。保留原失败、根因未确证及已通过exact/完整all重验，不删保护断言，不重跑已通过矩阵。


## C04-D10-H 最终声明集合与真实分派（限定子范围已验收，提交见Git）

固定Laravel framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`；真实PHP matcher35案及独立QUERY/Allow/顺序补证9案通过，仅证明RouteCollection匹配、绑定及合成OPTIONS，不是完整PHP业务HTTP。Symfony8为独立固定比较组件，不冒称Laravel已解析锁。Rust由同一真实声明派生逐method原串分派与完整methods动作view，保留覆盖位置、域优先union在Host过滤前、匿名声明、非fallback优先及终止边界；不以此关闭完整Routing/Controller/URLGenerator/Redirector。

最终冻结 `/tmp/openrust-c04d10h-verification-v4/freeze.json` SHA `ae563056251dd08bd99ea7b9ed38e88ac5b4bc020a0855698191271babfc75c0`；最终索引 `FINAL-HANDOFF.json` SHA `3d89f29ccbec2a1e2d4c71b1732887630a2d06333051ada17a7a2de0147a0461`。主控实际复核74份关联path/SHA及580项冻结运行输入；全部句柄终态，源码/锁稳定。核心、调用方与清理修复不同模型独审无剩余阻断。

- 定向与适用门禁通过：核心collection7/原action5在all及无默认特性各执行，HTTP collection7包含6次真实loopback Host请求，证明旧handler/middleware各0、raw域碰撞与Host前隐藏、GET/HEAD405及OPTIONS、直接POST；完整router124/120、模块5及CLI快照1。双严格Clippy无allow/降阈值；main11项为默认838 passed/34 ignored、全特性1625 passed/108 ignored、双严格Rustdoc、4种feature、cargo deny及Rust1.97两项MSRV。忽略项不算真实服务。
- 本轮新server SHA `0823c01a91f85b59660ea9128584744d44b8dca707096d548cb7a941dc627efe`、CLI SHA `908604cb93f0d0586214856e5f16c4c5b27eadbfd6cfde8813a5accda15e7f72`。fresh普通应用fmt/check/tests7通过，MySQL8.4.13认证2与独立公开consumer1通过，无生成后Rust补丁；自有库created/dropped均true。
- 应用真实TCP无Session11/MySQL16通过，H新增自然summary路由验证静态顺序与最终动作覆盖，其余保留已接公共能力；raw碰撞的网络证据由上述HTTP测试中的独立loopback提供，不能从应用11/16项外推全部碰撞矩阵。网络worker/正常stop均0，无强杀/超时，监听释放、敏感临时配置清除、自有MySQL库回收均true；共享服务未删改。
- 失败记录保留：首轮空体assert lint、提取前helper长度lint、提取后Router未限定的编译错误、旧POST405的Allow断言未同步新格式，以及网络前置错误导入旧common。第三次fixture编码/校验失败曾按§5停下，用户明确选择严格定向重验；类型仅明确为axum::Router，405仅同步为逗号加空格，OPTIONS仍无空格，未弱化断言。编译/网络前置失败不冒称真实请求失败；旧非零回执不改写。
- 网络前置修复另存 `network-v2/network.py` SHA `2e2cdeae20f1557c31504ca723df184d2dfad1265da132f9adda794dcb34f8a7`，输入绑定SHA `8868c320bd452fd537932a972195c145a326c03f6bcebbf06f7bb8e3a5f2bd80`。入口在子目录引入原v4common，资源HERE不变；旧helper枚举、checked/source/artifact校验未修改，parent/worker前后补验新输入。旧preflight记录为附带provenance而非该驱动四项自动校验对象；主控与独审已另行复算其SHA一致。主矩阵/生成不受此未执行入口修复影响，未伪称重新运行。
- 最终输入复用按逐source/argv/env影响映射保留：首轮核心/模块/CLI及后续未变生产证据可复用，修改的HTTP harness、失败项、完整router与双严格Clippy在适用最终输入实际补验。Agent文档更新不使Rust证据失效；签名URL及测试入口整合仅有后继只读审计，未混入实现。
- 3个本轮plain/mysql/publicconsumer package精确clean均实际0，回收437,055,488字节；580项运行输入、生成源码/锁、新server/CLI、50个framework及19,587个保护哨兵与所有非自有依赖不变，无Cargo/target锁。清理回执 `cleanup-result.json` SHA `500a55079fdd1e791f6cad6253e7318f6e40510e89690af186eef2547df5e8da`；登记缓存37,676,105,728→37,239,050,240字节，磁盘可用105,982,365,696/994,662,584,320字节，满足50/100GB与10%/5%规则，保留活跃根缓存。

上述相对证据路径均位于 `/tmp/openrust-c04d10h-verification-v4/`。PG及全部承诺后端发布矩阵、完整Routing与其余HTTP行为继续开放，不能由该子范围或单库日常结果关闭。


## T01 Session纯行为测试入口整合（限定试点已验收，提交见Git）

基线为H提交 `3065be5`。仅删除Cargo中独立 `http_session_extensions` test声明，并将原文件作为 `http_session` 私有模块；原扩展源文件逐字不变（SHA `f5151ad2603e226fbb1c2f14369f11185d618a170fc8b507fb3e8560c314848b`），不删用例/断言/fixture，不改生产API、依赖、feature、锁或模板。全仓显式test入口46→45，Session family8→7，相关执行入口2→1，family源文件仍10个。环境子进程、真实DB/Redis与其他门控入口保持独立。

最终证据 `/tmp/openrust-session-entry-pilot/FINAL.json` SHA `885625aca6a76bbe6011805e0abeb72b540bd41586aefcea0f1d29dc0a06f7b2`；before/after证据SHA分别 `d896dee473c871fe1a96f32e88ef587fcfb7153f11b20fce6c57b37fc9050dc3`、`5cf18a6edae86fefb4a6e5db1074a0d6af5cc45c18c3cb3460e38f15b1907c38`。主控实际复核35份命令日志SHA/退出0、3份源SHA、两feature的41项完整名字映射以及锁/扩展文件未改；原26+15项只对扩展15项增加 `session_extensions::` 前缀，无重复/漏项。撤销后不得跨store复制、双墓碑、取消/错误不执行业务或save、请求关闭/转移边界等原回归均随完整41项实际执行。

同Rust/Cargo1.99.1、main target、jobs1、incremental0、dev/test debug0及中性环境，session-only与all-features在修改前后均实跑首次/暖缓存no-run、名单与实际用例；两组合各41项、0失败/0忽略。下表为本次墙钟秒数，单样本不代表稳定提速：

| 组合 | no-run首次：前→后 | no-run暖缓存：前→后 | 暖缓存用例执行：前两入口合计→后单入口 |
| --- | --- | --- | --- |
| session-only | 11.754→1.097 | 0.164→0.156 | 1.443→1.412 |
| all-features | 2.299→1.272 | 0.187→0.183 | 1.461→1.405 |

首次基线session-only包含lib/server重编，修改后仅test入口失效，不能同类比较或据此计算提速倍数；warm Cargo timing units为空、test unit sections未分离链接，因此不能声称链接阶段加速。原两个可执行文件大小之和→本次单文件：session-only 23,957,904→21,147,712字节，all-features 37,542,432→33,847,632字节；这是受测文件大小，不是实际磁盘回收量。这里只证明入口减少及回归保全，未证明整体开发速度显著改善。

集中门禁全部实际0：fmt、默认/all-features严格Clippy、默认workspace838 passed/34 ignored及全特性1625 passed/108 ignored、双严格docs、cargo deny、Rust1.97 workspace/all-targets/all-features check；忽略项不算真实服务。全部T01命令无失败，无通过整套单线程掩盖隔离问题。README/CHANGELOG与Agent文档另作纯文档同步，不影响被测Rust行为。

本簇未重跑生成项目、MySQL、PG、网络或服务；H对应生产/模板/依赖输入未变，其历史证据保留，不命名为T01新验收。无新target目录/测试库/监听服务，主控收口采样全登记缓存37,482,668,032字节，磁盘可用105,749,102,592/994,662,584,320字节；预算内保留活跃main缓存，不做整target或openrust包清理。当前免费本地执行器只读查询因49374端口冲突及显式服务health502不可用，按规则回退gpt-6-sol执行，未改服务/端口、全局模型/账户或系统配置。后继扩范围须逐项确认隔离与实际收益，不按数量配额删测试。


## R2-R03 递归配置发现（任务分支限定验收记录）

`tasks/config-discovery-family` 基于 `e25ec7589c7662245f9d6cc4390b1fd06a920f1e`；本节仅记录独立分支限定验收，本地提交见Git，尚未与 ENV main `2187b6b` 集成。核心覆盖递归 TOML 发现、真实 basename、隐藏文件与目录 symlink 排除、自然序/碰撞范围、根领域相对内容迁移及模块独立overlay接线；同一 `ConfigSources` / 仓储保持来源顺序与点路径。晚到 Provider 默认值浅合并，白名单 `mergeableOptions` 数组对照 PHP 浅合并；只有子文件时按 PHP `after_final` 保留文件前 base 剩余父根覆盖，再重放文件后环境优先与 metadata/profile，模块 overlay 独立。PHP 探针和独立 delta 审查只证明这些指定边界，不代表完整 Config 或动态 Settings。

已实际执行：配置组修后 76 项、CLI 最终 21/21（含相对 cache 1 与当前分支强制重编快照 1）、模块配置与顺序各 1、公开 consumer 完整 6；严格 Clippy、Rustdoc、fmt 均 exit 0。CLI 末次仅将旧 gRPC、security.signed_links、auth.jwt 前缀测试断言改为真实相对配置，原 snapshot 不变。普通 fresh 生成 check/test 7 项及安装公开 consumer 精确 1 项通过；fixture installer 后续只修格式，独立格式检查通过，原具名语义测试不冒充该格式修正后的重跑。旧失败保留：配置环境层4项、模块缺 feature schema、consumer 未链接 Tokio macro、Clippy lint 三轮、共享 target 旧 CLI 可执行文件导致 0 项假通过、当前 CLI 快照注释丢失；均经对应输入批修/定向重验，不将旧结果冒充通过。CLI splitter decor 与 helper 提取分别经不同模型窄复审，无新增确定问题。最终回执 `/tmp/openrust-config-discovery-final-manifest.json` 当前 SHA `cd6483f40f80d73767fa32995064df254b5f5cf31e15617cf4861b40add6304c`（追加 cleanup_audit；35 任务源码及原功能证据未变）；root `/tmp/openrust-config-discovery-root-final-audit.json` 逐项核 128 项哈希一致，覆盖 35 源文件、日志、4 种生成变体及关联回执。helper、CLI decor 和 HTTP runner 的不同模型窄复审无剩余确定问题；纯 Agent 文档/矩阵/变更日志更新不改变已测 Rust 输入。原 verify manifest、ready receipt 和旧失败回执均保留。

`/tmp/openrust-r03-http-report.json` 记录根应用真实 TCP `GET /api/example/config-feature` 为 200、`enabled=true`，server exit 0、临时资源移除；报告绑定 binary/source SHA，仅证明该配置路由。最新 URL/TLS runner `a021…` 的 10/10 独立网络场景、两服务自然 exit 0、无源码漂移和二进制稳定另记，不替代 config-feature 证明。fresh MySQL 无 auth 生成 check/test 的测试实际 0 项，仅算编译；fresh MySQL 8.4 认证生成 `auth_flows` 2/2、0 ignored/skipped，测试库集合 61→61 且哈希一致，未留下本次新库。认证测试使用程序配置，不外推所有文件发现场景。owner 报告三个唯一生成包精确清理 813.2+4.7+424.0 MiB；真实 shell 清理命令均 exit 0、执行者终端回报；原始清理 stdout 未留存，不能独立复算其逐命令日志 SHA。紧邻共享 target `du` 记录 38,753,084→37,691,612 KiB，差 1,061,472 KiB；稍后的主 target 漂移不并入本次精确回收，也不称全 target 已清。登记 16 路径 54.48 GB、磁盘空闲 8.782%，50GB/10% 软警告，100GB/5% 硬线安全；未知 root-site/M5 缓存未碰。当前分支未执行 main 完整 workspace 组合，ENV 新命名尚未集成；历史 main/旧簇证据不转记 R03。完整 V1 Config、typed helpers、配置缓存、动态 Settings、发布全部后端矩阵保持开放。

## R2-T ConfigRepository 严格类型读取（隔离分支限定验收通过）

基线 `a5e6461`；冻结Spec `/tmp/openrust-config-typed-helpers-frozen-spec.md`、最终ready `/tmp/openrust-config-typed-helpers-ready-final.json`。源码独审无确定阻断；集合位置索引显式类型标注修复首次 E0282 后，`/tmp/openrust-config-typed-helpers-verify-batch1-attempt2/receipt.json` 记录 fmt/diff、仓储43、Collection4、模块1、公开consumer6项实际退出0；`/tmp/openrust-config-typed-helpers-clippy-docs/receipt.json` 记录严格Clippy lib/server/consumer各1与Rustdoc1退出0。旧 E0282 与普通生成首轮因驱动假定全新项目已有 Cargo.lock 而失败的回执保留，不能计作生产行为通过。后段最终 `/tmp/openrust-config-typed-helpers-verification-attempt2/FINAL.json`（SHA256 `fb140cf667227fd346fa044f94f6d4119cd002b107c991fd86d2fd95481749b0`）记录12条命令退出0：fresh普通生成 fmt/check/test原7项、安装公开fixture后check/exact1项、根server构建与真实GET `/api/example/config-greeting` 返回200及固定问候语；进程78122收到SIGINT后exit0，端口43228释放，日志有界且源码/二进制稳定。唯一新生成包 `openrust-typed-93492a40e6e9` 精确clean退出0（Cargo回报2121文件/649.1MiB），post dry-run为0项；Cargo `-p` 对path/version限定会忽略，本次唯一随机包名事前核无同名产物，不能把该清理口径扩到已有共享包。全登记资源约55.83→56.83 GB（十进制），结束磁盘可用86,953,000,960字节，超50GB/10%软线但低于100GB/5%硬线；共享缓存未清，旧未知目标不碰。默认数据库无代码改动，本簇不把旧MySQL证据转记为新验收；尚待root最终本地提交及main组合验收，完整L005.03/Collection及发布全后端开放。

## Signed URL 家族：任务分支定向验收（未合入 main）

分支 `tasks/signed-url-family` 基于 `af0f62b28a7a2dce65e07f02ee1026b9e30f8fb3`，本节记录分支限定证据，不是 main 集成或完整框架验收。固定 Laravel framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`；PHP 8.5 fixture 27 案（`tests/fixtures/laravel_signed_url.json` SHA `bc77dc16619ed6c4cfd77834f006cfdb37207607a5ab1563c3731a245dd0a684`），27 个 ID 均被实际库测试引用。最终原功能定向库 7/7、`http_router` 4/4、唯一模块 1/1、CLI 20/20、全新普通生成项目 7/7，均 0 failed/ignored；筛选命令的 filtered 数不计 ignored。首次模块 `--exact` 实跑 0 项，已用具名命令重验 1 项；此前 router、Clippy、CLI 失败及修复记录保留，不改写为首次通过。

公开 `UrlGenerator` 永久/临时签发、`RequestInfo` 原始查询验签、受管 `signed` 中间件、应用级忽略字段和当前/旧 key 验签均有分层测试。唯一模块草稿链接经过公开生成器、请求和 middleware；CLI 模板生成 helper、环境示例及快照已纳入分支。全新普通项目最终 fmt/check/test 实际通过；原回执的生成 helper 哈希文字已更正；执行者报告当时终端同次输出源模板与生成 helper 相同哈希，但生成目录已清，当前无法独立重算。因此保留实际生成测试 7/7，不以当前模板哈希代替已删除生成文件的复验。

真实 TCP 是测试内 Axum Router 绑定 `127.0.0.1:62451`，由 reqwest 经 loopback 请求并带签名 Host，断言签发、有效命中、篡改/过期拒绝、relative、局部与全局 ignore、运行期 key 轮换、旧 key 验签及 handler 副作用；正常优雅关闭后同端口成功重绑。这不等于根 server 或生成应用的跨进程生产部署。TCP 异常路径原先缺总请求时限及失败时受管清理；测试现设置单请求和整体时限，在请求错误、panic、超时及正常结束均受管停止、join、shutdown，并保留原错误；不同模型窄复审无新发现。后续清理增量定向 router 4/4 通过；首次严格 Clippy 因 `redundant_closure` 失败（`/tmp/openrust-signed-clippy-bounded-delta.log`，exit 101），改为 `.map(drop)` 后在最终测试源码上严格 Clippy exit 0。原失败日志保留；warnings-denied lib Rustdoc、Rust 1.97 lib check、fmt/diff 只在各自原输入上通过，未冒称 main 组合门禁。

本簇未改 DB/Redis schema 或 driver。补验使用已有 Docker MySQL 8.4，fresh `--db mysql --auth` 生成项目 `cargo check --tests` 与实际 auth 测试 2/2 通过（0 failed/ignored/filtered、无 skip）；只读残留检查未发现本次新建测试库。该 auth 测试不导入签名 helper，不把它算作 helper 的数据库 HTTP 验证。公开 TestApp consumer 1/1 通过；gRPC consumer 仅宿主 `cargo check` 通过，不称其测试通过。`VAPOR_RAW_QUERY_STRING` 的真实部署注入、代理剥离前缀入口，以及 PHP 对象/数组/Enum 参数形态仍开放；Rust 当前只覆盖 `BTreeMap<String, String>` 字符串参数。完整 URLGenerator/Redirector、main workspace Clippy/tests/docs及适用 feature/MSRV/deny、ENV 合入后的生成与服务验证均未由本分支证实。

分支回执 `/tmp/openrust-signed-final-verify-manifest.md` 当前 SHA `b79b983e921ffa6a7b012ff621183bb8e415afdcbcaa1943daec5a28184f9507`；`/tmp/openrust-signed-root-hash-audit.json` 已只读重核并绑定该修订回执，29 日志和当时 22 文件逐项匹配；更正前审计保留于 `openrust-signed-root-hash-audit-pre-correction.json`。该审计不覆盖后续 TCP 清理代码与日志。独立 delta 复审 `/tmp/openrust-signed-final-delta-review.md` 指出生成 helper 哈希及 TCP 异常清理两个 should-fix；前者笔误已更正，fresh MySQL 项目保留的 helper 与模板同次 SHA `3d7bcfd61ceb3f90dd753ffa768e62f6e562e1cd3f6f6526ed87eb074f716fb3`，但已删除的普通生成目录仍无法独立复算；后者由 `/tmp/openrust-signed-tcp-cleanup-delta-review.md` 静态关闭，最终 Clippy 已实际复验。补验 `/tmp/openrust-signed-supplement-final-manifest.md` 与输入索引 `/tmp/openrust-signed-supplement-input-sha.json` 记录 6 条命令、40 分支文件、44 生成文件、6 原始及 6 脱敏日志；root 逐项核 96 个哈希匹配。此后本节 Agent 文档更新属纯文档输入变化，不使 Rust 测试失效。补验后仅精确清理本次 fresh MySQL 应用与 migration 包，去重 `du` 实测回收 316,715,008 B；登记聚合 51,868,528,640→51,551,813,632 B，磁盘空闲 90,177,208,320→90,423,791,616 B，仍高于 50GB/低于磁盘 10% 软线，未触 100GB/5% 硬线。未知 root-site/M5 与其他共享缓存未清。清理日志与资源回执在补验 manifest，root 已核其当前 SHA `daf523e9e29af8203d2fb500fbbfcd094625bab564b16bf46cf3887af1b4813e`。分支尚未提交/合入，完整 main 组合仍待。

### 守护可靠性限定集成（2026-11-01）

源提交`8fa7cad281f19510e3f1f60fa4911b26bf8dafe1`的7个skill文件限定集成至main `31a6d38`，保留原todo/testing WIP。复用来源完整guard131（3项Windows原生跳过）、questions61和预算35的验收及terra独审结论；本次提交校验与diff通过，不重跑无关Rust门禁。未集成被拒绝的`cd487bb`。

公开plain stop/start后新PID71755，正式原Goal active；guard/supervisor/heartbeat均true，working/task_running且已发生重载后的真实工具调用。前后核验`/tmp/openrust-guard-reliability-reload-before.json`与`/tmp/openrust-guard-reliability-reload-after.json`：原baseline1793413935.123801/fraction0.8、账号历史与已结束grant一致；38问答ID无丢失，四行因运行中正常复核变化，未要求或宣称账本SHA不变。未rebase/fullgrant/重建Goal；旧缺失暂停历史的自动恢复边界仍开放。

### MIME 任务分支验证与产物回收（2026-11-01）

限定本地提交`5e2ec6b`位于`tasks/request-files-family`，28文件（21候选与7份Agent事实文档），工作区干净；尚未集成main。完整索引`/tmp/openrust-request-mime-final-evidence-index.json` SHA256 `002254eb43c595ff82c5fd308c44c238180239f930df71639beb37b122a31342`核对24份本轮日志与清理回执；原生Fileinfo/Windows、完整MIME及S3 ACL/迟到CopyObject/throw=false仍开放。

冻结v7的608项构建输入复核无漂移，代码15门禁通过，默认workspace955 passed/34 ignored，全特性1770 passed/111 ignored；回执`/tmp/openrust-request-mime-code-verification-v7/receipt.json`。fresh生成、真实TCP与MySQL九阶段全exit0，回执`/tmp/openrust-request-mime-services-prepare/run-20261101T080437Z/receipt.json`，无新增库残留。仍是任务分支限定证据，未证明main集成或完整Request/Filesystem已完成。

确认本轮归属、无进程/打开句柄后，正常Cargo dry-run与clean均exit0；根target30,115,557,376B→0，磁盘可用增加29,867,966,464B、16.968%→19.971%。未知外置65,982,464B、源码、数据库卷和日志保留。清理回执`/tmp/openrust-request-mime-final-cleanup-retry/receipt.json` SHA256 `49405499ac040bc6779687558f9704e40070865975e86302a0bce19d66c8409c`；首轮仅lsof解码失败，未clean，原回执保留。后续实现按需重建，不为纯Agent文档变化重跑Rust门禁。


## 原生 Codex 守护适配限定验收

原生后端复用现有守护生命周期和预算账本，以可信主线程、同项目与原Goal绑定，通过既有共享app-server的proxy读取状态、入队、启动及精确中断；不启动第二服务或接管Desktop writer。终态清理仅删除同线程/Goal/dispatch身份的自有队列，删除未知保留独立账本且不重放。入队后重新核对线程、唯一自有队列、原Goal和停止标记；启动proxy缺失仍保留发送意图，防止丢失清理身份或重复派发。

当前代码的隔离验证：完整守护147项（144通过、3项原生Windows跳过）、问答61项、原生适配28项全部通过，ResourceWarning按错误处理；Python AST、局部diff空白与本地文档链接检查通过。不同模型独立审查发现的入队后控制变化、后续proxy失败丢失发送意图两个阻断均修复并新增回归，复审无阻断。最终日志分别为`/tmp/openrust-guard-final-main-v2.log`、`/tmp/openrust-guard-final-questions.log`、`/tmp/openrust-guard-final-native-v2.log`。

首轮探索性完整回归的SIGKILL用例在tearDown等待监督回收时超时，其余通过；日志`/tmp/openrust-guard-final-main.log`保留，不回填成功。生命周期fixture的通知替身原误入Codex stdin协议，已改为立即退出，原断言与时限不变；定向真实子进程回归通过，日志`/tmp/openrust-guard-restart-regression.log`，最终完整147项结果以上述v2日志为准。

本机CLI为0.159.3，正式experimental JSON schema与proxy参数已核查；`codex app-server daemon version`实测默认共享控制socket不存在，未启动/重启服务，未改真人线程、Goal、额度或问答账本。当前Desktop真实恢复及proxy断连后的工具推进未验收，原生精确问答、可信真人控制水位与预算自动恢复仍保缺口；Windows跳过项不计原生通过。最后读取与queue/start没有CAS，仍有竞态，不能声称原子遵守其间的真人暂停。接入条件与最小路径见[平台说明](../../skills/agent-continuous-development-guard/references/platforms.md#原生接入条件与最小路径)。本簇仅skill和验收事实，不替代任何Rust业务源码门禁。

## 应用同进程启动

根应用用无参数入口运行 HTTP、队列和调度。MySQL 8.4 隔离库验证登录、文章写入及索引刷新、调度记录、同端口的 WebSocket 101 和 `SIGTERM` 关闭连接后退出 0。独立 `serve` 保留队列任务，单独 `queue work` 消费该任务，`schedule run --once` 独立完成。真实 Redis 回归验证无默认数据库时，默认启动消费 `example`，而 `serve` 留下待处理任务。

`tmp/default-app-runtime/` 保留 `live-receipt.log`、`redis-regression-final.log`、默认工作区 `workspace-tests-mysql.log`、严格 `clippy-final.log`、`doc.log` 和新生成 MySQL 项目的 `fresh-check.log` / `fresh-test.log`。配置后端或未编译功能的范围见[应用启动说明](../../../docs/src/getting-started/source-application.md)。

## 根应用 Docker Compose

`tmp/lead-compose-onELY4Bk/runtime-receipt.json` 记录冻结输入及 Docker Desktop Linux ARM64 Release 镜像。隔离项目确认外部数据库和 Redis URL 留在宿主配置，构建上下文排除 `.env` 与本地 SQLite 文件。真实 MySQL 8.4.13 与 Redis 上完成 15 项迁移，`/health`、`/ready`、`/api`、`/api/example` 均返回 200。HTTP 派发的任务已消费，调度产生记录，非 root 的 `/app/app` 能写入存储且 SIGTERM 退出 0，重启后迁移记录仍在。Buildx 导出的 ARM64 Linux ELF 与应用资源通过实际 `app list` 验证。验收容器、卷、镜像与临时导出已清理。验收使用源码根应用默认 feature。

宿主机挂载的验收位于 `tmp/lead-compose-binds-OQ2Zy3j0/`。Docker Desktop 的 Linux ARM64 镜像启用 Fortify JWT 与本地存储，真实 MySQL 8.4.13、Redis 8.10.3 和 27 项迁移通过。用公开 HTTP 上传的附件实际写入宿主 `storage`，重建容器后同一附件、MySQL 记录与 Redis 键仍可读。默认与自定义 `public` 均从宿主读取，容器写入被拒绝。缺失目录的补验由 Docker 引擎拒绝并确认宿主未创建目录。构建上下文实际排除了默认数据根和自定义根下的三类数据目录。以 Docker 的 Linux 原生测试卷验证数据库初始化拒绝不可写的非空根，接受空根并只调整根属主。

Mac ARM64 的 amd64 模拟证据位于 `tmp/lead-class-lifetime-U5c8kV7e/next-compose-x64/`。`emulated-03/receipt.json` 记录从 `1557bdad` 的旧命名卷切换至 `e77bfaf0` 的宿主机挂载。大小写敏感的隔离卷中重新应用归档数字属主，三份归档均通过 GNU tar 比较。MySQL 8.4.13、Redis 8.10.3 与应用附件在重建后仍可读取，原卷回退保留切换前数据。`ubuntu-artifact-receipt.json` 记录 Ubuntu 24.04 amd64 容器运行 Buildx 导出的 Linux ELF，验证了非 root 的 Artisan、HTTP 与 SIGTERM。两项均使用 Docker Desktop 模拟 amd64。

原生 Ubuntu 24.04.5 LTS x86_64 的[验收运行](https://github.com/hongyukeji/openrust/actions/runs/42520350003)以 `e99ccb15` 为输入，Docker 28.0.4 服务端为 `linux/x86_64`。隔离项目用真实 MySQL 8.4.13、Redis 8.10.3、27 项迁移和合成业务数据验证旧命名卷归档、空宿主目录恢复、只读 `public`、非 root 服务及 Artisan 迁移。三份归档均直接通过 GNU tar 数字属主和内容比较。重建保留 MySQL 行、Redis 键和 HTTP 附件，回退重新挂载三个原卷并核对旧数据及新写入隔离。运行回执保存于上述证据目录的 `native-42520350003/`。

## 生成应用 Docker Compose

从实际 `openrust new` 生成 minimal 和 Fortify Web，将应用及框架复制到含空格的新父目录后移开原目录。宿主 Cargo 解析搬迁后的 framework、工具及驱动补丁，BuildKit 在镜像内复现相对布局。两类项目均通过 Cargo check/test，Fortify 的登录及会话测试使用隔离 MySQL 8.4.13。

Docker Desktop Linux ARM64 实测 minimal 无锁文件构建和 Fortify Web 锁定构建。两个应用以 UID 10001 启动并通过 `/ready`、Artisan 命令及只读 `public` 验证。Fortify 执行 16 项迁移，真实 HTTP 无 CSRF 登录请求返回 419。重建保留应用存储、Redis 8.10.3 键和 Fortify 的 MySQL 记录。隔离夹具中无效的外部数据库和 Redis URL 未传入容器。应用与框架上下文均通过真实 BuildKit 探针，排除了合成密钥、嵌套 SQLite 和自定义数据目录。

`tmp/lead-class-lifetime-U5c8kV7e/next-compose-x64/` 的 `generated-relocation-live-02/`、`generated-relocation-live-04/` 保存搬迁、构建与重建回执，`generated-workspace-*.log` 保存 fmt、严格 Clippy、带隔离 MySQL 的工作区测试、Rustdoc 与依赖门禁证据。PostgreSQL 生成配置通过静态 Compose 解析与 Shell 语法检查。SQLite 和 PostgreSQL 的 Compose 运行及生成部署的原生 x86_64 仍需相应环境验收。

## 部署二进制按项目名限定验收

冻结树 `b3f456e4f2f21592b8bad33b7682cac9466dcc67`。根 Cargo 应用目标为 `app`，工具为 `openrust`。根及生成应用按发布项目名安装在 `bin/`。Artisan 在发布目录使用生成时确定的路径，源码模式继续从 Cargo 启动。命令以固定 Laravel v13.11.0 的包根 Artisan 为对照。部署路径和同名资源隔离是编译型应用的打包约定。

Linux ARM64 的 Buildx 和 Compose 实跑根、`blog`、`artisan`。镜像默认入口和导出产物同名，各自以真实 MySQL 8.4 和 Redis 完成迁移、就绪探针及 Artisan 命令。生成应用及框架源码迁到含空格的父目录后再构建。导出产物在含空格的挂载路径验证 Artisan、实际监听和 SIGTERM。原生 x86_64 的新布局留待发布矩阵。

默认工作区格式、严格 Clippy、测试、严格 Rustdoc 和离线依赖检查通过。真实 MySQL 8.4.13；工作区 2979 项，`blog` 33 项，`artisan` 33 项通过。进程测试覆盖 `artisan/public/storage/modules/bin` 五种冲突名、参数透传、搬迁及 127/126 错误。

## 迁移确认与路径编码限定验收

冻结树 `fff0dcfe1a4155d8af2144568ea366ed823ae9f5` 的实际根应用在精确 production 下拒绝无确认的迁移和回滚。无终端、管道肯定输入、非交互、终端否与空输入均未连接数据库。真实 MySQL 8.4.13 与 Redis 验证终端同意、显式强制、默认与命名库隔离、拒绝回滚无写入，以及缺历史和已有历史的 status 只读。根应用的真实迁移与 ARM64 Compose 使用生产源码相同的树 `1c6be577d65fc7463bc55afe365e6cc74ed75a9a`，最终差异只有 HTTP 测试断言。根和 fresh MySQL 生成 Compose 均实际以非 root 及显式授权迁移。SQLite 生成配置核对参数及空文件保护。

已发布目录的 Artisan 拒绝未完成标记。默认 HTTP 路径编码验证通过实际 TCP、路由、静态资源、fallback 和 CORS 前置测试。畸形百分号字面及单次合法解码保留。工作区格式、严格 Clippy、测试、Rustdoc、离线依赖、fresh minimal check/test 与可选翻译数据库的真实 MySQL 测试通过。

## Passport 资源令牌与撤销限定验收

冻结树 `2e8d45da5dc1f5ad87aa80edca992faa33615fba` 对照 Passport v13.10.0 的 `PassportServiceProvider` 与 League oauth2-server 9.4.1 的 `ResourceServer::validateAuthenticatedRequest` 和 `BearerTokenValidator`。公开入口 `openrust_passport::ResourceServer::validate_authenticated_request` 校验首个 Authorization 值、RS256 和现有时间声明，按 `jti` 读取应用同一主库撤销存储。成功后投影四个 `oauth_*` 属性。ID 和 scopes 使用明确的 Rust 类型，未提供的 subject 和 scopes 投影为显式 null，存储故障脱敏拒绝。

包内安全回归、根应用唯一示例和新生成 MySQL 应用经 MySQL 8.4.13 验证缺失记录返回 401、写入有效记录后放行以及撤销后恢复 401。根与生成应用仅配置 Passport 公钥，保持宿主所选认证、Cookie 和 Session。特性图证明 Passport 不启用 Core 的 auth 或 signing，标准生成应用保留 Session 迁移并精确追加四个 Passport 迁移。fresh minimal 无 Passport 依赖并通过 check/test。

本批工作区 fmt、严格 Clippy、全特性编译、workspace tests、严格 Rustdoc 和离线依赖门禁通过。TokenGuard 与用户解析、OAuth 签发路由及同意流程、OIDC Provider 和双应用 SSO 继续按 L090 与原队列验收。

## Artisan 与独立编译工具边界验收

冻结树 `01121f559af03fde07c68353b5462713641ccf4a` 对照固定 Laravel 应用 `06d016a` 的 Artisan 入口、框架 `d52c3b3` 的 `ArtisanServiceProvider` 和部署文档 `960eedf`。根、fresh minimal 和 fresh MySQL 的实际源码命令均由应用处理。发布包包装脚本与二进制的命令列表一致，源码 `build` 与 `help build` 按普通未登记命令退出。CLI 进程回归验证应用自用的同名命令仍可透传，包发现和开发入口分别保留。

独立 `openrust build` 的真实 Cargo 路径生成根和 fresh minimal 的 macOS ARM64 发布包。具名二进制在包根，含空格的输出路径可运行，Redis 下 HTTP 200、SIGTERM 退出 0。根应用另经 Buildx `--docker --platform linux/arm64 --env artisan-boundary` 导出 ELF，真实 MySQL 8.4.13 与 Redis 验证生产无确认拒绝、显式强制迁移、迁移状态、Artisan、HTTP 200 与 SIGTERM 退出 0。

工作区 fmt、严格 Clippy、真实 MySQL 8.4 workspace tests、严格 Rustdoc、离线依赖检查和两种生成应用 check/test 通过。当前验收范围是命令边界与编译打包。Laravel 的 `optimize`、`optimize:clear` 与 `reload` 及原生 x86_64 发布包在原矩阵独立验收。

## 本机 Cargo 与显式 Docker 发布验收

冻结树 `93aace7c5e1ab2cd0b15d5f751419b52f0b5423e`。编译打包是 OpenRust 的源码扩展，参考固定 Goravel a554ced 的编译入口和 a9e5a3f 的 Docker 独立流程。部署优化和长驻服务重载按固定 Laravel d52c3b3 与部署文档 960eedf 分别验收。

macOS ARM64 的根和新生成 minimal 通过源码 `./artisan build` 实际编译，PATH 中 Docker 调用会立即失败。Cargo 使用本机目标和私有最终产物目录。新生成应用由首次构建生成锁文件，包搬迁至含空格路径后可直接运行。两包与真实 Redis 一起验证 Artisan 包装脚本与直接入口、HTTP 200 和 SIGTERM 退出 0。已有目标保持原样，资源含 PEM 私钥夹具时没有发布目录。小型 Cargo 回归另验证现有锁文件保留、明确 feature、同包默认二进制及编译失败无产物。

显式 Docker 的 Linux ARM64 路径在根和新生成 MySQL 应用验收。两包均与真实 MySQL 8.4 和 Redis 验证无确认的生产迁移、显式强制迁移、迁移重跑、Artisan、HTTP 与 SIGTERM。工作区格式、严格 Clippy、测试、Rustdoc、离线依赖及新生成 minimal、MySQL 的 check/test 通过。Linux 宿主 Cargo 路径、原生 x86_64 和其他平台仍按发布矩阵验收。

## Artisan Linux 发布目录限定验收

冻结树 `4268d0f414bbfa56b3e8e4a58f7180a2bafdd5e0` 对照固定 Laravel 的源码 Artisan 入口与固定 Goravel 的构建工作流。根应用和搬迁至含空格路径的 fresh minimal 均实际从源码运行 `./artisan build`，使用同一 Compose/Buildx artifacts 目标并导出各自具名的单一 ELF64。根走默认目录，minimal 走含空格、逗号及引号的显式路径。保留应用资源与空可写目录。

Linux ARM64 的镜像和独立目录均在真实服务拓扑下运行。根使用 MySQL 8.4.13 与 Redis，生成 minimal 使用 Redis。验证迁移、语言资源 HTTP 读取、独立 Artisan 与 SIGTERM。资源私钥导致的构建失败及已有目标保护均经过负例，未输出子进程私密值。默认工作区 fmt、严格 Clippy、测试、Rustdoc、离线依赖检查和 fresh minimal check/test 通过。原生 x86_64 的该入口仍待发布矩阵。

## 模块仓储具名绑定验收

冻结树 `4f126f48207f7952014a4477749207026fa74e1c` 对照固定 nWidart 5971484 的 Provider 仓储单例与共享实例测试。已编译清单和运行发现均在注册阶段登记 `modules`，同名键与类型键解析到同一实例。模块未安装时返回未绑定，重绑定和清除实例沿用唯一容器。唯一示例从注册阶段的具名入口读取实际模块仓储。

工作区格式、严格 Clippy、测试、Rustdoc 与依赖门禁通过。测试使用真实 MySQL 8.4，根示例及两种注册路径均实际执行断言。实际生成 minimal 并通过 check/test，包含默认宿主的同实例断言。路径配置与其他模块 Provider 行为按各自合同验收。

## JSON 路径存在与精确写入验收

冻结树 `9e8c3fa5515c78651f3bae7d61a7ccb81392a6f4`。对照固定 Laravel d52c3b3 的 Builder、MySQL、PostgreSQL 与 SQLite Grammar。查询和嵌套条件组覆盖四个入口、JSON null、SQL NULL、分组、负下标、限定列与无新增绑定。MySQL 和 PostgreSQL 要求箭头路径，SQLite 裸列检查根 JSON。PostgreSQL 末段文本沿原生键存在语义。

真实 MySQL 8.4 验证示例按预设键隔离账户与分页，以及生成项目的 JSON 空成员、缺失和 SQL NULL、正反分组、参数顺序、精确更新与删除。工作区格式、严格 Clippy、测试、全特性编译、Rustdoc、离线依赖及新生成 minimal、MySQL 的 check/test 通过。PostgreSQL、SQLite 生成测试目标已编译，SQLite 安装迁移拦截。其实际 SQL 执行、SQLite 新条件的精确写入和最低版本矩阵继续保留。SQL Server 等尚无对应驱动。

## 原始 WHERE 参数与方言词法验收

冻结树 `d7d30533bed3a5970c6ebd25718ee7164a9035c0`。固定 Laravel d52c3b3 的 Builder::whereRaw 与 Grammar 按 WHERE 顺序保留参数。OpenRust 的 where_raw 和 select_raw 使用同一词法及绑定校验，读取、更新、删除共享原条件树。参数缺失、多余或混用原生占位符均返回错误。PostgreSQL 普通字符串引号边界不确定时拒绝，显式 E 续行可用。MySQL 和 SQLite 的行注释沿各自词法结束。

真实 MySQL 8.4.13 验证限定账户查询、注释中的问号、事务内精确 UPDATE/DELETE 与回滚，并验证唯一示例和生成应用的关联过滤以及 Fortify Web。workspace 格式、默认并行测试、严格 Clippy、全特性编译、Rustdoc、依赖门禁和生成检查通过。PostgreSQL 双会话真实 oracle 与 SQLite 实库留发布矩阵。

PostgreSQL 专跑配置真实隔离库的 `OPENRUST_TEST_DATABASE_URL` 后执行：

```sh
cargo test -p openrust-framework --features db-postgres --lib \
  database::query::where_raw_tests::real_postgres_raw_where_same_binding_mode_oracle \
  -- --ignored --exact --nocapture
```

## 原始 WHERE 布尔优先级与嵌套验收

冻结树 `7a0fcb3803810c6417cf0c6e8f95c0cb4a40aa1c` 对照固定 Laravel d52c3b3 的 Builder::whereRaw、orWhereRaw、whereNested、Grammar 与 DatabaseQueryBuilderTest。顶层保留原始 SQL 的布尔优先级和绑定顺序，显式嵌套组限定范围。原始片段末尾行注释用换行隔开后续条件或右括号。查询、更新和删除使用同一条件树。

真实 MySQL 8.4 验证精确读写、事务回滚、唯一示例的导入时间线 HTTP 账户隔离，以及新生成 MySQL 应用的复合条件和显式分组。格式、严格 Clippy、workspace tests、全特性及 SQLite 根目标编译、严格 Rustdoc、离线依赖检查、新生成 minimal 和 MySQL 的 check/test 通过。新生成 PostgreSQL 与 SQLite 的测试目标通过编译，实库留在发布矩阵。

## JSON 包含路径与布尔组验收

冻结树 `44d3515298e937fc5bd414af5b4de4668252c462` 对应固定 Laravel d52c3b3 的四个 JSON 包含方法、MySQL/PostgreSQL/SQLite Grammar 与查询构造测试。三后端 AST 验证限定列、箭头路径和索引、取反范围、参数顺序与分组。Rust 把路径中的 ` as ` 当作 JSON 键数据，避免把条件路径解释成列别名。

真实 MySQL 8.4 的唯一示例验证标签筛选、跨账户隔离及分页总数。新生成 MySQL 项目覆盖路径和特殊字符、缺失路径、SQL NULL、OR/NOT 及精确 UPDATE/DELETE 目标。workspace 格式、严格 Clippy、测试、Rustdoc、依赖、全特性全目标编译与生成项目 check/test 通过。SQLite 生成入口只编译，PostgreSQL 和 SQLite 实库仍按发布矩阵执行。

## SQLite 简单列 JSON 包含日常验收

冻结树 `b04a57b3a5de527d701495df017aa1094e33eca4` 对应固定 Laravel d52c3b3 的 SQLiteGrammar 和 PDO 绑定。简单列通过 `json_each` 与 `IS` 传入一枚标量绑定，结构化值在 SQLite 拒绝。AST 断言覆盖绑定顺序、原 MySQL/PG 表达式、空值、布尔、整数及 PHP 浮点文本。根示例用同一查询入口返回字段验证错误，生成模板保留发布专项。

默认工作区格式、严格 Clippy、测试、Rustdoc、依赖门禁和全特性全目标编译通过。真实 MySQL 8.4 下示例记录和新生成 MySQL 应用的包含回归实际运行。普通生成项目 check/test 通过。SQLite 根目标与 SQLite 生成项目测试目标编译通过。生成探针拦截安装迁移，未执行 SQLite 实库测试。SQLite 生成项目既有 query_insert_ids 的 encryption 特性检查仍产生两条警告。SQLite 当前版与最低版 JSON1 的实际运行留待[发布矩阵](#query-json-包含与-sqlite-写目标的发布运行入口)。

## 发布二进制 Artisan 与生产迁移验收

冻结树 `20b1b132149a7e5a19883c7b93ffe11974802afe`。固定 Laravel 的 Console Kernel 提供同一命令生命周期，固定 Goravel 的编译型入口提供显式 Artisan 前缀参考。根应用与新生成的 MySQL 应用均从该树执行源码命令列表、两种构建帮助与 Buildx 导出。产物在 Linux ARM64 实际运行，包根有唯一具名 ELF、Artisan、公开与运行资源和空可写目录。

两包在无 Cargo 的运行环境中比较了直接二进制与包装脚本的列表和帮助。真实 MySQL 8.4 验证无确认的生产迁移退出且空库不变，显式强制迁移写入历史，重跑保持历史。无参数直接启动通过 HTTP，收到 SIGTERM 后退出 0。工作区格式、严格 Clippy、测试、Rustdoc、依赖、全特性编译与新生成项目检查通过。同树的并行测试曾在既有测试库文件锁用例间歇失败，独立锁组通过，工作区完整复跑采用串行测试线程。原生 x86_64 与无 Docker 的一键编译留在发布矩阵。

## 源码 Artisan 构建命令验收

冻结树 `7d550d2fa7812b87439448c75b539b915f99b3ee`。根应用和生成应用的源码命令表通过 Provider 登记唯一的 `build`。命令与帮助复用同一定义，普通应用和模块命令仍来自正常 Provider。开发入口优先执行独立构建工具，使跨平台导出不依赖宿主应用编译和运行时服务。运行时同名普通命令不能替换源码工具保留项。

Linux ARM64 实际验证根应用和搬迁后的新生成应用在运行时 MySQL 不可达时使用 Buildx 导出。两种帮助同输出，前置和后置 `--env` 选择构建插值文件，环境变量可覆盖文件，无效选项退出 2，发布包的真实命令表不包含源码命令。工作区格式、严格 Clippy、测试、Rustdoc、依赖门禁及源码特性和 fresh minimal 校验通过。原生 x86_64 仍待部署矩阵验收。

## 发布包根入口限定验收

冻结树 `16d4562eecf85e352292b8c17c82f6b81288b3a3` 的根与 fresh minimal 发布包均在包根安装唯一项目同名 ELF。参照固定 Goravel 的具名编译产物与外置资源，保留 Laravel 的公开目录与可写 storage/bootstrap/cache。Docker runtime、导出目录和 Artisan 使用同一相对入口，源码生成对保留名仍可用。

Linux ARM64 在实际根和搬迁后的新生成应用执行 Buildx、镜像启动及独立包直接启动。真实 MySQL 8.4 与 Redis 证实根迁移，两个包均验证 Artisan、翻译 HTTP 与 SIGTERM。保留名拒绝创建目标，已有目标保持原样，资产复制失败时不会发布可执行 ELF。工作区 fmt、严格 Clippy、测试、Rustdoc、离线依赖及 fresh minimal check/test 通过。原生 x86_64 及源码命令目录另行验收。

## 开发态两阶段重载

macOS ARM64 使用 Watchexec 2.8.0、隔离 MySQL 8.4.13 与 Redis 8，通过根应用和 `openrust new --minimal` 生成应用的真实 HTTP 验收。根应用完成实际迁移，编译失败期间原进程持续响应，修复后收到 SIGTERM 并切换。选定的 `.env` 变更触发重载，其他环境文件和运行目录写入被忽略。

生成应用在框架路径依赖连续保存八次后只构建一次，真实 `/health` 返回新代码结果。在下一次 Cargo 构建尚未完成时，旧应用持续响应。终止监督进程后，应用、Watchexec、Cargo、构建脚本均已退出，监听端口已释放。进程组专项测试覆盖组首进程先退出且后代忽略 SIGTERM 的情形。

冻结源码摘要和实际运行回执位于 `tmp/dev-watch-20261109/live-03/`。工作区格式、严格 Clippy、带隔离 MySQL 的测试、Rustdoc、依赖检查以及生成项目的 `cargo check --locked` 和 `cargo test --locked` 均通过。使用自定义 Cargo runner 或依赖 Cargo 注入动态库路径的应用可使用单次运行入口 `openrust dev`。Linux 原生运行留待对应宿主验收。

## 标准依赖与队列开箱限定验收

范围为标准根应用与 `new` 的 MySQL/Redis 依赖、`queue.default`/原始具名连接、实际使用时构造 driver、显式 worker、同一模块 Jobs 与生成迁移目录；固定上游 framework `d52c3b3` 的 `config/queue.php`、`QueueManager::connection/resolve`、`QueueServiceProvider`，不把现有独立队列扩展等同完整 Laravel Queue。

| 验证范围 | 本批真实证据与结果 |
| --- | --- |
| 冻结主门禁 | `tmp/standard-queue-20261102/attempt5/results.json` 的12项全部exit0、日志SHA匹配：fmt，默认/全特性workspace严格Clippy、tests、warnings-denied docs，无默认驱动cache-redis+queue严格Clippy、no-default lib tests、PG-only db tests、Rust1.97.0 check、cargo deny。ignored/无服务环境的提前返回不计真实服务。 |
| 受验输入映射 | 692文件冻结；其后仅Redis测试四处资源后assert改fallible且条件全保留、品牌文字/包description、独立SQLite consumer显式opt-out。`post-cleanup-inputs.json`、`branding-only-delta.json`、`sqlite-consumer-opt-out-delta.json`、`final-inputs.json`保存映射。资源清理测试有默认/全特性定向tests/严格Clippy补验；品牌及最终错误时点说明有全特性warnings-denied docs补验。 |
| 真实MySQL8.4/PG18/Redis8 | 双库各1项Redis调度事务拒绝，均检查无任务/slot残留；无默认库模块HTTP→Redis→同容器handler→ACK 1项。根标准构建真实TCP与独立CLI worker分别走DB/Redis，serve不暗启worker、config:cache固定database、clear后切换Redis；四进程正常exit0、端口释放、任务库与Redis自有键无遗留。见`real-*.json`、`root-live-attempt2-receipt.json`。 |
| 四种fresh应用 | minimal check/test/build通过，12项为9 stock＋3公开consumer，并生成普通/UUID模型；standard和PG各check/test通过，迁移目录1＋test_app11（包含真实DB与无默认库Redis两个任务fixture），无ignored；默认MySQL auth+MFA+encryption check/test通过，认证2＋迁移目录1。见`fresh-*-check/test*.json/log`；runner检查服务测试未跳过。 |
| SQLite与缓存重启 | 外部SQLite保留原cache-redis闭包、显式default-features=false；`--locked` check及真实事务run通过，lock未改，未扩大内置后端承诺。minimal生成二进制实际缓存/清缓存重启、缓存隔离坏dotenv通过，二进制SHA及命令在`fresh-minimal-cache-restart.json`。 |
| 独审与保全 | 不同模型最终审查`/tmp/openrust-standard-queue-final-evidence-review.md`核日志SHA、源码差异、断言/清理和真实调用链；341份生成/运行源码与lock归档`generated-and-runtime-sources.tar.gz`，排除target、部署env与bootstrap/cache；逐文件SHA在`source-archive-manifest.json`。旧失败、诊断和每次回执均保留。 |

初轮fresh fixture只有console环境而缺代码app.env=testing导致Mailer拒绝，修复临时fixture显式覆盖后两个真实用例及完整standard tests通过；不改生产安全限制。首轮TCP fixture缓存后换探测端口而失败，生产正确固定server.port，修runner后通过。SQLite初次locked check失败由新默认依赖暴露独立consumer未opt-out，显式修manifest后通过；不得把这些原失败回填exit0。

默认名称格式/存在仍在启动验证，QueueServiceProvider尚未具备完整DeferrableProvider机制，与固定上游默认错误时点不等价；此确认缺口归既有R2/R10。fresh任务fixture使用公开单次worker入口，独立CLI进程/SIGTERM证据来自根应用，分别取证。完整after_commit、Bus/Chain/Batch、worker参数/默认值、全部官方driver、生产多节点及最低服务版本矩阵仍开放，不能据本簇关闭R10或ADR-001。资源最终收尾见同目录`final-target-cleanup.json`；只清确认归属、无占用可再生产物，源代码/证据/共享服务保留。提交事实以Git为准。

## 共享命令所有权前置限定验收

本簇承接固定framework `d52c3b3` 的ProviderRepository/Application延迟生命周期缺口，先解除命令目录跨register/boot/runtime的独占借用。启动及运行期共用CommandRegistry；App/ProviderRuntime返回`Arc<dyn Command>`快照并用Rust精确捕获`use<>`解除宿主借用。同名按当前`name()`覆盖原槽，名称回调在锁外，版本变化重扫；旧快照继续持有原实例，替换/快照析构不持锁。匹配、help、scope、实际dyn Command类型检查保持同一执行器。

| 范围 | 当前真实证据 |
| --- | --- |
| 固定输入与门禁 | `tmp/provider-shared-runtime-20261102/attempt1/inputs.json`的693输入，12项实际exit0且日志SHA匹配：fmt、默认/全特性workspace严格Clippy/tests/warnings-denied docs、精简queue+cache-redis严格Clippy、no-default lib、PG限定db lib、Rust1.97.0 check、cargo deny。无服务环境下skip/ignored不计真实服务。 |
| 公共行为回归 | 目录6项覆盖共享/保槽、旧快照、Drop重入、动态改名两方向及name重入版本漂移；App测试保存整个旧迭代器后force Provider，再执行旧实例及当前命令。原最长匹配/scope/取消/MFA命令身份断言保留。两处旧UFCS map调用同步迁移，独审问题修复后复审关闭。 |
| 唯一模块真实业务 | `module-maintenance-snapshot.json/log`：MySQL真实隔离库1项实际通过，模块附件提交后取消→重建App→旧/新命令快照实例分离→实际维护命令清理持久意图→幂等重执行。0 ignored，数据库集合无增删。 |
| 构建与fresh | `root-cli-build-resume.json`标准server/CLI实际构建；新minimal check＋9 stock tests、MySQL auth+MFA+encryption check＋认证2/迁移目录1，以及minimal实际`cargo run -- help`均exit0。对应provider-*/fresh-minimal-help回执日志SHA已核；MFA runner检查两真实认证测试未skip。 |
| 独审及源码保全 | 不同模型最终报告`/tmp/openrust-shared-command-final-review.md`无剩余代码阻断；归档113份生成源码/lock至`generated-sources.tar.gz`，逐文件SHA在generated-source-manifest.json；12主日志及11服务/生成回执SHA在final-integrity.json。 |

用户要求换账号时，正式原Goal paused，自有driver进程组SIGINT并退出130；Cargo退出码未捕获，不伪填130。旧root-cli-build.log及用户暂停WIP归档保留。用户随后明确恢复，并通过宿主恢复原Goal active；正式工具仍识别旧账号，沿原80%账本，不rebase/credits授权。services_resume.py只续未完构建/生成，不重跑无漂移的12主门禁和已通过模块测试，原Goal与完整目标不变。

主矩阵后至fresh前已确认无占用并正常Cargo clean：30,940,479,488B→0，二进制/源码/证据保留；最终再核无占用并正常Cargo clean：9,414,590,464B→0，磁盘可用161,398,218,752B；回执见同目录final-target-cleanup.json。模板没有直接受影响的借用命令调用，仍实际新生成check/test。SQL未变，MySQL覆盖受影响的模块维护链；PG限定lib不冒充PG真实服务复验。完整deferred的统一Resolver加载、when、manifest缓存及boot/失败/取消时序仍开放，本簇不关闭R2/R10/ADR-001。当前命令API是必要所有权前置，不以此声称完整Laravel Console/Artisan等价。

## SQLite 数据库 Session 对应限定验收

- 固定来源：framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`、docs `960eedf0911155ad16326ade49c0ea5d18eda0b8`；文件 SHA、PHP8.5 mbstring 12 案 oracle、冻结输入与全部失败记录在 `tmp/sqlite-s3-20261103/`。
- 冻结20最终主组合：fmt、全feature全target `-D warnings` Clippy、workspace tests、docs均exit0，2193 passed / 0 failed / 122 ignored；冻结18默认严格Clippy/tests/docs按无行为变化范围复用通过，1333 passed / 0 failed / 40 ignored。缺环境返回的skip和ignored不计真实服务。19仅Cookie精简feature表达式等价整改，20仅fixture按view feature局部mut作用域；三种精简feature严格Clippy、Rust1.97与deny均通过，默认1333和全部聚合数均含子harness，不是唯一行为数。
- 当前真实服务23项通过：MySQL8.4.13共12、PostgreSQL18.6共11；涵盖原生 handler、MySQL SQLSTATE/错误号、增强精度/撤销/GC/索引与并发、模块历史/old input/迁移、TCP 浏览器握手/持久截止/关闭、实际Chromium和默认原生 Auth HTTP。分项命令/退出码/实际执行判定/SHA见 `real-services-summary.json`；自有数据库增删差异0，曾因panic留下的单个自有库已核0连接后清理并保全回执。
- 实跑修复：基础设施错误由默认400改为500；SQLite跨连接旧schema在CREATE prepare时假报已有索引，改同physical先step schema读取再prepare DDL，保留重复/异列冲突断言；浏览器Session认证路由补cookies/session公开链并按能力启用。临时安全探针已移除，严格状态检查改Result以保证失败清理。
- 冻结18原生索引名称与GC up/down、三码/500断言和模块迁移在双库四项补验通过。最低SQLite3.26冻结20原生完整common contract与同引擎身份用例各1通过，另以DYLD追踪确认实际加载固定3.26.0 dylib且SHA一致；源16最低引擎74项lib Session、2项增强Store和4项Provider HTTP只按未变化子范围保留，不冒称全量当前重跑。不关闭完整Session、Guard解析、SQLite Auth/缓存锁/队列或完整Schema。
- 当前CLI源20生成四类fresh：minimal9、SQLite11、MySQL-MFA3、PostgreSQL-auth3项实际通过，四类check/test共12条new/check/test命令均0，无ignored；双库认证没有skip，数据库集合0增0删。SQLite附加用例实际链接生成config，验证自定义表/原生往返/官方索引；MFA步骤内嵌认证流程，数量不与根服务相加。
- 不同模型最终核700输入无漂移、成功日志SHA/数量/实际执行和复用边界，限定范围无blocker。333链接无缺失、mdBook实际build0。四类229份生成文件逐字节校验归档SHA `de86e1d02fb8c3a0579f110d4ba4342c530be9fa0ed7911be4ccbc418d23f35d` 后正常移除；main target40,236,716,032B、最低1,027,358,720B与生成3,637,743,616B均正常clean至0，二进制/HTML回收，磁盘可用157,301,743,616B。源码/WIP、依赖/native oracle、全部服务和原始失败证据保留；完整回执/审查在上述证据根，本地提交以Git为准。

## SQLite 配置与迁移限定验收

状态：S2限定源码、最后能力前置默认组合、文档与资源收尾已完成适用验收；本地提交以Git为准，不是完整SQLite或ADR-001交付。owner=root，main基线20cea6c，无新worktree，原Goal及27/5.4预算保留。证据根为 `tmp/sqlite-builtin-20261102/s2/`。

- 固定框架d52c3b3的SQLiteConnector/SQLiteGrammar/Migrator/Env及固定docs960eedf为来源；nullable外键、原生标量PRAGMA顺序/首条语句与错误边界经同C引擎SQLx/rusqlite和固定PHP8.5参考核对。PDO/reference-2为实际错误处理器下18项，Env为16项源码闭包/布尔转换，不冒称完整Laravel应用验收。
- frozen-13的fmt/全feature严格Clippy、SQLite定向及所有者12项实际通过；`main-gates-3/results.json`的13项默认/全feature/精简/MSRV1.97/doc/deny门禁均0。新增同物理handle析构见证代替num_idle；真实perform在归还后让另一任务借走唯一连接，owner仍正常结束、池开放、数据/历史保留。name collision、拒绝归还、有限复用、callback析构分别有真实SQLx测试。
- 最低引擎官方3.26.0归档SHA3与source_id已核，实际连接默认FK0。初次Provider失败定位到官方迁移crate传递启用RETURNING；固定2.0.4原样源码仅manifest禁default，workspace显式保留其余此前有效features，vendor来源/许可/文件SHA齐全。`minimum-3.26-gates-3`库46过/1 ignored子进程入口、所有者12、Root/真实清单3、单独同引擎身份1均通过；父锁测试实际执行子入口两次。生成列专项需3.31，不纳入3.26，当前3.51.3矩阵保留全部该断言。loader追踪塞满子进程pipe的前次超时原样保留，追踪拆为独立不派生子进程的身份用例，原锁断言未改未跳。
- 内核只确认既有FrameworkMigrator与example已登记清单、默认SQLite非事务执行、显式事务失败await rollback/异常关闭及取消所有者。main普通表删列重建保留约束/索引/触发器/行标识/sequence，main/TEMP隔离、表名大小写及不可访问rowid保护有真实证据，不包括完整Schema/attached/virtual/native JSON能力。
- `real-services/results.json`为MySQL8.4/PostgreSQL18各5个真实迁移/模块案例，数据库前后集合均无增删。后继变化为SQLite专属分支及原样迁移源码的feature来源修正，其他ORM有效features显式保持；独审按差异限定复用，不当作新全矩阵。
- `fresh-gates`真实new --db sqlite自动迁移、check/test通过；生成实际config::boot/ConfigLoader的9个隔离子进程核dotenv/process/namespace优先级、最终nullable字段和真实Provider PRAGMA。矩阵父测试和无参子入口分别计证，不把空入口算业务测试。原模板迁移清单及TestApp用例也通过；真实生成二进制/api为200、SIGINT退出0、端口释放。Session/认证/worker业务并未因此获验。
- `native`为真实根二进制migrate up/status/down/up、配置缓存null/namespace最终选择、唯一模块named reporting HTTP200及正常退出/端口释放；config缓存不等于Root池PRAGMA逐值矩阵，两者分列。
- 最后非内置feature能力前置的 `post-consumer-gates` fmt、默认严格Clippy、完整workspace测试与最新全feature文档4项均0；不同模型final-review核源差异允许上述作用域复用。mdBook构建0，当前改动本地链接检查无缺失，不等于全仓未实现章节完成。
- 12个独立consumer更新lock并metadata/check均0，都是单一本地迁移来源、无RETURNING；gRPC check有两处未构造fixture struct警告，不冒称12个consumer strictClippy。外部SQLite实际run首轮暴露错误顺序，后置仅cfg(not db-sqlite)能力前置并同步consumer断言，run-2已0；连接/事务/查询所有原断言保留。该改动在SQLite-enabled构建完全排除，独审允许按原范围复用min/fresh/native/allfeatures，默认新分支另验。
- TestDatabase显式close未poll/失败/取消保全目录，全部URL借用者先停/禁新借用是安全清理前提，不是外部连接跟踪器。跨进程sidecar取得全部锁后才删自有文件，未知/链接/占用失败保全；普通迁移不unlink sidecar。hardlink拒绝对应SQLite journal/WAL上游边界，不称Rust限制。
- 开放缺口：Root+Auth仍明确拒绝SQLite；S3认证/Session/DB缓存锁/队列调度业务SQL、完整Migrator/fresh与Schema、构造/Drop/驱动fault全矩阵、Windows原生、mount别名/替换路径TOCTOU均未完成。见证失败后的标关闭不撤销在途acquire，故障与禁重连不原子；不宣称已防全部静默重连或把此问题归为必要Rust适配。
- 资源：minimum target 1,768,640,512B→0，主target50,282,713,088B→0均有占用/源码/回执保护记录；仅保留实际后续native/fresh用的二进制及一份重建cache。最终61份生成文件逐项SHA核对/归档后移除，重建Cargo target5,565,292,544B→0；native库文件归档，敏感配置cache仅保选择值/摘要后清理，二进制/HTML再生产物回收。磁盘可用158,987,571,200B，记录final-resource-retirement.json；保留依赖源码/native oracle供既定S3，服务未停止。

## SQLite 连接基础限定验收

- 固定framework d52c3b3129e4c8e213c97cdc68f94b0b7d27f181、docs960eedf0911155ad16326ade49c0ea5d18eda0b8及PDO默认timeout为来源。db-sqlite/标准Provider、平铺配置、同仓储application_root、文件存在/URI/内存池与逐物理PRAGMA、owned TestDatabase、根模板及唯一模块reporting真实调用共同验收。
- `tmp/sqlite-builtin-20261102/s1/gates-final/results.json`主10门禁全部exit0：默认fmt/严格Clippy/workspace测试/文档，全特性严格Clippy/测试/文档、无默认驱动严格Clippy/测试、1.97.0+SQLite检查。deny四项通过；ignored不算真实服务。原失败日志完整保留。
- 最后SQLite-only导出补丁只影响此前未编译的单driver路径；默认/全特性已含PG/MySQL，编译模块与可达运行语句保持等价。setup显式Result<()>、cfg mut、同值错误工厂及测试fixture条件shadow没有删断言。主矩阵/MySQL/native按独审与输入映射复用，单driver由`clippy-sqlite-only-attempt4.log`严格workspace/all-targets、`fresh-retry-results.json`重新check/test/doc验证；生成9项原应用＋2项公开SQLite测试，无ignored。
- 实际MySQL8.4双物理连接UTC、同名库保护及owned迁移失败清理各1通过，库集合0增0减。`native-attempt2.json`根server具名SQLite HTTP200、正常退出与端口释放。首轮native fixture未启用TOML而404、首轮fresh缺SQLite-only公开导出等失败均保留，不回填通过。
- SQLite真实内存/文件验证每物理FK/busy60000、misc/special覆盖、原生日志默认、URI保留字符/原生psow/只读immutable、内存隔离与共享生命周期、事务/savepoint、owned显式关闭。`sqlite-engine.json`为实际3.51.3及编译选项，不等于最低3.26已验；未启用RETURNING版本抬升feature。生成源码及锁文件摘要保全。
- 9页mdBook与受影响文档本地链接检查通过。主矩阵target确认归属/无占用后正常cargo clean：38,868,230,144B→0，磁盘可用159,935,385,600B；最后生成target4,905,627,648B与book2,289,664B均确认无占用并正常清零，磁盘可用159,942,160,384B；55份生成源码/锁已归档，源码/WIP/依赖源码/服务/证据保持。
- 范围不含标准迁移互斥/全部既有迁移/SQLite安装器/既有认证Session队列调度SQL。复杂PRAGMA名称/数值级别、nullable外键、完整取消/Drop故障清理及最低版本仍为唯一todo确认缺口，不称必要Rust适配或完整SQLite/Laravel。SQLx要求UTF8路径时明确拒绝，不有损转换。

## 插入标识返回与事务执行限定验收

DB-I01 对应提交 `960aca43f11191cc9a68d1e03215b91be72fdeab`、树 `981451cdcb17f805591aceb440248aa0985b797d`。独立工作区精确输入核验后完成适用 13 门禁及 v2 fmt/严格 Clippy、真实 MySQL 3 项和 PostgreSQL 3 项、SQLite/MySQL-auth/PostgreSQL fresh new/check/test/metadata、实际 SQLite 3.26 的 7 项及 12 consumers locked metadata 与受影响编译；无跳过冒充通过。干净提交树 1508 blobs 与输入逐字节匹配，补 workspace/all-target/all-feature locked check 与 7 项定向测试后复用不变证据。

原始回执 `tmp/independent-testing-20261106/DB-I01-RESULT.json`、`DB-I01-CLEAN-TREE-RESULT.json`，推送核验 `tmp/push-20261106/db-i01-push-receipt.json`。旧 PostgreSQL SQL 文本断言失败及测试环境 pkg-config 失败保留，修正范围与后续通过分开。仅覆盖插入标识返回闭环，不关闭完整 Query/Eloquent/SQLite、PDO 会话日期格式或其他在途簇。

## 应用环境命令限定验收

CLI-E01 提交 `680908bdd7eb1d136d28196cfadb36830c29ffe7`、精确树 `a900f91b4880c8278c1a6a4d464bd315d9b45ae9`，基线为 DB-I01。六个交付文件与独立组合输入完全一致；13 门禁、env 2 项、Provider 生命周期 1 项、根/fresh 实际命令 16 次及 dotenv/缓存/转发证据见 `tmp/independent-testing-20261106/CLI-E01-RESULT.json`。

干净树 1510 blobs 核验后补 locked metadata、workspace/all-target/all-feature check 及上述 3 项定向测试，前后 SHA 不变，见 `CLI-E01-CLEAN-TREE-RESULT.json`。Windows commit.cmd 既有 CRLF checkout 转换单列，不影响编译输入。原文档 Clippy 失败保留；源码只修该注释后重新通过。不代表完整 Artisan、ANSI 组件或 Symfony 解析器已完成。

## SQLite 同机租约方言限定验收

SQ-L01 提交 `5c26fd0420345cc4756ddadab4f697a0ec65c83b`、树 `df4df11393d1e21872b597b8f092e85eef6b1020` 已推送并核对远端。1530 项组合输入绑定 15 项门禁、无 auth 的 7 项 SQLite 叶测试（父用例实际派生子进程）、2 项真实 article_transfer 生产链回归；MySQL／PostgreSQL 各 1 项原协调回归无跳过且测试库恢复。

最低 SQLite 3.26 的旧手工库缺少 HAVE_USLEEP，busy_timeout=500ms 因秒级 fallback 立即 BUSY；同原源码、256 个 ABI、修正 POSIX 构建后，原严格取消／竞争断言通过。纯 v2 叶文件曾因旧 mtime 复用诊断二进制，该行明确废弃；刷新 mtime、compiler-artifact fresh=false、无诊断标记后重新运行原 7 项通过，最低版真实生产示例 2 项通过。失败、诊断、native 来源与复编证明全部保留，不将其写作框架生产锁修复。

纯提交树 1516 blobs 与 9 个受验运行文件逐字节匹配，CLI 去除 Model factory WIP；补 workspace/all-target/all-feature check、上述 7+2，以及 clean CLI 实际生成 SQLite／MySQL-auth 的 new/check/test/metadata。PostgreSQL 仅生成及 metadata 核专属 fixture 排除，未冒称全 PG 生成测试。回执 `tmp/independent-testing-20261106/SQ-L01-RESULT.json`、`SQ-L01-CLEAN-TREE-RESULT.json`；推送 `tmp/push-20261106/sq-l01-push-receipt.json`。

边界：既有 SharedLocks 的 SQLite 同机租约方言；不代表 Laravel CacheLock、SQLite Queue／幂等、多主机文件锁或完整 SQLite 已完成。构建/生成产物按三次真实资源回执回收，仅保下一已安排步骤所需依赖缓存；主目录 target 保持不存在。

## API 前缀与官方案例限定验收

RT-A01 提交 `d9267695f9b5953cac518c5dde4760ef87188e21`、树 `4d8c77b1d936e38d5e565e07a0a021fa1bfaddb4` 已推送并核对远端。默认 `api`、官方 `api/admin`、前后与重复斜线、空前缀及根斜线共六组，根真实TCP与生成应用各六组通过；单次挂载、错误分类与API无Cookie边界同步验证。17项适用门禁及文档字面配置编译通过，生成应用完成new/check/test/metadata/build。

当前 main 纯组合1516项输入绑定精确树；workspace全目标全feature check、CLI全部测试、真实启动prefix回归，以及组合CLI实际生成与六组TCP通过。1515项与提交blob逐字节一致；唯一非运行输入 `commit.cmd` 按已有CRLF检出约定，只转换行尾后内容相同。

原文档缺字段及旧CLI快照失败保留；fresh初始脚本把显式文本fallback误当JSON，按生成路由及既有精确文本断言纠正后续验收，产品未改。回执 `tmp/independent-testing-20261106/RT-A01-RESULT.json`、`RT-A01-CLEAN-TREE-RESULT.json`；边界仅API前缀默认值、归一化与宿主分类，不关闭完整Routing、Sanctum或AR-P01布局。

## 应用错误国际化限定验收

I18N-E01 提交 `1dff30295a2dede862632414b10c4501a84acf05`、树 `a50725fe410d730a18267dfcdd2c05007f020565` 已推送并核远端。JSON、HTML及应用封装复用同一Provider目录，显式HTTP安全详情保留；5xx用户消息与诊断隔离。核心13项、唯一模块真实TCP5项、适用workspace/feature/strictdocs/MSRV/依赖门禁按版本输入映射通过；两块文档配置字面编译通过。

当前main精确1519项组合另过全目标全feature check、核心13/真实示例5/CLI全部测试与实际构建，SQLite与MySQL-auth生成应用各new/check/完整test/metadata通过，每个profile新增两项明确执行。公开 `App::exception_handler()` 读取实际应用Handler，核心与生成均不用私有registry或另建默认Handler。原SQ SQLite租约生成回归也实际执行；测试库清单恢复。

旧lint、HTTP详情覆盖、配置并发与生成Handler获取失败均保留。配置合并测试的全局环境读取风险已用子进程QUEUE_DRIVER受控负例复现，显式空snapshot后同污染场景及正常并发全feature lib1609项通过；不声称历史失败的唯一变量已知。初始0项过滤/误选macros诊断不计通过。回执 `tmp/independent-testing-20261106/I18N-E01-RESULT.json`、`I18N-E01-CLEAN-TREE-RESULT.json`；不关闭数据库翻译来源、完整Loader/Localization。

## 模型属性与认证前置限定验收

MB-A01 提交 `06b4d50c6c89001a2db0adee2ff54f13d78bffb8`，准确树 `f8e5800fe8721cbbc83030f754ee799481981a04`，已核origin/main。固定Laravel `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181` 的Model/HasAttributes/Authenticatable与DatabaseUserProvider为依据；GenericUser的literal原始属性与模型有效属性分开。源码51文件、规范输入1535项；唯一物理差异是非构建Windows提交脚本的既有CRLF/LF规则，所有构建源码/模板及执行位精确核验。

| 子范围 | 限定证据 |
| --- | --- |
| 原始行/属性/修改 | MySQL8.4环境Eloquent组76（含通用单元与实际raw行专项）、数据库认证8；PG18实际raw行1与认证8，无服务跳过且前后catalog一致。NULL/缺失/解码失败、PDO类别、getter/基础cast、异步setter/收据取消与committed读取分别保留断言。 |
| JSON与数值 | 同版RawValue两阶段校验Unicode/深度及重复成员，再转最终可见数字；普通scalar617保持。74项PHP8.5.12参考及大指数三入口6比较归零，文档字面样例实际编译运行。512KiB线程的8场景保最终Value析构/错误清理，成本仅为本机开发证据。 |
| 根与示例认证 | v8真实HTTP测试验证登录当场调用password getter、合法整数账户跨请求成功；同raw快照String模型正向读取与整数authority精确Unauthorized并存。原authority12/具名Guard10通过，生产typed-ID、表和连接校验未放宽。 |
| 最低SQLite | 指定并核实际加载SQLite3.26.0动态库及SHA，Model71/数据库Guard4/示例SQLite子树7通过。明确过滤的网络raw用例由本轮双库另验。默认bundled3.51.3只有构建静态链接来源证明，不伪造运行SQL版本读数。 |
| 主线/feature | fmt、default/all-target/all-feature strict、完整default/all workspace、两模式拒警Rustdoc、各driver/auth/hashing严格检查、Fortify无后端、MSRV1.97两模式及deny通过；default与PHP探针按不变输入映射复用。 |
| 生成与consumer | SQLite/MySQL/PG认证应用各实际new/check/test/锁定metadata通过，无skip；test摘要41/40/40含lib/bin重复，不相加冒充独立场景。12consumer metadata全部指向隔离源，两个受影响consumer执行锁定check；不声称12项都重新编译。 |

最终索引 `tmp/independent-testing-20261106/MB-A01-RESULT.json`，Root复核 `mb-a01-root-final-acceptance.json`；原失败保留。收尾回收firstparty、CLI、多feature/MSRV/native与fresh产物，仅留明确下一C07所需的一份依赖缓存，无在途命令、磁盘余量超过软线，主目录target不存在。

不关闭完整Eloquent、全部casts/日期/关系/持久save、完整PHP数组键/对象身份/无效UTF8/非有限值载体、PG TimeZone/DateStyle显示及未知数据库提交/跨连接顺序；C07组合与J01后继边界保留；数据库翻译来源的当前限定证据见下节。

## 命名空间与开发流程限定验收

职责命名空间及调用方限定迁移，源码提交 `b3f6117be1fc3132adb010b386a74bec4517e21b`。固定 Laravel framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`、docs `960eedf0911155ad16326ade49c0ea5d18eda0b8`；最终受验 v14 共1578项，manifest SHA `3cd019bd540c23d21c6ea6aa7292a31a1460ee9a92ba576994c5637b7bf8d7be`。Root提交审核 `tmp/namespace-alignment-20261106/pure-export/root-v14-acceptance.json` 绑定独立原始命令回执、日志SHA、输入与跨版本差分；独立汇总入口为 `tmp/independent-testing-20261106/AR-N01-V14-RESULT.json`，汇总写入不替代原始工具证据。

| 范围 | 实际证据与边界 |
| --- | --- |
| 纯职责迁移 | Container、Support、Console、Foundation、Database、Routing/Http、Translation、Filesystem、Encryption唯一实体及应用/宏/模板/示例调用同步；完整官方语义不因改名关闭 |
| 组合门禁 | v11原并发默认workspace、日常MySQL组合通过，70唯一/206次示例失败逐项闭合；framework日常组合1387 passed/23 ignored，忽略项不算实服务证据 |
| v12单文件影响及余门禁 | fmt、默认/全feature严格Clippy、默认重定向8项及带Session配置9项、framework doctest、双拒警Rustdoc、framework/root no-default、三个数据库driver及auth/hash/Fortify最小组合严格检查、Rust1.97全feature/无默认、cargo deny通过；PostgreSQL/SQLite仅编译 |
| 实际服务与生命周期 | 真实MySQL8.4 cold-prepare、queued Fortify、原raw/migration、Jail输入快照及persistent-foreign/busy-close/lifecycle专项通过；公开recover恢复本任务遗留记录，其他schema与基线不变 |
| 消费者及生成 | v6实际12个公开consumer、Web认证/JWT生成项目check/test、根默认及前缀TCP、URL反代工具；后续仅测试/文档及私有构造器生命周期整理，按正式回执映射，不冒称生成项目重新运行 |

测试配套显式启用example专用fixture，临时部署状态用例保持独立file配置；不恢复根生产模块状态JSON。cfg(test)在既有环境锁内、Jail前捕获原state_dir/cwd/数据库URL，保留缺失、非UTF8及非法路径语义；非cfg(test)动态机制未改变。

旧负对照缓存曾被误用于正式exact，失败及二进制来源证据保全；只刷新正式编译入口mtime、核真实Cargo来源并用新Jail回归区分后，正式专项通过。此为缓存身份校正，不记作生产清理算法修复。

Source、SQLite写目标、Passport/授权服务器、J01、SC及Model后继WIP不在本输入；其他数据库与最低版本实际运行、Windows及完整官方contract差距继续开放。资源回执与已明确安排的下一Source验收缓存保留由原测试对话记录；不因保留单份warm target虚报完整清零。

测试根索引原始字节发生metadata漂移，stage路径、模式及blob列表与交接前相同，未强行回写索引；主工作区690项WIP字节/模式/软链均保全。独立私有Git对象库重建精确匹配提交树 `eb740f4a4d22b5a4f8422fda28346509250c837b`，不以简化行尾归一化手算替代实际Git对象。

## 翻译来源限定验收

I18N-S01 的运行输入以 `0a8cb93f` 上的 Source v17 为准，1610 项源码/模式逐项冻结；提交候选仅后置更新使用说明和项目事实，不改变已验 Rust、模板、manifest、锁或测试。正式源码映射与原日志见 `tmp/i18n-sources-20261106/` 及独立验收目录 `tmp/independent-testing-20261106/i18n-source-runtime-v14` 至 `v17`。

- 默认 workspace、原并发 MySQL 组合、默认与全 target/feature 严格 Clippy、精简 feature/三 driver 编译、拒警双 Rustdoc、doctest、MSRV 1.97 两配置与当前锁 deny 均实际通过；其他后端只计编译，不计真实运行。
- 真实 MySQL 8.4 覆盖来源读写、双消费者、旧快照、坏行/回滚、提交后通知失败、文件/数据库组合及命名连接/前缀；首次文件模式安装迁移再选组合、queued 来源通知在根测试 target 实际执行，不以缺 URL 或 ignored 代替。目录/夹具前后恢复。
- 五类真实生成的 check/test 与锁图通过；gRPC 生产 `.txt` 片段旧命名空间已修，实际重编 CLI、重新生成、检查产物及依赖指纹。生成 MySQL 来源首次安装与显式 JWT 无 Cookie/Session 边界有实际证据。
- 12 个公开 consumer 的 locked all-target check 通过，配置、Container、Session 和 TestApp 四组适用行为测试实际通过；SQLite consumer 仅编译。TestApp 旧 `Server Error` 期望按既有 E01 bootstrap 英文契约更新为精确 `internal server error`，保原自定义 Handler/HEAD/应用计数隔离与安全断言并补 HTML 安全文案；完整 consumer 复验通过，原失败保留。
- 根默认/自定义前缀 TCP、内容协商、404、无 Set-Cookie 和退出端口回收通过。Source 首次 snapshot Drop 负控实际检出失败状态发布回归；本地文件首写取消窗口由 OpenDAL/Tokio 真实负控检出，修复后文件系统组及原组合通过。S3 取消/迟到发布与 Windows 未由这些测试证明。
- 正式额度、原失败、资源软线及自然边界回收证据保全；只保留后续已排步骤的一份共享缓存。Markdown 本地链接/锚点静查通过；mdBook 未安装，未执行书籍构建。

数据库 schema、修订及失效提示属于授权来源增强，不宣称 Laravel 内置数据库 Loader。完整 `get`/`choice`、默认 FileLoader、Loader/deferred/locale 生命周期、其他数据库及最低版本运行、Windows 文件来源仍未闭合，不将本有限簇当作完整 Laravel Localization 或完整框架交付。

## 事务来源与提交前检查限定验收

输入为Model提交`1aee9043`上的CTX v5，manifest `b6587ba058a83868f5017c6deefa31d04cfb82db32ae59facad36a9d517cf234`、1618项；原生Git规范化源码树`5056c267888aeb3a8a14cf9319cba422113b24d0`只改8路径。正式报告`tmp/independent-testing-20261106/TRANSACTION-CONTEXT-RESULT.json` SHA `6945ee85ef14018016b15ee990bbb7445a1d1de39930eff52036ab7c36dd0e57`，Root已复核46回执及全部引用日志摘要。

- 全target/allfeature严格检查、fmt、默认严格/workspace及日常MySQL workspace通过；默认62与常规82是结果target数，不是服务用例数。
- 真MySQL8.4.13的框架来源/外包/文章queue off/on四组各1项通过，均0 ignored/无skip；覆盖来源、顺序、回滚、取消、同Db并发及文章监听删除/owner变化。正向外包无Auth/Fortify；五负向编译按隐私/From/Deserialize/生命周期精确失败，不把编译错误当运行测试。
- 12 consumer编译及4实际组通过；fresh minimal19、Web/Fortify50、JWT95均0 ignored/无skip，框架path指向当前输入。三profile registry三元组→checksum与先前已验同profile完全相同，root共有key checksum相同；不声称fresh锁是root锁版本子集。
- 双拒警Rustdoc、14本地链接、no-default及三driver编译、实际Rust1.97两profile和`cargo deny --offline --locked`通过。Advisory仅缓存`309392b18f6b17022f82c701f300433126901068`（2026-11-06T16:34:32+01:00），未live刷新。

v4/v5原失败、夹具修正和资源自然暂停均保全；源/fixture SHA、inode/mtime与catalog恢复已核。完成产物按原生归属回收，最终共享target约9.74GB、磁盘余量约80.30GB/8.073%。这不是认证/同意证明、跨库原子性、未知提交故障注入或完整Model/L090/Passport验收；其他数据库/最低版本/Windows、外部OIDC/SMTP/副本和非UTF8文件系统等边界仍保留，ignored不算实测通过。

## 事件身份与响应限定验收

EV-R01精确运行输入为已提交Context `364233ca`上的v5，manifest `0b5b32f0073516a553eee539ec72ef98415308fa070868a0b60488cc824cbcdb`、1621项。Root与独立测试对话各自算得24路径规范源码树 `47b1f9dd62ca7583f53117bef7e60a65bc150553`。正式报告 `tmp/independent-testing-20261106/EVENTS-RESPONSES-RESULT.json` SHA `2d5371853fe5b9b39df3aa905a29b061447eea23441896ebd4d25a4f37bd0a6c`；Root复核49回执及86个唯一引用日志摘要（91物理日志路径），报告另计76原始命令日志，不混为用例数。

- v4全target/allfeature类型检查0；唯一私有helper lifetime按原诊断省略后，v5同profile严格检查0，函数体/断言及其余1620文件不变。精确快照fmt、默认严格、默认workspace63和日常MySQL83组结果target通过；ignored/skip明确保留。
- 响应12、跨包Modules2与Translation1、SessionGuard49、Fortify75普通项及六个原ignored的明确MySQL实际运行通过；QR导出互操作未验。真实MySQL具名until以exact+--ignored实际1pass/0ignored，原queue worker1和正常根defaults的HTTP认证/文章1通过；此前缩减profile的configuration_error保留，不改源码/断言求绿。
- 12 consumer alltarget及4组实际通过。fresh minimal19、Web/Fortify50、JWT95实际通过，无ignored/skip，path绑定当前framework及实际CLI；三配置完整registry映射与此前相同profile一致，root共有exact key checksum相同，不称fresh锁是根锁版本子集。
- 默认/全feature拒警Rustdoc、92本地链接、no-default/三driver检查共15 native步骤通过。实际Rust1.97两精确profile均fresh=false，编译器fingerprint103852…；离线locked cargo deny使用缓存advisory `309392b18f6b17022f82c701f300433126901068`，未在线刷新。

原严格诊断、HTTP缩减profile失败、wrapper误分类、软线暂停与全部原始回执保全；source/fixture/catalog保持。完成产物按原生归属回收，最终共享target约6.70GB、磁盘余量约82.78GB/8.322%，无在途Cargo/rustc/rustdoc。固定PHP15项另记真实源参考，不是Rust或queued/事务证明；后续JWT/日期取证不混入。完整接口投影、多参/defer-aftercommit/可变Model/全部mixed、官方事务connection载荷、完整Model/L090、其他数据库/最低版本/Windows及外部OIDC/SMTP/副本/非UTF8文件系统仍开放。

## 具名事件服务入口限定验收

冻结树 `a1b9a3e1825fd38cd08776753db9b8b1e070c7dd` 通过默认及 Session/Fortify/View 的 workspace 测试、严格 Clippy、格式、Rustdoc 与离线依赖检查。固定 Laravel `d52c3b3` 的 `events` 键解析到现有应用单例。独立验收核类型和具名身份、默认延迟事件注册与移除、唯一示例经具名入口触发 Provider，真实 MySQL 8.4.13 的监听失败事务回滚。新生成 Web/Fortify 及 JWT API 通过 check/test，其中 Web 的真实应用容器核对两种解析取得同一实例。Dispatcher 类型和完整契约继续开放。

## 默认事件派发器限定验收

冻结树 `4585727e16d76dc47adccd623f894d801eeba0da`。对应固定 Laravel `d52c3b3` 的核心 EventServiceProvider、Application 基础清单和应用监听 Provider。默认核心惰性登记 Dispatcher，与具名 `events` 共享实例。应用在其 Provider 启动前发布监听，保留登记次序。已经取得的副本可见更新，不跨应用安装监听组。模块扩展使用同一应用服务，显式排除或替换核心后，模块及延迟监听路径均不补位。

实际验证早期副本、强制替换、旧发布防回填、监听冲突和跨应用误用、启动期及运行期延迟监听、HTTP/控制台 Terminating、模块启停与生成 Fortify 路径。默认及 Session/Fortify/View/OIDC/Translation Database 组合通过工作区测试、严格 Clippy 与 Rustdoc，另有真实 MySQL 8.4.13 事务回滚及生成 Web/JWT check/test，格式及离线依赖检查通过。

完整应用监听 Provider 的官方路径和发现、契约接口、after-commit 与通用广播保持开放。

## 哈希与日志具名入口限定验收

冻结树 `cd849604b2c01961b461909c37256ef91ca05d6e` 对照固定 Laravel `d52c3b3` 的 HashServiceProvider 与 LogServiceProvider。应用的类型化及具名入口共享同一管理器和默认驱动。真实 Argon2id 默认解析、自定义源方向、具名装饰及默认频道写入均实跑。唯一示例经具名入口读取哈希管理器和日志频道，Web 生成应用核对三个具名键的实例身份。

默认及 Session/Fortify/View 工作区测试、严格 Clippy、格式、Rustdoc、离线依赖检查、MySQL 8.4.13 事务回滚及 Web/JWT 生成项目 check/test 通过。HashServiceProvider 延迟身份与完整日志频道契约仍开放。

## Hashing字节与单次叶选择限定验收

精确输入为Events提交`96aced57`上的Hashing v9：manifest `f90f39a2d111c2f18fd97f0984d9f8d829ef420f2fbd30b678c35ea2c54797ac`、1629叶，25路径Git规范源码树`08ee7f1c516d806791af5f1e852212de900a9c1c`（1629叶原始冻结树为`f7b61e50243e00b61e690c80f05e9fa5663d56f5`，唯一非任务差异为commit.cmd规定的CRLF工作树行尾，Git保持原LF对象）。正式报告`tmp/independent-testing-20261106/HASHING-OPERATION-RESULT.json` SHA `a48b20f2663e9528d59817fc978dff1f39c322c1612a2754585ec5a47392c3c0`；18分项回执、84原始日志引用与11清理回执分别保全。最终提交另含测试方正式生成的新consumer锁和七份事实文档，不将最终树冒充原受验源码树。

- 精确fmt、alltarget/allfeature类型与严格Clippy、默认严格检查通过；默认workspace63组、日常真实MySQL8.4.13 workspace83组通过。普通ignored与外部skip不当作实际通过。
- Hasher67项（原56+操作选择11）通过、0ignored；旧40测试175断言、Provider11测试51断言与legacy bcrypt3测试保留。固定PHP语料232 make（182返回、26Runtime、24InvalidArgument）及408 check（248true、136false、24exception）按阶段核对；56未调用不计执行，182 Rust roundtrip通过。
- 示例unit2、旧命令2、嵌套Manager实际parent1及明确exact-child1通过，各实际子运行0ignored。73字节原值true、72负值false，下一操作B拒绝且无额外make/stdout；两个实际CLI产物经固定PHP8.5.12核对原值true及两个负值false。Session49、Fortify75普通项通过，七个普通ignored保留。
- 新公开consumer严格/类型及实际2项通过；既有12consumer alltarget与四组实际通过。fresh minimal19、Web51、JWT96通过，后两类包含当前模板hashing_bytes回归；准确当前CLI/源码与fresh=false来源另证。
- 默认与全feature拒警Rustdoc、62本地链接、13组reduced/no-default/driver门禁通过。实际Rust1.97双profile fresh=false、compiler fingerprint103852…；离线deny0使用缓存advisory `309392b18f6b17022f82c701f300433126901068`（2026-11-06），未在线刷新，duplicate29与license-not-encountered1警告保留。

源SHA/mode、固定PHP fixture SHA/inode/mtime与数据库catalog保持。v3/v4/v7/v8失败、私有helper修复及提前读取companion回执的时序纠正保全；不以旧通过替新输入。自然边界精确回收完成产物后，单一stable缓存约7.58GB、磁盘余量80.81GB/8.124%，无在途native命令。完整增强认证选择、Manager getDrivers/forget/container/default及历史缓存所有权、Model binary/hashed与非UTF8 encoded、其他/最低数据库、Windows、OIDC/SMTP/副本/非UTF8文件系统等专项仍开放。

## 调用级默认属性 setter 限定验收

Hashing 提交 `651c2971` 上的 MB-S01 v6 冻结 manifest `ab2172856a453cb89ed10a36bbe26ab128d78d0acf32ce87f8fcff611d85091e` 共 1633 个文件。10 个任务路径对应 Git 源码树 `eb6a0a0f8d39c9d13bd278dc3866ba31ce988b7e`。有限提交另含正式独立 consumer 锁与验收事实记录。独立测试报告 `tmp/independent-testing-20261106/MODEL-DEFAULT-SETTER-RESULT.json` SHA `e3470a9d777e2c9ed4f91b80430f2d9db82448148a42e403048ceaabe29deefb`。

- 全特性检查和严格 Clippy、默认严格检查、精确输入 fmt 通过。v5 的恒等 `map_or` 失败保留。v6 只替换预览入口一行，仍用独立 presence 标志区分缺省和显式 null。
- 无 Hashing setter 6 项、有 Hashing setter 7 项，Model 定向 95 项包含上述 7 项。模块预览服务 5 项、真实 MySQL 8.4 HTTP 预览 1 项、公开模型状态 2 项通过。真实 HTTP 核对 null、dirty、changes、previous 及整行未写入，数据库 catalog 恢复。
- 新公开 consumer 通过正式锁、编译、严格检查及借用 Send 用例。生成的 minimal、Web、JWT 项目分别通过 19、51、96 项。
- 默认 workspace、例行真实 MySQL workspace、拒警 Rustdoc、链接、reduced/driver 编译、实际 Rust 1.97 和离线依赖门禁通过。外部跳过和原失败边界按回执保留。离线依赖结果使用本地 advisory 快照。

本批交付默认 setter 选择、借用及原 patch 规则。完整 getOriginal、fill/save/事件/关系、Model binary、Passport Client 业务组合、其他数据库与发布矩阵各自保留。

## 增强认证单次叶选择限定验收

HA-X01 固定 Hashing/认证父树扩展同一 `HashManager` 的操作级选择。增强 `EncodedPasswordHash`、AuthFlows 和验证码操作将同一真实驱动叶保持到能力检查和 KDF 完成。账户查询、dummy 校验、reset 的密码/验证码独立选择、条件写入和错误边界沿原生产路径。普通 UserProvider 仍消费其原 Hasher 契约。

v6 精确输入 1641 项、26 路径源码树 `adb661797d78500903b7d5799606eed801f210d0`，外部 consumer 正式锁 `9a09ae04c7efce2483be9c363a67ad0ab52b9406c4fe6e172c1d9dd87de30f52`。实际 Hashing 67、密码 6、验证码 4、Flows 边界 1、真实 MySQL 8.4.13 登录重置 1、唯一模块真实 HTTP 登录 1、Session 49、Fortify 75、外部 consumer 2 通过。v5 模块测试错误和验收计数纠偏保留，v6 仅改模块 fixture 的配置仓储访问器。按字节与路径证明映射复用未受影响定向证据。

同一 v6 输入通过默认工作区 63 组与日常 MySQL 83 组、最小/Web/显式 JWT 生成项目检查和 19/51/96 项测试、拒警文档、84 个本地链接、15 个文档/驱动相关命令、真实 Rust 1.97 编译与离线缓存依赖门禁。忽略项、外部 skip 和缓存 advisory 范围按正式报告分列。源、MySQL catalog 和 fixture 恢复/不变。其它数据库运行及发布最低版本矩阵仍待独立验收。

正式报告 `tmp/independent-testing-20261106/HASHING-AUTH-OPERATION-RESULT.json` SHA `1ea8af88680f43f95823187a4827af2c6cab57fcaebc018229f70e5c3c34ce92`。剩余 Manager 公开表面、历史缓存所有权、Model binary/非 UTF-8 编码和全量认证对应按唯一 todo 继续。

## 数值日期格式化与解析限定验收

NumericDate v6 通过共享 Calendar 的六位舍入和 `U.u` UTC 解析处理 JSON 数值与完整数值字符串。保留负秒与微秒字段行为，拒绝非法输入且不回显原值。唯一示例走现有生产 HTTP 输入管线，外部 consumer 禁用默认 feature。

精确输入 1648 项、16 路径，源码树 `b721ceeccdfd35a5fee3d42fa5a4c45b0b332f25`。定向实际通过 6 Core（重放 72 与 36 条 PHP 参考样本）、1 根 server 生产 HTTP、2 外部 consumer，零忽略。默认工作区 63 组，日常 MySQL 8.4.13 工作区 83 组均通过。原 Calendar 33 和 Request 53 个具名回归实际覆盖。原始报告分列忽略与外部 skip。格式、严格 Clippy、拒警 Rustdoc 和 95 个本地链接通过。源码、锁、夹具与数据库目录保全。v5 严格 Clippy 诊断留存，v6 仅补一行 Rustdoc 代码标记。

正式验收 `tmp/independent-testing-20261106/numeric-date-runtime-v6/NATIVE-RESULT.json` SHA `a538bbd8ecc78208cb153306161005b1fe0de72f656d31ab115b31c7107bb912`。正式外部 consumer 锁 SHA `76f5f97d369b96db728028756ac65464e25462a57341484a858ac365c36f0c28`。本簇为纯数值格式化与解析；完整 Calendar、Model 日期语义、Passport/JWT 消费与发布矩阵仍按各自待办推进。


## Eloquent 模型日期转换限定验收

固定 Laravel `HasAttributes`、`HasTimestamps` 与 `DateFactory` 的日期读取、写入、数组化和缓存对象行为，按固定 PHP 8.5.12 探针与 Rust 回归对照。根应用、示例、生成模板及 Passport 使用有序属性载体和有类型访问器。普通日期投影、存储格式及查询参数格式分别验证。

验收绑定源码树 `a5e36797b3ea042e28eb29699c637dae760097a6`。隔离 Workspace 测试、格式检查、严格 Clippy、Rustdoc 与离线依赖门禁通过。两文件复审修订只移动示例断言到清理保护段并恢复原 import 布局，随后重验严格 Clippy、格式、真实 MySQL 8.4.13 示例及 fresh MySQL Web/Fortify 项目。生成应用通过 check 和 55 项测试，Schema 专跑 1 项保留 ignored。测试库及账号目录恢复，验收见 `tmp/model-date-final-20261108/acceptance.json`。

PostgreSQL 日期绑定类型、秒精度与本地时间经过定向验证；实际 PostgreSQL、其他最低版本、追加字段及关联序列化按各自矩阵推进。

## Provider 包发现预览限定验收

L044.02 的 CLI 预览对应冻结源码树 `9e47c3c227267c739fdcb8ba3dd1396788e6e9a0`。固定 Laravel PackageManifest 对照覆盖宿主与包声明排除、全局排除和 Facade 别名合并。真实 Cargo 夹具编译生成源码，覆盖重命名、自定义库名、同名双版本、可选 feature 与普通直接依赖选择。非宿主目标仅验证输出中的条件编译。

工作区格式、严格 Clippy、MySQL 8.4.13 下的 workspace 测试、拒警 Rustdoc 和离线依赖门禁通过。测试 2917 通过、54 忽略。fresh minimal 应用将预览显式接入 `ApplicationDefinition::package_providers`，通过 check、23 项测试和真实 HTTP 200 服务解析。普通构建自动接入、清单新鲜度、运行期冲突与依赖排序、实际跨目标编译仍待验证。

验收见 `tmp/lead-package-discovery.lQRIAVuB/delivery-receipt.json`。

## Provider 包清单安装限定验收

L044.02 的构建接入对应冻结源码树 `80f0660e16f5929f4ceb76a5a2ae1cf39f970942`。普通构建以实际 Cargo 目标和应用特性选择清单。工作区格式、严格 Clippy、MySQL 8.4.13 下的测试、拒警 Rustdoc 和离线依赖门禁通过。测试 2917 通过、54 忽略。

fresh minimal 应用在清单缺省和已安装两种条件下各通过 23 项测试。普通 Cargo 启动消费外部 Provider，编译可用 Facade 别名，并在真实 TCP 上验证 HTTP 200、服务注入及 SIGTERM 正常退出。缺少当前特性清单、直接依赖 manifest 变化、锁变化及新增项目 Cargo 配置均在同一 target 中拒绝构建。各失效用例重装后恢复。相同清单重复安装保留文件时间。

fresh gRPC 应用同时生成 protobuf 和包清单，通过编译与 7 项测试。真实联合服务处理 HTTP、TLS Echo、错误 CA 和错误域名，再按 SIGTERM 正常退出。无清单对照也能复现旧停机错误。修复让控制台先派发终止事件，进程入口再关闭容器。 gRPC feature 严格 Clippy、2 项运行时单元测试和 4 项嵌入式联合服务消费者测试通过，覆盖真实双监听启动、绑定失败和超时回收。

同一目标专用 feature 在 macOS 无 Provider，在 Rust 1.97 的 Linux ARM64 容器内加载 1 个 Provider。该项使用最小 Cargo 夹具验证目标清单和实际链接。项目外 Cargo 覆盖、直接依赖 feature 激活、包来源冲突、运行期排序和更多平台矩阵仍开放。正式回执见 `tmp/lead-package-plan.cqhei5YN/delivery-receipt.json`。


## 包来源与启动顺序限定验收

L044.02 的包来源和依赖顺序对应冻结源码树 `47e58a83e180479acb06feec8353cdc6a16afe56`。固定 Laravel 来源核对框架默认、发现包与显式应用项的排列以及普通同类型去重。Rust 包段按显式依赖预检并稳定排序，优先等待确定会登记的依赖。五项回归覆盖同包首项、跨包冲突与移除、缺失与后置项、循环、条件嵌套和重复项声明的失效身份。

工作区格式、严格 Clippy、MySQL 8.4.13 测试、拒警 Rustdoc 与离线依赖门禁通过。工作区测试 2921 通过、54 忽略。fresh minimal 的两个真实 Cargo 包经普通 check、23 项测试和实际启动验收。消费者在注册阶段解析到被依赖包的 Marker，真实 TCP 返回 HTTP 200。跨包重复启动退出 1，输出双方来源且端口未开放。v1 清单拒绝后重装恢复，旧 helper 拒绝安装且已安装清单字节保全。

生成模块消费者原宿主类描述符接线修复后通过持久启停验证，覆盖原模块断言与禁用后宿主 API 可访问。fresh JWT 应用在真实 MySQL 8.4.13 下完成 check，测试 101 通过、2 忽略。正式回执见 `tmp/lead-provider-order.zIay19Ph/delivery-receipt.json`。

## Cargo 来源关联限定验收

L044.02 按 Cargo 元数据的来源字段区分同名同版本依赖，保留 Git 分支、标签与提交选择以及正常依赖的应用特性约束。项目内显式 `[patch]` 可证明替换。候选歧义或来源无法确认时拒绝生成，诊断和清单不披露源地址。

独立冻结树 `91c3c986996367f97f2fb0d94a3ab1ec3aa9e375` 通过工作区格式、严格 Clippy、真实 MySQL 8.4.13 下的工作区测试、拒警 Rustdoc 与离线依赖门禁。新增回归覆盖同名 Git 包串入、来源未知和候选歧义。真实 Cargo 夹具的七个预览覆盖双 Git 来源的默认及可选激活、工作区和应用配置内的 Git 补丁、离线 registry 补丁。

fresh minimal 应用以 registry 依赖声明和本地补丁安装 Provider 清单。普通 Cargo check、23 项测试和实际 HTTP 200 通过。旧版或过期清单拒绝构建，跨包冲突拒绝启动且端口未开放。完整证据见 `tmp/lead-cargo-sources.xT55JQYD/delivery-receipt.json`。

项目外 Cargo 覆盖、配置 `include` 与进一步的发现工作流仍按唯一 todo 推进。

## Provider 清单重建与检查限定验收

L044.02 对照固定 Laravel `PackageDiscoverCommand` 的无参数重建行为。源码 `./artisan package:discover` 使用独立 CLI，应用编译因清单失效时仍可重建。`--install` 复用同一入口，`--preview` 输出 Rust 源码，`--check` 比对同一目标与特性的完整预期内容。

独立冻结树 `fca81499a2e4e22504e839292c6f1cb3c0586e73` 通过工作区格式、严格 Clippy、真实 MySQL 8.4.13 下的工作区测试、拒警 Rustdoc 和离线依赖门禁。进程回归核对根源码及 PATH 中 CLI 的参数、退出码和 exec 语义，参数测试核对默认模式和模式互斥。

根 Artisan 以默认与无默认特性分别重建和检查，输出不同清单路径，Cargo.lock 保持原字节。新生成 minimal 应用通过 41 个步骤和 23 项测试。只读检查遇缺失不建目录，遇过期保留文件字节和修改时间。普通构建拒绝过期清单后，源码 Artisan 只运行框架 CLI 即可恢复。包路由实际返回 HTTP 200，重复 Provider 阻止监听，移除依赖并更新锁后原路由返回 404，健康检查仍返回 200。

证据见 `tmp/lead-package-workflow.qA4ZNuiu/delivery-receipt.json`。候选列表、排除原因、最终运行期顺序和项目外 Cargo 覆盖按唯一 todo 继续。

## Provider 包发现候选诊断限定验收

L044.02 的 `package:list` 与 `package:discover --preview` 共用 Cargo 选择、包排除及 Facade 编译条件。固定 Laravel 的清单先合并包排除项再选择条目；Rust 报告额外保留目标条件和已解析但未启用的可选依赖。候选身份按包名、版本和 Rust 依赖名记录，来源只显示类型。

独立冻结树 `6dc279e1ad8215d6e55a7dfc2777d6cb1f64ace8` 通过工作区格式、严格 Clippy、真实 MySQL 8.4.13 下的工作区测试、拒警 Rustdoc 和离线依赖门禁。

真实 Cargo 图验收覆盖双 Git 源、可选依赖、项目内路径替换、应用单包和全局排除、包级排除与同名 Facade。根和生成应用的 Artisan 输出与直接 CLI 一致，查询前后的清单字节与 mtime 一致。七份旧预览逐字节相同。生成应用通过 check 和 23 项测试，编译引用验证 Facade 获选类型，真实 HTTP 健康检查返回 200，被排除包的路由返回 404。

证据见 `tmp/lead-package-report.cirhwrb2/delivery-receipt.json`。运行期实例排序诊断、项目外 Cargo 覆盖与配置 include 按唯一 todo 继续。

## Provider 实际顺序诊断限定验收

L044.02 的 `package:doctor` 与正常引导使用相同配置选择、预装配和包排序，默认报告顶层尝试序列及稳定错误码。显式 `--boot` 从实际注册表报告有效槽位，保留当前实例来源。普通重复继续去重，强制替换保持槽位并更新来源。

独立冻结树 `8b3779460d9550bcb8cb0c76b79e72349ba70054` 通过工作区格式、严格 Clippy、真实 MySQL 8.4.13 下的工作区测试、拒警 Rustdoc 和离线依赖门禁。回归核对同一包排序、强制替换来源和内置命令目录。

新生成 minimal 应用通过 check 和 23 项测试。真实 Artisan 帮助及预检不调用 Provider 回调。`--boot` 的实际槽位与两包 register 和 boot 顺序一致，关闭回调按逆序执行。跨包冲突的两种诊断模式均返回同一语义码及双方来源。正常服务真实 HTTP 200，冲突服务退出 1 且未监听。根 Artisan 预检保持现有服务清单字节和 mtime，缺失时不创建文件。

证据见 `tmp/lead-provider-doctor.DYDYTseh/delivery-receipt.json`。项目外 Cargo 覆盖、配置 include 与发布平台矩阵按唯一 todo 继续。

## Cargo 文件配置增量校验限定验收

L044.02 以同一套读取逻辑遍历应用祖先目录、`CARGO_HOME` 与递归 `include`。按 Cargo 优先级关联文件中的 `[patch]`，相对路径按定义文件解析。v3 清单只保存配置摘要。旧版清单须先更新构建接线，再运行发现命令重建。

独立冻结树 `d2d33a3bdad548f4dbb0071e79b0a4b39d1041b4` 通过工作区格式、严格 Clippy、真实 MySQL 8.4.13 下的工作区测试、拒警 Rustdoc 与离线依赖门禁。真实 Cargo 图验证 `CARGO_HOME`、祖先递归配置、可选 include 和应用配置的覆盖顺序。

新生成 minimal 应用通过源码 Artisan 重建、Cargo check、23 项测试及实际 HTTP 200。锁文件字节保持不变时，普通构建仍拒绝递归 include 修改或可选文件新增造成的过期清单。重建后使用正确的 Provider，实际 HTTP 响应随选中的依赖切换。普通构建拒绝旧 v2 清单，源码 Artisan 可重建 v3。

完整证据见 `tmp/lead-cargo-config.qka4Ps36/delivery-receipt.json`。当前按应用目录使用文件配置，`cargo --config`、其他非文件覆盖和发布平台矩阵仍开放。

## 应用语言配置限定验收

AR-N01/L041 的翻译 Provider 按固定 Laravel 配置路径从同一个 `app` 快照读取默认及回退语言。根应用和新生成应用从 `config/app.rs` 设定默认值，部署环境通过 `APP_LOCALE` 和 `APP_FALLBACK_LOCALE` 覆盖。显式构造 Translator 仍可提供独立的类型化参数。文件和显式数据库来源沿同一配置入口。

冻结树 `25eba80718f7f5b09fc0f7d9315f8a893bebfce1` 通过格式、严格 Clippy、真实 MySQL 8.4.13 下的工作区测试、拒警 Rustdoc 和离线依赖门禁。回归核对语言配置的环境变量、TOML、早期和晚期 overlay、分组消息回退、全局 JSON 查询及异常呈现。唯一示例真实 TCP 检查无语言头、未知语言头和显式请求语言下的本地化。

新生成 minimal 应用通过 Cargo check 与 23 项测试。实际进程检查测试路由上的五次 HTTP 401。未指定或请求不支持的语言时返回配置默认语言，显式语言头优先。缓存使用生成时的语言，清理缓存后采用新环境值。非法语言标签启动失败且不输出输入值。证据见 `tmp/lead-core-next.rY8h72Eg/delivery-receipt.json`。

当前资源快照仍要求配置语言可匹配资源。合法但缺少资源的语言、运行时语言变更、默认文件 Loader 与延迟 Provider 生命周期、`choice` 和发布平台矩阵仍按唯一 todo 跟踪。

## Laravel 字符串复数选择限定验收

AR-N01/L041 的 `Translator::choice` 在同一目录快照读取字符串，指定语言未命中时按配置回退语言的规则选分支。使用固定 Laravel 的显式条件和旧版地区规则。选择后按参数顺序登记 `:count` 与字符串占位符，保留最长匹配、单次替换及 Unicode 首字 titlecase。Rust 调用方用整数、双精度或集合长度。结构化 ICU 消息继续通过 `translate`。

冻结树 `bd0a5eb665e347540eff3d5141d56f400deba004` 通过格式、严格 Clippy、真实 MySQL 8.4.13 下的工作区测试、拒警 Rustdoc 和离线依赖门禁。唯一示例的实际 TCP 用例检查中英文阅读摘要及请求语言隔离。

生成 minimal 应用通过 Cargo check 与 24 项测试。公开 Rust API 与固定 Laravel 源码在 PHP 8.5.12 中的 12,660 个对照一致，覆盖 280 个既定地区标签及未知地区、精确条件、空白、Unicode、字符串占位符顺序和 96 个默认异常处理器升级的整数越界。依赖数据的来源及许可与实现同行。

证据见 `tmp/lead-trans-choice.RKavaqy5/delivery-receipt.json`。独立 Loader 生命周期、运行时应用语言变更、通用值和回调替换以及发布平台矩阵仍沿唯一 todo 验收。

## 分组翻译资源限定验收

AR-N01/L041 分组文件在同一来源快照中保留对象与数组。已有文本与复数入口覆盖点路径、字面键优先、数组索引、语言回退和普通 `plural` / `select` 字段。四项回归通过，另以固定 Laravel Translator 和 Arr 在 PHP 8.5 容器核对七个文本及复数结果。

冻结树 `df1f3f5dc4f39770462880832918cd6da41adfde` 通过格式、严格 Clippy、真实 MySQL 8.4.13 下的工作区测试、拒警 Rustdoc 和离线依赖门禁。通用整组值与递归替换仍沿唯一待办交付。

## 翻译值读取限定验收

AR-N01/L041 的 `get`、`string`、`array`、`has` 和 `has_for_locale` 共享发布快照。已核对精确 JSON 键、分组回退、短键整组读取、嵌套值、递归占位符、有序替换及类型错误。使用 PHP 8.5 执行固定 Laravel Translator 与 Arr 核对 42 个可观察结果。Rust 保留 JSON 列表与关联映射两种表示。

运行树 `0272f25f3720f0031c1862c07e637eeb7fbf0dfb` 通过 fmt、严格 Clippy、真实 MySQL 8.4.13 工作区测试、拒警 Rustdoc 与离线依赖检查。唯一示例经真实 TCP 调用 `string`；新生成 minimal 应用通过 Cargo check 与测试，注册的公开 Loader 实际读取分组。仅加入包 README 的文档树 `5a728784209e34e7f0577c65d53c106692dd10ec` 补跑拒警 Rustdoc 通过。

PHP 标签回调及对象替换、缺键回调、完整 Loader 与运行时语言生命周期仍在唯一待办。

## 翻译标签与对象替换限定验收

AR-N01/L041 通过 `Replacement` 与 `Translator::stringable` 按序执行标签和类型化替换。回调在目录和注册表锁外执行，复用文本占位符与目录查找。固定 Laravel 的八项 PHP 8.5 行为包含重复和嵌套标签、换行、后续回调、非递归占位符、精确类型及枚举值。Rust 标签采用字面名称，枚举显式传入变体名或背书值。

冻结树 `601a25e1428fcf47ffac2c8853a8c9d11e1dab86` 通过 fmt、严格 Clippy、真实 MySQL 8.4.13 工作区测试、拒警 Rustdoc 与离线依赖检查。唯一示例经 TCP 调用标签和对象替换。新生成 minimal 应用通过 Cargo check 与测试，并实际调用登记的对象处理器。

## 翻译缺键和回退顺序限定验收

AR-N01/L041 的值读取、存在性和复数选择共享不可变快照。缺键处理器收到原始调用上下文，在替换之前调用；存在性检查与同译器重入抑制缺键处理。分组回退回调可重排、去重或清空语言序列。固定 Laravel 通过 PHP 8.5 核对 16 个结果，包含 `has` 内重入和自定义资源语言与复数规则语言不同的场景。

冻结树 `bf011379b6b35a3c9d9f49fcdc75e35c75165820` 通过 fmt、严格 Clippy、真实 MySQL 8.4.13 工作区测试、拒警 Rustdoc 和离线依赖检查。唯一示例经 TCP 触发应用缺键处理。新生成 minimal 应用通过 Cargo check 与测试，并实际验证两个钩子的登记和清除。

## 文件翻译多路径与局部覆盖限定验收

AR-N01/L041 的文件 Loader 区分有序普通路径、全局 JSON 专用路径与命名空间提示。普通分组递归覆盖，发布目录只覆盖已登记命名空间。PHP 8.5 执行固定 Laravel FileLoader 采集 7 项参考结果，覆盖层优先级、局部数组覆盖、安全子目录、缺失与未知命名空间。Rust 保留分组 JSON 格式、全局消息校验及按键排序的公开资源表示。

冻结树 `cc2bc8de0f25e712a166cfa5db1d7f1f30aa5c88` 通过 fmt、严格 Clippy、真实 MySQL 8.4.13 工作区测试、拒警 Rustdoc 和离线依赖检查。唯一示例经 TCP 使用根应用的追加路径，生成 minimal 应用通过 Cargo check 与测试并实际验证子目录分组和局部覆盖。默认 Loader 单例与延迟生命周期仍在唯一待办。

## 翻译职责命名限定验收

默认公开模式为 `TranslationLookup::Standard`，目录增强继续显式选用。翻译生产源码及数据库来源扩展生产 Rust 源码的品牌词扫描零命中，旧公开枚举引用及内部函数名在调用方、生成模板和文档中零命中。固定对照链接和版权声明保留。

冻结树 `816461f1b792b7f2006608910065b0494d8c14de` 通过 fmt、严格 Clippy、真实 MySQL 8.4.13 工作区测试、拒警 Rustdoc 和离线依赖检查。默认模式与显式 Standard 通过同一字面键和语言回退断言，生成 minimal 应用通过 Cargo check 与测试并实际调用 Standard。

## 加密与 Cookie 职责命名限定验收

应用具体类型为 `Encrypter`。具名 `encrypter` 与类型化应用契约共享实例及装饰器。OR1 扩展使用 `ContextualEncrypter`，保留独立密钥环和认证上下文。固定 PHP 四种 AES 载荷、轮换前的名称绑定 Cookie 与旧 OR1 密文均可读取；错误 Cookie 名和错误上下文继续被拒绝。Cookie/Session/CSRF 回归核对同一应用装饰器。

冻结树 `4b6eabb4b1d02a4cf2d7e9807f565a8b749b0fe8` 通过默认和 Session/Fortify/OIDC/View 的严格 Clippy、真实 MySQL 8.4.13 工作区测试、拒警 Rustdoc 和离线依赖检查。新生成 Web/Fortify 与 JWT API 应用实库通过，公开 Encrypter 解开由具名服务写入的属性密文。外部 Keycloak 专项与完整加密契约继续开放。

## Cookie 公开中间件与例外限定验收

冻结树 `9de8eada2c1d6f4f504dd8b2ac42427d4010df1e` 对照固定 Laravel `d52c3b3` 的 `Illuminate\Cookie\Middleware\EncryptCookies`。实际路由使用新公开类型和原请求范围，双应用的队列及显式 Cookie 验证例外隔离。Provider 安装与容器直接重绑均拒绝超限，Web 构建精确拒绝 Session 和 XSRF 例外。旧路由快照仍持有原配置。新 Web 生成应用经真实 Web 路由观察例外 Cookie，再运行 Fortify 的 CSRF 和 Session 流程。

默认及 Session/Fortify/View 的 workspace 测试、严格 Clippy、格式、Rustdoc、离线依赖检查、真实 MySQL 8.4.13 事务回滚与 Web/JWT 生成项目 check/test 均通过。首次默认门禁有一次所有权日志锁重取失败。保留原始失败，同一冻结树的精确探针和原命令复跑通过，具体锁持有者未确定。PHP 子类序列化和进程静态重置仍待对应。

## 日志职责命名限定验收

固定上游的默认单文件及应急文件使用 `storage/logs/openrust.log`，应急记录标签为 `openrust`。实文件验证默认、空应急路径、显式应急路径及频道配置修复。正常频道名称独立，既有 application.log 和 laravel.log 内容保持原样。唯一示例通过 HTTP 注入具名 logger 并实际写入新路径。

冻结树 `222ea32f794ad4c3c7d85fba37b6f8268cfa60e1` 通过 fmt、严格 Clippy、真实 MySQL 8.4.13 工作区测试、拒警 Rustdoc 和离线依赖检查。CLI 生成快照通过。fresh minimal 通过 check/test，实际写入自定义日志文件及默认文件，并在默认文件验证 openrust.EMERGENCY 记录。

## 云入口开关命名限定验收

固定 Laravel 的部署变量仅替换品牌段为 OPENRUST_CLOUD。唯一示例的真实 TCP listener 验证精确值 1 启用，两个旧变量、非精确值、应用 dotenv 和伪造请求头均无效。显式空列表及不含连接来源的代理列表均优先。实测客户端 IP、协议、主机和路径前缀。

冻结树 `488d85c66685e88b55dabade2b02b31cff325344` 通过 fmt、严格 Clippy、真实 MySQL 8.4.13 工作区测试、拒警 Rustdoc 和离线依赖检查。fresh minimal 通过 check/test，并由实际进程使用公开 RequestInfo 验证无开关和旧开关返回 http，新开关返回 https。实际云部署仍开放。

## 双因素事件命名限定验收

对照 Fortify c456642 的八个源文件，字符串身份为 `OpenRust\Fortify\Events\<Type>`。固定名称回归覆盖全部八项。启用事件的类、具名和通配符监听收到同一次派发，旧名称不匹配。唯一示例的账户流程经应用事件总线观察写入完成后的单次启用事件。独立真实 MySQL 回归验证具名监听收到原事务，事务末检拒绝时因子、审计和队列写入共同回滚。

冻结树 `3399ba7be46474f52d1279b3a3704aa923ea0300` 通过 fmt、默认和 Session/Fortify/View 严格 Clippy、真实 MySQL 8.4.13 工作区测试、拒警 Rustdoc 与离线依赖检查。新生成的 Web/Fortify 和 JWT API 项目通过 check 与实库测试。

Rust 导入冻结树 `cec10c7a7747adc0916ec6eac97529aaf9dfd1b5` 通过 fmt、默认及 Session/Fortify/View 严格 Clippy、真实 MySQL 8.4.13 工作区测试、拒警 Rustdoc、离线依赖检查及两类新生成项目 check/test。八个事件从 `openrust::fortify::events` 导入，编译测试核同一类型。逐项派发核三种监听、精确及通配符载荷、上下文与旧选择器拒绝。Web 和 JWT 生成应用均在真实启用动作收到单次事件，JWT 监听取得原事务。唯一示例与 MySQL 监听失败回滚回归通过。

## 第一方源码与生成模板命名收尾

普通实现注释和生成说明使用职责名称，整数校验私有实现使用 `valid_integer_value`。固定上游对照、许可、项目定位及示例实际调用的官方指南 URL 保留。分类扫描覆盖 Git 跟踪文本和文件名，并覆盖 fresh minimal、Web/Fortify、JWT API 三个生成目录的文本和文件名，未发现未分类生产残留。

冻结树 `64014794c56405188a21ef04bbe40dcd75a789b4` 通过默认及 Session/Fortify/View 的严格 Clippy、真实 MySQL 8.4.13 工作区测试、拒警 Rustdoc 和离线依赖检查。整数校验原回归和生成布局快照通过；三个 fresh 项目均通过 check/test。最终验收树 `dc0edba86c2141e346429b031c7f5212b2305f89` 较运行测试树仅三个文件四行普通 Rust 注释勘误，行数和可执行代码相同，并重新通过格式、差异及名称扫描。

## 默认文件 Loader 与无资源语言限定验收

固定上游具体类型已对齐为 `openrust::translation::FileLoader`。冻结树 `27f637b9abcaf54e35fdd223eca331f79d3f73e9` 通过默认和 translation-database/view 的严格 Clippy、真实 MySQL 8.4.13 工作区测试、拒警 Rustdoc 和离线依赖检查。fresh minimal 用公开类型实测文件路径与局部覆盖，缺资源语言仍可启动。

框架默认用应用根的 `lang`。公开译器和 Loader 分别可按类型或具名键解析，均共享实例。根应用和生成应用沿相同根路径规则。默认目录缺失、合法语言暂无资源时仍可回退或返回原键。显式坏目录、无效资源和不安全语言标签保持拒绝。运行时全局语言 setter、变更事件和 Provider 延迟装配仍开放。

冻结树 `5ec676adacb8c9ff4708fe76cc876d726750b544` 通过 fmt、默认及 translation-database/view 严格 Clippy、MySQL 8.4.13 工作区测试、拒警 Rustdoc、离线依赖检查。真实文件测试验证应用根覆盖、默认目录缺失、显式任务语言和资源移除刷新；fresh minimal 通过 check/test，并以无资源的 `fr-CA` 和 `de` 成功启动命令。

## 应用语言目录选择限定验收

冻结树 `ab3c17630171dc3214551652caa56b57b7fcc458` 对照固定 Laravel 13.x 本地化章节与 `d52c3b3` 的 Application 目录选择。根应用和生成项目均以 `resources/lang` 目录为先，缺失或为普通文件时选根 `lang`。两处目录不合并。显式根 `lang` 与模块命名空间保留。固定 Goravel `a554ced` 的文件 Loader 提供 JSON 文件和显式路径的编译型参考，目录默认遵循 Laravel。

框架单元测试覆盖空目录、普通文件、坏资源及构造时快照，唯一示例的真实 TCP 读取和生成 minimal 应用按语言配置实际验证。默认工作区格式、严格 Clippy、测试、Rustdoc、离线依赖检查，以及可选翻译数据库与真实 MySQL 8.4.13 测试通过。Linux ARM64 的根与生成项目均从仅含 `resources/lang` 的源码经 Buildx 导出。生成项目与框架在含空格目录重定位，镜像及搬迁后的导出目录均实际通过 HTTP 读取该目录文案。原生 x86_64 和完整 Laravel `path.lang` 公开接口仍按既有待办推进。
