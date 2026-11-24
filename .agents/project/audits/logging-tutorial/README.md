# Logging固定教程逐节核对

按交付映射The Basics顺序，已闭合Error Handling后推进Logging（29锚点）。固定文档960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md、框架1449bf241017087fbd38c118ecaabef3f6797fdc的LogManager/Logger及Monolog3.10.0渠道实现为参考。旧v0.x仅single/stderr/null/stack异步sink，缺少errorlog/Slack/Papertrail，复用当前Factory/Handler/Logger及原应急处理，不恢复旧管理器。正式文档与完整类型契约分别裁决，不用关键词命中或原回归关闭整章。

第一簇发现应用config已有errorlog条目而Driver不识别，实际调用误入emergency。补齐0/4两输出模式到原生stderr Handler，保留级别、格式化、占位符与bubble；原生可执行程序无PHP SAPI，不能配置PHP ini错误目标。真实Application::handle_command经子进程验证stderr、占位符和低级别过滤且不生成应急文件；无效类型仍按既有应急记录和原消息回退。首次测试将LogManager::build的现有应急返回误当成Err，已修正夹具而不改产品恢复策略。

当前整章未闭合：Slack/Papertrail驱动实际未接线；deprecations、其余渠道、context、原生Formatter/Processor/Factory及Pail所有过滤器继续逐入口/实际消费者核对。三语仅同步本簇可用性，后续结果与提交号按批次追加。禁止推送。

第一簇最终补核发现固定createErrorlogDriver只传type/level，未传bubble；因此按构造器默认始终继续堆栈，而不错误继承通用bubble=false。子进程实际Artisan验证后续文件成员确实收到记录。5目标31项0忽略通过（含既有真实HTTP并发与隔离、真实Unix syslog、文件、上下文、监听器及原生句柄）；workspace/all-targets严格Clippy20.96s、fmt、三语构建及3组浏览器检查全0，实际main HTTP／Artisan／Ctrl+C回归通过。证据target/tmp/logging-tutorial-20261124/errorlog-{consumers,clippy,fmt,docs,main,artisan,browser-final,runtime}.log；首次夹具失败不计通过。已开始的网络驱动独立新文件尚未编译／验收，不纳入本簇提交。整章29节继续开放。


## 第二簇：网络渠道与协议消费者

复用原Handler::Registered、LogRecord及JSON错误归一化；补Slack、现有papertrail别名及monolog/syslog_udp，普通级别、bubble、action_level、formatter、processor、emergency和监听器仍走原链路。已有hostname0.4.2作为直接依赖复用，锁文件仅增依赖边，不升级第三方版本。固定Laravel骨架papertrail默认指向SyslogUdpHandler，原生保留已有papertrail配置别名并公开相同通用处理器配置入口。UDP支持0/1/2三种RFC、数字字符串端口、facility/ident、多行逐报文及Unix port0；hostname读取系统，默认标识为应用名，而非PHP运行时标识。

Slack以固定Monolog3.10.0的SlackRecord及Curl/Util为依据，使用既有reqwest库，不写第二套HTTP/TLS。同步契约通过独立渠道线程持有单一运行时和客户端，调用仍同步等待，不谎称无阻塞或异步日志。上游连接／超时／正文故障最多5次，本地照此重试；HTTP500本身不触发传输失败，307不跟随，旧应急回退及同渠道恢复不变。原生增加每次尝试10秒的可配置timeout，避免无限等待；这是明确传输边界差异。错误链移除Webhook URL，不将密钥写入诊断。

原始Monolog源码及MIT许可保留在target/tmp/logging-tutorial-20261124/reference/{monolog,psr-log}。reference.php.fixture为本仓独立编写的公开API对照程序，复制到参考目录为reference.php后，由既有docs-reference-php:8.5.12-intl镜像以--network none、只读挂载运行。实际生成6组Slack期望值（附件、纯文本格式化、短附件、无context、null／重复末段排除、长数组截断），输入与结果归入中性tests/reference/network-log.json；原生实际HTTP发送完整JSON逐字段相等。首次PHP空数组输出被误当原生输入对象，已保留原输入类型仅采集其expected；接收端继承非阻塞模式造成首次读取竞态已显式改阻塞并保留超时，不降低协议断言。实际报文对照暴露exclude_fields的isset/null与首个同名末段差异，已复用点分删除核心补齐，不跳过反例。

