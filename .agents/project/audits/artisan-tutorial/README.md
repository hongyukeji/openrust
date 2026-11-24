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
