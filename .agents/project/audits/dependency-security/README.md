# SQLite 与 RSA 安全依赖

2026-11-20，以 `88436e19312cf9f38b8246ac1d67b7b49ca08bc8` 隔离快照开发和验收。没有合入并行的队列、工具链文件或数据库时间戳改动。macOS ARM64、Rust1.94.0、jobs=3、增量关闭。

## 实现与契约

- Passport rsa0.9.10 与 WebAuthn 间接 rsa0.10.0-rc.19 统一为稳定0.10.0，移除 RUSTSEC-2023-0071。适配稳定类型名、PKCS1借用类型、BoxedUint公开模数／指数编码，去除整数前导零后仍交 AWS-LC 在线签名与验签。随机密钥生成使用独立rand0.10，其余既有随机接口保持rand0.8。
- 固定 Passport CLI `--length` 契约不改变，保留原已验收1024位和显式自定义长度生成；RSA新版本生成默认最小长度有变化，fallback使用其hazmat的`new_unchecked`保留上游长度行为，仅用于离线密钥生成，不用于在线私钥运算。没有因此放宽在线AWS-LC策略。固定旧版 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 仅参考既有密钥结构，未移植新协议实现。
- crypto-glue0.2.2仍请求RSA移除的sha2 feature且自身deny(warnings)，故保存原包与MPL2许可证，在根patch中只改RSA声明为0.10.0＋pkcs1v15，并将两个弃用导入映射为同一类型的新名。其他源码不变；`README.openrust.md`记录移除补丁条件。
- SQLx0.9.0的SQLite依赖上界排除修复版本；SQLx0.9.1虽然放宽上界，也改动连接/TLS层。为限定范围，vendor/sqlx-sqlite保留0.9.0全部Rust源码及MIT／Apache2许可证，只将libsqlite3-sys依赖精确改为0.39.1，实际捆绑SQLite3.54.1，修复RUSTSEC-2026-0459。独立包夹具同步patch入口。

## 实际结果

- `native.log`：SQLite实际版本下限3.54.1、WAL双连接绑定二进制读写与integrity_check1函数，原SQLite查询／JSON／Schema／高级写入4函数，Passport lifecycle10、Passkeys configuration/contracts/resources/aaguids共10；合计25函数全过，无缺服务跳过计成功。
- `postgres.log`：专属tmpfs PostgreSQL180006，JSON谓词／更新的两种prepare模式、迁移失败事务／数据／账本回滚3函数通过；显式映射REQUIRE变量。
- `mysql.log`：现有Passport MySQL专项真实8.4.13，仅精确筛选授权码／PKCE／机密客户端／单次兑换和资源守卫签名／时间／撤销／错误2函数。保留MySQL专项名称与历史范围，不当作PostgreSQL认证整合。
- `ceremony.log`：真实Chrome CTAP2分别强制ES256(-7)与RS256(-257)跑完整原生注册／认证／一次性state／challenge／Origin／RP hash／UV／userHandle／签名／counter／AAGUID回归；每轮确认服务端实际提供该算法，两个算法全部通过。
- Rust1.94 `clippy --workspace --all-targets --all-features --locked -- -D warnings`、`fmt --all --check`、根`build --all-features`及`build --no-default-features`通过；真实原始`./artisan inspire`、编译后`artisan list --format=json`、根HTTP五路径和SIGINT／端口回收通过。最初smoke误用不存在的`list --json`失败，改正契约后最终全过。
- 独立包夹具以专属源码生成离线锁文件，再`build --offline --locked -p package-probe-app`通过。初次无锁且错误包名的尝试失败并保留日志，不计成功；该夹具构建不等于全包行为验收。
- 三语版本支持正文、VitePress构建、通用12浏览器组合通过。

## 审计剩余项

`cargo deny --format json --locked --workspace --all-features check advisories`仍退出1，但只剩4条informational=unmaintained：chrono、chrono-tz、paste、pin-utils，没有vulnerability类型；未新增ignore或降级策略。chrono／chrono-tz涉及公开Date与SQLx日期类型，paste／pin-utils为间接依赖；替换需独立契约回归，不能为清空工具输出盲改公开日期API。正式发布仍需维护依赖迁移与用户批准。

