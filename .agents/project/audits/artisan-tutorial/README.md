# Artisan Console固定教程逐节核对

2026-11-25，在Logging核心教程收尾后按交付映射进入本章。固定文档960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md，框架1449bf241017087fbd38c118ecaabef3f6797fdc。48锚点逐项登记，06-08历史已验机制作为复核起点，不能凭旧测试或同名命中关闭本章。

首轮公开入口核对：现有Artisan::command只接受名称及Output回调，不能解析教程签名或读取闭包输入；Input只暴露Clap匹配。CommandDefinition的isolatable只有名称锁和一小时租期，尚缺自定义ID／租期契约；缺少通用PromptsForMissingInput及命令级问答／表格／进度入口。Sail、Tinker生态独立核定，不能把计划命令或普通回调当成专用REPL。事件、生成／注册、程序化执行、队列及Dev按现有真实消费者重新确认，不重复开发。

先沿用Clap注册与派发加入签名解析及输入回调；公开标识采用项目原生命名，不变更CLI、JSON、事件字符串。旧v0.x console/command.rs只提供name/about/usage及异步run，无签名Parser和这些高层入口可直接移植；继续复用主线Console Kernel／注册作用域／应用容器及错误管线。方案参考固定Illuminate Console Parser、ClosureCommand和Command源码，不另造派发器。

## 第一簇：签名、闭包输入与声明式依赖（2026-11-25）

前章最终本地提交a86b54b8b1b89b74f1f1f0d7b4293e2ef50468d4。新增CommandDefinition::from_signature，将必填／可选／默认／数组位置参数、布尔／可选值／数组选项、缩写、跨行与说明转换为既有Clap定义；保持Console Kernel、列表／帮助、错误／事件和应用装配。Input的argument／option／arguments／options提供相同分组，未知名称None，未设置可选标量整组为null，数组保持数组；自定义Clap类型继续通过matches读取，不冒称所有类型已JSON化。

Artisan／Console Application原command保留Output闭包类型推断及成功退出0，新增command_with接收显式Input／Output返回Result或异步回调。把所有形状直接泛化到旧command的首轮失败已发现E0282及HRTB兼容问题，因此没有要求全项目修改旧调用来掩盖破坏。异步首参Arc<Service>由当前应用make自动解析，复用已有容器／延迟Provider，无反射或独立DI系统；Application参数回调仍可显式解析更多依赖。独立消费者证明列表／帮助不构造、首次缺绑定返回错误、后绑定后每次执行使用相同所属容器，两个应用经真实TCP各自读取自己的服务；独立Artisan子进程验证旧闭包、非零7、含空格／中文、数组／布尔和异步依赖，注册用途出现在真实JSON帮助。

固定Parser＋Symfony Console7.4真实ArgvInput由PHP8.5.12在无网络临时容器运行，10向量保存于signature-oracle.json。对照发现标量重复取最后值，数组无值追加null；现复用Clap出现组保留空组，不引入魔法字符串或改CLI数据格式，完整向量在Rust直接复验。PHP脚本只存target/tmp，未跟踪。重复名称／缩写及不合法位置顺序在注册返回错误，避免派发期panic。

最终4框架目标18项（含5专项）＋根4目标30项，共48项0忽略；根包含6个直接提取三语签名／依赖示例，非平行重写。工作区格式、全workspace/all-targets严格Clippy38.70s、main(pail)构建及三语构建退出0。实际main HTTP /up／静态／HTML404／JSON404／HEAD、Artisan inspire/list/pail help、独立Pail捕获真实500／过滤／超时清理及Ctrl+C通过；使用本批隔离SQLite／运行目录，不访问开发队列。三语两挂载前缀、手机／桌面12组实际Chromium页面检查通过。日志target/tmp/artisan-tutorial-20261125/{signature-tests,signature-tests-v2,signature-tests-v3,signature-final,root,fmt,clippy,main,docs,runtime,browser,gates-final}.log；首次借用Arc／消费Application及生成夹具缺Facade导入的编译失败不计通过，已修正且最终完整复验。