7项网络专项（含16并发真实HTTP＋独立Artisan、默认Slack/UDP/file堆栈、后续无上下文请求、重试5次、后续恢复、超时、307/500、私密URL排除、Unix数据报）通过；加原5目标31项共6目标38项0忽略通过。严格Clippy首次发现测试锁的词法生命周期与比较时重复分配，已修正，未加豁免。随后触32GB守卫安全中断，持锁释放82,322,956,288字节；源码、日志、依赖与数据库保留，恢复后原门禁继续，最后workspace/all-targets严格Clippy1m23s、fmt、三语构建与浏览器全0。实际main Artisan queue:work指定不存在连接，退出1且错误确实经配置Slack Webhook上报，未创建应急文件；不访问开发队列或数据库。另固定PHP真实stderr实测确认error_log无条件追加换行，现补独立Errorlog Handler，两种type及自定义无分隔符格式器均以子进程复验。

兼容邻接核对补齐渠道level大小写不敏感，配置CRITICAL／NOTICE与DEBUG实际二进制均通过。证据network-{consumers,clippy,fmt,docs-final,main,artisan,browser,runtime}.log及network-gates-resumed.log。文档里明确同步等待、超时、UDP无确认及Socket平台上限，不以协议接收通过冒称真实供应商账户或Windows实机已验。

整章仍开放：继续直接提取三语示例，核对Pail安装／跟随／全部过滤、deprecations优先级、编译型tap依赖，以及固定示例HtmlFormatter、BrowserConsoleHandler、MemoryUsageProcessor、PsrLogMessageProcessor与外部NewRelic前置；不以通用注册器代替这些具体消费者。网络批次已验范围独立提交，不夹带并行版本统一，也不夹带尚未验收的logging_tutorial_docs.py。


## 第三簇：具体格式器、处理器与浏览器消费者

固定教程使用具体HtmlFormatter、MemoryUsageProcessor、PsrLogMessageProcessor、BrowserConsoleHandler；此前只有通用注册器，不能替代对应入口。现增加公开log::formatters::HtmlFormatter、log::processors两种类型及log::BrowserConsoleHandler，并经原渠道工厂解析html、memory_usage、psr_log_message、browser_console；同名应用注册仍优先，不引入平行日志器。HTML日期复用既有Date，标量复用既有php_float_string，Slack与HTML共享原JSON归一化和四空格输出。共享占位符补齐含花括号键的最长非递归匹配及浮点标量文本；原结构化JSON值、事件名及持久化结构不变。八级真实上游向量同时验证HTML默认／指定日期、HTML转义／嵌套值／空值／浮点数、批量输出与PSR实际字段移除。原PHP工具及第三方参考仅保留target/tmp/logging-tutorial-20261124/reference；取消上一簇误跟踪的reference.php.fixture，不以换扩展名继续跟踪对照脚本。端到端期望JSON仍是正式Rust回归输入。移植的格式及样式算法按MIT保留框架licenses/log-formatting.txt与根THIRD_PARTY_NOTICES，必要来源／版权声明不去品牌。

MemoryUsageProcessor读取现有memory_stats的原生RSS；不是PHP分配器指标，realUsage=false明确拒绝，useFormatting=false返回数字字节。JSON上下文不承载PHP DateTime等不透明对象，因此不虚构dateFormat对象调用。BrowserConsoleHandler复用原生Handler与请求作用域Buffer；默认未启用仅检查Control原子标记，不包装正文；配置或在返回前创建后才包装HTML／JS，记录按应用／请求隔离，CLI、JSON、HEAD、无正文、已编码、部分内容及二进制文件不注入。替换正文保留原响应元数据和Cookie扩展，移除已失效的Length／ETag，不收集整个流，保留trailers，出错不补脚本。正文消费发生在请求关闭后，不能复活已关闭的Facade；流内使用提前取得的自有Logger，私有流记录通道仅将其输出并入所属响应，不重新开放Request作用域。