证据目录`/private/tmp/openrust-dependency-security-20261120/`保留源码、原包、更新／失败尝试／定向测试／Clippy／格式／根入口／浏览器／审计日志。`source-sha256.json`覆盖验收及交付输入，`references.json`记录上游原包与官方公告SHA。容器已由批次夹具按标签清理，未删除任何数据卷。仅核范围本地提交，不推送；最终主线队列与PostgreSQL完整认证整合另行验收。

差异检查保留上游原包中的4个文件原始空白（crypto-glue/Cargo.toml.orig、MPL许可证、sqlx-sqlite/deserialize.rs），不为Git空白提示改写许可证或原源码；其余本批文件通过`git diff --cached --check`。这些原文由上游SHA与源码差异清单约束。

## 核心整合依赖复核

2026-11-21，在 `b45427830` 干净归档中，以 Rust 1.94.0 执行 `cargo metadata --locked --offline --all-features --format-version 1`、`cargo deny --locked --workspace --all-features list --format json --layout crate` 及 `cargo deny --format json --locked --workspace --all-features check advisories`。前两项成功；公告检查退出 1，仍只有以下四条维护状态错误，没有 vulnerability 类型诊断，未加 ignore 或放宽策略：

- chrono 0.4.45：RUSTSEC-2026-0543。
- chrono-tz 0.10.4：RUSTSEC-2026-0555。
- paste 1.0.15：RUSTSEC-2024-0436。
- pin-utils 0.1.1：RUSTSEC-2026-0441。

完整元数据共 683 包（668 registry、14 local、1 git），均声明许可证。许可证清单已导出，属于依赖盘点，不代表最终许可证批准。唯一 Git 依赖 oxideav-jpegxl 0.0.14 固定在 `1c2f5a86a052752b0e8181f842a9d4e3460ec0e5`；没有浮动 Git 分支。MPL-only 包为 crypto-glue、cssparser、cssparser-macros、dtoa-short、selectors、webauthn-attestation-ca、webauthn-rs-core、webauthn-rs-proto；既有第三方源码保留要求继续适用。本次不改变依赖或公开 Date API，四条维护问题和最终发布审计继续保留。

证据在 `/private/tmp/openrust-core-review-20261121/`：`metadata.json`、`licenses.json`、`licenses.log`、`advisories-current.jsonl`。初次使用不受支持的 `--disable-fetch` 失败，未计成功；上述记录使用修正后的实际命令。该结论仅覆盖此固定源码与锁文件，不替代后续扩展提交后的整合审计。

数据库翻译交付后的 `599bb7d30` 归档再次执行锁定全feature元数据与公告检查：683包的版本、来源与许可证声明集合与上述盘点完全相同，仍只有同样4条unmaintained错误，没有vulnerability诊断。日志 `metadata-delivery.json`、`advisories-delivery.jsonl`。本次仅文档与设计记录，不变更数据库或缓存实现；原三库／缓存各驱动验收不由本次单一PG复核替代。


## paste 与 pin-utils 的上游替代回移

2026-11-21，在 `58c622e6988a003572fedf91097b32674c85d186` 的独占快照中接入五个真实父包的最小上游补丁，保留原包版本、源码、许可证与来源校验值。根 workspace 排除这些第三方包，通过 crates.io patch 接线；独立 package 夹具同步相对路径。没有新增 ignore、修改公告策略或将原包改名避开检测。