仍未闭合整章：typed CommandInput、通用缺失输入提示、Isolatable自定义ID／租期、问答／表格／进度、命令信号接口、类／生成注册及Dev／事件等尚按48节继续核对。Tinker目前只是Planned条目，不能算REPL已提供；Sail与PHP扩展生态独立裁决。保留此前06-08已验行为，不将本批数量或编译通过代替剩余公开入口验收。服务／子进程已退出，保留可复用缓存与原始日志，不推送。

## 第二簇：CommandInput与类型化数据读取（2026-11-25）

第一簇已本地提交dc8137855。新增公开console::CommandInput及Input::input／input_value，保留arguments／options分组和合并快照，复用support::input_scalars、data路径、Collection、BackedEnum、InputInterval及既有日期引擎／DateFactory；没有复制转换器或新建全局输入。固定CommandInput源码实际array_merge(options, arguments)，与其注释的“options优先”不同；现以真实执行为准，位置参数覆盖同名选项。all_keys空选择返回整组，指定缺失键置null；only则省略缺失路径，两者不能互相冒充。

对应公开入口逐项消费者：new／arguments／options／all／all_keys／to_array／get；has／exists／has_all／has_any／missing／missing_any／filled／filled_all／is_not_filled／is_not_filled_all／any_filled；string／str及_or／_strict、boolean／integer／float及_or／_strict、clamp；date／interval／enum／enums；array／collect／only／except；四类when条件入口。PHP动态属性改为get／exists，all的可选键集合改为all_keys；when返回显式Option<R>、enum版本再包Result，None表示未执行，可or_else回退，不保留PHP“假值再隐式返回输入对象”的混合类型。该返回形状是明确Rust适配，不把未实现的方法包装成约束；Dumpable／任意PHP对象反射不算本章类型化读取承诺。

固定CommandInput＋InteractsWithData经真实PHP8.5.12运行14组缺失／null／空白／布尔／0／数组／数值字符串，结果input-oracle.json由Rust公开入口逐项比较。数组转字符串的PHP警告单独保留stderr，不篡改输出或冒称原生会触发PHP warning。新四项专项还验证日期配置时区和显式格式、非法日期／区间／枚举、来源类型及失败后下一命令恢复，真实TCP HTTP及独立Artisan都消费类型化输入。ConsoleError新增from_source，保留Clone及既有显示／退出流程，同时通过Error::source保留底层DateError／IntervalError／枚举错误，不提前字符串化。

最终6框架目标52项＋根4目标33项，共85项0忽略；其中含原请求输入30项回归、三语9个直接示例，不能叠加为额外功能。范围格式、workspace/all-targets严格Clippy40.24s、main(pail)及三语构建全0。实际main HTTP／HTML与JSON404／静态／HEAD、独立Artisan与Pail真实错误捕获／清理／Ctrl+C通过，12组三语双前缀手机／桌面Chromium检查覆盖新增typed-command-input。证据target/tmp/artisan-tutorial-20261125/input-{tests,signature-final,root,fmt,clippy,main,docs,gates,runtime,browser}.log及input-oracle-stderr.log。初次PHP自动加载缺Collections映射的失败保留，修正来源加载后再取正式JSON；没有改源实现或伪造向量。进程／服务均已退出，继续复用原缓存，保留日志、依赖与隔离源码。下一簇核对Isolatable，整章仍未关闭，不推送。

## 第三簇：Isolatable与数据库锁的整秒期限（2026-11-25）

第二簇已本地提交35ee055c6。新增contracts::console::Isolatable及可选isolatable_id／isolation_lock_expires_at；Artisan::add_isolatable、Console Application::add_isolatable_command和Kernel契约register_isolatable_command接入同一注册／执行管线并自动添加--isolated。Rust擦除后的dyn Command无法自动发现另一Trait，明确选择携带Trait的注册入口；不能把普通add误写为会自动检测。私有适配器只借用原命令、克隆装配定义，不引入第二套命令执行器；原CommandDefinition::isolatable和既有add保留。