3项组件消费者＋3项浏览器消费者与既有日志／网络／算术弃用回归共9目标49项0忽略通过；浏览器含16并发真实HTTP、直接公开类型、自定义及默认格式、独立Artisan子进程、trailers、JS／encoded／partial协议和后续JSON／HEAD。真实Chromium执行实际HTTP响应，验证级别／分组／样式／上下文及恶意结束script文本不执行。正式三语现有33个Rust示例和6个新增JSON配置直接提取、编译并运行，共39项0忽略。首次夹具缺依赖、Header类型／Host／独立CLI未注册格式器及错误生命周期假设均保留失败日志；仅最终49／39算通过，没有降低产品URL校验或请求关闭契约。最终workspace/all-targets严格Clippy51.81s、workspace格式、三语构建均0；实际main（pail功能）HTTP /up／静态／HTML404／JSON404／HEAD，Artisan inspire／list／pail帮助及Ctrl+C退出0通过。证据components-{consumers-final2,docs-consumers-final,clippy,fmt,main,docs,runtime,browser}.log及components-doc-browser.log。验收快照吸收已提交workspace版本统一，不夹带他人身份认证计划。

整章继续开放：Pail跟随／过滤完整消费者、弃用命名渠道优先级、容器依赖tap与Facade邻接公开入口待收尾；固定示例的NewRelicHandler尚不应被通用自定义回调冒充为已支持，需要单独核对官方原生Agent及可用平台前置。当前提交不代表29节或全部Monolog类API闭合；现有UTC轮转／时间戳策略、供应商账户及跨平台发布边界保留。


## 第四簇：Pail教程与异常报告认证重入

第三簇已独立提交f00abcb99c7e1b262d385f0852b4b8df3bdf8a39。继续按Pail v1.2.5／fdb73f5eacf03db576c710d5a00101ba185f2254核对固定Logging最后8节；沿用06-08原认证时序、Options、Files、打印器、Provider与命令实现，不重造tail进程或Pail持久化格式。完整Pail七个消费者目标真实运行，涵盖从HTTP到独立Artisan、所有过滤器合取／空值／身份、-v／-vv、ANSI／quiet／silent、文件分段UTF8／截断／轮转、多个会话与两应用、超时／取消／信号／broken pipe、环境与Provider门禁、原生路径、22组Provider情况及432个打印器固定PHP向量。普通harness父用例12项（auth1／files4／options7）全部通过；子进程入口和harness=false消费者另列，不叠加为功能计数，无忽略。

首轮auth_timing实际失败：新异常报告上下文先调用Auth解析身份，此时认证回调中的显式请求日志进入Pail；原Pail任务标记只覆盖它自己发起的认证，不能覆盖异常报告先发起的解析，导致再次调用回调并传播InvalidIdentity使请求中断。没有删除原反例或吞掉认证错误：RequestGuard／SessionGuard复用AuthContext按请求、Guard名记录正在解析的私有AtomicBool，用RAII在成功、错误及Future取消时释放；嵌套读取只返回已知身份，不启动新解析。最外层错误仍返回，未完成身份不缓存，后续请求／重试正常。无新增公开标识、事件或存储格式。原Pailauth_timing未降低断言，第二轮包含错误回调／错误页／取消后重试及后续HTTP全通过，日志pail-consumers{,-v2}.log保留红绿证据。

额外auth_configured_extensions两项及SQLite实际Session HTTP回归一项通过；同二进制其余6项按精确筛选未执行，不当作MySQL／PG／完整Auth模块通过。格式、workspace/all-targets严格Clippy38.88s、三语构建0；实际main（pail功能）HTTP、HTML／JSON404、Artisaninspire/list/help、独立pail --level=error -vv --timeout=2捕获真实500报告、超时会话清理及Ctrl+C全0，来源与数据均隔离在target/tmp，数据库配置只指向本批SQLite文件，无开发数据库操作。证据pail-{gates,auth-extensions,session,fmt,clippy,main,docs,runtime,runtime-output}.log。同步三语认证重入说明；Windows和真实供应商发布矩阵不因此关闭。下一簇待验Facade转发仍留在工作区，明确不夹带本次提交。


## 第五簇：渠道装配接口与格式器语义

第四簇提交8f33376e02c991e4292e94a2b018f7ced0f463f8。按29节逐项继续核对，发现不能用普通格式闭包代替固定tap示例中的具体LineFormatter，也不能用整体set_formatter替代示例逐Handler装配。现提供log::formatters::LineFormatter、Logger／NativeLogger::get_handlers与HandlerRef::set_formatter；复用原处理链与Date／JSON／PHP标量机制，不增加第二套输出目标。HandlerRef只在独占装配期间借用，按既有写时复制策略保持其他共享Logger配置不变，热写路径无新增格式配置锁；配置失败时NativeLogger无可装配渠道处理链，应急策略原样保留。直接Native消费者覆盖共享格式不变、真实同文件输出和应急输出；不将此有限装配API说成全部Monolog可变运行期API。

