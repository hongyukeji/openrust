# 框架对齐实施 Agent 提示词

以下正文可整体复制给开发 Agent。它要求实际执行，不能用另一次建议或只读清点代替开发完成。

---

请在 `/Users/Home/Codes/Rust/openrust` 持续实施已经记录的开发计划。先完整理解用户要求及当前源码，再修复、验收和更新记录。

## 先读文件与当前事实

先读根 `AGENTS.md`、`.agents/project/README.md`、`.agents/project/conventions.md`、`.agents/project/current.md`、`.agents/project/todo.md`，以及范围内其他AGENTS.md。随后完整读取：

1. `.agents/project/plans/framework-parity-remediation-2026-11-22.md`：同一文件内包含26条对话要求、现行决定、调研来源、具体开发计划和计划核对记录，须完整阅读。
2. `.agents/project/plans/framework-parity-data-2026-11-22.json`：`work_items`包含193条任务（116项main、77条validation_rule，parent指向C07）；`audit_snapshot`是原快照；`audit_tables`保留14张原始审计表，包括188行方法映射、全部声明／候选／裁决队列、类型／helper／规则／分类／覆盖清单。每张表用columns字段顺序解释rows数组，全部值保留原CSV字符串。完整读取数据，不能只读前若干行；后续契约裁决写入audit_reviews并标明数据集、行索引与类型／方法，历史表不改写，任务状态在work_items维护。
3. 主计划的“原始审计全文（历史快照）”附录：原审计正文已全文合并；本批交接只有主计划、统一JSON和本提示词3个文件，不再寻找旧审计目录或旧任务CSV。
4. `.agents/project/plans/laravel-rebuild-baseline.md`、`.agents/project/plans/locale-bcp47.md`、`.agents/project/audits/laravel-fixes-verification-2026-11-22.md`及主计划所链接的功能验收。

开始时核对实时HEAD、索引、全部WIP／未跟踪文件、活跃进程和构建归属，保留所有协作者改动。本计划重做时读取HEAD为 `8277bdb4a4d8a5156780ea8fc959a140d7bc56c6`，原审计为 `d4a40d7f46e708e6e2b12dee3033148042984638`；不能拿旧快照覆盖当前代码。已有middleware／三语文档／api_throttling等在途工作可能变化，按实际冲突协调。

特别纠正旧计划：BCP 47已由 `02fdc4ce8` 本地实施并有验收记录，后续 `24563cc34` 修正欢迎页回退。默认资源／文档是 `en`、`zh-Hans`、`zh-Hant`。不要重新当作未开始迁移，更不能恢复旧下划线目录和公开别名；其他部署自有数据／旧载荷升级及Windows／真实供应商仍需条件，历史验收不自动证明本次修改通过。

固定比较基线：Framework v13.40.0／`1449bf241017087fbd38c118ecaabef3f6797fdc`，应用v13.11.0／`06d016a364a37430eec9cbc52209adce3d7667ce`，文档 `960eedf0911155ad16326ade49c0ea5d18eda0b8`。本地完整参考在 `target/tmp/laravel-code-audit-20261121/`，执行时确认现存来源和版本，不自动升级基线。

## 当前实施停点

本计划已进入实现。接续前读主计划“当前实施与验收”及JSON的audit_reviews，不能按历史缺失表从零重写。当前Lottery、消息袋、部分manager/锁/进程/译器接口和Validation77条原缺失规则已有代码与定向证据；其中23条标量有199组固定PHP属性方法对照，25,499组完整固定Validator对照及41项定向回归通过，含真实DNS/文件/时钟，严格Clippy及三语站点验收通过，严格Clippy通过。既有/typed规则与全部输入语义矩阵及完整API/类型/模块/包裁决仍未完成，不能把定向通过扩张为整组全量完成。之后的代码/正文按实时状态重验。

正文/JSON断言已新增，3035组正文/实体、60组allowed tags、4064组JSON真实固定TestResponse+PHPUnit12.5.8对照通过。之后C13的2128组真实Session Store对照及C15捕获配置/相对URL准备正在复验，不能把代码出现当完成；接续先检查最新test-response-session日志。C40真实file跨进程回归已通过。最新安全停线清理记录为disk-cleanup-content.json（11profile，净释放63.518466048GB）；不重复清空热缓存。

## 实际执行

先Q01事实刷新和Q02账本建档，再按S1—S8依赖持续开发；Q02随各模块持续裁决，不要求先写完一万行结论才开始修已证实问题。优先JsonSchema、ViewErrorBag、Validation完整规则、真正流式sink与Reverb完整signin，同时完成依赖。每簇包含源码、公开消费者测试、实际入口及文档，不只补名字或编译通过。

