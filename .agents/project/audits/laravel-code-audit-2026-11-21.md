# OpenRust 全仓 Laravel 对照审计（2026-11-21）

## 2026-11-21 修复与重新验收

本节于2026-11-22完成收尾核对，为 L01–L22 的修复结论；下方“初审历史”保留原始发现，不能将初审时的“未实现／未运行”当成当前状态。修复工作起点为 `259b8a93a`，保留其他在途改动；2026-11-22用户授权将本轮完成的代码、测试、文档和记录本地提交，未授权推送。范围是这 22 项明确接口与行为，不是全部 Laravel API 的等价证明。历史扩展 R1–R6 未重复开工。

### 来源、复用与必要适配

- 重新核对固定 Framework `v13.40.0 / 1449bf241017087fbd38c118ecaabef3f6797fdc` 的源码和测试、应用骨架 `v13.11.0 / 06d016a364a37430eec9cbc52209adce3d7667ce`，沿用下方各项固定上游链接。固定参考源码在 `target/tmp/laravel-code-audit-20261121/laravel-framework/`，补充研究与 PHP8.5 实际探针在 `target/tmp/laravel-alignment-fixes-20261121/research/`；没有改用最新分支。
- 已有容器、HTTP 内核、Blade、事件分发、Query grammar、PCRE2、task-local 与包发现均直接复用。没有增加第二个管理器或全局可变服务。Guard／StatefulGuard 原来已有原名入口，不重复重命名；只迁移本次确认的 Auth／Queue 契约与消费者。
- PCRE2 原有 Rust builder 不暴露 PHP `D` 所需的原生选项，新增 `vendor/pcre2` 本地补丁：源自 rust-pcre2 0.2.11，提交 `9f269262a71d88605c67bd3fcc8633970aa6ad81`，仅开放 `RegexBuilder::dollar_endonly(bool)` 并设置原生 `PCRE2_DOLLAR_ENDONLY`。保留 MIT／Unlicense，默认关闭。网络固定源码与 registry 原文件一致，未重写正则引擎；正反测试覆盖结尾换行、D、mD。说明见 `vendor/pcre2/README.openrust.md`。锁文件没有顺带升级依赖。
- Rust 不支持 PHP 任意参数和动态调用：公开原名保留，异步 I/O 返回 Future／Result；Mail 取消为 `Ok(None)`；调度时长用 Duration；Query 回调可能修改查询，因此执行／子查询／toRawSql 持有可变 Builder；文件 `move` 用原名 raw identifier `r#move`，append／prepend 显式 separator。Locale 的原名 setter 用 `&self -> Result`，读值返回 owned String；仅作用于当前应用执行作用域。所有权差异和迁移用法已同步三语正文。

### 逐项结论

下表日志均相对于 `target/tmp/laravel-alignment-fixes-20261121/`；源码路径相对于仓库。回归用例的核心集中在 `vendor/openrust/framework/tests/laravel_alignment_{validation,http,services,database}.rs`，另运行实际消费者，而非以方法名存在或编译通过关闭问题。

