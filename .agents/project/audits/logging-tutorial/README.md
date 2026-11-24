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