完整覆盖C01—C54、R01—R14、X01—X24、E01—E19和Q01—Q05，共116项主任务，及77规则子任务、原188方法子项。对10550处声明、1651类型、120 helper线索、163文档线索、38核心模块与19包逐项核对；数字均是原静态清单，有交叉且未做全量当前语义认证，不能当独立缺陷总数或对齐率。新发现同样建任务，不能只做54组便宣称全仓对齐。

核对每条的所属类型、可见性、继承／trait／宏／Facade、参数／默认值／返回、错误和执行时机。名称存在不等于契约等价，缺少名称不等于能力不存在。PHP动态能力逐项给typed映射及可验证职责，不以“Rust不同”豁免可实现行为，不造空类型、空命令、no-op、常量假结果或错签名包装。

每个功能簇先查固定实现和测试，按规范查旧版可复用代码，再联网核对GitHub成熟项目／官方生态／Rust库。参考本次已实际读取的Blueprint、PostgresGrammar、jobs迁移、PendingRequest、SocialiteProviders与EasyWeChat来源，并为后续功能补固定来源、许可、采用理由及必要差异。不能只写“以后调研”；联网失败如实记录，不编造来源或改用浮动最新版。

必须遵守用户已经作出的决定：

- 队列用标准jobs／job_batches／failed_jobs；禁止恢复旧五张辅助恢复表、追加发布账本、逐投递完成账本和专有恢复命令。保留正常批次、失败重试、取消、追加和链。SMS自己的Outbox职责单独核对，不误删。
- 表／字段／状态注释简短准确，状态按真实值说明；复用现有Translator／FileLoader／.comment，按配置语言执行；新增语言增资源而不改核心三语分支。迁移文件名、结构、正反向和ledger对照固定应用，不删除数据来重建。
- 微信三入口继续归 `services.wechat.web`／`official_account`／`mini_app`，模块归wechat；驱动保留weixinweb／weixin，QQ保留qq，小程序保持独立code2session。复用Socialite、HTTP及state，不新增登录管理器、表或自动账号合并策略。
- BCP 47保留script/region，`zh-CN`不等于`zh-Hans`；浏览器选语是应用策略，不能污染Translator精确查找或改变输入标签。扩展消费者和真实升级边界按既有方案核对。
- 保留Settings、数据库翻译、Activitylog、Pay、OIDC、SMS、gRPC等功能；可选服务／配置／迁移归所属Provider和现有包发现，不预装到默认骨架。既有安装升级明确处理ledger，不擅自删表或搬迁开发数据。
- 品牌实际核对覆盖自有源码、注释、提示、Cargo元数据、模板、README及三语正文；以OpenRust为主体，外部协议／包名／来源／许可真实保留。不要为清关键词改动协议或伪造归属。
- 不恢复rust-toolchain.toml，保留Cargo rust-version，显式Rust1.94.0。保持正常并发、增量和可复用热缓存，不全局限为3、不禁增量、不要求每批冷构建。
- 输入／图像／Hash／捕获资源上限逐项评估默认值和可配置性，明确保留差异，不为对齐直接删除防护。

Q03必须调查产物增长及规则实际失效链路：进程、缓存路径和归属、TMPDIR／target对子进程传递、复用、正常／失败／取消收尾和磁盘停线。以证据定位后修实际入口，不以再写一条规范或降低开发性能代替修复。Q04完成品牌全域核对，Q05保护队列简化与工具链历史决定。

19包分别建固定参考和API／命令／Provider／驱动／存储／生命周期映射；Framework清单不能替代包审计。E清单逐项给可移植能力、PHP工具职责、实际替代及未覆盖部分；计划中的优先级不是用户批准的排除项。未实现仍列未提供／待开发，不得随意标“不适用”清空任务；也不为同名创建空包。

## 验收和连续工作

实施期间复用这三个交接文件维护任务与进度：统一JSON的work_items记录逐项状态、命令和证据位置，主计划集中记录阶段验收及未完成条件，提示词保持交接入口；统一JSON中的历史审计表与快照保留原事实，新增逐条裁决集中写入audit_reviews，详细运行日志留在项目target/tmp，不按每项任务新增分散文档。每条记录当前裁决、上游／本地锚点、代码和文档、实际命令／环境／结果、失败和修复映射、剩余条件。把实施状态、与上游关系、验收状态分别写清，避免“保留差异”自动变“通过”。原组关闭不能代替子项验收。