默认无后缀、一小时；自定义ID追加至既有framework/command-{name}键，空字符串与未定义不同，缓存键前缀不重命名。Ttl复用既有Seconds／Until，显式输入支持相对与绝对期限。没有--isolated不读锁元数据也不锁；冲突默认退出0，可指定退出码。获取／释放复用现有Cache ScopedLock和PendingAcquisition，只有可见性扩大到crate内部；正常、错误和Future取消保留所有者检查与后台清理，不重造RAII或强制删除新拥有者。源代码ForceRelease与当前所有者安全释放的既有原生差异保留；进程强杀仍依赖期限，未冒称运行时清理可跨进程执行。

实际六存储矩阵首轮array／file通过，SQLite期限反例失败且保留日志isolation-sqlite.log，未跳过或放宽等待。定位数据库边界对通用毫秒期限向上取整，会让整秒锁多保留近一秒。固定DatabaseLock::expiresAt真实PHP方法在固定时钟验证-1／0／1／3600公式，原始结果lock-expiry-oracle.json；现仅将数据库锁get／refresh的整秒期限设为当前Unix秒＋租期，再交既有边界持久化，保留值缓存与显式亚秒租约的向上取整策略。新增真实SQL续租读取expiration验证当前秒＋3，并在1250ms后验证一秒锁到期；旧拥有者释放不能影响替代拥有者。数据库夹具使用上游整数列，不扩大列类型掩盖毫秒问题。

最终array／file／SQLite／MySQL8.4.13／PostgreSQL18.7／Redis8.10.3六矩阵各2项store消费者通过：默认Trait及旧Definition、空ID／不同ID、普通无锁／冲突退出0及17、执行失败释放、取消释放、相对／绝对到期、续租持久化及所有者；共享后端还用独立Artisan子进程核对真实互斥，array明确只保证本进程。独立真实TCP验证HTTP读取同一命令锁并在释放后恢复。Console Application和Kernel契约入口各自实际调用，不只检查方法声明。MySQL用TestApp::mysql私有schema，SQLite私有文件，PostgreSQL／Redis用验收专属临时实例；三库不访问开发数据库，Redis核对INFO8.x及PING，不执行共享清库。临时容器全部移除，无新增数据卷。

最终主批6框架目标26项＋根4目标36项共62项0忽略，含正式三语12个直接提取示例；另8项既有缓存实际回归通过（file跨进程／本地并发、三库事务、MySQL／Redis scoped回调及Redis所有者锁）。六矩阵对相同store函数的重复执行不叠加为新功能数。首轮Output没有Debug的测试unwrap_err编译问题按匹配JoinError修复，没有给产品加无必要derive或lint豁免。workspace fmt、all-targets严格Clippy34.89s、main(pail)构建及三语构建全0，实际main HTTP／Artisan／Pail错误捕获与清理／Ctrl+C、三语双前缀手机与桌面12组浏览器通过。日志target/tmp/artisan-tutorial-20261125/isolation-{tests,tests-v2,sqlite,sqlite-v2,matrix-v2,matrix-final,gates,signature-final,root,fmt,clippy,main,docs,runtime,browser}.log和isolation-final-{backend}-{case}.log；源码哈希见accepted-source.json。未验证Windows新Trait／取消路径或真实Memcached／DynamoDB等供应商，未将此范围扩张为整个Cache或Artisan已闭合。继续缺失输入提示、I/O、生成／注册、信号及其余教程入口，不推送。

## 第四簇：完整全局输入与原子修改（2026-11-25）

第三簇已提交714f869b4。本簇先用真实Artisan复现继承全局选项未进入Input助手的问题，再保留原注册定义身份，增加完整解析定义和调用名称元数据。分派、Kernel::call、子任务和Facade调用都携带实际名称；别名保留在arguments.command。单独CommandDefinition::parse仍只解析声明的输入，不伪造应用级命令参数。

固定Illuminate Console Application结合Symfony Console v7.4.0在真实PHP8.5.12执行三组默认／别名／显式全局选项，原始结果global-input-oracle.json由Rust消费者整组比较。verbose助手为布尔，env为可空字符串，ansi为null／true／false，no-ansi合并为ansi；matches仍保留原生计数和原始开关，既有CLI解析、环境多值及ANSI同时指定时的原输出优先级不改。此处不是把实现差异当语言限制，而是保留底层兼容同时修复公开助手。来源自动加载失败与修正后的原始输出均留在临时日志，不提交PHP脚本。