三语新增的实际CustomizeFormatter通过ClassDescriptor声明构造依赖，Provider register只登记类型与tap闭包，首次创建渠道时才从所属容器make；在正式教程消费者中boot后改配置Before→After，第二次记录仍用缓存After，forget后重建才用Late，证明不是启动时构造或每条日志重新注入。无反射模拟器、无平行DI；直接提取三语代码编译运行。新增Log Facade的driver／get_default_driver／set_default_driver／extend／forget_channel原管理器转发，实测两应用隔离、同一缓存、旧句柄保留、清理后新目标及原生工厂。Facade／Manager／Logger／Native四类各8个具名级别方法真实调用并验证32条输出，而非只检查宏声明。

LineFormatter支持固定示例构造与format／format_batch、各标准token及按字段token、换行／空上下文开关；八级固定PHP结果逐字节对比。实际对照暴露Map::remove会交换末项，现对Line及PSR使用shift_remove，保留剩余字段顺序，并把PSR断言改成有序JSON字节比较。新增CLI子进程按固定memory示例通过真实monolog/stream stderr，覆盖simple Memory处理器加带参数PSR处理器及字段移除。保留原生RSS、结构化错误快照与UTC策略，不虚构PHP对象及Throwable调用；其他第三方Formatter完整类API不计本教程交付。

补核第三簇默认格式器组合发现，省略formatter与显式default曾被错误合并：固定prepareHandler省略时使用框架格式，default才保留处理器自带格式。现对文件／轮转／stderr／errorlog／Slack的自带Line、syslog／UDP自带无时间前缀Line、Browser自带样式分别接线；未配置formatter的原有默认字节不变。显式default按上游补齐ISO日期或空context／extra的[]，这是可观察的缺陷修复，三语说明及Unix syslog断言已同步。Browser的实际None/default/custom路径、HTML格式器及危险script文本经HTTP验证，真实Chromium验证控制台级别、分组、字段与CSS参数，未把脚本字串存在视为执行成功。原生类型与HandlerRef不宣称任意第三方类自动可用。

具名deprecations实际HTTP证明logging.channels.deprecations优先于LOG_DEPRECATIONS_CHANNEL=null，输出不中断并不写默认文件；现有trace／testing／无效渠道回归继续通过。三语补齐配置及运行期兼容警告边界，编译器诊断不冒充运行期事件。最终受影响日志十目标55项0忽略整批通过；后补Native直接getter反例后组件七项全通过，生产源码未变，不将两轮重复计数为额外功能。正式三语36段Rust及6份JSON，共42个直接提取消费者通过。最终workspace格式及严格Clippy通过（完整37.15s，新增getter反例后的增量0.52s），真实main HTTP／Artisan／独立Pail捕获／Ctrl+C通过，三语最终构建与浏览器三组通过；未导出整workspace Rustdoc或新增跨平台矩阵。证据lifecycle-{consumers-final,handles-final,docs-consumers-final,fmt-final,clippy-final,main,docs-final2,runtime,browser,doc-browser-final}.log。Clippy曾指出无必要let及函数指针数组复杂度，按代码结构修正，没有豁免。所有本批服务／子进程已退出，继续复用项目内独占缓存，保留原始日志。

### 唯一待决的教程生态例子

