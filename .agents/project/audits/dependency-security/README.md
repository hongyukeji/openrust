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