| 问题编号 | 核对结论 | 实际修改 | 验证结果 | 未验证范围／剩余阻塞 |
|---|---|---|---|---|
| L01 | 原报告行为差异成立，已修复 | PendingRequest 的 replaceHeaders 只覆盖传入键；补 withHeader/contentType/asMultipart/maxRedirects/sink | `alignment-recheck.log` 的 HTTP client 20 项：回环捕获、旧头保留、同名替换、多值／无效头、multipart、重定向和 sink 失败 | sink 仍是受 body_limit 限制的缓冲下载，非无限流式下载；已写三语边界 |
| L02 | 原差异成立，已修复 | integer 默认遵循 PHP 过滤语义、限制有符号范围；补 integer:strict／boolean:strict | `alignment-delivery.log`：bool／整数／浮点／前导零／正负号／ASCII及Unicode空白／溢出，与固定PHP探针对照 | 不宣称所有 PHP 类型转换全集相同 |
| L03 | 原差异成立，已修复 | min/max/size/between 阈值用现有 BigDecimal，负数和小数可解析；按 numeric/string/array/file 分流 | 同日志：负数、零、精确小数、大数和科学计数法边界；文件按精确 KB 比较 | 未扩展到未列入支持矩阵的规则 |
| L04 | 原差异成立，已修复 | Request 统一 prefetch，并补 ajax/pjax/userAgent/fingerprint/user；StartSession 复用判断，响应后保留请求 Session | `alignment-delivery.log` HTTP7：X-Moz、Purpose、Sec-Purpose 精确忽略大小写、反例、previous URL／route、fingerprint 注册顺序与自动 HEAD | 不代表所有代理／Session 安全路径重审 |
| L05 | 原差异成立，已修复 | 根 config/env.rs 与 session.rs 复用 Str::slug，统一默认 Session cookie／cache／Redis 前缀 | `latest-root.log` configuration2：应用名、空格、重音、@、显式覆盖；`cache-real-final.log` 实际三缓存与三库database缓存 | 默认 cookie 改名使旧默认登录会话不可见；默认前缀改变不自动迁旧缓存，显式配置可保留旧命名 |
| L06 | 原差异成立，已修复 | Contracts／Facade／PendingMail 的 sendNow 统一 Option；always* 地址默认、具名 mailer、Return-Path 与 SMTP/SESv2 接入同链 | `alignment-delivery.log`：取消无运输／无 MessageSent、不同入口一致；启动默认与并发作用域；`view-mail-recheck.log` 31 项（含协议模拟） | 真实 SMTP／SES／其他供应商和最终收件人未验；无供应商变量提前返回的分支不算供应商通过 |
| L07 | 列明的十类规则缺口成立，已补齐 | numeric/date/in/regex/required_if/exclude_if/accepted/url/uuid/array:key；错误消息 attributes／values 翻译，精确排除文件投影 | `alignment-delivery.log` Validation7＋HTTP7：正反输入、missing/null、nullable、嵌套／通配符、条件值翻译、regex D/mD；固定PHP数值日期探针 | 仅承诺已声明的规则／日期语法／regex修饰符；PHP规则与PCRE全集仍是待核对候选 |
| L08 | 原缺口成立，已修复 | Validator after/sometimes/safe/stopOnFirstFailure；FormRequest withValidator/after/全局停止，保留 Precognition 顺序 | 同日志；`alignment-recheck.log` precognition8：授权／验证失败、after负例、safe数据边界、文件排除与请求隔离；官方前端浏览器见下方最终验收 | 不能把默认 ignored 的前端测试直接算通过 |
| L09 | 契约未落地问题成立，已闭环 | Filesystem/Factory/Cloud 由既有Adapter/Manager实现；Provider绑定；Facade、copy/r#move/append/prepend/json/stream贯通 | `alignment-delivery.log`：容器解析trait真实消费者、本地操作与失败、S3回环copy/move编码／ACL／复制失败保留源；`core-final.log` local9 | S3为本地协议端点，真实AWS权限／签名／区域尚未验；Cloud只适用对象存储 |
| L10 | 原公开入口缺口成立，已补齐生命周期 | Builder withBindings/withSingletons/withScopedSingletons/withSchedule/registered/booting/booted/prefersJsonResponses | `alignment-delivery.log`：钩子顺序、失败不boot、独立应用、scoped隔离、调度懒注册；HTTP显式异常策略优先 | 不宣称全部ApplicationBuilder公开接口全集 |
| L11 | 指定Auth契约差异成立；Guard已有原名不重复改 | Authenticatable/UserProvider/Factory原名，根模型／生成器／Fortify/Passport/Sanctum/Socialite及其他受影响包同步；修 Factory.shouldUse 转发递归 | `latest-root.log` auth_models6、`core-final.log` Auth/Session/Sanctum；`extensions-last.log` 四扩展常规测试；Passport Chromium3；生成应用见下方最终验收 | 真实OAuth提供方／设备仍未验；Fortify曾出现一次Session存储失败，复验事实见下方，不写成已定位根因 |
| L12 | 指定Queue原名缺口成立，已修复 | Job/Queue/Worker原名及模型／生成器／CLI／Horizon消费者同步 | `core-final.log` queues5、worker5含三库database与Redis分支；`horizon-redis-delivery.log` 原ignored9显式实跑 | SQL Server被明确过滤，SQS／商业队列未做真实供应商验收；helper ignored不计通过 |
| L13 | 原缺口成立，已复用现有日志机制 | withContext/withoutContext/shareContext/sharedContext/flushSharedContext；区分应用启动默认与当前作用域上下文 | `alignment-delivery.log`：已解析／后建频道、stack、覆盖／移除、并发隔离；`core-final.log` logging14 | 无跨请求共享可变全局 |
| L14 | 原缺口成立，已修复 | Dispatcher与Facade push/flush/forgetPushed，保留flush可重复分发而不消费清空 | `alignment-delivery.log`：顺序、多次flush、forget、fake、并发scope及defer交互 | 未把defer／afterCommit冒充push生命周期 |
| L15 | 指定断言缺口成立，已补充真实测试入口 | TestResponse JSON path/structure/errors、redirect/session/cookie/download；复用原HTTP测试内核 | `alignment-delivery.log`：实际响应成功和故意失败断言；Session响应后可检查 | 不执行PHP unserialize；不宣称Laravel全部测试断言已移植 |
| L16 | 固定版本新增能力缺口成立，已实现 | Schedule.whenTaskRunsLongerThan 与Finished/Failed监听，Duration阈值 | `alignment-delivery.log`：相等不触发、超过触发、缺runtime、失败与后台完成不重复；`core-final.log` scheduling17 | 后台helper忽略项不是额外通过项 |
| L17 | 原缺口成立，已实现及同步ORM | beforeQuery/afterQuery/toRawSql，复用grammar插值；可变子查询一次性消费，事务安全snapshot | `verified-query-redis.log` 同一函数实际逐个连接PG18/MySQL8.4/SQLite：修改、映射、顺序、重复、失败、子查询、事务；`verified-orm.log` MySQL／SQLite关系消费者6项通过 | 不代表所有SQL方言、ORM关系和分页全集 |
| L18 | 原缺口成立，已实现 | UrlGenerator forceScheme/forceHttps/forceRootUrl/formatHostUsing/formatPathUsing；owned策略保留请求隔离 | `alignment-delivery.log`：named/domain/signed URL、路径编码、不同请求隔离；`core-final.log` urls5 | 正式域名／真实反向代理与HTTPS仍待部署验收 |
| L19 | 原缺口成立，已修复渲染上下文链 | Factory renderWhen/renderUnless/renderEach/getShared；boot／scope共享；blocking renderer传ScopeSnapshot与Facade上下文 | `alignment-delivery.log`：条件、循环key/index/iterator、空raw/view、覆盖与并发；HTTP真实渲染；`view-mail-recheck.log` view5复验 | 不复制PHP内部渲染计数器；无第二个模板引擎 |
| L20 | 用户认证HTTP能力缺口成立，已实现声明范围 | Broadcast userRoutes/authenticateUser、resolver、路由属性、具名连接／guest／socket与reflash；Reverb签名复用既有HMAC | `latest-broadcast.log` 真HTTP1覆盖成功／拒绝与签名；`core-final.log` broadcasting3 | Reverb服务端尚未实现完整pusher:signin客户端协议；其他未支持driver明确失败；托管Pusher未实测。HTTP endpoint通过不等于端到端用户登录完成 |
| L21 | 原缺口成立，已实现 | RedisManager purge/extend复用既有连接缓存、connector与事件分发 | `verified-query-redis.log` 隔离Redis8显式实跑：具名清除重建、旧Arc仍可用、dispatcher替换／事件开关、自定义connector；实际版本见services.json | 不伪造PHP的phpredis/Predis双驱动 |
| L22 | 原trait及具体入口差异成立，已修复 | Contracts／Translator／Lang同步getLocale/setLocale/getFallback/setFallback；task-local、canonical locale、config cache贯通 | `alignment-delivery.log`：trait／concrete／Facade、嵌套恢复、并发隔离与配置缓存；`latest-root.log` localization13；translations三库各8 | owned String／&self Result是明确Rust适配；不引入进程级语言覆盖 |

