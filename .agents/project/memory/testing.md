# 测试基线

## 当前数据库运行政策

- 日常功能簇、main组合及数据库型生成项目默认真实MySQL8.4；普通无数据库生成、真实HTTP和其余适用门禁保持。PG18/MySQL8.4服务与支持范围不删减，编译/features不算真实后端验收。
- PostgreSQL/其他后端专属SQL、驱动、故障，或明确方言/迁移/锁差异风险，按实际影响当场执行必要定向，不机械重复全套双库。
- 发布前统一执行全部已实现/承诺支持后端（含PG/MySQL及SQLite等对应范围）、版本和迁移/事务/数据安全/队列/集群/生成项目矩阵；最低版本和跨主版本风险按发布规则。未实现、未执行或失败项保留发布待验/阻塞，日常MySQL通过只支持日常范围提交。
- 已运行PG命令自然结束并完成自有资源清理；未开始的常规PG阶段不再安排，不停删共享PG服务、测试或feature。历史双库证据原样保留，以下历史记录不自动成为新发布范围通过。
- CI常规数据库job为MySQL；手动`database_scope=release-backends`保留原PG18集成子矩阵，不代表完整发布已验收。后端专属例外按具体风险定向执行并留证；发布积压以[todo](../todo.md#发布前全后端兼容性验收)为准。

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

本簇实现见 [ADR-084](../decisions/adr-084-session-previous-and-precognition.md) 与 [HTTP](../core/http.md)、[Session](../core/session.md)。不关闭整个HTTP/Session、完整FormRequest或Laravel验证规则集。

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

按ADR-083交付受管理RouteMethods、真实Route/参数/名称通配与可信应用Resolver；全局前后、路由中间件/短路、错误、domain候选、group/nest/merge/forward、GET/HEAD/QUERY/custom/any、OPTIONS/fallback/静态与请求隔离均有真实生产回归。完整Routing、可选参数/声明defaults/约束/模型与枚举绑定、previous/Precognition不由本簇关闭。

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

旧部分验收保留在`/tmp/openrust-c04d7-preflight3-independent-evidence-index-v2.json`（`99141a96b89c3df6544e4cf45abab4ff21736d668b51a1db1fae314a18971fda`）；只证明当时源码且根清理尾stdout缺失，不代替final3。最终final3已完整封存根清理检查，旧证据缺口没有通过补造成功行消除。不同模型源码与实证复审均无blocker/should-fix。最终收尾仅改项目文档，不改已验收运行源码；限定本地提交以Git为准，不push/tag/release。

## C04-D6 Request URL（限定子范围已验收）

公开API与差异见[ADR-078](../decisions/adr-078-request-inspection.md#c04-d6请求地址与查询串契约)。本簇覆盖Info的请求origin、URL、解码路径/分段/星号匹配/prefetch，以及唯一Request当前query增删；原始OriginalUri与GET/HEAD当前输入分开，JSON输入优先于HTTP方法。未关闭真实路由命中、previous、Precognition、TrustProxies完整转发Host/Port/Prefix。

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

实际新生成目录为`/tmp/openrust-c04d6-generated-{plain,auth-postgres,auth-mysql}`，不是旧D1/D5项目；三份request_input helper均与当前模板逐字节一致（SHA256 `43074067dcb7161c26b03937114afe2fe0f1ad1d662559956d57d4e7fae91ff9`），真实调用普通GET、JSON GET与JSON POST三类URL变更断言。两线Symfony固定源码与真实PHP8.5各198 canonical+7操作参考，Rust逐ID全对照；非法UTF-8两项单列400适配，raw NUL仅私有helper，不冒充HTTP wire允许。复跑源码与来源在[fixture](../../../tests/http/fixtures/laravel-request-url/README.md)。

失败与纠正保留边界：

- 首次模块失败因fixture给GET统一加JSON头；按固定Laravel优先JSON来源，拆三case而非改框架或放宽普通GET page3/replace4断言。
- 真实PHP golden首次揭露fixed-020嵌套Cookie前缀过滤偏差；撤销UrlQuery模式、复用原Input变量树，php_variables.rs对HEAD无diff，两份golden期望未变。修后29项Request定向与完整workspace均通过。
- 库Clippy首次文档/切片lint、完整all-targets测试断言/路由长度lint均已修并按原严格门禁通过；没有跳过测试或CI。
- Redis服务短时消失使默认CLI真实复用测试失败；在既有共享服务恢复后原断言通过，未由本任务新建或重配置容器。
- 全特性首次gRPC readiness在5秒期限失败；精确单例原期限通过后整套1393通过。不宣称根因已修；源码可触发的临时端口释放竞争/启动任务错误被就绪轮询掩盖仍需后继诊断整改。
- 网络首轮仅到业务ready后，独占bind未listen负例在macOS产生TimeoutError；两次真实5.003秒探针确认后，仅明确接受拒绝或超时、保意外连通必失败，并记录类别。最终实录TimeoutError约5.001秒，不冒称精确ECONNREFUSED。TLS/业务断言均未削弱。

12门禁各次actual_exit=0/source_changed=[]；早期门禁后唯一运行文件变化为已审网络负例平台修复，Rust/模板/golden未变，最终serverbuild与network使用新完整快照。验收后仅文档收口另执行diff/链接检查，不冒称重跑Cargo。资源收尾：锁兼容probe与dry-run通过，锁内回收root target 20,511,793,152 bytes；随后实际执行`cargo clean --dry-run`及`cargo clean`均exit0（`/tmp/openrust-c04d6-cargo-clean-cli.result.json`），root target最终0。五个旧consumer缓存文件树hash/bytes保持不变，总target约2.486656GiB；本任务三fresh目录与新PHP源码树已按精确owner清理，旧B2 PHP runtime、旧D5目录、所有D6证据及三共享容器均保留；无构建/服务遗留。最后CLI清理检查磁盘余量约17.774%。批量清理日志`/tmp/openrust-c04d6-final-clean.log`与临时资源结果`/tmp/openrust-c04d6-temp-clean.result.json`实际通过，不把“计划清理”当完成。

## C04-D5 GC lottery（限定子范围已验收）

公开行为与固定源码SHA见[ADR-082 D5](../decisions/adr-082-session-store-and-flash.md#d5-gc-lottery与公开driver契约)。默认`[2,100]`在start后、业务前调用实际请求驱动的GC；DB/Memory按已持久绝对expiry清理，Cookie/Redis保持noop/TTL职责。与Laravel当前lifetime回溯last_activity、Array严格小于的安全差异仍公开，不以本簇关闭完整Session。

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

契约见[ADR-082 D4](../decisions/adr-082-session-store-and-flash.md#d4实现契约与明确差异)。以下均为本簇实际运行；忽略项不算真实服务，代理TLS不算Rust原生TLS或集群。本簇完成限定子范围，不关闭整个Session或已公开差异。

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

四份固定官方源码哈希见[来源记录](../../../tests/http/fixtures/laravel-session/provenance.json)。PHP读取表达式探针不是完整官方类或Laravel请求链oracle；异常弱类型差异保持开放。客户端Cookie重放/无全局撤销、秒精度与浏览器容量边界分别记录，不声称整个Session能力完成。

## C04-D3 Session扩展生命周期（限定子范围已验收）

契约见[ADR-082 D3](../decisions/adr-082-session-store-and-flash.md#d3扩展生命周期契约与验收边界)。D2的3cabae2不继承为D3通过；完整Cookie driver另列紧邻D4。

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

契约见[ADR-082 D2](../decisions/adr-082-session-store-and-flash.md#d2基础store方法与公开扩展契约与验收边界)。D1已由d47162d提交，其完整验收不继承为D2通过。

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

契约见[ADR-082](../decisions/adr-082-session-store-and-flash.md)，当前实现见[Session](../core/session.md)。以下定向结果仅证明各自源码快照，不代替后续修改后的最终门禁；本簇限定提交以Git为准。

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

契约见[ADR-081](../decisions/adr-081-request-response-cookies.md)。以下均为本簇实际证据，按范围记录，不沿用B2门禁；早期定向记录不替代最终快照；空domain修后根双库已按final独立证据重验。

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

- 已通过本簇适用门禁，限定公开Request/Fluent的标量、Stringable、保键Collection、枚举、clamp、条件分支、日期/区间和上下文；不代表完整Carbon工具箱或完整C04。contract与Rust适配见[ADR-079](../decisions/adr-079-request-input.md)，固定源/许可证/再生成见[SOURCES](../../../src/support/calendar/date/SOURCES.md)。
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

Spec、ADR-075、core/模块/CLI 文档与唯一队列已同步。该簇不改数据库读写契约，真实 DB HTTP 用来验证模块启动与应用装配；完整 Laravel/laravel-modules、模块管理命令、生产集群仍未完成。

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

## R2-A 类型化容器基元

新增 src/core/container.rs，15项回归实际通过：typed dyn接口、覆盖与bind生命周期、singleton并发单次构造/应用隔离、root scoped/reset与子scope、同链/跨根等待环、取消恢复、嵌套缺绑定码/链、source保留与秘密Debug/HTTP脱敏、在途reset、scope Drop缓存释放、保存Resolver延迟解析、关闭singleton owner/waiter、业务析构重入、弱句柄引用环释放。scope/工厂全用显式句柄，不创建全局容器。

冻结源码后实际执行 cargo fmt --all --check；默认及all-features workspace all-target Clippy -D warnings；cargo test --workspace（279 passed/22 ignored）与all-features（502 passed/32 ignored）；warnings-denied默认/full workspace doc；cargo deny check；cargo +1.97.0 check --workspace --all-targets --all-features；root no-default check，全部通过。忽略和无服务skip不记真实外部服务通过。本簇未改DB/queue/auth/配置加载或CLI模板行为，不将R1双库/生成验证转记新Provider通过。

/tmp/openrust-r2-container-consumer-mqyjcnb4 是仅依赖公开openrust的独立下游，check/test通过，1项真实公开dyn绑定/root reset/scope断言；首轮fixture错误使用Tokio重导出属性宏且未声明直接Tokio包，已改为公开Runtime显式block_on后重跑通过，未因此改变框架API。根argv恰为cargo run，/health、HTML、/api、/api/example各200，SIGINT0。启动仍是旧生产Plugin路径，不把它当新Provider已接入。

不同模型gpt-6-sol逐轮静态审查发现秘密Debug、错误码/链和Resolver强引用环，均修复并补测试；最终冻结弱句柄版本无blocker/should-fix，审查没有替代运行。Error保留内部source，Debug仅code/status，HTTP仍按5xx隐藏详情。无新依赖，Cargo.toml/Cargo.lock未变。

资源：独占/tmp/openrust-r2-target在软限阶段确认无构建进程后dry-run/clean回收63458files/19.3GiB；最终再次dry-run/clean回收45539files/14.1GiB，磁盘余119GiB。未知归属root/consumer缓存未触碰，日志和临时consumer源码保留用于诊断。日志前缀/tmp/openrust-r2-。

本次仅容器基元；Provider/Registry切换、异步shutdown/managed任务与HTTP/job自动scope、配置合并/模块overlay、TestApp和modules迁移均未完成。AppBuilder仍使用唯一旧生产引擎，无新旧运行开关；后继按R2合同和todo继续。

## R1 当前工程边界与验收入口

按 [ADR-073](../decisions/adr-073-root-application-and-example-module.md)，根 openrust package 同时提供框架 src/lib.rs 与默认 server 薄 src/main.rs。default-members=["."]、default-run="server"；Laravel 应用职责直接位于根，唯一 app/example 通过 app/mod.rs 注册薄业务 Plugin。以下是结构迁移子步骤的新执行记录，不表示 R1 综合 CRUD、全能力示例或 Laravel/modules 1:1 目标已完成。

本布局已实际通过 fmt、默认/all-features workspace all-target Clippy、默认/all-features workspace tests、warnings-denied 默认/all-features Rustdoc、deny 和 no-default check。默认 263 passed/22 ignored，全 feature 486 passed/32 ignored；默认不再由独立 example 包隐式强开 auth/grpc 等 feature，不与旧默认测试计数机械比较。根显式 34 个测试 target 保留原框架+demo 的 33 个与原 example auth HTTP 测试；模块注册有/无、普通业务路由、声明错误数据库启动失败回归已执行。底层框架源码、dependencies/features 未改；根锁无新增或升级 registry name/version/checksum，仅移除 20 个不再由根示例 build 使用的工具节点。

根无参数 cargo run 实际验证 /health、公开 HTML、/api 和 /api/example 均 200，业务响应包含 chrono::Utc::now 产生的时间；SIGINT exit 0。现有真实子进程 SIGTERM 退出测试保留。根 CLI config show 转发通过。

PostgreSQL 18.6/MySQL 8.4.13 各执行 cargo test --locked -p openrust --all-features 的 db_connection/db_migration/auth_database/auth_runtime/queue/schedule/auth_mfa/auth_oidc/queue_crash_recovery/schedule_calendar/queue_notifications/example_auth_flows，共 12 targets/48 passed/0 ignored；testing backend database 集成各 1 passed。原 example 的认证/JWT/PAT/入队流程已迁为模块专有测试并通过同一根 Server 装配。第一轮 fixture 未声明 app.name 而沿用默认 OpenRust，预期 openrust 的断言失败；现显式声明 fixture 名称后双库重跑通过，未改框架默认值。使用既有服务和 TestDatabase 隔离库，不修改业务数据或服务配置；测试 helper 的 Drop 清理为 best effort，不能把未能确认归属的历史隔离库批量删除或宣称均已清理。

/tmp 新生成普通/PG auth/MySQL auth 三类应用，均 check/test 通过，双库 auth 各 1 项真实 TCP 流程通过，普通工程公开 resource 宏回归与无参数 cargo run /health 200、SIGINT0 通过。进一步在普通工程仅添加 app/orders、修改 app/mod.rs 注册清单和声明已有 serde/chrono 依赖，src/main.rs 与 bootstrap/app.rs 哈希保持不变；check/test 与真实 /api/orders HTTP、正常退出通过。这是新增模块/第三方业务依赖使用证明，不是完整 CRUD。

根没有示例 build.rs/build-dependencies。模块自有 proto/generator.toml、generator.lock 在 /tmp 临时工具中 cargo run --offline --locked；generate.py --check 实际验证 generated/echo.rs 重生字节一致。生成类型通过 grpc feature 编译，不等于已注册本模块真实 gRPC 服务。

最终 CI 双库命令均通过：每库 cargo test --workspace --features db-<backend> 为 278 passed/22 ignored，auth,db-<backend> 为 324 passed/25 ignored；另显式 cargo test -p openrust --features auth,queue,view,encryption,db-<backend> --test example_auth_flows，各 2 passed/0 ignored。MySQL 使用具备隔离建库权限的测试管理员，未改服务配置。

Rust 1.97.0 的 cargo check --workspace --all-targets --all-features 通过，SQLite/queue consumer 实际 run、gRPC consumer locked check 通过；stable 下八个 consumers locked test/metadata 全通过，metadata 均独立单成员/默认成员。cache/communication/SQLite/queue/gRPC 实际 run 通过，gRPC 15 项与四类 RPC、realtime 真实 Redis 跨进程 1 项通过。SQLite 仍仅是自定义 driver consumer，不等于完整官方支持。

最终 README 更新后重新执行默认/full workspace fmt/Clippy/tests/docs、deny/no-default，全部通过，计数仍为 263/22 与 486/32；根裸 run/HTTP/退出再次通过。额外干净生成 gRPC 应用 check/test 通过（无业务测试，因此仅证明模板/Proto 编译，不代替真实 RPC）；生成项目锁独立解析，未修改源码根锁。模块 proto --locked 离线重生字节再次一致。代码独立审查的 proto 锁定/教程导入修复已落实；不同模型 gpt-6-sol 最终静态源码复审无 blocker/should-fix，未参与CI编辑的代理独立复核也无 blocker/should-fix，限定提交收口中。

最终确认无 Cargo/rustc/server 进程后，对独占 /tmp/openrust-r1-root-target 先 cargo clean --dry-run 再 cargo clean，实际回收 44051 files/14.1 GiB，磁盘剩余 118 GiB（约13%）。本任务在软限时也只于安全阶段清理该独占目录；未知归属根 target 与预存 consumer 缓存不清。/tmp 生成项目和日志保留作本轮证据。后继未挂载 app/example/extensions.rs 与未审 R2 contract 是独立 WIP，不计本次已验证功能。

以上仅为 ADR-073 过渡路径的 R1-S 结构子交付。最终 modules/example 目录迁移依 ADR-074/R2-M；统一 Provider、完整 CRUD、全能力示例、供应商实发及完整集群尚未交付或不在本子步骤范围，不能转记为这些目标通过。

## R1 ADR-072 历史验收事实

以下是 14c143f 及其前置布局的实际记录，保留原命令和目录以追溯当时证据；它们不是当前可直接运行的路径或本次 ADR-073 验收。

按 [ADR-072](../decisions/adr-072-framework-and-application-projects.md)，根 openrust/src 是框架库，examples/demo 是根 workspace 成员，共用根 Cargo.lock，无嵌套 workspace。根 default-members=["examples/demo"]，根与 demo 目录无参数 cargo run 默认 server；全部原 tests/server 迁入 examples/demo/tests/server。CLI 模板是唯一骨架来源；本轮迁移实现与适用验收已完成。

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

## R1 旧布局验收事实（ADR-071）

以下保留旧 ADR-071 根 application/server 与 framework/openrust 布局的实际验收；不能作为 ADR-072 新布局通过。目录与分发依据旧 ADR-071。根为 application/server，框架库为 framework/openrust，默认成员只有根应用；apps/example 已迁为 examples/example。下文保留的原专项记录是迁移前行为证据，含 `cargo test -p openrust --test ...` 的旧命令；当时应用集成 target 使用 `-p application`，框架单元测试仍用 `-p openrust --lib`。

实际执行：fmt，默认/all-features workspace all-target Clippy，默认/all-features workspace test，默认及 warnings-denied all-features Rustdoc，cargo deny；MSRV `cargo +1.97.0 check --workspace --all-targets --all-features`，no-default-features check。依赖审计仅保留既有重复版本告警。最终默认 workspace 349 passed / 26 ignored，all-features 480 passed / 32 ignored；忽略或缺服务导致 skip 不计真实服务通过。

真实 PostgreSQL 18 与 MySQL 8.4 各执行 `cargo test -p application --all-features --test db_connection --test db_migration --test auth_database --test auth_runtime --test queue --test schedule -- --nocapture`，各 6 targets、31 passed、0 ignored；从现有服务读取授权测试连接，使用 TestDatabase 隔离数据，不改共享容器/配置。数据库连接 target 中依赖真实复制的分支仍有独立环境门禁，本次未提供复制拓扑；完整复制/集群不属于 R1 新验收。

干净 `/tmp` 默认生成应用 check/test 通过，普通 cargo run 后真实 TCP `/health` 为 200、SIGINT 正常退出。PG/MySQL 认证生成应用分别 check/test 通过，`generated_auth_routes_run_over_http_with_isolated_database_and_fake_mail` 各真实执行 1 项；使用当前模板、database/migrations 与 resources/views。发现模板 fake 配置缺 view 段后已同步修复模板/综合示例并重新生成验收。公开 `tests/consumers/cache-custom-driver` 已通过 locked 实际运行，所有 consumer lock 同步本地框架依赖图。

根普通 cargo run（无 --bin/--package）真实 TCP `/health` 返回 200，SIGINT exit 0。

验证隔离：默认 cargo run 与 all-features 子进程测试禁止交叉占用同一 target/debug/server。首次 all-features metrics 子进程超时发生在并行默认 cargo run 覆盖二进制期间；停止交叉运行后整组重跑通过。此项不是新语义验收或放宽超时。原真实供应商/复制/集群、SQLite、最低后端版本与 R2 以后 contract 缺口继续保留。

## M5 WebSocket 与 SSE 跨节点路由

按 [ADR-059](../decisions/adr-059-realtime-transport.md) 启用 `realtime` / `realtime-redis`。在本机已有 Redis 8.10.2 上通过独立示例执行：

```bash
OPENRUST_REALTIME_TEST_REDIS_URL=redis://127.0.0.1:6379/ \
  cargo test --locked --manifest-path tests/consumers/realtime/Cargo.toml --test cluster -- --nocapture
```

2026-10-27 实际通过：两个独立 API 子进程经 Redis Pub/Sub 路由 WebSocket 和 SSE；真实 TCP 客户端验证 Origin allowlist、跨节点用户撤销、SIGTERM 时两类会话关闭与进程退出。测试随后启动一个 Redis 地址为 `127.0.0.1:1` 的 API 进程，确认在线发布返回 `realtime_transport_unavailable`，不存在本地静默成功。另 `cargo test --manifest-path examples/demo/Cargo.toml --features realtime --test realtime -- --nocapture` 通过 AuthLayer、Origin 与 SSE 响应头聚合 HTTP 测试；实时 service 单元测试 4 项覆盖过期、载荷/Origin 限制、连接投递/撤销、慢消费者上限。

本簇门禁实际通过：workspace fmt、默认/all-features all-target Clippy、默认/all-features workspace tests、warnings-denied all-features workspace Rustdoc、根与独立示例 cargo-deny、独立示例 all-target/all-features Clippy 及 `git diff --check`；真实跨节点命令如上。cargo-deny 的 advisories/bans/licenses/sources 均通过，保留既有重复依赖与未匹配 MPL allowance 警告。整组验证结束时根 `target/` 与 `tests/consumers/realtime/target/` 合计约 13 GiB，所在卷可用 86 GiB（低于 10%）；无构建进程、Cargo dry-run 分别预览根 target 12.5 GiB 与示例 target 2.7 GiB 后，对两处 target 执行 Cargo clean，回收约 15.2 GiB；清理后仓库 target 均为空，卷可用空间为 99 GiB（约 10.7%）。软告警剩余量由仓库外磁盘占用造成，未删除其他文件。

该证据不代表 Redis 断开期间 Pub/Sub 可恢复消息，也不代表 PostgreSQL/MySQL 全栈集群、负载均衡、worker/scheduler、滚动升级或性能门禁通过；M6 仍需复验。用法与边界见 [tests/consumers/realtime/README.md](../../../tests/consumers/realtime/README.md)。

## M5 gRPC G1 基础与生成

按 [ADR-040](../decisions/adr-040-grpc-integration.md) 增加可选 `grpc` feature、配置校验、服务注册/冲突检测、默认及具名共享 Channel、`grpc serve` 命令、应用自有 proto/build.rs 生成和 loopback TCP 示例。默认 feature 不启用 tonic/prost/protoc；G1 只接受明文 `http://` client endpoint，TLS/认证/health/配额由 G2 交付。

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

## M5 ADR-054 TOTP MFA 与恢复码（F7 子簇）

`cargo test -p openrust --features auth-mfa auth::mfa::tests --lib` 覆盖 RFC 6238 SHA-1 向量、6 位格式及允许时间步边界。`examples/demo/tests/server/auth/mfa.rs` 的真实 TCP/数据库生命周期测试在 PostgreSQL 18 与 MySQL 8.4 各通过 1 项：`OPENRUST_TEST_DATABASE_URL=... cargo test --manifest-path examples/demo/Cargo.toml --features 'auth-mfa auth-oidc db-<backend>' --test auth_mfa --test auth_oidc -- --nocapture`。覆盖 password 与 OIDC challenge、Session 登录/既有 Session 阻断、挑战来源绑定/过期/五次失败、TOTP replay、恢复码单次消费、enrollment/禁用/重置、密钥轮换、审计与 refresh 撤销；服务端测试还验证此前密码登录 helper 无法绕过 MFA。

CLI 新增 `openrust new --db <backend> --auth --auth-mfa --encryption`。把当前 tracked/untracked 源码复制到干净临时 workspace（无 registry patch、无开发者绝对路径），分别生成 PostgreSQL 与 MySQL 应用；两个生成包均实际通过 `cargo check` 和 `cargo test`，其中生成的完整 auth HTTP/fake-mail 测试各执行 1 项并使用 PostgreSQL 18 / MySQL 8.4 隔离数据库。`cargo test -p cli` 18 项通过。

MFA 子簇门禁：fmt、默认/all-features workspace all-target Clippy、默认/all-features workspace tests、默认/all-features docs（all-features warnings denied）、no-default-features check、`cargo deny check` 均通过。deny 仅有既有重复 crate 版本与未匹配 MPL-2.0 allowance 警告；advisories、bans、licenses、sources 均通过。Twilio 真实发送仍受外部授权凭据/目标约束，与 MFA 验收无关；Keycloak 单 client OIDC 真实验收见上一节，完整 SSO 为后续独立 F7 子簇。

## M5 缓存驱动与命名实例

`cargo test -p openrust cache::tests` 通过 memory namespace/TTL/remove、file key hash/过期/替换/并发、命名 stores 和外部 driver 注册；`cargo test -p openrust --features cache-redis cache::tests::redis_store_roundtrips_ttl_on_live_service -- --exact` 与 `cargo test --manifest-path examples/demo/Cargo.toml --all-features --test server cache_plugin_uses_the_configured_shared_redis_store -- --exact` 均在本机 Redis 8.10.2 真实服务通过。CLI 实测复用可达 loopback Redis，不触碰 Compose；干净 `/tmp` 源码副本生成默认应用后 `cargo check`、`cargo test` 通过，独立 custom-driver 示例 `cargo test/run --manifest-path tests/consumers/cache-custom-driver/Cargo.toml` 通过。默认与 all-features workspace tests、all-target/all-features clippy、fmt、warnings-denied 默认/all-features docs、cargo deny 与 no-default-features check 通过。workspace server 的数据库集成 target 因未设置 `OPENRUST_TEST_DATABASE_URL` 跳过；本缓存簇不修改数据库路径，不以该 skipped target 代替双库验收。Redis 启动 Compose 的冷启动分支尚未在现有可用 Redis 环境下触发；已验证的是复用路径。Redis 最低兼容版本仍待 v1 发布前专项确认。

## M5 F4a 缓存 tag generation

`cargo test -p openrust cache::tests` 覆盖 memory generation 失效、命名空间隔离、旧 entry 写入拒绝、无能力 file driver 拒绝；Redis 真实服务测试使用两个独立 Redis 连接验证共享读取、代际失效和旧 entry 回填被拒绝，`根目录集成宿主` 聚合测试通过真实 Redis 验证应用集成。Redis tag generation 不设置 TTL，tagged value 必须有 TTL；未创建真实跨进程双节点测试，F4c 最终矩阵仍需补足。F4a 的 workspace default/all-features tests、clippy、warnings-denied docs、fmt、cargo deny 和 no-default check 均通过；数据库集成测试因未配置 `OPENRUST_TEST_DATABASE_URL` 跳过，与本簇无关。

## M5 命名数据库连接实例（ADR-045）

`cargo test -p openrust --features db-postgres named_instance -- --nocapture`、`database_selector` 与 `migration_action` 定向测试通过。`cargo test --manifest-path examples/demo/Cargo.toml --features db-postgres --test db_migration named_database_instances_keep_migrations_and_transactions_isolated -- --exact --nocapture` 和 MySQL 对应命令各通过 1 项，使用 PostgreSQL 18 与 MySQL 8.4 真实隔离库。测试同时配置 default、analytics、reporting 三个独立实例，验证连接身份、默认句柄兼容、具名副本读取、prefix、显式 migration 目标、未迁移表隔离及事务数据不串用。具名副本使用对应实例自己的隔离数据库 URL，验证的是实例绑定边界，不是物理复制延迟；复制语义由读写分离专项另行覆盖。

干净源码副本生成 `apps/named_database_sample` 后直接 `cargo check -p named_database_sample` 与 `cargo test -p named_database_sample` 通过；扫描应用及根 manifest 未发现临时 registry patch 或机器绝对路径。默认/all-features workspace tests、all-target clippy、warnings-denied docs、fmt、cargo deny 与 no-default-features check 通过。`cargo deny` 保留既有重复依赖及未匹配 MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。未覆盖跨数据库事务、自动查询路由或生产集群负载。

## M5 Redis Session driver

启用 `session-redis` feature，通过 `RedisSessionStore::connect(dsn, namespace)` 与 `SessionPlugin::with_store` 显式使用 Redis；数据库 Session store 仍为默认。真实 Redis 8.10.2 测试命令：`cargo test --manifest-path examples/demo/Cargo.toml --features session-redis --test auth_runtime redis_sessions_are_shared_between_apps_and_revoke_across_instances -- --exact --nocapture`。PostgreSQL 18 和 MySQL 8.4 分别运行 `cargo test --manifest-path examples/demo/Cargo.toml --features db-<backend>,session-redis --test auth_database --test auth_runtime`，各 14 项通过（auth_database 7、auth_runtime 7）；两个独立 App 使用两个 Redis connection manager，共享同一 namespace。验证真实 Redis 创建/读取/TTL 过期/删除、重复创建拒绝、删除后 save 不复活、不同 namespace 隔离、ID 轮换清除旧记录、跨 App 登出撤销、CSRF 缺失拒绝与有效 token 放行；namespace 校验、URL/Debug 脱敏及不可达 endpoint 的启动失败也覆盖。用例走访现有 Redis 8 服务，没有修改或停止容器。

`cargo +1.97.0 check -p openrust --no-default-features --features session-redis` 与 server feature check 通过。默认/all-features workspace clippy、test、warnings-denied docs、fmt、no-default check 与 `cargo deny check` 通过；deny 仍有既有重复版本与未匹配 MPL allowance 警告，advisories/bans/licenses/sources 均通过。两个 App 在同一测试进程内，未代表多进程集群门禁；Redis 6.2 最低版本兼容和在线 Redis 故障期间的端到端恢复留待发布/集群验收。

## M5 Redis Queue driver（ADR-046）

本次使用已运行的真实 Redis 8.10.2 服务。`cargo test --manifest-path examples/demo/Cargo.toml --features queue-redis --test queue_redis_driver` 3 项通过：四个独立连接 worker 对 40 个任务竞争、活动唯一键、失败保留/重试/幂等 retry ID/进度、独立子进程强杀后租约到期恢复、非法 namespace 与不可达服务失败关闭。`cargo test --manifest-path examples/demo/Cargo.toml --features queue-redis --test queue_redis` 2 项通过：Redis QueuePlugin 无需 DatabasePlugin；存在数据库 scheduler 注册时 build 失败。另用 `examples/queue` Redis-only 启动 API，真实 TCP `POST /jobs` 返回任务 ID，再由独立 worker 使用同一 namespace 运行；两个任务进程均正常响应 Ctrl-C。

`cargo fmt --all --check`、默认及 all-features workspace clippy、默认及 all-features `cargo test --workspace`、warnings-denied 默认及 all-features docs、`cargo check -p openrust --no-default-features` 和 `cargo deny check` 通过；all-features tests 运行 Redis targets，workspace 其他 PG/MySQL integration tests 在未提供 URL 时依项目约定 skip。deny 输出有既有重复版本与 unmatched MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。Redis driver 不访问 SQL，故没有把数据库队列既有 PG18/MySQL8.4 验收冒充为 Redis 双库测试；Redis Cluster 分片、Redis 6.2 最低版本、持久化/故障切换及生产多节点负载仍待 v1 总门禁。

## M5 HTTP 有界运行与就绪切换

2026-10-26 定向测试：`cargo test -p openrust serve::tests -- --nocapture` 覆盖未绑定/绑定就绪、drain 中停止接入、在途请求完成、非合作请求到期取消及 bind 失败；`cargo test --manifest-path examples/demo/Cargo.toml --test server serve_process_exits_successfully_after_sigterm -- --nocapture` 通过真实 TCP readiness 和真实子进程 SIGTERM，all-features 下同一测试额外注入仅供测试的加密 key 后通过。全量门禁：fmt、diff check、all-target/all-features clippy、默认/all-features workspace tests、warnings-denied 默认/all-features docs、`cargo deny check` 和 `cargo check -p openrust --no-default-features` 均通过。deny 输出既有重复 crate 版本与未匹配 MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。此项未改数据库/队列协议，不需要双库测试；未覆盖集群依赖 readiness 或跨节点 drain。

## ADR-039 源码交付验收

当前 workspace packages 为根 `openrust` 与 `tools/` 下的 `cli`、`macros`、`testing`、`performance`；根 package 内含 `server` bin，所有 package 均显式禁用发布，内部依赖通过 workspace path 解析。仅根 package 是 `default-members`，以保证裸 `cargo run` 选择 `server`；完整验证显式使用 `--workspace`。2026-10-27 目录收纳后，Cargo metadata、CLI fixture 与 workspace 检查确认这五个 package 及 `db_routing` bench target；PostgreSQL 与 MySQL feature 转发的 benchmark 编译检查通过，真实数据库 benchmark 未因目录调整重跑。CLI 保留 `openrust` 命令，并默认生成 `apps/<name>`，应用与 migration 加入现有 workspace。生成器保留根 manifest 其他内容，拒绝重复名、现存目录和不支持的外部目标；dev/serve 按目标应用定位二进制和配置。

2026-10-26 干净源码副本（无 `.git`、临时 registry patch 或开发机绝对路径）生成默认、PostgreSQL、MySQL 与加密认证应用。默认应用 `cargo test` 通过；PostgreSQL 18 和 MySQL 8.4 的加密认证应用各通过 `cargo test`，真实数据库 HTTP auth flow 均执行 1 项通过。两个 workspace 应用以不同配置及端口分别启动，HTTP 根路由返回各自应用名，证明配置和启动目标隔离；CLI 单测覆盖重复/非法/外部目标失败无半成品。

本仓库最终门禁：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`、`cargo deny check`、`git diff --check` 均通过。另在真实 PostgreSQL 18 与 MySQL 8.4 服务分别执行 openrust/testing/server 的数据库、认证、队列、崩溃恢复与调度 feature 集成测试并通过。`cargo deny` 仅有既有重复依赖版本和未使用 MPL allowance 警告；advisories/bans/licenses/sources 均通过。C1–C3 文档整改不属于本簇。

以下 `-p openrust-*` 命令只代表历史验收；当前命令使用新包名。历史临时 patch 不作为 ADR-039 黄金路径证据。

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

CLI 模板快照测试：`cargo test -p openrust-cli`。生成 PostgreSQL、MySQL `--auth` 项目后运行 `cargo fmt --all --check`、`cargo check`、`cargo test`。旧模板的历史集成验证使用 Cargo patch 指向本地根 `openrust` 与 `testing`。新的源码交付验收按 ADR-039，在干净源码副本中生成 apps 后直接运行，不继续以 patch 作为默认操作入口。

2026-10-26 根模块迁移后，两种生成项目都以真实 PostgreSQL 16 / MySQL 8.4 服务运行 `generated_auth_routes_run_over_http_with_isolated_database_and_fake_mail` 并通过。测试覆盖实际 HTTP、JWT/PAT 认证、数据库流程、可信代理 client IP 与 fake mail。

2026-10-26 当前日常基线复验：`cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-postgres --test auth_database` 在现有 PostgreSQL 18 容器通过 4 项；`--features auth,db-mysql` 在现有 MySQL 8.4 容器通过 4 项。按当前 CLI 模板新生成的 `--auth` 项目分别以本地 Cargo patch 指向未发布 workspace，并在真实 PostgreSQL 18 / MySQL 8.4 服务上通过 `cargo fmt --all --check`、`cargo check` 和 `cargo test --test auth_flows`（每库 1 项真实 TCP HTTP 流程，使用隔离数据库与 fake mail）。生成目录位于 `/tmp/openrust-auth-pg-resume` 与 `/tmp/openrust-auth-mysql-resume`，没有纳入仓库。

2026-10-26 ADR-031 出站 HTTP：历史基线为 `cargo test -p openrust --features http-client http::client::tests` 15 项及 `根目录集成宿主` 聚合接入 1 项；CLI `--http-client` 生成项目 check/test 通过，证据保留于原验收记录。

2026-10-27 ADR-051/052 出站策略与韧性：`cargo test -p openrust --features http-client http::client::tests -- --nocapture` 21 项通过，涵盖目标策略、DNS 固定、重试上限/抖动/Retry-After、单次 body 限制；`cargo test --manifest-path examples/demo/Cargo.toml --all-features --test http_client -- --nocapture` 6 项通过，聚合 HTTP consumer 验证目标拒绝、依赖隔离、容量与等待上限、取消释放、熔断打开/半开恢复。总门禁 `cargo fmt --all --check`、`cargo test --workspace`、`RUST_TEST_THREADS=1 cargo test --workspace --all-features`、default/all-features workspace all-target clippy、warnings-denied default/all-features Rustdoc、`cargo check -p openrust --no-default-features`、`cargo deny check`、`git diff --check` 全部通过。`cargo deny` 仅输出既有未匹配 MPL-2.0 allowance 与重复版本警告，advisories/bans/licenses/sources 均通过。无需外部服务；只访问回环 TCP 测试服务，不访问公网目标、云 metadata 或供应商。

2026-10-26 ADR-033 邮件/SMS：`cargo test -p openrust --features 'mail-smtp,sms-twilio,test-util' mail::tests:: -- --include-ignored` 通过，含真实本机 SMTP 对 Mailpit v1.32.0 的投递；`curl --noproxy '*' http://127.0.0.1:18025/api/v1/messages` 确认 `user@example.test` 收到 `Welcome`。Twilio transport tests 覆盖 POST URL、Basic auth、form/template fields、2xx receipt、4xx脱敏、timeout及单次调用不重试；测试不访问 Twilio。CLI 9 项测试与 `--sms-twilio` 生成项目 `/tmp/openrust-sms-generated-20261026` 的 check/test 通过；default/all-features workspace test、all-features clippy、warnings-denied docs、cargo deny 与 diff-check 通过。未提供实际 Twilio 账号凭据与授权号码，真实 SMS 投递仍未验收。

2026-10-26 ADR-032 请求级国际化：6 项 `i18n` 单测与 5 项 `examples/demo/tests/server/i18n/runtime.rs` 通过，覆盖 locale 规范化、Accept-Language q 值、TOML config、JSON resource 合并/参数/fallback、请求并发隔离、错误/validator message、稳定 code/字段路径、未翻译应用错误保留与显式 locale override。新生成项目 `/tmp/openrust-i18n-generated-20261026` 使用本地 Cargo patch 通过 `cargo check`、`cargo test`；其真实 server 进程收到 `Accept-Language: zh-CN` 后对 404 返回中文 message。default/all-features `fmt --check`、workspace clippy/test、warnings-denied docs、`cargo deny check` 通过。无数据库行为，不要求双库集成；邮件/SMS 的实际供应商验收留在 M3 通信簇。

## 迁移验收记录

- `cargo metadata --no-deps --format-version 1` 确认根 package 为 `openrust`，workspace 共四个成员：根 package `openrust`、`cli`、`macros`、`testing`。
- 无默认、默认、关键单独 feature、all-features 编译已检查；workspace default/all-features clippy 与 test、warnings-denied rustdoc、`cargo deny check` 均通过。`cargo deny` 仍会输出重复版本依赖提示，但无 advisory、license、ban 或 source 失败；JWT 使用 `aws_lc_rs`，不再解析到受 RUSTSEC-2023-0071 覆盖的 `rsa 0.9`。
- M2 数据库读写分离的 PostgreSQL/MySQL 真实复制拓扑已单独通过（见上文）；其余尚未验收的范围包括 Redis 最低版本、生产负载下多 worker/旧 token 竞态、M6 全栈集群、S3 bucket-version 专项与完整性能门禁，继续按 `todo.md` 和各专题计划推进。实时在线跨节点单项验收见本文件 M5 ADR-059。
- PostgreSQL 14 / MySQL 8.0 结果是历史兼容性证据；当前暂定下限已采用 PostgreSQL 14+ / MySQL 8.0+，但这些结果不替代发布前对完整承诺能力的最低版本复核。Redis 6.2+ 的真实兼容性在 M5 driver 落地后验证。

2026-10-26 ADR-034 应用数据加密/本地轮换：`cargo test -p openrust --features encryption security::tests` 8 项通过，包含 RFC 8452 AES-256-GCM-SIV 向量、二进制/空明文、随机 nonce、context、active key 写入/历史 key 解密、篡改/格式/越界、配置脱敏与多 Encryptor 互解；`cargo test -p openrust-cli` 11 项通过，包含 32-byte Base64 key generation 与 `--encryption` 模板；`cargo check -p openrust --bin server --features encryption` 通过。实际 CLI 生成项目 `/tmp/openrust-encryption-generated-20261026` 使用本地 Cargo path patch 通过 `cargo check`、`cargo test`，并用环境 key 执行 `cargo run -- config show`；输出中的 active key ID 与 key map 值均为 `********`。最终 default/all-features workspace tests、all-features clippy、warnings-denied default/all-features docs、`cargo deny check` 和 `git diff --check` 通过；cargo-deny 保留既有重复依赖与未使用 MPL allowance 警告，advisories/bans/licenses/sources 通过。该簇无数据库读写，不要求新增双库集成验收。测试应用 all-features 时显式注入测试 key ring；生产默认配置不含 key。

2026-10-26 ADR-035 审计簇：`cargo test -p openrust --features test-util audit::tests` 6 项通过，覆盖 allowlist、敏感详情/Debug 脱敏、错误传播、database/custom plugin 和公开 fake；PostgreSQL 18 与 MySQL 8.4 的 `cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test auth_database` 各 5 项通过。新增测试使用 `TestDatabase` 隔离库和完整 audit 表名映射，验证权限变更与审计同事务提交、外层回滚、审计 insert 失败导致权限回滚，以及最多 N 行的重复清理。CLI 新生成的 PostgreSQL 与 MySQL `--auth` 项目均通过 `cargo fmt --all --check`、本地 Cargo patch `cargo check` 和真实 `auth_flows` HTTP/TCP 数据库流程测试。workspace default/all-features tests/clippy、warnings-denied default/all-features docs、no-default-features check、`cargo deny check` 通过；deny 仅有既有重复依赖与未使用 MPL allowance 警告，advisories/bans/licenses/sources 通过。

2026-10-26 ADR-036 signed links：`cargo test -p openrust --no-default-features --features signing signing::tests` 4 项通过，覆盖用途/subject/TTL、Debug 脱敏、shared store 撤销与保留旧 key 的轮换；PostgreSQL 18、MySQL 8.4 完整 `cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test auth_database` 各 6 项通过，覆盖两个 signer 实例跨库撤销、自定义完整表名映射、过期记录有界清理及应用插件注册。新生成 `--auth` 项目分别使用本地 Cargo patch，通过 `cargo fmt --all --check`、`cargo check`、真实数据库迁移及 TCP HTTP auth flow。最终 `cargo fmt --all --check`、`git diff --check`、workspace default/all-features clippy/test、warnings-denied default/all-features docs、`cargo check -p openrust --no-default-features`、`cargo deny check` 均通过；deny 仅有已存在的重复依赖与 unused MPL allowance 警告，advisories/bans/licenses/sources 通过。没有新增依赖。

2026-10-26 M3 认证/安全总验收：本机预置 PostgreSQL 18 / MySQL 8.4 容器凭据与仓库 CI 默认值不同，未触碰预置数据库；使用仓库 `compose.ci.yaml` 的无持久卷 PostgreSQL 18 / MySQL 8.4 服务、独立测试库后，`cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test auth_database --test auth_runtime` 各 12 项通过。按当前模板生成的 PG/MySQL 项目分别用真实数据库执行 `cargo test --test auth_flows`，各 1 项真实 TCP HTTP 流程通过。覆盖注册/重复注册、邮箱验证、JWT 与 PAT、refresh 单次轮换/重放撤销、密码重置、refresh logout、Session 登录/退出与 CSRF、共享限流、可信代理、Policy/RBAC 与 PAT ability 交集。生成项目各自还通过 `cargo fmt --all --check` 与 `cargo check`。临时 Compose 服务已停止并移除，既有 PG/MySQL/Redis 服务保持运行。

2026-10-26 M3 数据输出/查询边界：`cargo test -p openrust http::sort::tests` 覆盖排序解析、字段 allowlist、重复/未知/格式错误输入；CLI generator 单测覆盖 `--uuid-id` / `--soft-delete` 和生成 feature 注册。临时生成项目（本地 patch 到当前 workspace）分别在现有 PostgreSQL 18 与 MySQL 8.4 上通过格式检查、`cargo check` 和真实隔离数据库 test：迁移 UUID 主键及 UTC `deleted_at`，插入生成 UUIDv7，显式 `not_deleted()` 隐藏软删除行，普通 `find_by_id` 仍能读取，清空 `deleted_at` 后恢复。HTTP README 补充严格输入 DTO、响应字段 allowlist、显式 SeaORM Condition、关联预加载示例；没有引入 ORM 自动过滤或动态查询 DSL。测试只使用现有服务与临时目录，未输出凭据，未改动容器。

2026-10-26 M4 typed events/queued listener：`cargo test -p openrust --all-features events::tests` 与 `cargo test -p openrust --features queue --lib events::queued_tests` 通过，后者覆盖 mapper 成功、mapper error 与 Queue dispatch error 传播/短路。外部下游 consumer crate 在 `/tmp/openrust-events-consumer` 以 `queue,db-postgres` 通过离线 `cargo check`。真实双库命令均运行成功：`cargo test --manifest-path examples/demo/Cargo.toml --features queue,db-postgres --test queue database_queue_dispatch_worker_concurrency_failure_and_retry -- --exact` 在现有 PostgreSQL 18.6 通过；`cargo test --manifest-path examples/demo/Cargo.toml --features queue,db-mysql --test queue database_queue_dispatch_worker_concurrency_failure_and_retry -- --exact` 在临时 MySQL 8.4 通过。覆盖事件 transaction context、事务回滚不留任务、提交后 worker 消费、普通 dispatch 消费；测试显式释放 Events/Queue/Db/App 后 `TestDatabase::close` 正常删除隔离库。预置 MySQL 8.4 容器因容器内 socket PID lock 错误持续重启，未改动；仅使用 `compose.ci.yaml` 在空闲 53306 端口创建 `openrust-ci` 临时 MySQL，测试后 `docker compose ... down -v` 仅清理本次创建的容器/网络/卷。修复后的最终门禁通过：`cargo fmt --all --check`、`git diff --check`、workspace all-target/all-features clippy、default/all-features workspace tests、warnings-denied default/all-features docs、`cargo deny check`、`cargo check -p openrust --no-default-features`。未触碰预置 PostgreSQL/MySQL/Redis 数据，无凭据输出。

2026-10-26 ADR-041 事务与 AuthFlows 事件：在任务专属 Compose project `openrust-m4-auth-tx`（PostgreSQL 18、MySQL 8.4，端口 55432/53306）分别执行 `cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test db_connection transaction_events_rollback_before_commit_and_ignore_after_commit_errors -- --exact` 与 `cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test auth_database auth_flow_events_share_account_and_refresh_transactions -- --exact`，四项均实际运行通过。验证提交前拒绝回滚、提交后错误不掩盖已提交结果、注册校验拒绝不建账户、泛化登录失败事件，以及登录成功事件失败无 refresh token 遗留。临时服务由本任务创建；预置服务未改动。

2026-10-26 ADR-042 共享锁与请求幂等：在任务专属 `openrust-m4-auth-tx` PostgreSQL 18 / MySQL 8.4 服务，对每个 backend 实际执行 `cargo test --manifest-path examples/demo/Cargo.toml --features db-<backend> --test db_coordination -- --exact shared_locks_and_request_idempotency_are_database_scoped`，均 1 项通过。测试使用 `coord_` 自定义表前缀的 TestDatabase 隔离库及两个独立 SeaORM pool，覆盖迁移索引、共享锁 owner 竞争、过期接管/fencing、旧 owner 释放与事务内检查、同键并发仅一 owner、scope 隔离/指纹冲突、事务回滚同时撤销业务写入和完成快照、成功完成后的 JSON replay、过期 lease 接管及每次最多 N 行清理。测试运行时没有临时 services 以外的依赖；没有声称进程强杀、真实 HTTP middleware 或外部副作用 exactly-once。

2026-10-26 F2 国际化：`cargo test -p openrust --lib i18n::tests::` 9 项通过，新增覆盖结构化 plural/select、英文与波兰语 CLDR 类别、无效/缺失参数 fallback、按实际 fallback 资源 locale 选择复数规则、精确小数、英中日期差异及显式时区跨日；`cargo test --manifest-path examples/demo/Cargo.toml --test i18n` 6 项通过，包含并发请求 locale 及按请求语言选复数。干净源码副本 `/tmp/openrust-f2-source-wgP07b/source`（无 `.git`、无 registry patch/开发机路径）使用 `cargo run -p cli -- new i18n_sample --db postgres` 生成到 `apps/i18n_sample`，其 `cargo check` 与 `cargo test` 通过。工作区 `cargo fmt --all --check`、default/all-features workspace tests、default/all-features clippy、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`、`cargo +1.97.0 check -p openrust`、`cargo deny check` 通过；deny 输出为既有重复依赖版本警告，advisories/bans/licenses/sources 均通过。F2 无数据库读写行为，不要求 PostgreSQL/MySQL 集成验收；F1 已有双库证据不由此替代或重做。

2026-10-26 F3 安全邮件模板：新增可选 `mail-templates`（MiniJinja 2.24）、静态 `include_str!` catalog、locale 分层回退/布局继承、HTML 自动转义、严格缺参、100,000 fuel 与 256 KiB 有界输出、主题换行/长度校验及 text/html MIME alternative。`cargo test -p openrust --features 'mail-templates test-util' mail::template::tests` 4 项通过（含 FakeMailer）；`cargo test --manifest-path examples/demo/Cargo.toml --features mail-templates --test mail_templates` 1 项通过；CLI 邮件生成器定向测试通过。干净源码副本 `/tmp/openrust-f3-final-aKylsZ/source`（无 `.git`、无 registry patch/开发机绝对路径）生成 PostgreSQL 应用及 mailer，`cargo fmt --all --check`、`cargo check -p mail_sample`、`cargo test -p mail_sample` 通过。Compose Mailpit v1.32.0 使用 SMTP/UI 11025/18025（默认 1025 端口被占用），显式运行 `smtp_submits_rendered_template_to_mailpit` 成功；Mailpit `/api/v1/message/<id>` 确认 Subject `Greeting`、Text `Hello, Mailpit`、HTML `<p>Hello, Mailpit</p>`，随后停止本次启动的 Mailpit。default/all-features workspace tests、default/all-features clippy、warnings-denied all-features docs、`cargo +1.97.0 check -p openrust --features mail-templates`、`cargo deny check`、fmt 与 diff check 通过；deny 有既有重复依赖及未匹配 MPL allowance 警告，advisories/bans/licenses/sources 通过。本簇不改数据库行为，不需要新增双库测试；队列派发/通知重试边界留给后续功能簇。

2026-10-26 M4 生成与集成收口：CLI `generate job` 单测覆盖数据库前置、重复保护、多任务插件注册、queue feature 与 `QueueMigration` 登记、显式失败 handler。无 `.git` 的干净源码副本中分别生成 PostgreSQL 与 MySQL 应用并生成一个/多个 job；应用及 migration 的直接 `cargo check`、`cargo test` 均通过，manifest 无 registry patch 或开发机绝对路径。`queue-demo` 示例分别以 `queue,db-postgres` 与 `queue,db-mysql` 通过编译；PostgreSQL 18 隔离库中实际执行迁移，真实 TCP `/health` 返回 200，HTTP `POST /jobs` 派发，`schedule run --once` 派发 heartbeat，独立 `queue work demo` 进程消费两项任务。数据库服务为已存在的 `openrust-m4-auth-tx` PostgreSQL 18 容器，仅创建并删除本次隔离数据库；curl 明确绕过本机 SOCKS 代理。PG18/MySQL8.4 的 `queue_notifications` targets 本次各 1 项真实服务测试通过。`cargo fmt --all --check`、all-target/all-features clippy、default/all-features workspace tests、warnings-denied all-features docs、`cargo deny check`、queue-only feature check 与 diff check 通过；deny 只有既有重复依赖与未匹配 MPL allowance 警告，advisories/bans/licenses/sources 通过。M4 功能簇完成；仍未宣称生产负载或完整集群验收。

2026-10-26 M5 通用插件公开契约：新增外部 integration consumer target，只通过 `openrust` 公共 API 组合 Provider/Consumer 插件，验证 section 配置、服务、命令、HTTP 路由与异步 `finish`；缺依赖、重复服务/命令、无效配置值脱敏和重复路由启动错误均有覆盖，兼容 `insert`/`command` 的后注册替换也有测试。配置 section 错误保留完整字段路径且不输出配置值；应用路由组装将 Axum 注册 panic 转为 `http_route_registration_failed`，直接 Axum API 边界写入 Rustdoc/README。`cargo test --manifest-path examples/demo/Cargo.toml --test plugin_contract` 8 项通过；`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo test --workspace`、`cargo test --workspace --all-features`、warnings-denied all-features docs、`cargo deny check` 通过。deny 保留既有重复依赖与未匹配 MPL allowance 警告，其余 advisories/bans/licenses/sources 检查通过。本簇不修改数据库功能，不新增双库集成要求；后续 Db driver contract 需要独立 PostgreSQL/MySQL 验收。

2026-10-26 M5 Db 外部扩展契约：`DatabaseTableNames` 增加 SQLite 双引号引用与通用 `Db::initialize[_with_replicas]` backend 校验；保留 64-byte 通用标识符上限及 PostgreSQL 63/MySQL 64 backend 限制，未知 SeaORM backend 明确以 `unsupported_database_backend` 拒绝。v1 正式后端与 `DatabasePlugin` URL/feature 检查保持 PostgreSQL/MySQL。独立 `tests/consumers/db-custom-sqlite/` 使用相对 path 依赖、独立 workspace 和 `sqlx-sqlite`；`cargo +1.97.0 check` 及 `cargo +1.97.0 run --locked` 实际通过，真实 SQLite 连接、backend、重复初始化错误、表名 quoting、应用自有 SeaORM migration、事务提交/回滚均有运行断言；独立 package 的 stable fmt/clippy 通过，CI MSRV job 已添加运行入口。PostgreSQL 18 与 MySQL 8.4 分别实际执行 `db_connection::plugin_opens_pool_and_executes_a_query` 和 `db_connection::transaction_commits_success_and_rolls_back_callback_errors`，四项均通过。Workspace fmt、all-target/all-feature clippy、默认/全 feature tests、warnings-denied all-feature docs、workspace `cargo deny check` 与 `cargo deny --manifest-path tests/consumers/db-custom-sqlite/Cargo.toml --config deny.toml check` 通过；deny 报告既有重复依赖及未匹配的 BSD-2-Clause/MPL allowance 警告，advisories/bans/licenses/sources 检查通过。SQLite 不进入官方 v1 数据库矩阵，认证/队列/共享锁/限流和 `TestDatabase` 等专属能力不由本簇宣称兼容。

2026-10-26 M5 Queue 外部扩展契约：`JobRegistry` 与 `JobContext` 通过公开 API 支持外部 driver 注册/执行 typed Job、注入取消信号、进度 reporter 和 AppState；未配置 reporter 明确返回 `queue_progress_unsupported`，ownership token 保持内置数据库 driver 私有。`cargo test --manifest-path examples/demo/Cargo.toml --features queue --test queue_driver_contract` 5 项通过，覆盖无需数据库的 await 内直接执行、typed Job/进度 reporter、合作取消、未知任务、重复注册、无 reporter 显式失败及上下文边界；README 与 Rustdoc 明确该入口不派发/持久化、不提供事务、自动重试或任务组。`cargo check -p openrust --no-default-features` 通过。独立 nested workspace `tests/consumers/queue-custom-driver` 通过 `cargo run --locked` 实际派发并运行 typed Job，all-targets clippy 通过；无绝对路径或临时 registry patch。PostgreSQL 18 与 MySQL 8.4 分别实际运行 `db_migration`、`queue`、`queue_crash_recovery`、`schedule`、`schedule_calendar`、`queue_notifications`，各 16 项通过。Workspace fmt、all-target/all-features clippy、default/all-features tests、warnings-denied default/all-features docs、cargo deny 与 diff check 通过；cargo deny 仅报告既有重复依赖版本和未匹配 MPL allowance 警告，advisories/bans/licenses/sources 通过。外部 memory driver 明确不提供持久化/跨节点/事务保证，不代表 Redis driver 或集群验收。

2026-10-26 M5 跨协议验证与调用作用域（验证通过，尚未提交）：新增公开 `validation::Validate` / `validate` 并让 HTTP Validated extractor 复用原字段路径/错误转换；新增 task-local `ExecutionContext`、白名单 `ExecutionMetadata`（safe request ID/Locale）、层级 cooperative cancellation 和不超过父调用的 deadline budget。HTTP middleware 建立 request/locale/deadline/cancel scope，非法 request ID 移除后重新生成；`App::run_command` 建立匿名 scope 与默认 locale；typed Job 与 `JobRegistry::execute` 建立每次执行 scope，JobRegistry 执行入口强制 deadline/cancellation，数据库 worker 保留已有 timeout 与停机宽限控制，缺省 locale 使用应用默认值。数据库 queue 用有版本的 payload envelope 持久化 request ID/locale，legacy raw payload 保持可读、任务组继承 metadata、failed-job retry 原样保留；保留字段 `__openrust_execution` 在含 metadata 新派发中被拒绝，完整且合法的版本 envelope 作为框架格式，畸形旧结构按原 payload 保留；自定义旧 driver 收到非空 metadata 默认显式返回 unsupported。worker span 记录 job id/name/attempt/request_id，不记录 payload；request ID 关联不代表 W3C/Otel trace link。定向 HTTP timeout/cancellation 测试 1 项、queue external driver contract 4 项、JobRegistry deadline/scope 测试 1 项、metadata malformed/roundtrip 单测已实际通过。PostgreSQL 18：`cargo test --manifest-path examples/demo/Cargo.toml --features queue,db-postgres --test db_migration --test queue --test queue_crash_recovery -- --nocapture` 实际运行 migration 1、queue 1、crash recovery 5 项通过；MySQL 8.4 同一命令实际运行 1+1+5 项通过。两库 queue target 含持久化 worker 和任务链中的 request ID/locale round-trip；均使用既有 `openrust-m4-auth-tx` 健康服务及 TestDatabase 隔离库，未改容器或输出凭据。最终门禁已通过：`cargo fmt --all --check`、`git diff --check`、workspace all-target/all-features clippy、默认/all-features workspace tests、warnings-denied 默认/all-features docs、`cargo deny check`、`cargo check -p openrust --no-default-features`。`cargo deny check` 返回成功，仅报告既有重复依赖及未匹配 MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。本地 scoped commit 尚未创建。

2026-10-26 M5 F4b 单进程同键加载合并：新增有界 `CacheStore`/`Cache` `get_or_load_with`，键按 store、namespace、logical key、返回类型和稳定 load ID 隔离；配置校验 key/waiter 上限与 loader timeout。单测覆盖 8 路合并、类型/load ID 隔离、leader 取消重选、容量/等待者拒绝、超时与错误广播回收。`cargo test --workspace` 与 `cargo test --workspace --all-features` 通过；其中 all-features openrust 184 项通过、2 项 ignored；`cargo test --manifest-path examples/demo/Cargo.toml --features cache-redis --test server cache_plugin_uses_the_configured_shared_redis_store -- --nocapture` 在真实 Redis 上通过，覆盖 Redis tag 失效和应用聚合 singleflight。`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、warnings-denied all-features docs、`cargo deny check`、`cargo check -p openrust --no-default-features` 通过；deny 保留既有 MPL allowance 未匹配和重复依赖告警，advisories/bans/licenses/sources 均通过。此单进程功能无数据库写入，不要求双库测试；PostgreSQL/MySQL 集群一致性不由此记录宣称，跨节点 SWR 留待 F4c。干净源码副本中当前 CLI cache 模板生成 check/test 已于同轮执行记录（未使用 registry patch 或机器绝对路径）。

2026-10-26 M5 F4c stale-while-revalidate 与应用后台任务生命周期：定向 SWR 测试覆盖 fresh/stale/最终过期、tag 失效、刷新失败与 stale 上限、并发刷新限制、取消及停机 join；真实 Redis 双连接 server 测试通过。PostgreSQL 18 与 MySQL 8.4 分别以两个独立 App/数据库 pool/Redis 连接实际运行 `db_coordination::f4c_swr_uses_shared_locks_across_apps_and_redis_connections`，各 1 项通过，验证共享锁降低重复刷新并观察共享刷新结果；此为单进程多 App 专项，不代表多节点集群验收。干净源码副本生成应用直接 `cargo check`、`cargo test` 通过，无 registry patch 或开发机绝对路径。`cargo test --workspace`、`cargo test --workspace --all-features`、default/all-features all-target clippy、warnings-denied default/all-features docs、`cargo check -p openrust --no-default-features`、`cargo deny check`、fmt 与 diff check 通过；deny 仅有既有未匹配 MPL allowance 与重复依赖警告，advisories/bans/licenses/sources 均通过。完整集群、多节点缓存负载及外部通知未由本簇覆盖。

2026-10-26 显式同步任务执行：复用 ADR-043 的 `JobRegistry::execute`，没有增加重复 API。定向 `cargo test --manifest-path examples/demo/Cargo.toml --features queue --test queue_driver_contract` 5 项通过，新增纯进程内 await 行为断言，不配置数据库/Redis；明确进度需 reporter、取消为合作语义，且直接执行不提供事务派发、持久化、自动重试或任务组。workspace fmt、clippy、tests、warnings-denied docs 与 `cargo deny check` 均通过；deny 的重复 crate 版本与未匹配 MPL allowance 为现有告警。

2026-10-26 M5 F5 资源查询授权（ADR-047）：`cargo test -p openrust --features 'auth db-postgres' auth::query::tests` 验证非空范围构造、空 OR 组与规则错误 fail-closed，以及嵌套 OR 筛选仍位于 owner scope 的 AND 内。真实隔离库 HTTP 测试 `cargo test --manifest-path examples/demo/Cargo.toml --features 'auth db-postgres' --test auth_query_authorization` 与 MySQL `db-mysql` 对应命令均通过 PostgreSQL 18 / MySQL 8.4：owner 只能读取匹配行；越权 ID 返回空；分页只计授权行；混合授权 ID 批量修改/删除只影响本人行；关联查询校验父记录并用子实体规则过滤子集合。独立 consumer `cargo check --manifest-path tests/consumers/query-authorization/Cargo.toml` 通过。裸 SeaORM、直接关联加载、原生 SQL、自定义 driver 不被自动拦截；完整业务 CRUD、导出组合和集群压力仍待相应验收。

## M5 ADR-048 稳定键有界分批读取

`cargo test -p openrust --features 'auth db-postgres' batch_options_enforce_the_documented_row_bound` 验证默认 100、最小 1、最大 1000 及越界拒绝。PostgreSQL 18 与 MySQL 8.4 分别使用已有 loopback 服务运行 `cargo test --manifest-path examples/demo/Cargo.toml --features 'auth db-<backend>' --test auth_query_authorization -- --nocapture`，各实际执行 1 项通过：owner scope 分页读取、取消后用最后完整主键游标续读、无越权/重复行、复合主键升降序和跨页顺序、callback 错误停止及连接继续可用。测试凭据从既有容器配置在进程内构造，不输出凭据、不创建或更改服务。独立 consumer `cargo check --manifest-path tests/consumers/query-authorization/Cargo.toml --locked` 通过。

## M5 ADR-049 数据库迁移互斥

在既有 PostgreSQL 18 与 MySQL 8.4 loopback 服务上，分别执行 `OPENRUST_TEST_DATABASE_URL=<service-url> cargo test --manifest-path examples/demo/Cargo.toml --features db-<backend> --test db_migration -- --nocapture`，4 项均通过。真实隔离数据库和独立子进程验证首次迁移竞争只运行一个 migration callback、第二个 up 等待并读到最新状态、短等待预算返回 `migration_lock_timeout`、持锁进程强杀后恢复，以及既有 up/down/status 与具名数据库隔离。凭据从容器配置在进程中构造，不输出、不更改或停止服务。fmt、default/all-features all-target warnings-denied Clippy、default/all-features workspace tests、warnings-denied default/all-features docs、no-default-features check 与 `cargo deny check` 通过；workspace 运行时不配置的 DB 集成 targets 不代替上述显式双库执行。未宣称生产多节点迁移演练。

## M5 ADR-050 文件存储

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

## M5 ADR-060 密码哈希容量

使用 `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test -p openrust --features auth auth::password::tests --lib`，7 项通过；`cargo test -p openrust --features auth --lib`，160 项通过；`cargo test -p openrust --all-features --lib`，258 项通过、2 项忽略。`cargo test --manifest-path examples/demo/Cargo.toml --features auth --test auth_runtime auth_plugin_rejects_invalid_password_capacity_during_startup -- --exact`，1 项通过。PostgreSQL 18 和 MySQL 8.4 复用已运行容器，分别在 `OPENRUST_TEST_DATABASE_URL` 指向容器本地管理库时执行 `cargo test --manifest-path examples/demo/Cargo.toml --features 'auth db-<backend>' --test auth_database auth_flows_run_registration_login_refresh_reset_and_logout_on_the_configured_database -- --exact --nocapture`，各 1 项通过；URL 凭据从容器配置在测试进程内构造，未输出，数据由 `TestDatabase` 创建隔离库并在结束时删除。

门禁：`cargo fmt --all --check`、`git diff --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`、auth feature 下根库/`根目录集成宿主` 定向 Clippy 与 Rustdoc、`cargo check -p openrust --no-default-features`、`cargo deny check` 均通过。首次对仅 `auth` 的根包 all-target Clippy 出现已有的 `CurrentUser::test_user` dead_code 警告，随后匹配范围的库 Clippy 和完整 workspace all-target Clippy 均通过。cargo deny 的 advisories/bans/licenses/sources 通过，保留既有重复 crate 与未匹配 MPL allowance 警告。默认 workspace 未配置 DB URL 的其他集成 targets 按条件跳过；双库 AuthFlows 用例已显式执行。无 all-features workspace 矩阵记录；此子簇匹配 `auth` feature、默认 workspace 及双库用例均已覆盖。

## M5 ADR-061 依赖就绪探针

`Readiness::register_probe`/`dependencies_ready` 与 HTTP `/ready` 已实现；最多 64 个稳定命名探针，整轮 2 秒、最多 16 并发且同一时刻只运行一轮。探针失败仅在服务日志记录探针名与稳定错误码，HTTP 返回通用 503。探针注册/重复名、失败恢复、并发上限/单飞及 ready 503 脱敏单测通过；根 `cargo test -p openrust --lib` 在新增单飞测试前 139 项通过，随后相关 readiness 5 项与 health 1 项再次定向通过。PostgreSQL 18/MySQL 8.4 均使用现有容器，经本地 TCP 故障代理实测真实 DB pool 断连时探测失败、代理恢复后 pool 重新连通并恢复 readiness；健康 pool 探针各另通过 1 项。Redis 8 现有服务运行 `cargo test --manifest-path examples/demo/Cargo.toml --features cache-redis --test server cache_plugin_uses_the_configured_shared_redis_store -- --exact --nocapture`，真实 CachePlugin store 探针通过。`cargo clippy -p openrust --all-targets --features 'db-postgres,cache-redis' -- -D warnings`、`cargo clippy -p server --test db_connection --features 'db-postgres,db-mysql' -- -D warnings`、`RUSTDOCFLAGS='-D warnings' cargo doc -p openrust --no-deps --features 'db-postgres,cache-redis'`、`cargo deny check`、fmt 与 diff check 通过；deny 的 advisories/bans/licenses/sources 通过，保留既有 duplicate crate 与未匹配 MPL allowance 提示。随后完整 workspace 默认 Clippy 与 tests 已通过；此记录仅描述 ADR-061 专项证据，不代表当前 M5 全部验收完成。

## M5 ADR-067 低基数指标与 OpenTelemetry（WIP）

2026-10-27：可选 `metrics` feature 提供显式启用的 Prometheus `/metrics`，HTTP 指标使用固定 method/status 类标签；真实 TCP 子进程测试确认默认服务不提供该路由、配置开启后可抓取，正文无 request ID 或路由标签。HTTP middleware 有界并发超限快速返回 503。可选 `otel` feature 通过 OTLP/HTTP batch exporter 导出 spans；本机临时 Collector 0.120.0 实际收到 HTTP spans，并通过真实跨协议 consumer 观察到 HTTP 与 typed gRPC spans 保留同一个 W3C Trace ID。同步 exporter 在 Tokio blocking pool 中 flush，测试覆盖 server lifecycle drain。HTTP span 不写 URI/request ID/traceparent；OTel 属性只含协议字段。PG18/MySQL8.4 真实隔离库登录突发在 2 connection pool 下运行 16 次请求，两个后端均观察到密码哈希容量拒绝并在突发后成功登录；样本结果见当前任务记录，尚不足以建立正式回归阈值或分离 API CPU/内存基线。

当前轮门禁：`cargo fmt --all --check`、`git diff --check`、默认与 all-features `cargo clippy --workspace --all-targets`、默认与 all-features `cargo test --workspace`、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features` 与 `cargo deny check` 均通过；default workspace 中未设置 `OPENRUST_TEST_DATABASE_URL` 的集成测试按条件跳过。all-features workspace 测试 270 项通过、2 项忽略；默认 230 项通过。deny 的 advisories/bans/licenses/sources 通过，仍有既有 duplicate crate 与未匹配 MPL allowance 警告。PG18/MySQL8.4 突发单次样本分别为：PG pool=2、16 请求、4 成功/12 容量拒绝、623.1 req/s、p50 10.9ms、p95/p99 17.8ms、恢复 7.8ms；MySQL pool=2、16 请求、4 成功/12 拒绝、488.7 req/s、p50 16.8ms、p95/p99 23.9ms、恢复 8.7ms。单次本机小样本不能作为回归阈值；`/usr/bin/time -lp` 包含编译与测试进程，不能分离服务 CPU/内存。仍缺稳定重复压力模型、服务资源分项及过载下真实 API 吞吐/错误率报告，因此 ADR-067 保持 Proposed，M5 继续 in_progress。

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

R3-A 按 ADR-076 将 `HttpStatusMode::Semantic` 设为框架、配置、无请求上下文错误 fallback 与生成应用默认；`always_ok` 保留显式覆盖。默认 HTTP 行为测试覆盖创建 201、404、405、415、422、429、500，显式覆盖覆盖成功、异常、fallback、提取错误和 timeout 的 HTTP 200。另测试故障 I18n Provider 的早退分支：`semantic` 返回 500、`always_ok` 返回 200，响应头和正文 request ID 均与输入 ID 一致。metrics 未启用路由现按语义返回 404；原先把 200 当默认值的断言已改正。ADR-008 已明确旧 always_ok 默认是历史决策，索引登记 ADR-076。

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

状态：B1 实现与下述验收已有实际证据；最新源码的全部适用门禁已通过，最终资源收尾完成，限定本地提交见Git。不得据此宣称已完成全部 C04、Laravel Request 或严格 1:1。契约见 [ADR-079](../decisions/adr-079-request-input.md)。

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

固定Laravel为d52c3b3，Symfony8对象14案实际构造只作ADR-078独立固定比较点；Laravel无已解析composer.lock，不声称锁定该版本或完整PHP HTTP。共享构造器顺序为HTTP范围→空目标→允许状态→头；to先生成目标，与固定上游一致，guest先put/intended先pull未变。核心、调用方、驱动均经不同模型独审，无剩余问题。完整Redirector、action/signed、URL参数其余语义及HTTP/Session/发布全后端矩阵继续开放。


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


## R2-T ConfigRepository 严格类型读取（隔离分支限定验收通过）

基线 `a5e6461`；冻结Spec `/tmp/openrust-config-typed-helpers-frozen-spec.md`、最终ready `/tmp/openrust-config-typed-helpers-ready-final.json`。源码独审无确定阻断；集合位置索引显式类型标注修复首次 E0282 后，`/tmp/openrust-config-typed-helpers-verify-batch1-attempt2/receipt.json` 记录 fmt/diff、仓储43、Collection4、模块1、公开consumer6项实际退出0；`/tmp/openrust-config-typed-helpers-clippy-docs/receipt.json` 记录严格Clippy lib/server/consumer各1与Rustdoc1退出0。旧 E0282 与普通生成首轮因驱动假定全新项目已有 Cargo.lock 而失败的回执保留，不能计作生产行为通过。后段最终 `/tmp/openrust-config-typed-helpers-verification-attempt2/FINAL.json`（SHA256 `fb140cf667227fd346fa044f94f6d4119cd002b107c991fd86d2fd95481749b0`）记录12条命令退出0：fresh普通生成 fmt/check/test原7项、安装公开fixture后check/exact1项、根server构建与真实GET `/api/example/config-greeting` 返回200及固定问候语；进程78122收到SIGINT后exit0，端口43228释放，日志有界且源码/二进制稳定。唯一新生成包 `openrust-typed-93492a40e6e9` 精确clean退出0（Cargo回报2121文件/649.1MiB），post dry-run为0项；Cargo `-p` 对path/version限定会忽略，本次唯一随机包名事前核无同名产物，不能把该清理口径扩到已有共享包。全登记资源约55.83→56.83 GB（十进制），结束磁盘可用86,953,000,960字节，超50GB/10%软线但低于100GB/5%硬线；共享缓存未清，旧未知目标不碰。默认数据库无代码改动，本簇不把旧MySQL证据转记为新验收；尚待root最终本地提交及main组合验收，完整L005.03/Collection及发布全后端开放。