固定formatter段还列出NewRelicHandler及default。该类不是“只缺账号”：它依赖PHP newrelic扩展，当前原生专用适配未实现，不用register_handler通用回调冒充已支持。官方[原生C SDK仓库](https://github.com/newrelic/c-sdk)已归档，并[声明不再支持](https://docs.newrelic.com/docs/apm/agents/c-sdk/get-started/introduction-c-sdk/)；改接Log API或OTel会改变原APM集成边界，不能擅自引入新方案并说与原示例等价。已通过专用提问等待用户选择“单列待选型供应商集成”或“本轮选型原生适配”；未答复不视为批准。本项及相关父节保持core_verified_vendor_decision_pending，其他已验教程入口按section-map逐项登记。已有真实供应商账户与Windows／发布矩阵按原边界单列，不阻止后续独立章节。

完整Log／Monolog类API与固定教程范围分别处理：本批不声称交付动态setApplication、write的所有PHP对象参数、通用事件Dispatcher替换、Conditionable、全部Normalizer／LineFormatter运行期setter或PHP扩展生态。未交付接口不包装为语言限制；其余类API仍需所属完整契约专项，不能凭本教程状态关闭。当前只核定固定29节的约定消费者，2项父／子格式器节因上述供应商范围未决保持开放。


### 紧接的核心余项

通用可扩展处理器仍需正式的Formattable能力入口：现有register_handler只返回接收record／formatted的Fn，内置default分支虽已验证，仍不能把“回调自己忽略文本再格式化”当作可声明自带格式器的类型契约。下一簇复用同一处理器注册／输出管线，增加能力声明并验收省略／default／显式formatter和构造失败；不另建并行日志系统。该核心余项不依赖NewRelic范围答复。当前阶段只提交以上已验范围，整章继续开放。

补充最终证据：组件七项覆盖直接Native getter、写时复制及无装配链的应急输出；浏览器三项又补真实第二应用及恢复请求、固定HTML formatter/dateFormat字面组合，全部通过。最后严格workspace Clippy含该组合0.61s。证据lifecycle-handles-final.log、lifecycle-browser-{literal,final}.log、lifecycle-clippy-browser-final.log。三语增加弃用配置说明后构建final2与浏览器final全0；没有重复计为新增生产功能。

## 第六簇与核心教程最终核对（2026-11-25）

第五簇已提交4a446de516a0e1eef14fefc5fd74ab76f6147f8d。本簇接通公开FormattableHandler Trait及LogManager::register_formattable_handler，沿用同一注册仓储、过滤、格式器与Handler输出链；BrowserConsoleHandler实际实现该能力，不用普通闭包忽略传入文本来冒充类型契约。省略formatter维持框架既有输出，显式default才按处理器自带格式器，显式其他formatter不调用默认getter。getter在首条通过级别过滤的记录才执行；失败不缓存成功值、下一条可恢复，成功缓存后复用。用户代码外不持锁，并发首次可竞争但采用同一已发布格式器；递归走既有工厂重入保护，不新增全局锁或平行系统。

五项专项实际验证具名类型构造、原有回调、文件内容、默认getter过滤／延迟／失败重试／缓存／并发16写入、真实TCP HTTP及独立Artisan、递归默认getter恢复。递归错误沿既有IO包装链保留LogError::CyclicStack；首轮测试错误期待顶层直接枚举，现按真实typed error chain核对，不改产品错误协议。另首轮Arc引用夹具编译问题已修，失败日志保留。正式三语新增类型实现与注册示例，直接提取39段Rust及6份JSON共45项编译执行，示例实际stderr精确APP:formal own，不维护平行代码。

最终11个日志目标61项通过、0忽略，含公开Browser类型实际注册、16并发HTTP及第二应用隔离；workspace/all-targets严格Clippy29.23s、fmt、实际main（pail功能）构建全部退出0。实际二进制HTTP /up／静态／HTML404／JSON404／HEAD，Artisan inspire／list／pail help、独立Pail捕获真实500及级别过滤／-vv／超时清理、Ctrl+C退出0通过；本批SQLite及运行目录隔离，不访问开发队列或数据库。真实Chromium执行HTTP响应，验证控制台级别／CSS／字段及script转义；三语最终构建与浏览器／范围／API快照三组均通过。证据target/tmp/logging-tutorial-20261124/formattable-{gates-final,consumers-final,docs-consumers-final,clippy,fmt,main,runtime,browser,docs-final,doc-browser-final}.log。未新增Windows、真实供应商或完整Rustdoc导出结论。

另用固定Illuminate Log Logger、Monolog NullHandler(error)与真实Dispatcher契约在PHP8.5.12实际运行INFO／ERROR：低级别is_handling=false不派发事件，ERROR派发一次，见filtered-events.json。该基线Logger.php181–185已有过滤早退，现有原生行为正确，无需按旧记忆改变事件语义。PHP夹具仅置临时目录，不提交脚本。

用户2026-11-25明确选择将New Relic单列供应商集成，详见[LOG-S01](../../plans/log-supplier-integrations.md)。专用APM仍未实现、待选型，不是仅缺账号，也不用通用回调冒充支持；本决定覆盖第五簇当时待答复状态。依此核定核心29节全部有入口／消费者裁决，section-map无核心待验项。结论仅为固定Logging教程的约定范围，不扩张为全部Monolog类／PHP扩展生态API、供应商功能、跨平台或发布矩阵完成。历史阶段证据保留，不改写当时未完状态。所有本批子进程／服务已退出，保留复用缓存、源码与原始日志；未推送，下一章按计划为Artisan Console。