Input可克隆，set_argument／set_option复用原Clap定义重建并验证输入；失败原子回退，保留定义身份、关停令牌、数组无值组、空字符串、前导短横、非UTF-8参数及未修改字段。ANSI支持布尔或null，verbose可用布尔及原生数值计数。它是经过CLI约束的原生适配，不模拟任意PHP可变对象：跳过可选位置后只改后位、清空非空默认数组等CLI无法表达的状态会报错，synthetic command元数据不能用于重定向命令。上述限制在三语正文明确，不以替代写法证明完整InputInterface。

最终7框架目标33项＋根4目标39项，共72项0忽略，其中三语正式Markdown直接提取15例；测试数不等于新增功能数。专项6项覆盖原子回退、类型和值保留、Kernel身份、真实TCP HTTP成功／JSON500／后续恢复、独立Artisan成功与失败、三组源码整组向量及原始verbose计数。实际main验证/up、静态文件、HTML／JSON404、HEAD、独立inspire／list／pail帮助、Pail真实错误捕获与超时清理、Ctrl+C退出0。三语双前缀／两宽度12组Chromium检查覆盖editing-command-input。fmt、workspace all-targets严格Clippy39.19s、main(pail)构建及三语构建退出0；站点仍有既有chunk-size提示，不冒称无警告。

本轮中途快照误含未完成认证改动导致Clippy失败；仅将隔离快照恢复已提交认证基线，并在最终组合吸收aec95b92e及后续已提交记录，未改动活跃认证文件。另一次include_str相对层级错误、一次新增测试格式检查失败均修复重跑，不计失败为通过。最终日志input-edit-final-{signature-final,root,fmt,clippy,main,docs}.log、input-edit-normalized-{gates,resume}.log、input-edit-{runtime,browser}.log；失败证据单独保留。源码组合和SHA见accepted-input-edit.json。可复用缓存／依赖／日志与隔离源码保留，无本批运行服务或容器；共享守卫中途清理后已恢复验收。继续缺失输入提示、I/O、注册与生成、信号等剩余节，整章未闭合；不推送。

## 第五簇：缺失输入提示与完成钩子（2026-11-25）

第四簇已提交dfa549032。按固定PromptsForMissingInput源码接通contracts::console::PromptsForMissingInput、MissingInputPrompt和CommandRegistration<C>，保留Command trait对象之前显式选择.prompting()／.isolatable()；原Artisan::add、Console Application和Kernel注册管线不替换。装配器组合／嵌套保留已有标记，原IsolatedCommand也转发提示标记。旧v0.x仅有名称／帮助／异步run，没有可复用提示Trait；复用当前Clap、console::Term、容器、锁及命令执行。

必填标量／数组按原声明顺序定位；每个缺失参数重新获取问题定义，支持默认名称／说明、文本标签与占位说明、异步应用／可变输入／输出回调。数组回调返回Vec<OsString>是明确CLI token适配；不伪装任意PHP值序列化。默认文本拒绝空及"0"，自定义回调可返回"0"。至少收集一项才执行after_prompting_for_missing_arguments，可修改输入；随后恢复原约束最终验证，再读取隔离ID／期限及执行。已有参数和help不询问；--no-interaction不询问且缺必填仍失败；回调错误、无效结果及Future取消不持有或泄漏隔离锁。

首次回归暴露在已build的Clap定义上mut_arg会使索引错配，--isolated值被误认成silent；现从未build的原定义单独生成交互解析器，严格帮助定义保留。最终验证从原严格定义重建并带回完整全局选项、版本／help元数据，避免自动help重复。旧全局输入、Kernel定义身份、非UTF-8和隔离消费者均回归通过。编译夹具缺Trait导入／静态名称、锁可变借用、版本与自动help重复、遗漏两处解析器调用、Clippy多余import等失败日志保留，全部修复后复验，没有增加lint豁免或修改Clap依赖。最后补验确认自定义回调可修改当前输入，后一问题能读取该修改；quiet／silent抑制程序的问题及验证文案，用户键入回显仍由终端显示。