用实际Cargo清单、测试文件和读取的URL／REQUIRE变量确定命令。必须覆盖公共API正反例、真实应用／Artisan／Worker、失败恢复、事务与并发隔离；断言自身也要证明错误输入会失败。共享SQL验PG18＋MySQL8.4＋SQLite，共享缓存验Redis8＋database＋file，database覆盖三库。使用现有隔离夹具和专属测试库，每用例对象也隔离，不用串行化掩盖共享表冲突。缺变量提前返回、ignored或没起服务不算通过。

使用cargo +1.94.0及锁文件，内层Cargo传RUSTUP_TOOLCHAIN=1.94.0；在完整独立源码快照（含当前WIP与生成输入）和工作流独占复用CARGO_TARGET_DIR执行。临时源码、日志、生成应用和产物只放项目根target/tmp，TMPDIR传给全部子进程。执行受影响测试、fmt和严格Clippy；涉及开发者文档三语同步，按现有指南执行内容检查、站点构建、真实浏览器根／子目录和API消费检查。

大构建前与阶段边界检查磁盘；可用空间≤32GB（31,250,000KiB）停止新增构建，按规范协调现有构建安全退出、核对归属并取得Cargo排他锁，自动清理本项目全部编译产物，保留锁inode、源码、WIP、日志、依赖下载、独立交付、数据库与数据卷，复查实际df释放量。禁止整删根target或移到项目外绕过管理；安全锁拿不到时记录具体阻塞，不能带锁外删除。

Fortify偶发Session失败保留原失败证据，重试通过不等于找到根因。真实微信／QQ／支付／投递／AWS／托管服务／Passkeys设备、正式域名和Windows条件不足时，列明未验证目标与恢复条件；模拟、容器或虚拟设备结果与实际目标分开。继续不受阻塞的开发和验收，不因单项外部条件缺失停掉其余工作。

持续做到本轮内部可执行任务处理和验收完成；确有无法自行解除的条件，保留具体未完成任务，不通过修改文档口径宣称完成。不得因人为额度百分比或额度不可读停线。修改限任务范围，保留协作者工作。

用户最新授权计划内开发与验收，并要求已完成且验收的依赖闭合批次及时本地暂存、提交；只提交所属改动，保留协作者WIP。未授权推送、版本发布／部署、真实资金或破坏性数据操作。必要核心改动由当前实施者直接完成，无须例行转交或等待核心负责人批准，只有真实文件冲突或外部阻塞才协调。

最终用表格逐项列：任务ID／实际处理／代码与文档／实际验证／与上游仍有的差异／未完成及恢复条件。分别汇总已验证等价、必要Rust适配、用户扩展、尚未提供生态能力、外部阻塞和未裁决项。不能把记录完成当开发完成，不能用整体百分比或“全部1:1”替代证据。

当前追加事实：C09运行期值/规则/消息/译器/typed异常工厂和C53延迟事务Builder已实现，40项定向通过、1浏览器专项未执行，严格Clippy/fmt/三语构建通过。证据mutation-pipeline-second.log、pipeline-{sqlite,postgres,mysql}.log。C09扩展/replacer和C53动态容器等继续处理；不可把当前checkpoint当成整体关闭。

C08追加：命名扩展/隐式/依赖/replacer/resolver、Factory trait/Facade、实例单/复数入口及include/exclude数组键策略已实现。extensions-fifth.log55项和92扩展/16数组策略固定PHP对照通过，严格Clippy及三语站点通过。官方Precognition助手此前忽略项现已真实浏览器验过；不得仍写为本checkpoint未验。动态PHP、通用:index等替换和完整typed域继续核对，整体未完成。第二次32GB停线安全清理净58.524GB，详见主计划。

最新接续：C11–C15受支持范围已通过89项回归、追加metadata7项与TestApp地址专项，2128组Session参考及捕获取消/恢复已验。严格Clippy与三语build/browser/scope通过（testing日志）；随后正在补Query来源/join/HAVING与条件，查看query最新日志，不将旧快照验收当新源码结论。完整差异与Q02仍开放。

最新Git接续：已分批提交c84c4a9fb、ba651bbbd、8ff569d77、3de36e733、244683407；b74bfa561移出误纳入的协作者模板测试，文件仍在工作区。Query C16–C19补验中，不能混同此前Clippy验收。第四次安全清理见disk-cleanup-query.json；后续构建使用tests/build_guard.py入口，保留既有独占target与项目内TMPDIR，不清空热缓存。8项守卫回归本次重跑通过。

2026-11-23接续：C16–C19受支持核心156组固定PHP、44项本地及隔离PG3+1/MySQL3实际通过，新增真实Session重定向错误袋16项通过；fmt、严格workspace Clippy及三语站点全流程通过。详主计划Query补验节，保留typed/driver/空HAVING差异；C20分类绑定未实施。