### 验收环境、失败记录与证据

- Rust 1.94.0；独立源码 `target/tmp/laravel-alignment-fixes-20261121/source/`，独占且复用的 `target/tmp/final-acceptance-20261121/cargo-target/`，TMPDIR `target/tmp/la/`。本批资源压力曾触及停线，取得Cargo锁后清理项目编译产物，保留源码／日志／下载／数据；`disk-cleanup.json`记录。为当前受影响工作流关闭增量，未改仓库日常策略、工具链或并发全局设置。
- PG18、MySQL8.4与Redis8均用本批tmpfs容器、随机回环端口、独立库和前缀；SQLite／file在项目临时目录。`services.json`记录容器身份，未操作开发库或开发Redis。database缓存三库及Redis/file真实驱动均传REQUIRE和实际连接；`cache-real-final.log`37项通过／1个子进程helper ignored，未设置环境的其他供应商分支不算实测。
- 最新 `delivery-results.json` 的alignment、三库Query、Horizon Redis9、Passport Chromium3、全workspace/all-targets/all-features严格Clippy均退出0。alignment分组为HTTP7、services17（Redis另显式实跑）、Validation7；原始日志保留ignored／filtered，不累计为通过。部分旧批次先失败后修复，以后续定向复验为准，不删除失败日志。
- 复验中实际发现并修复：blocking View渲染未传作用域；Auth Factory转发递归；Passport迁移脱离Application Facade作用域；旧schema测试未考虑已批准的本地化comment。Schema comment沿用`schema-comments.md`明确适配，夹具先核验真实译文，再规范化comment对照固定PHP结构，未删注释或放宽其他列属性。缓存短TTL改为有上限的实际过期轮询，Redis锁前缀按进程隔离；测试Redis URL去除固定`/0`以保留测试显式DB选择。
- Fortify accounts_http曾出现一次Session Storage失败；同二进制在隔离MySQL连续12轮及后续完整扩展批均通过，见`fortify-stability.log`、`extensions-last.log`。没有可靠证据定位该次失败的根因，保留记录，不声称已修复一个已确认的生产缺陷。
- 最终源码内容比对：工作区与隔离源码的3,246个文件（排除内部记录）一致，`delivery-source-check.json` 无差异；发现并纠正隔离快照中HasOneThrough过时的可变接收者，移除因此产生的无必要测试改动，再运行最终`verified-alignment.log`与`verified-clippy.log`，均退出0。最终锁文件仅将pcre2改为本地来源；保留tempfile原getrandom版本，未捎带依赖更新。根workspace及独立pcre2的fmt通过，`git diff --check`通过。
- `verified-precognition-browser.log`显式执行原ignored用例，laravel-precognition2.0.0／Chromium153.0.8010.12：204预验证、422字段错误、200正式提交、文件校验／省略文件／上传均通过。第一次配置漏写浏览器.app内可执行路径，修正验收环境后通过，保留失败日志，不算产品修复。
- `generated-results.json`最终PG／MySQL各bootstrap与完整生成应用均退出0。使用本批隔离容器中新的`alignment_generated_final`数据库，验证安装／重复安装／定制保留／迁移、实际HTTP／CSRF／Session／回环SMTP、登录／邮箱验证／密码／2FA／恢复码／重置／限流／关停、生成代码严格Clippy及feature-off构建。初次bootstrap误用无fortify的default feature，改为显式`--features fortify`；复用前次数据库曾因时间戳迁移重复建表失败，最终改为新的专属数据库，未修改默认应用feature或删开发数据。失败批及源码在临时目录保留。
- `advisories-delivery.jsonl`：锁定all-features依赖公告0 errors／0 warnings（工具另提示未设置策略文件，使用默认策略）；`licenses-delivery.json`：706个resolve包均有许可证声明，pcre2保留Unlicense OR MIT及原文。实际发行交付物的许可履行仍需发布核对，不能以元数据声明替代。
- 三语正文最终`docs-content-delivery.log`、`docs-delivery-build.log`通过；`docs-delivery-browser.log`覆盖三语桌面／手机及根／子目录12组合；`docs-delivery-scope.log`覆盖6组全站导航、每语19个支持边界页。未将旧API导出当作当前源码的新API快照。
- 本批服务版本实际查询为PostgreSQL18.7／MySQL8.4.13／Redis8.10.3，见`service-versions-final.json`。SQLite使用测试内独立内存数据库。最终`verified-orm.log`六项全部通过（MySQL与SQLite关系消费者），`verified-query-redis.log`三库Query与Redis原ignored两项显式实跑均通过。
- 2026-11-22核对收尾：`resource-closure.json`确认仅回收本批登记的三个容器；操作前后数据卷集合保留，未删除卷或开发数据。保留热构建缓存、源码、日志和失败证据。收尾核对可用空间约51.92 GB／48.35 GiB，高于32GB停线；空间会随其他工作变化，以df实时值为准。未暂存、提交或推送。

### 使用与迁移影响

三语正文覆盖Validation、Filesystem、Mail、Auth、Queue、HTTP测试、配置默认值、调度、Query、Log/Events、URL/View、Broadcast、Redis、Translation。Auth／Queue自定义实现需按新的原名契约更新；Query执行及含回调的子查询可能需要可变绑定；Mail sendNow须处理None；文件append／prepend明确separator。Session默认名和cache/Redis默认前缀的变化不会自动搬迁旧数据；可显式保留旧名称，不能通过清空开发数据“迁移”。

### 另列待核对与真实目标

- 初审`api-candidates.json`未判定的候选仍需逐方法核对，不能由L01–L22的修复推广为全部通过。Validation剩余PHP规则／完整日期与PCRE选项、完整TestResponse、其他Query／ORM能力属于未承诺全集的候选，不混入本批完成项。
- Reverb完整`pusher:signin`为尚未实现的客户端协议能力，不能写成只缺外部凭据；已有用户认证HTTP签名不代替完整握手。其他广播驱动、Queue failed provider、Scout等支持范围沿用原矩阵，不重开历史R1–R6。
- 真实SMTP／SES收件、S3 IAM与区域、托管Pusher／OAuth、SQS、正式HTTPS及最终Windows／发布矩阵未验；恢复条件继续见[todo](../todo.md)。本地回环协议／Chromium／隔离数据库是本轮实际验收边界。

