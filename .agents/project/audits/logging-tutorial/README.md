# Logging固定教程逐节核对

按交付映射The Basics顺序，已闭合Error Handling后推进Logging（29锚点）。固定文档960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md、框架1449bf241017087fbd38c118ecaabef3f6797fdc的LogManager/Logger及Monolog3.10.0渠道实现为参考。旧v0.x仅single/stderr/null/stack异步sink，缺少errorlog/Slack/Papertrail，复用当前Factory/Handler/Logger及原应急处理，不恢复旧管理器。正式文档与完整类型契约分别裁决，不用关键词命中或原回归关闭整章。

第一簇发现应用config已有errorlog条目而Driver不识别，实际调用误入emergency。补齐0/4两输出模式到原生stderr Handler，保留级别、格式化、占位符与bubble；原生可执行程序无PHP SAPI，不能配置PHP ini错误目标。真实Application::handle_command经子进程验证stderr、占位符和低级别过滤且不生成应急文件；无效类型仍按既有应急记录和原消息回退。首次测试将LogManager::build的现有应急返回误当成Err，已修正夹具而不改产品恢复策略。

当前整章未闭合：Slack/Papertrail驱动实际未接线；deprecations、其余渠道、context、原生Formatter/Processor/Factory及Pail所有过滤器继续逐入口/实际消费者核对。三语仅同步本簇可用性，后续结果与提交号按批次追加。禁止推送。

第一簇最终补核发现固定createErrorlogDriver只传type/level，未传bubble；因此按构造器默认始终继续堆栈，而不错误继承通用bubble=false。子进程实际Artisan验证后续文件成员确实收到记录。5目标31项0忽略通过（含既有真实HTTP并发与隔离、真实Unix syslog、文件、上下文、监听器及原生句柄）；workspace/all-targets严格Clippy20.96s、fmt、三语构建及3组浏览器检查全0，实际main HTTP／Artisan／Ctrl+C回归通过。证据target/tmp/logging-tutorial-20261124/errorlog-{consumers,clippy,fmt,docs,main,artisan,browser-final,runtime}.log；首次夹具失败不计通过。已开始的网络驱动独立新文件尚未编译／验收，不纳入本簇提交。整章29节继续开放。