- rav1e 0.8.1 参考官方提交 [`c247d53ae43dd1312dbd90117c45e4c0ee6b06ce`](https://github.com/xiph/rav1e/commit/c247d53ae43dd1312dbd90117c45e4c0ee6b06ce) 替换 paste 为已有 pastey 0.1.1；imgalifier-avif 0.1.0 同步成熟的相同宏接口。没有更改编解码算法或公开接口。
- aws-smithy-http 0.63.6、aws-smithy-runtime 1.11.3、aws-smithy-types 1.5.0 回移官方 [`66b12d3e2165f98ff62146b46fb248c5845e8ea4`](https://github.com/smithy-lang/smithy-rs/commit/66b12d3e2165f98ff62146b46fb248c5845e8ea4)：删除未使用的 pin-utils 依赖，以标准库 `std::pin::pin!` 保留已有 pin 和生命周期行为。没有改 SDK 请求、重试、凭据或传输协议。
- 已发布可直接升级的父包尚不能同时移除这些依赖并维持 Rust 1.94.0，采用最小回移。各包 README.openrust.md 保留发布包 SHA、变更来源及删除条件：兼容最低 Rust 的正式上游发布包含替代后升级并移除本地 patch。未为升级 AWS 改动最低 Rust 版本。

Rust 1.94.0、jobs=2、dev debug=0、关闭增量，最终验收结果：

- 图片专项14函数全部通过，包含真实 AVIF／HEIC／JPEG XL 与动画、元数据、损坏输入和限制回归。独立五父包 probe 的 AVIF 往返及 ByteStream 多块／空流／错误传播两函数也通过。
- 真实 RustFS 1.0.5（摘要 `sha256:b81d16a90ee25bd9511280127ca1471788c6e874cf0f0da759645f46b2dc1d74`）对象读写、失败上传清理、无客户端凭据的预签名 GET／PUT 两函数通过；精确选择真实服务分支，未把 ACL 条件跳过计为覆盖。此结果不代表真实 AWS／STS 验收。
- `clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`、`fmt --all --check` 和独立 package-probe-app 的 `build --locked --offline` 通过。第三方原包仍有 rav1e 的两处 lifetime 和 imgalifier-avif 的五处 cfg 编译警告，未改写无关上游代码；工作区严格检查通过。夹具最初重复键已修正，修正后锁定构建通过，失败日志保留。
- 完整依赖图681包：661 registry、19 local、1固定Git，全部声明许可证；paste 和 pin-utils 已移除。`cargo deny` 许可证／来源清单成功；公告检查退出1，只剩 chrono／chrono-tz 两条 unmaintained，无 vulnerability 诊断。时间依赖替换仍在独立专项实施，不将维护风险写成已消除。

证据与源码保留 `/private/tmp/openrust-core-matrix-20261121/`；父包原始文件比对与 probe 在 `/private/tmp/openrust-paste-pin-20261121/`。本快照不包含同期 settings 和后续时间迁移，不冒称最终发布整合。依赖替换无产品调用变化，未重建文档站。只核范围提交到本地，不推送。

已回收闲置独占 target 6,220,716 KiB、本批 RustFS 容器和镜像，数据卷清单一致，所有卷保留。清理期间可用空间净增 5,766,256 KiB（5.905 GB），清理后 51,639,824 KiB（52.879 GB／49.248 GiB）；并行构建可能影响实测净值。

暂存差异检查仅提示 rav1e/PATENTS 第108行的上游文件末尾空行；为保持专利授权原文不改写，该提示保留，其余本批差异无空白错误。

## 原生时间依赖替换

2026-11-21，时间专项采用 Jiff 0.2.46 原生 `Timestamp`、`Zoned`、`TimeZone`、civil 日期与 `SignedDuration`，删除 chrono／chrono-tz 调用和直接依赖。调度沿用 croner 4.0.3 的官方 `jiff` feature，使调度与框架共享同一原生类型版本。Jiff 1.x 已发布，但本次不为追随新主版本引入第二套调度时间类型。数据库边界使用 SQLx／Tiberius 官方 `time` feature；time 的 `large-dates` 保留 PostgreSQL 的扩展日期范围。没有自制 chrono 兼容层、改名隐藏公告或新增 ignore。

固定参考及采用理由：

- [Jiff 官方项目](https://github.com/BurntSushi/jiff)、[0.2.46 TimeZone::tzif](https://docs.rs/jiff/0.2.46/jiff/tz/struct.TimeZone.html#method.tzif)：使用成熟解析与歧义时间处理，避免自行实现时区算法。框架配置统一通过 `Date::timezone(name)`，将官方 IANA 2025b vanguard 数据交给 Jiff 原生 `TimeZone::tzif`，保留原 chrono-tz 0.10.4 与 PHP Calendar 的负夏令时标记。官方 jiff-tzdb 0.1.4 虽同版 2025b，但其 rearguard 数据刻意改变 Dublin 等地区 DST 标记，不适合直接替换；采用固定 SHA 的 IANA data/code 和上游 zic 生成 598 名／341 去重 TZif，203282 字节，无 build.rs。保留默认 backward 别名、不额外启用 backzone，避免改变原历史别名行为。来源、命令、许可及复现见 [固定时区数据](../../../../vendor/openrust-framework/resources/calendar/SOURCES.md)。`DateTimezone` 仍是原生 `TimeZone` 别名，未增加时区抽象或算法。升级 IANA 数据须同步 Calendar POSIX tail／首记录与 Faker 国家地区列表，整体重跑历史偏移、DST 和 PHP 格式向量。
- [croner 4.0.3](https://docs.rs/croner/4.0.3/croner/)：直接采用上游已提供的 Jiff 入口，不维护 Cron 时间转换兼容层。
- [SQLx time 支持](https://docs.rs/sqlx/0.9.0/sqlx/types/index.html)、[Tiberius time 支持](https://docs.rs/tiberius/0.13.0/tiberius/)：Rust 原生边界转换只发生于驱动，保留 SQL 字段和值的原有契约。SQLx PostgreSQL 无符号五位年份的文本解码限制并未由此次更换解决；扩展年份专项使用驱动二进制路径，不冒称文本路径支持。
- 固定 Laravel 13.40.0 `1449bf241017087fbd38c118ecaabef3f6797fdc`、Carbon 3.8.4、PHP 8.5 timelib 与既有 Calendar 来源继续作为行为基准。旧版可复用的扩展 civil／POSIX tail／首历史记录保留，历史查询后端改为 Jiff；Calendar 不因 Jiff 原生类型的 ±9999 年范围而缩窄已有 PHP 表达式范围。

公开调用调整已同步三语正文：`Date::fromUtc(Timestamp, TimeZone)`、`fromLocal(civil::DateTime, TimeZone)`、`asDateTime() -> &Zoned`；Date 与 TimeZone 使用 Clone，日期运算通过借用保留不可变复用。月末 Carry／Clamp、DST gap 向前与 fold 优先保留原偏移、应用／任务测试时钟隔离、微秒与 SQL 时间文本格式继续由原有及新增回归约束。负 epoch 小数使用欧几里得除法保持旧 Unix 秒向下取整语义，不能直接以 Jiff 截零的 `as_second` 代替。

阶段验证：框架库及全部 test target 在 Rust 1.94.0、jobs=2、关闭增量、dev debug=0 的独占源码中通过 `cargo check -p openrust-framework --tests`。独立 Date probe 4 函数与数据库 time/Tiberius probe 2 函数通过；Factories／调度 helper probe 4 函数通过。完整运行测试、三库及最终公告结果见本节后续记录，编译检查不计作行为验收。主要证据 `/private/tmp/openrust-jiff-full-20261121/`；轻量 probe `/private/tmp/openrust-chrono-20261121/`、`/private/tmp/openrust-database-time-20261121/`。


运行验证（Rust 1.94.0、独占源码与 target、增量关闭、debug=0）：

- Date／Calendar 库单元最终 33（包括维护 retry、固定时区及历史 RFC 文本）、request_date_clock 2、schedule_clock 5、support_date 5、time_travel 3 通过；三语 39 个受影响 Rust 代码块去重提取为 13 模块实际编译运行。固定数据入口已用替换 `TZDIR` 的独立进程验证，不依赖系统当前数据。
- Faker identity 116、user-agent 2、Eloquent 3、logging 14、mail 32 通过。mail 另有 2 个显式忽略的子进程／独立 SMTP 夹具，不计作覆盖。SQLite 和 TDS 原生类型边界通过；真实 PostgreSQL 18.6 与 MySQL 8.4.13 分别验证日期／时间小数精度及两种 prepare 模式，PostgreSQL 二进制路径包含 12000 年。
- Passport lifetime 2 通过；Pail options 7（新增 Zulu、原偏移、负 epoch）、files 4、432 条固定 PHP printer 向量及真实 HTTP 到独立 Artisan 的日志捕获通过。初次验收发现 `%:z` 不接受 `Z`，改用 Jiff 官方原生解析；显示继续保留日志原始偏移。
- Settings 时间文本精度单元通过，0／毫秒／微秒／纳秒及负 epoch 保留原 AutoSi 的 0／3／6／9 位分组。SQLite、真实 PostgreSQL 18.6、MySQL 8.4.13 分别运行同一组保存 9 与 HTTP／Worker 运行时 4 用例，全部通过；PostgreSQL／MySQL 的独占验收容器均正常回收，未删除数据卷。

完整日志在 `/private/tmp/openrust-jiff-full-20261121/` 的 `time-runtime.log`、`framework-time-regressions.log`、`postgres-time.log`、`mysql-time.log`、`extensions-runtime.log`、`settings-postgres.log` 与 `settings-mysql.log`；不把缺少服务环境的早期返回计为真实数据库覆盖。

补充运行验证：`pail-mysql.log` 使用真实 MySQL 8.4.13 执行 Pail 隔离 Worker、失败回调、后续消费和自定义应用 storage 路径，均通过，容器已回收。`seeding-runtime.log` 的 SQLite 实际 Artisan Seeder 及 `faker_named_dates_preserve_provider_locales_and_instance_zones`、`faker_qualified_date_ranges_match_extension_boundaries`、`faker_datetime_version_and_file_extensions_use_seeded_and_real_outputs` 四项均通过。更正根 Seeder 夹具的 Zulu 解析为原生 `Timestamp::parse`，避免 `%:z` 的 Zulu 限制。旧 Date probe 独占 target 约 49 MiB 已确认无占用并删除，保留其源码与日志；完整验收 target 暂交主代理复用最终 API 与缓存验收。

固定时区数据最终采用官方 IANA 2025b `zic -b slim` 的 vanguard 输出，默认 backward 别名、不启用可选 backzone，以保留原 chrono-tz 0.10.4 的历史别名行为。598 个原有名称共享 341 个 TZif、203,282 字节；归档 SHA、许可证与可重建命令见 `vendor/openrust-framework/resources/calendar/SOURCES.md`。两次生成的映射与数据 SHA 完全一致。直接程序将旧 chrono-tz 2025b 与 Jiff 原生数据在可表示下界至 2100 年的每次历史 transition 前一秒／当秒／后一秒对比，597 个旧名称共 198,039 个 offset／DST 样本零差异；额外的 PHP `Factory` 名称和其他共 598 个名称均通过 Jiff 原生解析。全量旧库对比使用既有旧工具链 rlib 独立输出，新 Jiff 入口使用 Rust 1.94.0，不能将旧参考程序编译计作当前工具链验收。

依赖最终复核：`cargo tree --workspace --all-features --target all -i chrono --locked --offline` 无活动路径，`cargo deny --all-features check advisories` 退出 0、`advisories ok`，没有增加 ignore。Cargo.lock 仍可能包含 serde_with 未启用的 optional chrono 锁项；实际全 feature／全 target 依赖图无 chrono，不能声称 lock 文本中已完全删除该名称。

最终固定数据下 `final-time-runtime.log` 的 lib 32、request_date_clock 2、schedule_clock 5、support_date 5、time_travel 3 全过；新增覆盖 Dublin／Casablanca／Prague／Windhoek 的负夏令时 PHP `I` 与 fold 偏好，以及维护 retry gap 拒绝／fold earlier／显式偏移。追加 Faker 标准 RFC 历史偏移秒舍入回归后，`final-lib-runtime.log` 的库单元 33 全过。RFC 输出交 Jiff 官方 `DateTimePrinter` 的分钟精度规则，避免 `%:z` 原生可选秒改变旧文本；ISO 8601 同样保持分钟偏移和紧凑分隔符。PostgreSQL typed TIMESTAMP 参数新旧解析对 `12000`、`+12000`、`-0001` 及小数结果一致，原 SQLx 文本解码限制与参数解析区别记录，证据 `openrust-vanguard-20261121/pg-bind-{new,old}.log`。

最终整合检查：独占源码中 `cargo +1.94.0 clippy --workspace --all-targets --all-features --locked --offline -j1 -- -D warnings` 退出 0，包含最终 SMS／Socialite 与新增时间资源；日志 `final-clippy.log`。Fortify／Passkeys 的测试直接 include 根 User，故其 Jiff 只放 dev-dependencies，库生产代码使用既有 Date 入口，无额外生产时间依赖。三语 `node docs/tests/check_content.mjs`、最终 `cargo +1.94.0 fmt --all --check` 与 `git diff --check` 通过。严格检查日志中 patched 第三方 imgalifier-avif 的既有 `asm` cfg 警告受 Cargo dependency lint cap 管理，不影响工作区 `-D warnings` 结果；不为时间专项修改该外部源码。


## 最终组合依赖与来源盘点

2026-11-21，对 `b168b31aa063a289a5a025ed5080a99f19bb6c7e` 的独立 Git 归档再次执行 Rust 1.94 锁定全 feature 元数据、`cargo deny --locked --workspace --all-features list --format json --layout crate` 和 `cargo deny --format json --locked --workspace --all-features check advisories`，均退出 0。公告摘要为 0 errors／0 warnings，无新增 ignore。原始 metadata 含 679 包（658 registry、20 local、1 固定 Git），全部声明许可证；启用图许可证／来源清单为 640 包，均有许可证声明。此为盘点，不代替最终分发许可证批准。

唯一 Git 依赖 oxideav-jpegxl 0.0.14 固定 `1c2f5a86a052752b0e8181f842a9d4e3460ec0e5`，无浮动引用；第三方补丁保留其原版本、来源与许可证，IANA 数据另保留官方来源、SHA、许可及可重复生成命令。`cargo tree --locked --offline --workspace --all-features --target all -i chrono` 无启用路径。锁文件保留 serde_with 的未启用 optional chrono 条目，不能把公告检查成功写成 lock 中完全无 chrono。同期新增支付扩展不属于此盘点。

证据：`/private/tmp/openrust-final-integration-20261121/{metadata-final.json,licenses-final.json,licenses-final.log,advisories-final.jsonl,chrono-tree-final.log}`。后续依赖变化与实际发行目标仍需各自审计，当前未作发布批准。


## 2026-11-21 模拟验收组合依赖复核

244842943隔离快照叠加SMTP已有rustls-native-certs feature及9ee4881ca测试，以Rust1.94运行锁定全workspace/all-features的cargo deny公告检查及许可证来源list，均退出0。公告摘要0 errors/0 warnings，启用来源/许可证清单642包；工具未找到配置而使用默认策略，无新增ignore。本轮不升级依赖版本，仅lettre锁项新增已有依赖边。证据`/private/tmp/openrust-production-simulation-20261121/{advisories-final.jsonl,licenses-final.json,licenses-final.log}`；不包含并行Activitylog/Horizon，也不代替最终分发许可/敏感信息/版本/发布批准。


## 2026-11-21 最终本地收尾复核

`0855b54f2` 与本轮已验快照的依赖输入相同：锁定全feature公告0 errors／0 warnings；启用图672项均有许可证声明，唯一Git来源仍为固定oxideav-jpegxl `1c2f5a86a052752b0e8181f842a9d4e3460ec0e5`。38个本地包的LICENSE／NOTICE条目齐备，Docker交付复制根LICENSE及THIRD_PARTY_NOTICES。有限敏感模式扫描未跟踪运行.env；9处命中均为公开测试密钥、无效示例或文档marker，经人工核对。声明齐备及扫描通过不代替实际发行许可审查，也不证明不存在其他秘密。

证据统一在项目内 `target/tmp/final-acceptance-20261121/{advisories.jsonl,licenses.json,release-checks.json}`。最终发布版本、tag、推送、Release及部署均未授权，继续保留用户批准门禁。

## 2026-11-27 Rust 1.95 与 Smithy 上游包

Rust 最低支持版本升为 1.95。基线锁文件与更新后的 AWS 依赖分别在 macOS ARM64、Rust 1.95.0 下通过 `cargo check --locked --workspace --all-targets --all-features`。更新后仓库和独立包夹具均解析到 crates.io 的 `aws-smithy-http 0.64.3`、`aws-smithy-runtime 1.16.4` 和 `aws-smithy-types 1.9.5`，依赖图中没有 `pin-utils`，已删除三个本地回移及各自的 patch 接线。[Smithy 上游修改](https://github.com/smithy-lang/smithy-rs/commit/66b12d3e2165f98ff62146b46fb248c5845e8ea4)

SQS 回环协议与队列消费者 3 项、SES v2 回环 Artisan 邮件发送 1 项、隔离 RustFS 1.0.5 对象读写及预签名 3 项通过。RustFS 容器已回收。托管 AWS、DynamoDB 运行时及其他平台未在本批实测。日志位于 `target/tmp/rust-195-upgrade/` 下的 `baseline.log`、`aws-check.log`、`fixture-metadata.log` 和 `aws-consumers.log`。