## 初审历史（以下结论对应修复前源码）

## 结论与基线

**不能认定当前代码已全面按规范 1:1 对齐 Laravel。** 应用目录、Provider／包发现、原生服务复用及多项核心机制已有对应实现，但本轮确认了 **6 项可推导出不同结果的行为问题、16 项公开 API／契约缺口，共 22 个可派工单元**。一个单元可含同一负责人应一起核对的方法；不是“全仓只有 22 个缺失方法”，也不是 22 个已运行复现的 Rust bug。

源码审计基线：`b5854f596574026e6acfeb063d8db01f94a00bab`。固定 Laravel Framework `v13.40.0 / 1449bf241017087fbd38c118ecaabef3f6797fdc`，应用骨架 `v13.11.0 / 06d016a364a37430eec9cbc52209adce3d7667ce`，文档导航 `960eedf0911155ad16326ade49c0ea5d18eda0b8`；未追随 latest 偷换版本。依据为[项目规范](../conventions.md)、[固定基线](../plans/laravel-rebuild-baseline.md)、当前 Rust 实现及实际下载的官方固定源码。

P2：应优先修正的行为差异或常用契约缺口；P3：命名、辅助入口和扩展点。优先级针对本项目移植目标，不代表已发现安全漏洞或已证明生产故障。

## 方法、证据与限制

- 清单覆盖根应用 `app/bootstrap/config/database/routes` 及 `vendor/openrust` 的生产 Rust 文件：初始快照共 1,474 文件、416,119 行，其中 framework/src 1,086 文件、360,093 行。计数只界定扫描范围，不表示逐行人工阅读或正确性覆盖率；框架根导出、应用入口、Cargo 和相关三语文档另按职责核对。
- 对可规范化匹配的 PHP/Rust 路径建立 425 对公开方法候选，其中 205 对出现全框架未检出标识符。候选不是缺陷：剔除了 trait 默认实现、宏展开、其他文件转发、PHP/PDO/Guzzle 特有接口及已经记录的 Rust 适配，再人工核对下表选定条目的双方实现。该候选文件仍有待后续逐方法判定的部分，不以关键词不存在直接关闭或认定不兼容。
- 网页工具未返回有效正文，实际通过 HTTPS GitHub REST API 下载上述两个固定提交的源码包。版本及来源写入临时 provenance；本轮不进行新的框架/依赖选型。
- 用现有 `php:8.5.12-cli-alpine` 在无网络、只读容器中执行实际固定 `ValidatesAttributes::validateInteger`：`true / 1.0 / 1 / "1" / U+2003 字符串` 的默认及 strict 结果已记录。请求头与预取参考仅运行从固定源码复制的表达式，未启动完整 Laravel HTTP；Rust 对应结果为实现分支静态推导，未编译复现程序。
- **本轮没有运行 Cargo 构建、全量测试、Clippy、三库/三缓存或真实供应商矩阵，也没有逐行证明全部代码等价。** 未在下表出现不等于全部通过。历史 `66/66` 只适用于已命名单元，不能扩写成所有公开 API 或发布验收完成。
- 审计开始时工作区干净；收尾出现其他协作者的 `.agents/project/todo.md`、Telescope Watcher、测试及三语正文改动，本轮保留，不将这些在途改动当作已验收版本。报告仅新增本文件，临时源码和证据在项目 `target/tmp/laravel-code-audit-20261121/`，未修改实现、暂存、提交或推送。

临时材料：[源码扫描清单](../../../target/tmp/laravel-code-audit-20261121/source-inventory.json)、[API 候选](../../../target/tmp/laravel-code-audit-20261121/api-candidates.json)、[来源与参考输入摘要](../../../target/tmp/laravel-code-audit-20261121/provenance.json)、[PHP 探针](../../../target/tmp/laravel-code-audit-20261121/probe.php)、[实际输出](../../../target/tmp/laravel-code-audit-20261121/probe-output.jsonl)。这些是可重建的临时证据，长期结论以本报告的固定提交与下表代码/上游位置为准。

## 已确认行为差异

下表证据为人工核对双方代码；L02 另有实际上游方法执行，L01/L04 另有上游表达式探针。各项都需要修复负责人补 Rust 回归后才能关闭。