固定PHP8.5.12实际运行原Concerns Trait、原Symfony ArrayInput及BufferedOutput，三组回调／数组／after钩子向量prompting-oracle.json由Rust正式Trait／Facade逐组比较；首次ArrayInput构造时误提前验证的失败保留，改为原bind后fill再validate流程。不是另写PHP提示替代实现。新五项实质测试覆盖类型组合、嵌套／提示后锁ID、来源错误、空答案、取消及恢复、真实TCP HTTP与独立Artisan；一个终端子进程夹具在常规测试中ignore，不计通过。

终端验收脚本另执行六场景：PTY默认说明／占位问题及空／0重试后成功，--no-interaction拒绝缺参，无TTY立即错误，Ctrl+C（实际SIGINT，子进程-2／shell130），以及quiet／silent抑制问题与验证提示；不冒称优雅服务关闭或Windows实机已验证。三语正式Markdown直接提取18个消费者，另三个语言版本分别在PTY内真实执行自己的默认文本问题。默认文本使用现有原生console::Term展示，不声明与上游Prompts画面像素一致；未国际化默认问题遵守项目简体中文提示规则。捕获HTTP／程序化输出不会读取进程stdin；使用显式参数或自定义回调，边界已写入三语。

最终6框架目标26项、根4目标42项，共68项父测试通过，另终端夹具1项ignore明确排除，PTY专门实跑独立记录；数据组和子进程不叠加为新功能数。fmt、workspace/all-targets严格Clippy28.56s、main(pail)和三语构建全0，12组双前缀／两宽度浏览器通过；实际main HTTP／静态／HTML与JSON404／HEAD、Artisan inspire／list／Pail真实错误捕获与超时清理／Ctrl+C通过。证据prompt-frozen-tests-v2.log、prompt-frozen-{root,fmt,clippy,main,docs}.log、prompt-frozen-tty.log／prompt-tty-results.json、prompt-frozen-formal-tty.log及逐语终端日志、prompt-frozen-browser.log和prompt-frozen-runtime.log；accepted-prompting.json保存源SHA。终端夹具目录清理，热缓存／依赖／日志／独立源码保留，无本批存活进程或容器。

关联实测发现Artisan Facade连续call重建命令对象，不能把本批测试使用容器共享计数器说成命令实例持久化已经实现。registering-commands登记该缺口，下一簇立即复现／修复并补交；本批只闭合单次输入提示流程，prompting-for-missing-input待该关联项复验后最终核定。命令事件相对交互／锁的完整顺序仍随events节继续审计，不将现有observation回归冒充整节完结。其余I/O、生成／注册、信号和Dev／Tinker等未验节继续，不推送。

## 第六簇：按应用复用命令装配及实例（2026-11-25）

第五簇已提交229301926。实际反例registry-before.log显示，同一应用两次Facade调用输出1／1而非1／2。固定Foundation Console Kernel::getArtisan只在尚无实例时构建Artisan；真实Illuminate Console Application＋同一个Symfony命令执行成功／成功／失败／恢复，PHP8.5.12向量为1／2／counter-failure-3／4，见registry-oracle.json。这里没有声称运行了完整PHP Foundation Kernel的HTTP应用；其一次装配分支是源码核对，向量来自实际Console Application。首次格式化输出缺Symfony String依赖的结果不计通过，改用其官方OUTPUT_RAW写相同数字，计数及执行管线未替换。

参考旧v0.x CommandRegistry的应用内共享所有权，直接复用当前Artisan::Clone及Entry::Command／Closure的Arc；本次只有一次装配，不移植旧revision／Mutex增量注册机制。默认ConsoleApplication绑定保存弱应用引用和OnceCell<RegisteredCommands>；默认Kernel只装配一次，其他Kernel克隆同一命令Arc及调度存在标记。原注册主体完整移动为私有函数，Rustfmt仅调整排版／尾逗号与等价闭包；Provider及注册顺序、别名、默认值、命令名与事件字符串不变。手工Kernel::new仍保留自身的显式注册表。每次调用独立持有输入、输出、deferred与抑制状态，不缓存调用输出；执行时不持有初始化锁，所以可以重入嵌套调用。

