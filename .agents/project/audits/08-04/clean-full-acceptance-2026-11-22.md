# 清理后全面验收（2026-11-22）

## 范围与当前状态

用户要求清除本项目编译产物，再完整测试与运行。基线为 `fb5240e1ae08855857d9b9988ac5453d8623e608`，本记录随本轮修复按用户授权本地提交，不包含推送。独立复核报告 `../laravel-fixes-verification-2026-11-22.md` 为其他协作者的文件，保持原文。

本轮可执行的本地测试与运行验收已完成，发现的问题已修复并完成对应复验；首次失败日志保留，不将首次全量退出码改成成功。所有源码快照、编译目录、临时文件、原始失败与复验日志在 `target/tmp/clean-full-acceptance-20261122/`，短临时目录为 `target/tmp/cf/`。使用 Rust 1.94.0，保留增量编译、正常 Cargo 并发及依赖下载；dev/test 去除调试符号，不是 release 构建。

## 清理

先清理确认归属本项目且闲置的编译产物，保留源码、日志、下载、交付物、数据库和卷；后续四次触及 32 GB 停线，安全停构建、持 Cargo 排他锁并保留锁 inode 后清理。证据分别为 `cleanup-result.json`、`disk-stop-cleanup.json`、`disk-stop-second-cleanup.json`。第二次停线清理净释放 33,799,680,000 bytes，第四次清理前另安全回收已完成的一次性 Passport 构建目录（净释放 5,205,692,416 bytes）；随后按停线规则清理全部项目编译产物，净释放 33,613,029,376 bytes；不得将多次净释放量相加解释为当前剩余空间。

## 问题与处理

| 项目 | 根因 | 实际处理 | 复验状态 |
| --- | --- | --- | --- |
| 日期格式 | Jiff 0.2.46 的 strftime Display 默认宽松模式，不报告非法格式；原负例 `%Q` 本身为合法时区指令 | 使用现有严格 `jiff::fmt::strtime::format`，覆盖 date/time、unique 与信用卡到期日期；非法负例改为 `%` / `%J`，三语说明同步 | seeding 70 项通过（含 MySQL／SQLite 消费者与新非法格式回归）；`retry-openrust-seeding.log` |
| integer 浮点 | 独立复核 R1：未应用 PHP 默认 precision=14 的标量转换 | 复用已有 php_float_string，再通过同一整数过滤入口；补公开 Validator 的正反例 | 公开 Factory/Validator 回归通过；`retry-openrust-framework-laravel_alignment_validation.log` |
| regex 输入 | 独立复核 R2：通用 text 转换误接受 bool/null | regex 入口限定字符串或数值；保留 nullable 的跳过契约，补公开 Validator 正反例 | 同上，另 validation_http/web、密码、email、路径、输入消费者通过 |
| CSRF 视图夹具 | Blade 无参数指令与后续指令紧邻，或 word 紧邻关闭指令，不满足固定 Laravel 的 `\B@` 匹配边界 | 夹具显式分行，并为 @enderror 后的读取提供外层 message，以非空哨兵验证作用域恢复；保持生产编译器；固定上游提取正则的 PHP 实跑在 research/directive-boundaries.log | 3 项 HTTP/CSRF/Session 通过；`final-csrf.log` |
| 发布迁移夹具 | all-features 同时发布 Sanctum 与测试包迁移，旧断言只允许一份且读取未排序首项 | 精确匹配 fixture 和 Sanctum，验证重复发布幂等与准确数量 | `retry-openrust-framework-test_app.log` 通过 |
| HEAD 覆盖夹具 | GET 替换会同步替换 HEAD，旧断言预期旧 handler | 按固定 Laravel 契约检查 204 与旧 handler 未执行 | `retry-openrust-http.log` 通过 |
| 应用迁移夹具 | 扩展迁移改变了盲目计数和默认回滚顺序 | 应用迁移夹具显式指定 database/migrations，并检查每个应用迁移恰好登记一次；扩展由各包与完整生成应用验收 | 三库迁移等 12 项通过；`retry-openrust-migrations.log` |
| 队列链故障注入 | SQLite 触发器搜索整个 JSON，误匹配头任务 chained 内的数据 | 使用 json_extract 精确匹配当前 payload.data；保持生产 Queue 行为 | SQLite／MySQL／PostgreSQL 全部通过；`retry-openrust-job_batches.log` |
| UTC 夹具 | UTC 常量与 TZif("UTC") 内部表示不同，实际 UTC 语义相同 | 夹具使用明确 UTC 常量，保留其他时区隔离断言 | seeding 70 项通过；未放宽其他非 UTC 时区断言 |
| 安装应用夹具 | 递归按 storage/tmp 名称过滤，丢掉包内合法源码；另强制关闭增量及限制并发 | 仅应用根过滤运行目录，子目录完整复制；继承正常增量与并发，生成应用仍使用独占 target | Passport 默认 CLI 生成应用实跑通过；`retry-openrust-passport_install.log`；完整 PG／MySQL init 另列结果 |
| 验收编排 | 专属 URL 共用数据库导致 schema/表污染；Redis 双服务器测试用同一实例；SSO 未设 Node | 按测试专属 URL 创建独立库，另启专属 Redis lock 实例，补 Node 与浏览器环境 | 根与扩展复验通过；初次错误入口与失败日志原样保留，具体服务结果见下表 |

