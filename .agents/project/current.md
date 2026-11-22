# 当前交接

## 当前任务：Request语言协商与应用策略拆分（2026-11-22）

用户授权实施。框架Request新增getLanguages／getPreferredLanguage，复用既有ICU和严格qvalue规则，按请求缓存并在parts_mut后失效；应用SetLocale删除头解析／权重计算，保留三语映射和请求语言设置。修复数字变体误认文字体系（欢迎页审计W02）。注册仍在bootstrap/app.rs，不将三语策略放进框架Middleware配置类，不新增管理器、依赖或配置键。

86项定向测试、真实Kernel／Chromium44组合、全workspace/all-targets严格Clippy、fmt及三语站点构建／两前缀导航／桌面手机通过。Symfony7.4／8.0各21个原始PHP用例通过，Rust21个适配场景中20个结果一致、1个非法扩展按既有ICU校验跳过；BCP47、严格权重／q=0、多头及候选Result等差异已明确记录，不称完全等价。六份三语正文、项目规范、MIT声明同步。详见[验收记录](audits/03-07/welcome-language-2026-11-22.md#request-语言协商与应用策略拆分2026-11-22)。证据与隔离源码在target/tmp/request-languages-20261122/，复用已空闲独占缓存；未操作开发数据，未重验Docker／Windows／正式部署。用户随后授权本轮本地提交并向主对话框发送交接；保留其他协作者的审计、API限流及计划改动，不推送。验收期间其提交未改变本轮输入。

## 历史任务：欢迎页复用译器回退（2026-11-22）

基线 `02fdc4ce8`，用户授权处理欢迎页控制器中重复的语言判断。本轮移除标题探测、手工选择文档语言及硬编码英文兜底；HTML `lang`／`Content-Language` 使用应用当前语言，文案与文档链接在 Blade 中逐键翻译。三语 `welcome.json` 提供 `documentation_url`／`deployment_url`；新增语言只需提供资源，不修改控制器或框架。该结论覆盖下方历史任务中的“标题语言决定页面语言／手工选择文档语言”规则。

欢迎页11项、startup28项共39项通过；实际Kernel＋Chromium36组合通过，覆盖关闭识别与部分法文资源、三语／脚本别名、桌面手机及明暗模式。Rust1.94.0定向严格Clippy、全项目fmt通过；三语文档同步配置回退与缺键边界，站点构建、两前缀导航及桌面手机检查通过。详见[本轮记录](audits/03-07/welcome-language-2026-11-22.md#欢迎页回退职责修正2026-11-22)。用户随后授权将本轮代码、测试、三语文档及项目记录本地提交，并将方案与验收结果发送主对话框；不包含推送。保留协作者的todo及全量审计／计划。临时材料在 `target/tmp/welcome-resource-links-20261122/`，复用已空闲的独占Cargo缓存；未操作开发数据，未重建Docker或执行Windows／发布环境验收。

## 历史任务：BCP 47 全项目迁移（2026-11-22）

基线 `59cb9f0d1`，master；本轮在用户和协作者在途修改上实施，默认三语、公开配置／API／事件／序列化载荷、语言资源及文档目录／URL 统一为 en／zh-Hans／zh-Hant。用户已授权实施及隔离验收，随后明确授权暂存和本地提交；本记录随本次提交保存，**不包含推送、发布或操作开发／生产数据**。本节覆盖下方历史任务的 Git 授权与旧下划线约定。

按[计划](plans/locale-bcp47.md) A→E 完成：复用 ICU 完整 Locale 规范化，区分地区与文字、保留支持的扩展；Faker 仅私有 provider 适配，Number／翻译／邮件／通知／Worker 采用同一公开标签。lang 和 docs 实际迁至 zh-Hans／zh-Hant，浏览器选择三语是应用策略，关闭中间件的欢迎页回退与有效链接保留；Blade 缓存版本38。三库只读 inventory 和默认 ROLLBACK 迁移脚本已交付，不覆盖冲突。

Rust1.94.0 定向测试、fmt、workspace/all-targets严格Clippy、三库翻译与升级、Faker实际消费者、启用8扩展9项配置消费者、Passport MySQL12项、真正邮件通知Worker与生成器、MySQL完整初始化／生成应用通过。真正Kernel＋Chromium欢迎页36组合、三语文档站12组合及搜索／切换／404、独立二进制冷／缓存／搬迁运行、原始Dockerfile Linux release构建与6语言HTTP／资源复制通过。Schema注释的旧zh-CN简体期待已纠正，三库实际消费者复验通过。后续收尾已复用 TestApp::mysql 修复 seeding 共享表冲突，保留正常并发，新增隔离／清理回归后完整71项连续三轮通过；最后两轮同实例运行，逐轮确认没有遗留测试数据库。逐项命令、失败记录和边界见[验收记录](audits/03-07/locale-bcp47-2026-11-22.md#后续收尾2026-11-22)。

用户后续要求全部处理好，本地 `.env` 的 APP_LOCALE／APP_FALLBACK_LOCALE／APP_FAKER_LOCALE 已由 zh_CN 更新为 zh-Hans，仅改这三个值，其他字节及权限不变，私密备份保留在本批 target/tmp/ 下。使用该配置的隔离副本通过 Artisan、冷／缓存启动及三语8个HTTP场景；真实配置缓存原本不存在，未清开发缓存、未访问开发／生产数据库或卷。应用自有数据、设置及旧队列仍须部署者按[升级说明](../../docs/zh-Hans/digging-deeper/localization.md#bcp47-upgrade)审阅迁移。独立private-use／grandfathered／extlang为ICU Unicode Locale的公开支持边界，显式失败。Windows、正式托管及真实外部供应商未验，见待办。未变更共享缓存引擎／持久化键协议，未启动无关三缓存矩阵；配置、Blade及进程翻译缓存已验。

源码和独占Cargo缓存复用 `target/tmp/welcome-locale-20261122/`，本轮日志与临时材料在 `target/tmp/locale-bcp47-20261122/`；保留增量和正常并发，所有子进程TMPDIR在项目内。最终按范围对账验收输入与交付源码，HEAD及索引未变；本批隔离服务退出、缓存／源码／日志／独立交付保留，开发数据及卷未动。本节以下均为历史，仅代表其当时源码与授权。

## 历史任务：欢迎页与翻译契约二次复核（2026-11-22）

基线 `d4a40d7f4`；修复深层 Config 父键替换残留、数组下标和关联列表操作、关闭 SetLocale 后非三语语言的欢迎页元信息及文档链接、Blade 空键／集合／回退／复数边界、App 事件错误传播顺序及首次解析译器污染应用默认值。复用既有仓储、Arr、译器和作用域快照，语言标识保留下划线；不重复修改已完成项。缓存版本37，六份三语正文同步。

最终100项定向测试、workspace/all-targets严格Clippy、fmt、三语文档构建与导航／浏览器检查通过；固定PHP最小夹具26项、真实Kernel浏览器24组合通过。Rust1.94.0，源码和Cargo缓存复用 `target/tmp/welcome-locale-20261122/`，本轮证据 `target/tmp/welcome-locale-review-20261122/`。详见[本轮复核](audits/03-07/welcome-language-2026-11-22.md#d4a40d7f4-基线复核)。模板trans()服务对象尚未实现；未验完整PHPUnit、Docker镜像、独立部署、Windows／优化release或真实队列worker专项，不涉及数据库／缓存矩阵。

本提示词未授权暂存、提交、推送或发布，索引保持原状；其他协作者的BCP47迁移计划及全量审计保留原样，本轮不实施语言迁移。


## 历史任务：统一请求与模板国际化（2026-11-22）

在已提交欢迎页三语识别 `443f40af4` 上继续改造：语言选择从 WelcomeController 移入应用 SetLocale 中间件，经 bootstrap/app.rs 全局注册，候选语言直接定义在应用中间件中，为 en／zh_CN／zh_TW，不新增 app.supported_locales。中间件通过 App::setLocale 设置当前请求语言，控制器、校验及模板共用请求语言，保留其他请求、任务与应用默认值隔离；已有 Content-Language／Vary 保留。

Blade 已接入 __／trans／trans_choice 以及 @lang 区块和带参数 @lang／@choice，直接复用 Lang；新增语言快照传入既有阻塞渲染线程，缓存版本 36。欢迎页直接翻译，控制器仅准备页面数据和显示元信息。三语国际化／Blade 正文及项目规范同步更新。支持边界与完整证据见 [验收记录](audits/03-07/welcome-language-2026-11-22.md#统一请求与模板国际化)。

按固定 Laravel 教程重新复审：补齐 Application／App 的语言入口和 LocaleUpdated 事件，配置、译器和事件在当前请求／任务内同步；修复别名判断和 Config 父组覆盖一致性，移除新增 supported_locales 默认配置。三语正文以 APP_LOCALE／APP_FALLBACK_LOCALE→App::setLocale／currentLocale／isLocale→模板翻译为主线，明确浏览器识别是应用策略及 Rust 的 JSON／await／Result 边界。

最终 70 项定向回归（含三语原始路由示例实际编译运行）、工作区全部目标严格 Clippy、fmt、三语文档构建和导航／桌面手机检查通过；精确别名设置场景追加后 application_locale 4 项及 Clippy／fmt 补跑通过。固定 Laravel 翻译 Trait 在隔离 PHP 8.5.12 中 4 项对照、独立部署实际服务三语×桌面手机×明暗 12 组合通过。复用 target/tmp/welcome-locale-20261122/，Rust 1.94.0；曾触及磁盘停线，持锁清理净释放约 38 GB 后重新完成最终验证，日志与源码保留。未重建 Docker 镜像、未验 Windows／优化 release，未访问开发数据库。最终证据和限制见上述报告末节与 tutorial-closure.json。2026-11-22 用户授权将本次国际化实现、测试、三语文档及验收记录本地提交；本记录随该提交保存，不包含推送。

## 历史任务：清理后全面测试与运行（2026-11-22）

用户要求清除编译产物后重新全面测试，基线 `fb5240e1ae08855857d9b9988ac5453d8623e608`。本轮可执行的本地验收已完成：首轮 67 组覆盖 20 个 workspace 包、19 个库与 303 个集成目标；原失败退出码保留，对应目标均按根因修复并实际补验。详见 [本批报告](audits/08-04/clean-full-acceptance-2026-11-22.md) 的结果表和失败映射。

生产修复为 Faker 严格日期格式、integer 浮点边界及 regex 类型约束；测试夹具同步隔离、迁移归属和 Laravel 契约，六份三语文档更新。三库、Redis／database／file、Cluster/TLS、邮件 12 场景、PG/MySQL 完整生成应用、Passport／Precognition／OIDC 浏览器、Telescope／Horizon／Pulse 真实本地进程和桌面手机界面、standalone 搬迁运行均通过；最终全 workspace 严格 Clippy、fmt、doctests 和三语文档检查通过。缺环境或 ignored 自身不计服务通过；真实供应商、正式部署、Windows 及未实现能力继续见 todo.md。

本批目录 `target/tmp/clean-full-acceptance-20261122/`，短 TMPDIR `target/tmp/cf/`；Rust 1.94.0、独占 target，保留增量和正常并发。磁盘四次停线均安全停构建持锁清理，保留锁 inode、源码、日志、下载、数据库和卷；收尾可用约 51.82 GB／48.26 GiB。5 个本批测试容器已回收，原开发服务状态与卷集合保持不变。3,246 项输入仅 artisan.bat 的 LF/CRLF 有字节差别，归一化后全部内容一致。2026-11-22 用户授权将本轮修复、测试、三语文档及验收记录本地提交；不包含推送。其他协作者独立复核原文和欢迎页国际化在途改动保留。用户随后要求清理构建产物，已持锁清理并净释放约 18.02 GB；验收源码、日志、下载、独立交付和数据保留，详见本批 user-cleanup-result.json。

## 历史任务：Laravel 对齐审计 L01–L22

2026-11-21 用户明确要求修复 [Laravel 对照审计](audits/laravel-code-audit-2026-11-21.md) 的 L01–L22。固定 Laravel Framework v13.40.0、骨架 v13.11.0；本任务独立执行，不调度其他对话，不重复处理历史扩展 R1–R6。以下旧收尾记录为历史结果，不代替本批源码验收。

- 当前实现已补齐列明的行为、契约和生命周期，包含 Validation／FormRequest、Filesystem 完整绑定链、Mail取消／地址默认、Auth／Queue原名迁移和实际消费者。三语开发者文档同步用法、所有权与迁移影响；逐项结论、失败记录和边界见审计最新修复节。
- 已实际运行三库Query、三库database cache与Redis／file、三库数据库翻译，以及四扩展、Horizon Redis9、Passport Chromium3、官方Precognition2.0.0浏览器；PG和MySQL完整生成应用验收通过。2026-11-22最终核对3,246个源码／文档输入一致；Eloquent MySQL／SQLite消费者6项、三库Query与Redis补跑、workspace/all-targets/all-features严格Clippy、根及pcre2格式、三语构建与浏览器均通过。ignored／未配置的供应商分支不算通过。
- 临时批次为 `target/tmp/laravel-alignment-fixes-20261121/`；复用独占 `target/tmp/final-acceptance-20261121/cargo-target/`，Rust1.94.0、TMPDIR `target/tmp/la/`。本批曾触及磁盘停线，持锁清理编译产物后恢复；不删除日志、下载、源码、开发数据或卷。当前受影响工作流因资源压力关闭增量，未改变日常策略。
- 本批三个隔离容器已按登记身份回收，数据卷集合保留；源码、证据和热缓存保留。2026-11-22收尾可用约51.92 GB／48.35 GiB，高于停线，详见本批`resource-closure.json`与实时df。
- Reverb用户认证HTTP签名已验，但完整pusher:signin客户端协议仍未实现；真实供应商、正式部署、Windows及其余API候选分别见待办。不能以本批通过推导全部Laravel API或真实目标通过。
- 2026-11-22用户已明确授权将本任务完成的代码、测试、三语文档和审计记录暂存并本地提交；本次授权不包含推送、发布或部署。

## 历史：上一轮发布前本地收尾

2026-11-21 按用户要求独立完成发布前的本地收尾，不再联系其他对话或委派子代理。真实服务目前不具备条件，统一保留在 [todo.md](todo.md)，不算验收通过，也不阻塞本轮可执行的本地工作。

根应用继续使用 `bootstrap/packages.rs::manifest()` 装配包清单，默认 Provider 不预装扩展。开发与 Docker 共用根 `.env`；`docker/.env.example` 仅提供可选 Docker 字段参考，Compose 不加载 `docker/.env`。注释与未国际化提示优先简体中文。

## 本轮已确认

- 根 17 个定向测试目标共 69 项通过；六扩展组合 110 项通过，11 个 Redis 忽略项已用隔离 Redis 8.10.3 补跑。
- PostgreSQL 18.7、MySQL 8.4.13、SQLite 与 Redis／database／file 缓存的定向组合通过；缺连接提前返回的测试不计为数据库验证。
- Telescope 初始化采集边界与终止写入修复；数据库缓存过期测试改为符合整秒存储契约的有限轮询；生成应用夹具只过滤根运行目录，保留包内同名源码。
- Telescope／Horizon／Pulse 的真实本地服务、桌面和手机浏览器通过；Linux ARM64 Debian Bookworm、Rust 1.94.0 的四个选定目标通过。Windows 原生构建受 Parallels 共享文件系统 `os error 87` 阻塞，条件和后续矩阵已列待办。
- 全 workspace／all-targets／all-features 严格 Clippy、格式通过；全 feature 锁定依赖公告 0 errors／0 warnings，672 项许可证来源均有声明。实际发行许可义务仍按交付物复核。

## 本轮完成

修复已本地提交为 `0855b54f2a984ac25b68ab74d4e2ca83962d68fc`，验收快照代码／测试与该提交独立归档逐字节一致。PostgreSQL／MySQL完整生成应用、重复初始化与资源安装、实际HTTP及独立开发profile二进制搬迁运行通过；未执行优化release构建。

19库可追溯API已从该提交新归档导出，覆盖2,171项输入指纹，产物在 `target/tmp/final-acceptance-20261121/api-export/`；三语联合站点在同批 `api-source/docs/.vitepress/dist/`。最终结果与失败尝试边界统一见 [08-04](audits/08-04/README.md#2026-11-21-发布前本地收尾)。19库／2,171项输入API导出、三语联合构建、14项过期／非法输入负例、7项CLI拒绝、API与普通页面各12浏览器组合及6组全站导航全部通过。最后可用约59.29 GB／55.22 GiB。本地收尾不等于正式发布，真实目标与Windows矩阵仍见待办；其他对话的未跟踪Laravel对照审计保留，未纳入本轮提交和全部API等价结论。

## 产物与证据

本轮全部临时源码、编译目录、日志和浏览器材料均在项目 `target/tmp/final-acceptance-20261121/`；子进程使用项目内 TMPDIR 与独占 CARGO_TARGET_DIR。已安全回收本批闲置Linux编译缓存7,486,048 KiB，保留源码、日志和锁inode；本批容器已回收，Windows恢复初始suspended。此前清除构建产物时旧 `target/tmp/` 内容一并被移除，旧审计中记录的路径不能当作仍现存的证据；本轮重新生成必要材料。不得整目录删除根 target；32 GB 停线按 [构建产物管理](conventions.md#构建产物管理)安全清理。

## Git 与发布授权

保留用户与其他协作者改动，包括本轮出现的未跟踪审计报告。2026-11-11 用户授权仍有效：阶段实现、验收、三语说明和记录完成后可核定范围本地提交。未授权推送、改发布版本、打 tag、创建 Release 或生产部署。最终发布批准与真实服务验收继续单列。