新增四个实质消费者覆盖同实例连续计数、失败后同实例恢复、首次注册失败不发布半成品并重试、嵌套无死锁、48个真实HTTP并发首次调用只装配一次、两应用互不串状态，以及独立Artisan二进制入口与嵌套Facade复用同一注册结果。Application::handle_command按既定设计消费应用，测试不新增Clone绕过终止职责，而是在一次真正CLI入口内验收嵌套调用。框架最初缺container Trait导入改为既有container_handle；测试误重复消费Application的编译失败已修正并保留。原Prompt计数预期由隔离调用的12／0／11恢复同实例的12／12／23，明确由实例复用修复支持，不能继续依赖临时容器计数器说明自有命令状态。

三语新增command-lifecycle正文及实际闭包计数示例，由正式Markdown直接提取，连续调用均得到1／2；每应用／独立进程边界、并发字段安全、执行时解析请求级依赖、不捕获请求身份等原生约定写清楚。Rust共享命令要求原有Send＋Sync，保留具体实现自身的线程安全责任；没有把请求数据塞进静态注册表。

最终8框架目标35项＋根5目标46项，共81项父测试通过；其中21个正式三语示例、5项调度时钟／时区／亚分钟回归、Runner优先组过滤／启动／关闭及旧控制台回归。一个终端夹具仍ignore不计通过，当前组合另用正式三语示例逐语真实PTY执行通过。workspace fmt、all-targets严格Clippy1m15s、main(pail)及三语构建退出0；实际main的/up／静态／HTML及JSON404／HEAD、独立Artisan inspire／list／Pail真实错误捕获与清理／Ctrl+C通过；12组三语双前缀／两宽度浏览器覆盖新生命周期锚点通过。没有重跑未受影响的六后端锁矩阵或Windows发布矩阵。

磁盘在框架测试完成后触及32GB停线，已由共享构建守卫持锁清理项目profile和编译站点产物，事件记录before=31,994,896,384、after=115,385,188,352、released=83,390,291,968字节，锁inode保留；不是删改源码／日志／依赖／数据库。空间恢复后继续冷编译完成其余门禁，已通过的测试日志与源SHA保留。证据registry-final-{tests,root,fmt,clippy,main,docs,gates,runtime,formal-tty,browser}.log、registry-before.log、registry-after*.log、registry-source*.json／stderr；accepted-registry.json保存当前源SHA。正式示例的三个终端夹具目录再次清理，保留共享热缓存、独立源码和日志，无本批存活服务或容器。

本关联生命周期缺口已闭合，prompting-for-missing-input按已核定教程范围更新为通过；完整事件相对交互／锁顺序继续随events节核查。registering-commands其余目录／类型发现与生成模板入口仍待逐项核定，不据此关闭该节或整章。继续I/O及其他余项，不推送。

### 第六簇提交组合补验（2026-11-25）

cfbe860c0提交窗口中，本地历史进入独立认证提交d8c7ee4b7；accepted-registry.json如实保留原验收基线229301926，不能把旧认证组合的验收冒称为新父提交组合。已重新从Git读取8ce3432b7至cfbe860c0的67条已提交路径，未复制任何当前认证／I/O在途代码；逐条与HEAD blob比较。补验当前纯提交组合35框架＋62根父测试共97项通过，含初始化16项、21个三语示例、Runner与调度；终端夹具ignore仍不计通过。fmt、workspace all-targets严格Clippy24.81s、main(pail)和三语构建退出0，实际main HTTP／Artisan／Pail／Ctrl+C及12组浏览器再验通过。证据registry-head-{framework,root,fmt,clippy,main,docs,gates,runtime,browser}.log，组合源哈希见accepted-registry-combination.json。未改写原始验收证据或覆盖其他在途改动。