| 编号／优先级 | 建议负责人 | 问题与影响 | 当前代码 | 固定上游 | 关闭条件 |
|---|---|---|---|---|---|
| L01 / P2 | HTTP 客户端 | replaceHeaders 直接替换整个 HeaderMap，丢弃未指定的旧请求头；固定上游只覆盖传入的同名键。Accept / X-Trace 后再替换 Authorization 可见差异。 | [vendor/openrust/framework/src/http/client/pending_request.rs:144](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/http/client/pending_request.rs:144) | [src/Illuminate/Http/Client/PendingRequest.php:482](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/PendingRequest.php#L482) | 实际回环 HTTP 捕获旧键保留、同名替换、多值头与无效头边界；同步核对尚缺的 withHeader / contentType / asMultipart / maxRedirects / sink 原名入口，复用现有客户端。 |
| L02 / P2 | Validation | 默认 integer 语义不同：上游 true、1.0 可通过，Rust 拒绝；U+2003 包围的字符串 Rust trim 后接受，上游拒绝；Rust 还接受超出 PHP 有符号整数范围的 JSON u64。integer:strict / boolean:strict 未提供。 | [vendor/openrust/framework/src/validation/rule.rs:176](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/validation/rule.rs:176) | [src/Illuminate/Validation/Concerns/ValidatesAttributes.php:1629](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php#L1629) | 固定 PHP 与 Rust 对照布尔、浮点、正负号、前导零、ASCII/Unicode 空白和边界数；严格模式分别验收，不把默认 integer 简化为 JSON 类型判断。 |
| L03 / P2 | Validation | min / max / size / between 的参数存为 usize；合法的 integer\|min:-5 在规则解析阶段失败，数值小数阈值亦无法表达。 | [vendor/openrust/framework/src/validation/rule.rs:30](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/validation/rule.rs:30) | [src/Illuminate/Validation/Concerns/ValidatesAttributes.php:1828](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php#L1828) | 负数、零、小数、超大值与各规则边界；数字大小与字符串/数组/文件长度分别验收，避免简单改浮点导致精度损失。 |
| L04 / P2 | Request / Session | StartSession 仅检查 purpose / sec-purpose / x-purpose，且大小写敏感地搜索子串；上游检查 X-Moz / Purpose / Sec-Purpose 并忽略大小写精确比较。预取 GET 会错误更新或跳过 previous URL。 | [vendor/openrust/framework/src/session/middleware/start_session.rs:80](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/session/middleware/start_session.rs:80) | [src/Illuminate/Http/Request.php:319](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Request.php#L319) | 真实请求覆盖 X-Moz=prefetch、Purpose=PREFETCH、Sec-Purpose=not-prefetch；断言 previous URL / route，复用统一 Request::prefetch。同步核对缺少的 ajax / pjax / userAgent / fingerprint / user 公开入口。 |
| L05 / P2 | 根配置 / Support | Session 默认 cookie 使用自制 snake + _session；固定骨架实际为 Str::slug(APP_NAME) + -session。例如 My App 得到 myapp_session，上游为 my-app-session。cache / Redis 前缀使用的自制 slug 也没有完整转写与 @ 字典语义。 | [config/session.rs:13](/Users/Home/Codes/Rust/openrust/config/session.rs:13) | [config/session.php:131](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/config/session.php#L131) | 默认应用名、空格、连续大写、重音、@ 等与固定 Str::slug 对照；保留显式 SESSION_COOKIE / CACHE_PREFIX / REDIS_PREFIX。复用既有字符串实现；说明 cookie 改名导致旧登录会话不可见及缓存命名空间迁移影响。 |
| L06 / P2 | Mail | MessageSending 取消时普通 send 返回 Ok(None)，sendNow 却转为 MailError::Cancelled；固定上游 sendNow 保留 SentMessage\|null。另缺 alwaysTo / alwaysFrom / alwaysReplyTo / alwaysReturnPath，Manager 仅读全局 mail.from。 | [vendor/openrust/framework/src/mail/mailer.rs:47](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/mail/mailer.rs:47) | [src/Illuminate/Mail/Mailer.php:365](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Mailer.php#L365) | 事件返回 false / Stop 时无发送、无 MessageSent、send / sendNow / PendingMail 一致返回空结果；补全原名全局地址入口及 mail.to / reply_to / return_path 适用配置，覆盖具名 mailer 与定制器顺序。 |

## 已确认公开 API／契约缺口

本表确认相应公开入口或绑定未交付，不据此断言现有替代路径运行失败。实现前先核对最新工作树、现有适配记录和固定上游，不要求复制 PHP 运行时或另建一套框架。

| 编号／优先级 | 建议负责人 | 已核对缺口 | 当前代码 | 固定上游 | 关闭条件 |
|---|---|---|---|---|---|
| L07 / P2 | Validation | 内置规则解析白名单不足：numeric、date、in、regex、required_if、exclude_if、accepted、url、uuid、array:key 等标准规则被 InvalidRule 拒绝。自定义规则能力不等同已提供原生规则。 | [vendor/openrust/framework/src/validation/rule.rs:102](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/validation/rule.rs:102) | [src/Illuminate/Validation/Concerns/ValidatesAttributes.php:2026](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php#L2026) | 按固定规则清单分批建立支持矩阵，优先补列出的常用规则；覆盖缺失/null/嵌套/通配符/条件规则、错误消息与翻译；同 Validation 负责人处理 L02/L03/L08。 |
| L08 / P2 | Validation / FormRequest | Validator 缺 after、条件回调 sometimes、stopOnFirstFailure、safe 等入口；FormRequest 未提供 withValidator / after / 全局停止扩展点，只有准备、授权、规则、通过等基础阶段。 | [vendor/openrust/framework/src/foundation/http/form_request.rs:20](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/foundation/http/form_request.rs:20) | [src/Illuminate/Validation/Validator.php:447](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Validator.php#L447) | 按上游阶段顺序执行钩子，区分 bail 和全局停止；覆盖授权失败、验证失败、Precognition 成功短路、已验证数据边界与请求隔离。不得破坏已有正确的 Precognition afterValidationHook。 |
| L09 / P2 | Filesystem | 公开 Filesystem / Factory / Cloud trait 没有由内置适配器/Manager 实现；Provider 仅绑定具体 FilesystemManager。Contracts 虽声明 copy / move，Adapter / Facade 未实现；append / prepend / json 等常用原名入口也缺失。 | [vendor/openrust/framework/src/filesystem/filesystem_service_provider.rs:15](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/filesystem/filesystem_service_provider.rs:15) | [src/Illuminate/Filesystem/FilesystemServiceProvider.php:51](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/FilesystemServiceProvider.php#L51) | 真实消费者按 Contracts 编译、从容器解析 Factory / 默认 Filesystem / 适用 Cloud；本地与 S3 复用现有适配器验收复制/移动/覆盖/失败及 Facade，明确不适用能力，不能只加空 trait 或重新造磁盘管理器。 |
| L10 / P2 | Foundation | ApplicationBuilder 缺 withBindings / withSingletons / withScopedSingletons / withSchedule / registered / booting / booted / prefersJsonResponses。底层容器或其他装配入口不能替代这些公开名称与调用时机。 | [vendor/openrust/framework/src/foundation/configuration/application_builder.rs:41](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/foundation/configuration/application_builder.rs:41) | [src/Illuminate/Foundation/Configuration/ApplicationBuilder.php:388](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Configuration/ApplicationBuilder.php#L388) | 用 bootstrap/app.rs 消费原名接口，验证 register/boot 钩子顺序、请求与 worker scoped 隔离、调度仅注册不提前执行、JSON 偏好与显式异常配置优先级。 |
| L11 / P3 | Auth Contracts | Authenticatable / UserProvider / Factory 仍使用 get_auth_identifier、retrieve_by_id、should_use 等公开拼写，未提供 getAuthIdentifier / retrieveById / shouldUse 等原名；Rust 能表达这些名称。 | [vendor/openrust/framework/src/contracts/auth/authenticatable.rs:10](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/contracts/auth/authenticatable.rs:10) | [src/Illuminate/Contracts/Auth/Authenticatable.php:12](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Contracts/Auth/Authenticatable.php#L12) | 建立逐方法映射并提供可用原名契约；消费端/生成模型/Fortify/Passport/Sanctum/Socialite 一起编译及认证回归。Guard / StatefulGuard 已有的原名别名不重复列问题，不做无差别全局重命名。 |
| L12 / P3 | Queue Contracts / Worker | Job / Queue 公开 get_job_id / get_connection_name / get_queue / is_deleted / has_failed 等及 Worker::run_next_job 未保留原名入口。 | [vendor/openrust/framework/src/contracts/queue/job.rs:8](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/contracts/queue/job.rs:8) | [src/Illuminate/Contracts/Queue/Job.php:19](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Contracts/Queue/Job.php#L19) | 按原名实现消费者与自定义 Job/Queue，核对失败/释放/删除状态语义及 runNextJob；同步 Horizon、队列 CLI 与扩展回归，保留具语言原因的辅助接口。 |
| L13 / P2 | Log | 缺 withContext / withoutContext / shareContext / sharedContext / flushSharedContext。已有独立 Context、每条日志 context 和 logWithContext，不提供上游已解析/未来频道共享上下文的完整机制。 | [vendor/openrust/framework/src/log/log_manager.rs:68](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/log/log_manager.rs:68) | [src/Illuminate/Log/LogManager.php:535](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/LogManager.php#L535) | 现有及后创建频道、stack、同名键覆盖、按键移除、清共享上下文与清频道上下文分别对照；验证请求/队列任务隔离，不增加跨请求污染的进程全局变量。 |
| L14 / P3 | Events | 缺 push / flush / forgetPushed。已有 defer / afterCommit 属于不同生命周期，不能当作同一公开 API。 | [vendor/openrust/framework/src/events/dispatcher.rs:73](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/events/dispatcher.rs:73) | [src/Illuminate/Events/Dispatcher.php:199](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Events/Dispatcher.php#L199) | 同一事件多次 push、flush 重复调用、forgetPushed、普通 listener 及 fake 的交互按固定源码验收；上游 flush 不自动消费清空，勿自行改变。 |
| L15 / P2 | Testing | TestResponse 仅有 assertStatus / assertOk / assertHeader / assertContent / assertExactJson 五类断言，缺常用 assertJsonPath / assertJsonStructure / assertJsonValidationErrors / assertRedirect / assertSessionHas / assertCookie / assertDownload。 | [vendor/openrust/framework/src/testing/response.rs:14](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/testing/response.rs:14) | [src/Illuminate/Testing/TestResponse.php:880](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Testing/TestResponse.php#L880) | 用实际 HTTP、Session、Cookie、下载响应写成功和故意失败用例，检查断言消息；若需扩展响应捕获则复用现有测试内核，不引入第二套 HTTP 测试入口。 |
| L16 / P2 | Scheduling | 固定 v13.40 基线明确要求 whenTaskRunsLongerThan，当前 Schedule 没有该 API / 阈值回调机制。队列已有 whenJobRunsLongerThan，不能替代调度任务。 | [vendor/openrust/framework/src/console/scheduling/schedule.rs:31](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/console/scheduling/schedule.rs:31) | [src/Illuminate/Console/Scheduling/Schedule.php:584](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/Schedule.php#L584) | 覆盖 ScheduledTaskFinished / ScheduledTaskFailed、runtime 缺失、阈值相等/大于、后台完成；时间阈值采用有依据的 Rust 类型适配并记录单位。 |
| L17 / P2 | Database Query | Query Builder 无 beforeQuery / afterQuery / toRawSql。当前 toSql / getBindings 及 QueryExecuted::toRawSql 存在，但不等同 Builder 回调或执行前调试入口。 | [vendor/openrust/framework/src/database/query/builder.rs:399](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/database/query/builder.rs:399) | [src/Illuminate/Database/Query/Builder.php:3461](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php#L3461) | 在 PostgreSQL18/MySQL8/SQLite 验证回调顺序、查询变更、结果映射、重复执行及 bindings 展开；不要自行实现危险字符串拼接，复用既有 grammar/QueryExecuted 插值能力。 |
| L18 / P3 | URL / Routing | UrlGenerator 缺 forceScheme / forceHttps / forceRootUrl / formatHostUsing / formatPathUsing。已有显式 secure 参数、withDefaults、forRequest，但原名及相应生成策略没有提供。 | [vendor/openrust/framework/src/routing/url_generator.rs:241](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/routing/url_generator.rs:241) | [src/Illuminate/Routing/UrlGenerator.php:738](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/UrlGenerator.php#L738) | 以请求作用域实现原名或记录具体所有权适配；覆盖反向代理、域名路由、路径格式化、签名 URL 与并发请求隔离，不能改成共享可变全局。 |
| L19 / P3 | View | Factory 缺 renderWhen / renderUnless / renderEach / getShared。当前 make / first / share / render 已有；不把 PHP 的内部渲染计数器一概要求照搬。 | [vendor/openrust/framework/src/view/factory.rs:150](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/view/factory.rs:150) | [src/Illuminate/View/Factory.php:196](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Factory.php#L196) | 条件真假、循环 key/index/iterator、空集合 raw\| 与空视图、共享数据覆盖/作用域按固定 Factory 对照；沿用既有 Blade 渲染链。 |
| L20 / P2 | Broadcasting | 仅提供 /broadcasting/auth 频道认证；缺 Broadcast::userRoutes、/broadcasting/user-auth、authenticateUser 与对应用户认证解析入口。频道授权不等同 Pusher 用户认证。 | [vendor/openrust/framework/src/broadcasting/broadcast_controller.rs:26](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/broadcasting/broadcast_controller.rs:26) | [src/Illuminate/Broadcasting/BroadcastManager.php:103](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Broadcasting/BroadcastManager.php#L103) | 具名广播连接、已登录/未登录、用户解析回调、自定义路由属性、Session reflash 与正确签名响应；明确实际驱动支持范围，不能把本地协议验收说成托管平台实测。 |
| L21 / P3 | Redis | RedisManager 缺 purge / extend，连接缓存无法按 Laravel 原名入口清除或扩展连接器。Rust 固定 redis crate 不应强制伪造 PHP phpredis / Predis 双驱动。 | [vendor/openrust/framework/src/redis/redis_manager.rs:91](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/redis/redis_manager.rs:91) | [src/Illuminate/Redis/RedisManager.php:248](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Redis/RedisManager.php#L248) | Redis8 隔离实例验收具名连接清除后重建、已持有 Arc 生命周期、连接事件和自定义连接器；复用现有连接/Manager，不为拼 API 另造驱动层。 |
| L22 / P3 | Translation | Translator trait 使用 get_locale / set_locale，具体实现公开 locale / withLocale；Lang Facade 已有 getLocale，但不能补足直接 Contracts 的命名差异，getFallback / setFallback 原名入口也缺失。 | [vendor/openrust/framework/src/contracts/translation/translator.rs:29](/Users/Home/Codes/Rust/openrust/vendor/openrust/framework/src/contracts/translation/translator.rs:29) | [src/Illuminate/Translation/Translator.php:563](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Translation/Translator.php#L563) | 覆盖直接 trait/具体类/Facade 原名调用、嵌套 withLocale 恢复、fallback、并发请求隔离与配置缓存。保留现有安全的 task-local 机制，写明实际可变性约束。 |

## 全仓范围核对表

“结构/证据核对”只表示本轮核对了目录、装配、公开导出或既有验收记录，未表示这些模块所有方法已重新验收。以上 22 项是本轮可以具体派工的发现。

| 范围 | 本轮核对深度与结果 |
|---|---|
| 根应用／bootstrap／config／database／routes／入口与 Cargo | 核对 Laravel 目录、包发现、Provider、编译型迁移装配、MSRV 与工具链约定；L05、L10；未执行安装/迁移或切换开发数据 |
| foundation／container／contracts／pipeline | 追踪统一装配及接口映射，L09–L12、L22；未逐个重跑容器生命周期矩阵 |
| http／routing／session／cookie | 请求链、客户端、路由生成及 Session 的重点实现对照；L01、L04、L18；未复跑全部代理/流式/会话安全回归 |
| validation | 规则解析、数值判断、Validator 与 FormRequest 阶段的重点源码对照；L02、L03、L07、L08 |
| database／pagination | Query/Schema/Eloquent 目录、宏/traits、既有单元证据和新增候选核对；L17；没有重新证明全部 SQL 方言、ORM 关系和分页方法等价 |
| filesystem／cache／redis | Contracts/Provider/Adapter、缓存 trait/typed 宏及连接 Manager 核对；L09、L21；未执行驱动矩阵 |
| auth／sanctum／hashing／encryption | 认证契约原名与已有 Guard 别名核对、其余结构/历史证据核对；L11；不是本轮密码学/权限安全审计通过声明 |
| mail／notifications／translation | Mailer/Manager、语言 trait/作用域重点对照，Notifications 结构及历史证据核对；L06、L22 |
| bus／queue／events／console | Worker 契约、事件延后机制和调度固定版本新增项重点核对；L12、L14、L16；未复跑多进程/多节点队列矩阵 |
| log／support／config／view／testing | 上下文、字符串、视图及测试 API 重点核对；L05、L13、L15、L19；集合/辅助工具尚非逐方法等价证明 |
| broadcasting／concurrency／process | 频道认证与用户认证区别、进程/并发结构及历史证据核对；L20；未复跑子进程安全和实时协议矩阵 |
| ai／grpc／image | 已纳入源码清单及职责盘点；无本轮完整 Laravel API 对照或运行时通过结论，独有能力按既有单独契约验收 |
| tests／Docker／脚本及 8 个本地依赖补丁 | 核对 Cargo patch 清单、来源/适配说明及既有验收入口；这些依赖不按 Laravel 类/方法重命名，本轮未重新审计第三方库全部内部算法，也未复跑脚本/部署矩阵 |
| 18 个独立包 | 下表逐包复核当前导出/来源/Provider及修复状态，结合已有扩展审计；没有将历史结果写成本轮重新执行 |

## 扩展逐包状态与已声明边界

[既有扩展审计](extension-design-review.md)顶部 R1–R6 已有修复状态；[扩展最终验收](extension-final.md)及包内记录是历史行为证据。本轮不重复创建六个已关闭问题，也不据此认定所有上游功能齐备。

| 包 | 当前审计处理／必须保留的边界 |
|---|---|
| activitylog | Support 导出、来源材料及参考输入修复已存在；不重复列旧 R3/R4/R5 |
| cashier | 原名 Stripe 入口和供应商配置修复已存在；以 [API 映射](cashier-api-mapping.md)判定 JSON/模型适配及未提供范围，不能宣称两个官方 SDK 全覆盖 |
| fortify | 复用 Auth/Session/验证器和现有 Passkeys；L07/L08/L11 修复时须消费回归 |
| horizon | prefix/path/domain/middleware 与自动路由旧问题已修复；原生面板及平台边界仍按已有记录，不重复开 R1 |
| oidc | 与 Socialite/Passport 职责不同，已有成熟库选型比较；真实 OP/租户配置仍属于外部目标验收 |
| package-discovery | Cargo 编译期发现是有依据的语言适配；不要求 PHP Composer 动态加载 |
| pail | 固定来源、命令和日志复用已有证据；平台日志来源按既有适配处理 |
| passkeys | 复用成熟 WebAuthn 库和官方客户端；虚拟认证器不等于真实设备全部验收 |
| passport | OAuth 服务端/迁移已有独立验收记录；L11 改认证契约时必须一起回归 |
| pay | 微信/支付宝已声明直连商户范围；不等同全量 Yansongda SDK 或真实资金验收 |
| pennant | 固定来源和参考输入修复已存在；作用域与批量能力沿用现有服务 |
| pulse | 原生面板与官方 Livewire 差异已声明；不因 PHP 不适用直接推断任何前端都无法复用 |
| reverb | 最新连接证书验证及 HTTPS 发布记录已存在；L20 先核对与底层广播协议的适用关系 |
| scout | 固定 CollectionEngine 真实入口及篡改拒绝已修复；外部搜索引擎全集不在已验范围 |
| settings | 精简 typed group / Provider 有成熟参考；保留已声明版本行与并发修改机制，不随意推倒重写 |
| socialite | 微信网站/公众号/小程序按用户要求归 wechat，QQ 独立；这是明确用户决定，不能以原生 Socialite 命名不同当作违规 |
| telescope | 旧许可证/Watcher/UI 取舍已有证据；当前 Watcher 和 custom_storage 存在协作者 WIP，收尾未验收，不用旧快照覆盖 |
| translation-database | Loader 复用现有 Translator；不等同 Translation Manager UI 或模型字段 Translatable；L22 须消费回归 |

此外，Queue failed provider 当前只接 null / database-uuids，官方其他 file / dynamodb / database 数字 ID provider 不在当前交付范围。此类限制，以及 Scout 外部引擎、完整官方面板、真实第三方账号/设备、其他平台和最终发布门禁，应继续在支持矩阵注明；**不混入“已支持路径的实现错误”，也不静默算成已对齐**。若用户要求完整上游全集，需要独立逐项排期。

## Agent 分配与统一验收

| 工作包 | 问题 | 文件交叉与接续条件 |
|---|---|---|
| HTTP 客户端 | L01 | 独立客户端目录；验证改请求头没有改变既有重试/签名/附件行为 |
| Validation | L02/L03/L07/L08 | 同一负责人统筹规则与阶段，避免并行修改 rule.rs；认证/邮件消费者随后回归 |
| HTTP 与根配置 | L04/L05 | Request/Session/配置协同；字符串原生实现先核对，缓存命名空间变动须说明 |
| Mail | L06 | 同时更新 contract/Facade/PendingMail，避免仅修一个 sendNow 签名 |
| Filesystem | L09 | 契约绑定与操作一起完成，避免把声明接口误当实现 |
| Foundation | L10 | 应用启动/Provider 与作用域顺序验收，禁止另造装配系统 |
| Auth | L11 | 模型、生成器、认证扩展共同回归，保留其他 Agent 的改动 |
| Queue/Schedule | L12/L16 | Worker/Horizon 与调度阈值各自验收，不能互相替代 |
| Log / Events | L13/L14 | 可以分别负责；涉及共用生命周期时先核对在途文件 |
| Testing | L15 | 补常用断言及失败用例，供其他包复用 |
| Database | L17 | 使用隔离 PostgreSQL18/MySQL8/SQLite，不碰开发库 |
| Routing / View | L18/L19 | 可分别负责；请求作用域和渲染上下文保持既有隔离 |
| Broadcasting | L20 | 认证/路由/协议链验收，区别频道与用户认证 |
| Redis / Translation | L21/L22 | 可分别负责；Redis8 与语言作用域隔离回归 |

每个负责人先重新核对 HEAD/WIP 和固定上游，在本报告对应行建立原名/签名/行为映射，再完成实现、消费示例、正反回归和三语正文；确有语言差异时写明具体约束与等价验证。按当前规范使用 Rust 1.94.0、隔离源码与项目内复用 target，运行相关 fmt/Clippy/定向测试；共享数据库改动覆盖三库，共享缓存改动覆盖 Redis/database/file。禁止仅靠“现有测试通过”或修改状态文案关闭问题；提交/推送按用户当次授权。

## 本轮收尾核验

PHP 局部参考探针退出码 0；本报告所有问题行的 Rust/固定上游位置由脚本定位核实。报告表格列数和 32 个本地链接存在性核对通过；`git diff --check` 通过。源码清单仅 Telescope `src/watchers/mod.rs` 发生协作者在途漂移，其余已登记生产 Rust 输入未变；HEAD 仍为上述审计基线。此处输入未变只表示快照一致，不等同重新执行测试。没有创建新的构建缓存；临时参考源码和探针保留供修复负责人复现，未触发磁盘停线清理。


## 2026-11-22 清理后复验补充

后续独立复核发现 L02 的浮点 integer 尚需经过 PHP precision=14 标量转换，L07 的 regex 尚需拒绝 bool/null；本轮已分别复用既有 php_float_string 和限定 regex 类型修复，并通过公开 Factory／Validator 正反回归 9 项及实际消费者复验。详细结果与原始失败映射见 [清理后全面验收](08-04/clean-full-acceptance-2026-11-22.md)，原独立复核报告保留。该补充修正此前列明行为的遗漏，不扩展为全部 PHP 规则等价证明。

## Fortify Session 夹具隔离

2026-11-26 在隔离 MySQL 8.4.13 上复现。旧夹具在同一 schema 删建固定名称的表，进程内的串行锁无法隔离其他测试进程。两个同时存活的 HTTP 应用共享会话表时，第二个应用删除该表后，第一个应用查询得到 MySQL `1146 / 42S02`，随后 `GET /state` 返回 HTTP 500 `session storage is unavailable`。历史失败日志没有记录该请求的 SQL 错误链，本次证据确认的是可复现的夹具故障。

现用 `TestApp::mysql` 为每个应用分配独占 schema，关闭监听后等待夹具清理。Rust 1.94.0 下，`accounts_http`、`email_verification_http`、`password_reset_http` 在同一隔离实例并行执行，15 项通过。新回归额外跨进程并行执行 12 次，全部通过，结束后无残留的测试 schema。原始记录保存在项目内 `target/tmp/fortify-session-isolation/before-sql-verified.log` 和 `after.log`。
