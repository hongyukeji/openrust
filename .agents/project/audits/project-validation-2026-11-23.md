# 提交收口、全量回归与初始化验收（2026-11-23）

用户授权：核对并提交本地未提交内容；完整复验当前项目、启动和初始化体验，发现问题修复并提交；验收通过后推送当前master。此授权不包含强推、版本发布、生产部署或清空开发数据。

## 本地积压收口

起点c89577865。剩余两份模型/交接记录仅保存此前已发生的操作与验收，不在本轮重新操作开发库；按其原始历史边界保留，不把旧快照当作当前全量通过。两份translation许可原文逐字核对已完整包含于THIRD_PARTY_NOTICES.md，Docker发行明确携带该声明；删除冗余独立副本，并将choice_case引用改为发行声明，保留全部版权、来源与许可内容。

全量运行验收结果随后按实际日志补充，尚未执行的项不计通过。

## 开发服务器夹具修正

全量遍历首轮artisan_entry失败：欢迎页控制器已按既有设计直接由视图读取配置，旧夹具仍引用未传入的$name，导致等不到FirstServe标题。夹具改用config('app.name')，保持原端口占用/重试、SecondServe环境重载、SIGTERM退出与端口释放断言；两项真实进程测试重新通过（0.54秒）。未修改产品控制器或降低断言。原始失败与补验分别保留在openrust-test-artisan_entry-1.log、artisan-entry-fixed.log。

## 迁移生成源码修复

全量迁移回归发现实际生成源码仍输出getSchemaBuilder/dropIfExists，缓存表Rust模板还残留mediumText/bigInteger。修复已核定的Rust生成片段与模板，保留列类型、SQL、默认值和命令行为。同步字符串断言，增加实际创建表/修改表/空迁移/缓存迁移生成物的Rust编译与命名deny检查，避免生成器和字符串断言一起保留旧名而误过。隔离PostgreSQL18/MySQL8.4与SQLite的13项根迁移回归通过，原失败和各次复验日志保留。38份Rust模板逐项AST复核仅此缓存模板有漏项，其他模板无旧Rust标识符。

验收编排另发现专属数据库URL提前返回导致漏注入、Redis限流缺少必需前缀，已修正编排并将相关目标标记重跑；不作为产品缺陷。安装器首轮还被本代理并行的补验重建覆盖了共享main产物，后续改为独占缓存串行验收并重跑，不把该失败写成框架功能失败。

## 内置语言候选的夹具回归

默认自动发现已包含框架18语，旧locale_preference与set_locale_middleware仍把fr／it作为不支持的语言。仅给这些拒绝场景显式限定原候选集合，不改变运行时发现机制；增加没有应用fr／it目录时，通过加密偏好、查询参数和选择回调实际使用内置语言的正例。最终17＋32项通过，原失败日志保留。修复后的49项使用最终邮箱解析修复组合编译运行；全工作区严格检查及分发启动继续在同一批次收尾，尚未推送。

## 邮箱规则解析回退修复

validation_email首轮暴露已知内置email的未实现参数被通用扩展规则分支接收。按api-f01/email-rule.md既有明确契约，在保留email及email:rfc的同时，对其他email参数直接返回InvalidRule；不冒充已实现DNS／spoof／strict／filter，也不改变应用自定义规则名称的解析。增加组合／未知参数反例，同步三语验证正文的实际支持边界。

最终14个验证／对齐／Precognition目标共75项通过；Precognition外部前端夹具1项ignore不计通过。包括9381组既有RFC数据、实际TCP FormRequest、生命周期、扩展、消息、密码、三库存在性及Web行为；三语内容检查通过。该结论基于本轮最终解析源码，不把首次失败或磁盘中断计入通过。

## 数据库翻译公开示例修复

全workspace实际Rustdoc测试发现translation-database的README及DatabaseLoader文档仍使用withTable／withMaxRows／withFallback／loadFrom。仅将Rust代码示例同步为现有原生命名；接口、加载和SQL行为不变。最终4项Rustdoc编译／执行通过，PostgreSQL18加载目标首轮通过；MySQL／SQLite及三库复验继续在补充矩阵记录。不以Rustdoc通过替代驱动行为验收。

## Precognition 实际浏览器的语言前置

最终额外启用固定2.0.0前端助手的ignore专项，浏览器默认协商到zh-Hans，实际中文校验提示与旧英文断言不一致。仅给该测试浏览器固定en-US并核对navigator.language；不关闭应用语言协商、不替换产品词条。Node语法检查与真实Chromium153.0.8010.12的204／422／200、文件上传／遗漏及前端验证状态全部通过（precognition-browser.log，2.30秒），原失败日志独立保留。Rust源码未变化，已通过的两套严格Clippy仍对应同一Rust组合。

## 最终已验收组合与范围

最终产品源码／三语正文以216eae945b3a0e44158e0a16eb7ea32346335145为准。固定提交Git归档与验收快照逐个比较3526个非.agents文件，仅Cargo.toml存在验收用第三方patch绝对路径；将其规范回仓库相对路径后逐字一致。artisan.bat与Git归档均按既定行尾规则核对。后续本记录与当前交接更新不改变产品源码。

