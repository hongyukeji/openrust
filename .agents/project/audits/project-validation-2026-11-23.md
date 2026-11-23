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