## 参考与边界

固定 Laravel Framework v13.40.0，提交 `1449bf241017087fbd38c118ecaabef3f6797fdc`。校验差异复现使用独立复核的真实 PHP trait 探针；修复复用框架已有 PHP 标量转换，没有平行校验器。日期格式采用锁定 Jiff 0.2.46 的官方严格 API，源码 `fmt/strtime/mod.rs` 明确区分 lenient Display 与返回 Result 的 format，不自行解析格式。

依赖公告检查为 0 errors / 0 warnings；706 项依赖有许可证声明，不替代交付物的发行许可核对。真实供应商、正式部署、Windows、其余发布矩阵以及已记录未实现的 Reverb 完整客户端协议不因本地测试通过自动关闭。

参考入口：[固定 Laravel ValidatesAttributes](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php)、[Jiff 0.2.46 strict format](https://docs.rs/jiff/0.2.46/jiff/fmt/strtime/fn.format.html)、[Jiff Display 错误行为](https://docs.rs/jiff/0.2.46/jiff/fmt/strtime/struct.Display.html)。在线官方资料与锁定本地源码一起核对；不以浮动 latest 页面替代锁定实现。


## 新增夹具隔离修复

- `tests/cache.rs`：两个锁服务器的夹具使用随机 CACHE_PREFIX，验证旧 owner 无权释放新锁，并主动释放有效 owner 的锁。初次失败遗留无 TTL 的固定名称锁，不能把再次 get 失败当作新的生产缺陷。完整缓存 37 项通过，另单独重复锁场景通过；跨进程 file 锁 helper 的 ignored 标记由父测试实际调用，未作为独立跳过计入服务通过。
- `tests/schema_comments.rs`：MySQL 的 `getTables(None)` 按固定 Laravel 默认列举所有可见 schema，不应改生产目录查询范围。夹具显式传当前数据库名，仍严格要求 17 张表及逐列本地化注释；三库共 3 项通过。固定源码为 Schema/Builder.php 与 Grammars/MySqlGrammar.php 的 compileSchemaWhereClause。
- `vendor/openrust-telescope/tests/watchers.rs`：每个 TestApp 使用随机短表前缀及标准 `prefix_indexes=true`，保留请求数量 4 的准确断言，避免两个用例在共享隔离库累加成 7。SQLite、PG、MySQL 各 2 项通过，最终 SQLite 配置再验日志为 `final-sqlite-prefix-indexes-watchers.log`。未改生产 watcher。
- 原始验收入口中的无根 lib、重复 --no-fail-fast、错误 SMTP 包名、未配置 Node、未配置 Horizon wrapper 环境和清理后 deps 目录不存在均属于本批临时编排错误，已使用正确入口实际补验，不算产品缺陷，也不以旧退出码改写为成功。


## 最终验收汇总

首轮按 67 组覆盖 20 个 workspace 包、19 个库目标及 303 个集成目标；根应用没有 lib，误加入的 lib 入口为编排错误，不计为一个通过的库。`full-test-result.json` 的 exit=1 原样保留。后续按失败目标与实际消费者定向补验，未重新重复整轮已通过目标；最终源码严格 Clippy 覆盖 workspace、all-targets、all-features。

| 范围 | 最终结果 | 本批证据 |
| --- | --- | --- |
| 框架、根应用与扩展 | 首轮失败目标已按下表复验；条件缺失的供应商分支不计通过 | full-batches-results.json、acceptance-results.json、followup-results.json、supplemental-results.json |
| 三数据库 | PostgreSQL 18.7、MySQL 8.4.13、SQLite 的 Query、迁移、队列、数据库缓存和翻译消费者通过 | alignment-three-database.log、retry-openrust-migrations.log、retry-openrust-job_batches.log、translation-*.log |
| 缓存与 Redis | Redis／database／file，双 Redis 锁实例、真实 Cluster 和 TLS Cluster 通过 | final-cache-isolated-repeat.log、final-cache-alias-repeat.log、redis-cluster.log、redis-cluster-tls.log |
| 扩展存储与队列 | Activitylog／Pennant／Scout／Pulse／Telescope 的适用 SQL 与缓存组合通过；Horizon 忽略项经原生 wrapper 实跑 | pgsql-*.log、mysql-*.log、retry-redis-extension-ignored.log、final-*-telescope-watchers.log |
| 浏览器与进程 | Passport、官方 Precognition、OIDC 双 RP SSO；Telescope／Horizon／Pulse 桌面手机界面及真实进程生命周期通过 | passport-browser.log、precognition-browser.log、retry-openrust-oidc-sso.log、runtime-browser.log |
| 邮件 | STARTTLS／SMTPS、TLS1.2／1.3、错误密码／CA／hostname、明文及收件策略拒绝共 12 场景通过 | final-smtp-tls-auth.log、smtp-evidence/results.json |
| 完整生成应用 | PG／MySQL 初始化、重复安装、定制保留、迁移、Fortify HTTP／Session／CSRF／SMTP／密码／2FA／恢复、严格生成应用检查及 feature-off 通过 | init-pgsql.log、init-mysql.log、application-results.json |
| 默认应用与独立运行 | 默认构建及配置通过；按文档启用 standalone 后，空 PATH、异地 cwd、两次搬迁、HTTP／静态资源、Artisan、SIGTERM 和端口释放通过 | default-package-config.log、standalone-final-results.json、standalone-runtime-final.log |
| 静态与文档检查 | Rust 1.94.0 fmt、workspace/all-targets/all-features 严格 Clippy、workspace doctests、三语正文／构建／浏览器／边界检查通过 | final-source-fmt.log、final-complete-clippy.log、final-doctests.log、docs-results.json |
| 依赖 | 公告 0 errors／0 warnings，706 项有许可证声明；不代替发行法律审核 | advisories.jsonl、license-declarations.json |

### 原始失败与复验对应

| 失败目标／入口 | 最终通过证据或核对结论 |
| --- | --- |
| csrf | final-csrf.log，3 passed |
| logging | real-openrust-framework-7.log，14 passed |
| precognition_http | real-openrust-framework-9.log，8 passed；浏览器 helper 另由 precognition-browser.log 实跑，不把 ignored 自身当通过 |
| test_app | retry-openrust-framework-test_app.log |
| Passport authorization_code | real-openrust-passport.log，53 passed；浏览器入口另见 passport-browser.log，未配置的分支不计通过 |
| 根 --lib | 根 Cargo 包无 lib，此错误入口不适用；应用 bin／集成测试及默认构建已验 |
| HTTP／job_batches／migrations／passport_install／seeding | retry-openrust-{http,job_batches,migrations,passport_install,seeding}.log |
| cache／schema_comments | final-cache-isolated-repeat.log、final-cache-alias-repeat.log、final-schema-comments.log |
| 共库污染的 schema／Pulse file／Telescope database | retry-openrust-framework-database_schema.log、retry-{pgsql,mysql}-pulse-file.log、retry-{pgsql,mysql}-telescope-database.log |
| Telescope watchers | final-pgsql-telescope-watchers.log、final-mysql-telescope-watchers.log、final-sqlite-prefix-indexes-watchers.log |
| 清理后 deps 缺目录造成的编译失败 | followup-results.json 中对应 Validation／Activitylog／Cashier 目标实际补跑为 0，最终全 workspace Clippy 为 0 |
| OIDC 缺 Node／Horizon 缺 wrapper／SMTP 包名错误 | retry-openrust-oidc-sso.log、retry-redis-extension-ignored.log、final-smtp-helper.log 与 final-smtp-tls-auth.log |
| 首次 Clippy 引用根包未声明的 fastrand | 改用已有 Str::random；final-complete-clippy.log 退出 0 |
| 首次搬迁用了默认开发 binary | 按文档 --features standalone 重新构建，standalone-final-results.json 两项均为 0；原 application-results.json 失败保留 |

## 输入与资源收尾

核对工作区与最终快照共 3,246 个受跟踪源码／配置／文档输入（排除内部 .agents 记录）：3,245 项逐字节一致，唯一 artisan.bat 为工作区 LF 与快照 CRLF，符合 .gitattributes 的 eol=crlf，归一化后内容一致；没有修改 artisan.bat。证据为 final-input-fingerprints.json、final-input-line-endings.json。Windows 原生执行仍未在本轮验收。

仅停止并回收本批登记且标签核对一致的 5 个隔离容器；没有删除数据卷，卷集合操作前后均为空。原开发 PG／MySQL／Redis 保持运行，原 app／nginx 保持 exited，本轮未改其状态。收尾无本批构建进程，可用约 51.82 GB／48.26 GiB，高于 32 GB 停线；以实时 df 为准。证据 resource-closure.json。源码、失败与成功日志、独立运行验收交付和可复用 Cargo 缓存保留；三语站点曾构建通过，dist 已在磁盘停线清理时回收，不声称当前仍保留站点产物。

本轮生产改动仅 Faker 和 Validation 三份文件；其余为测试夹具、六份三语文档及内部验收记录。按用户授权仅本地提交本轮已验收修复及相关记录；不推送，其他协作者的独立复核报告和欢迎页国际化在途改动保持原样。真实供应商、正式部署、Windows、其他发布平台／优化 release 构建、尚未实现的 Reverb 完整 pusher:signin 协议沿用 todo.md 的剩余范围，不因本轮通过关闭。


## 用户要求的后续构建清理与提交

2026-11-22 用户再次要求清理构建产物，核对无活动构建后取得 8 个 Cargo profile 的排他锁，保留锁文件 inode，仅清理编译内容；净释放 18,016,694,272 bytes，清理后可用 70,150,967,296 bytes（约 70.15 GB／65.33 GiB）。源码、验收日志、依赖下载、独立交付、数据库和卷保留，证据为本批 user-cleanup-result.json。前文可复用缓存及空间为验收收尾时的历史快照，不代表清理后缓存仍存在。

本地提交前复核 20 份生产代码／测试／三语文档与本批验收快照逐字节一致，既有结果适用于本次提交范围；未为提交重复编译。仅更新记录中的提交授权与后续清理状态，不包含欢迎页国际化或另一份未跟踪独立复核报告。