| 范围 | 实际结果 |
| --- | --- |
| workspace全量 | Rust1.94.0，21个包；362个集成目标、21个lib/bin目标和20个Rustdoc目标，共403个目标已遍历；已定位失败经修复及关联复验闭合。 |
| 补充持久化／缓存 | PostgreSQL18.7、MySQL8.4.13、SQLite3.54.1；Redis8.10.3的实际INFO检查。补跑数据库翻译、Activitylog、Pennant、Scout、Pulse、Telescope的具名三库及database/file/Redis缓存消费者。 |
| 默认ignore的本地集成 | Horizon真实Redis元数据／任务／失败与重试、Pulse摄取、Telescope缓存、三库Query回调、批次分页、PostgreSQL参数专项通过。Horizon首次仅因本代理容器名不满足隔离前置而失败，按真实专用容器前缀重跑8项通过；未改测试保护或产品代码。 |
| 初始化与开发入口 | 实际基本init、扩展安装、重复／交互／指定env／失败路径、完整认证初始化及配置保留、生成项目严格Clippy通过；实际HTTP、API认证和嵌套前缀、WebSocket、SMTP、密码／双因素／恢复等通过，详见init-acceptance.log与init-auth-mysql.log。开发serve重载及端口清理2项通过。 |
| Worker与调度 | 真实SQLite／MySQL／Redis Worker硬超时、失败／重试与后续消费、默认受管进程及显式关闭通过；三库短信Worker包括加密／脱敏、429、SIGKILL前后恢复与避免重复调用通过；调度实际进程、锁与任务输出通过。 |
| 网络／传输 | Redis Cluster、Cluster TLS、Reverb TLS；SMTP STARTTLS／SMTPS／明文及认证／收件策略通过。真实短信与付费供应商未调用。 |
| 工具门禁 | fmt；workspace all-targets默认及all-features两套-D warnings Clippy；21包命名正反消费者；三语内容／构建／浏览器／已有API快照检查；固定2.0.0 Precognition助手实际Chromium通过。 |
| 独立启动 | standalone为本机dev功能构建，空PATH／无Cargo下实际key:generate、migrate、list JSON、inspire、queue:work、schedule:list、migrate:status通过；默认HTTP＋队列＋调度及关闭Runner后HTTP均通过。GET首页／up／静态资源／storage、同URL网站public优先、JSON／HTML404、目录搬迁、Ctrl+C退出和端口回收通过，分发携带LICENSE和THIRD_PARTY_NOTICES。 |

matrix-results.json保留各次原结果，取每个非initial目标的最后一次结果，共443条目标／补充验收记录（包括辅助构建）均exit0；不是443个互不重复的测试函数。gate-results.json另有14项最终门禁均exit0。原失败与磁盘中断不计通过；中断时丢失的辅助二进制先重新构建，未以旧产物假验收。

主要证据位于项目target/tmp/project-validation-20261123：matrix-results.json、gate-results.json、final-head-verification.json、versions.json、final-cleanup.json及各目标日志。大型全量验证仅对本批子进程设置CARGO_PROFILE_DEV_DEBUG=0／CARGO_PROFILE_TEST_DEBUG=0以控制符号体积，未改正式profile、增量或正常并发。共用夹具目标及补充持久化／验证专项显式顺序执行，内部并发与跨进程用例仍实际运行；全量遍历的其他纯目标保留默认并发。复用既有rust-naming/cargo-target缓存；触32GB停线后守卫停止本批、取得锁并释放68,895,707,136字节，保留源码／日志／下载／数据卷，之后从未完成项继续。最终本批容器及独立二进制进程均已回收，保留可复用缓存与本轮源码／日志／分发文件。

## 尚未纳入及发布边界

- 收尾时新出现的并行在途改动不在上述固定提交中：framework/cache及contracts/cache（含macro_support新文件）、events/event_service_provider、queue/worker_attempt_marker、fortify/tests/two_factor_provider；仍在变化、没有本批验收证据。保留原工作区及索引，不代提交，不把当前活动工作区称为全量通过。范围以实时git status为准。
- Windows／其他CPU及操作系统、完整release与容器发行矩阵、本机未提供的SQL Server／外部真实供应商／官方SDK鲜签及物理认证器不在本轮结论中。内部标记ignore的子进程夹具由父测试实际调用；独立外部前置用例未满足时不计通过。不将本次回归视为全框架上游契约或既有待办全部完成。

## 本轮本地提交索引

cf27fe3aa（历史记录收口）、4e639e764（必要许可归档引用）、3da7f3233（serve夹具）、b788882d7（迁移／缓存生成器与实际编译）、a210e483c（内置语言候选回归）、deb5f061c（邮箱参数解析）、593fecfec（数据库翻译Rustdoc）、216eae945（浏览器语言）。最终验收记录单独提交；用户本轮明确授权在核对远端线性关系后正常推送master，不强推、不推其他分支／标签。新在途工作不夹带。
