# Laravel 对齐修复独立复核（2026-11-22）

## 当前结论：返修与复验已闭环

2026-11-22 用户要求处理复核问题后，重新检查发现其他 Agent 已在当前工作区完成 R1/R2 及全量验收失败项的修复。本轮保留并复用这些已暂存实现，没有覆盖、重复实现或重新暂存其他协作者改动。初审提交为 `fb5240e1a`；复核期间原 Agent 已将该批修复本地提交为 `8ff57b2ef`。下述关闭结论针对该修复提交中的框架与测试，并非初审旧提交本身；本轮没有执行 Git 提交。

| 问题 | 当前实现与验收 | 结论 |
|---|---|---|
| R1 / L02 | integer 浮点分支复用已有 `php_float_string`，再进入同一整数过滤分支；公开入口覆盖 `1.0`、正负 `1e13`、正负 `1e14`、同值 i64、分数及溢出 | 已修复，回归通过 |
| R2 / L07 | regex 先限定 String/Number，再执行原匹配；保留通用 text 以及 nullable 跳过机制；覆盖 bool/null 拒绝、整数/浮点/字符串接受和 nullable null | 已修复，回归通过 |
| 原全量失败项 | 逐项核对后续成功原始日志，环境与夹具错误均有修正或正确入口复验，见下表 | 本次复核清单已闭环 |

### 本轮独立实际运行

- 将当前 `laravel_alignment_validation.rs` 复制到本批隔离目录，用 Rust 1.94.0 的 rustc 编译完整集成测试目标，链接已存在且闲置的框架库及其依赖，执行 **9 passed / 0 failed / 0 ignored**。包含公开 Factory/Validator，不再仅为初审源码片段探针。没有重建整个 Cargo 依赖树，也没有写入其他工作流的 target。
- 框架库所在工作流为 `target/tmp/welcome-locale-20261122/`。其源码快照与当前框架 **1,461 个受跟踪文件逐字节一致**，锁文件也一致。测试编译、执行、源码核对证据分别在 `target/tmp/laravel-fix-review-20261122/integration-build-results.json`、`validation-integration.log`、`validation-integration-result.json`、`framework-library-source-check.json`。
- 当前完整工作区相对全量验收的 3,246 项输入指纹仍有 9 项差异，均为另一条在途任务的欢迎页国际化与 Docker/部署说明；当前框架、上述回归测试和本清单失败目标的修复代码与已验收输入一致。本轮未把另一个任务的差异算作本次修复或验收。
- 重新执行根 `cargo +1.94.0 fmt --all --check`、三语 `node docs/tests/check_content.mjs`、暂存与未暂存的 `git diff --check`，均通过。

### 原全量失败项的关闭依据

下表为本轮实际读取的后续日志，均相对 `target/tmp/clean-full-acceptance-20261122/`。这些由原 Agent 运行；本轮未再次启动完整数据库、浏览器或供应商矩阵。首次 `full-test-result.json` 的失败退出码保持原样，不改写历史。

| 初次失败 | 已核对的关闭依据 |
|---|---|
| CSRF | `final-csrf.log`：3 passed；夹具指令边界及 message 作用域预期修正，生产 Blade 不变 |
| 日志 Unix socket 路径 | `real-openrust-framework-7.log`：logging 14 passed；使用项目内短 TMPDIR |
| Precognition MySQL 环境 | `real-openrust-framework-9.log` 对应 precognition_http：8 passed，浏览器 helper 另有实跑材料 |
| 发布迁移 | `retry-openrust-framework-test_app.log`：11 passed；精确核对 fixture 与 Sanctum，保留幂等断言 |
| Passport 授权环境 | `real-openrust-passport.log` 对应 authorization_code：53 passed；提供真实隔离 MySQL URL |
| 根误用 --lib | 根包确无 lib；改用真实 bin/集成目标，`application-bin-tests.log` 及默认构建记录通过 |
| HTTP HEAD 替换预期 | `retry-openrust-http.log`：16 passed；GET 替换同步替换 HEAD，检查新状态及旧 handler 未执行 |
| SQLite 队列链故障注入 | `retry-openrust-job_batches.log`：3 passed，1 子进程 helper ignored；触发器改为只匹配当前 payload.data，三库消费者实跑 |
| 迁移计数 | `retry-openrust-migrations.log`：12 passed；限定应用迁移路径并核对每项恰好登记一次，不再盲目统计扩展表 |
| Passport 安装环境与源码过滤 | `retry-openrust-passport_install.log`：1 passed；保留包内合法 storage/tmp 源码，继承正常并发与增量 |
| Faker 日期格式 | `retry-openrust-seeding.log`：70 passed；使用已有 Jiff 严格 format API；原 `%Q` 实为合法格式，非法负例改为 `%` / `%J` |
| 后续缓存与注释隔离失败 | `final-cache-isolated-repeat.log`：37 passed；`final-cache-alias-repeat.log`：1 passed；`final-schema-comments.log`：3 passed |
| 最终静态检查 | `final-complete-clippy.log`：workspace/all-targets/all-features 严格 Clippy 完成，无失败；先前失败日志仍保留 |

更完整的环境及夹具根因见 [清理后全面验收](08-04/clean-full-acceptance-2026-11-22.md)。本清单没有剩余待修复项；真实供应商、Windows、正式部署、已声明未实现的 Reverb 完整 pusher:signin 协议仍是项目原有边界，不在此次返修中宣称完成。

## 以下为初次复核历史

### 初次结论与范围

复核提交 `fb5240e1a`，修复起点 `259b8a93a`，对照原审计 [L01–L22](laravel-code-audit-2026-11-21.md)。22 项均找到对应实现及验收材料，但 L02、L07 各有一处具体行为差异，需要返修，不能全部关闭。其余项本轮未发现新的确定缺陷，不等于全部 Laravel API 已验证等价。

固定参考为 Laravel Framework v13.40.0，提交 `1449bf241017087fbd38c118ecaabef3f6797fdc`；应用骨架 v13.11.0，提交 `06d016a364a37430eec9cbc52209adce3d7667ce`。参考本地固定源码中的 `src/Illuminate/Validation/Concerns/ValidatesAttributes.php`，不是浮动分支。

本轮只审查与记录，没有修改生产代码、暂存、提交或推送。进入时，`framework/tests/csrf.rs`、`framework/tests/test_app.rs` 有其他协作者的未提交修改；收尾时又出现 `tests/http.rs`、`tests/migrations.rs`、`tests/passport_install.rs`、`tests/seeding.rs` 修改，说明其他协作者仍在处理验收问题。本轮均保留，未将其算成本轮修复，也未把尚未出现的复验结果计为通过。

## 确定需要返修的两项

| 编号 / 优先级 | 位置 | 反例与实际行为 | 固定 Laravel 行为 | 关闭条件 |
|---|---|---|---|---|
| R1 / L02 / P2 | `vendor/openrust-framework/src/validation/rule.rs:231`，浮点分支 242–244 | JSON 浮点值 `100000000000000.0`（`1e14`）通过 `integer`；实现只检查无小数部分及 i64 范围 | 上游 `validateInteger` 第 1629 行调用 `FILTER_VALIDATE_INT`。默认 PHP precision=14 将该浮点转换为 `1.0E+14`，过滤失败；同值的整数输入应通过 | 复用既有 PHP 标量转换机制，对齐默认浮点过滤语义；通过公开 Factory/Validator 验证 `1.0`、`1e13`、`1e14`、负数及整数同值对照，保持 strict、溢出规则 |
| R2 / L07 / P2 | `vendor/openrust-framework/src/validation/validator.rs:450`，调用 `standard_rules.rs:85` 的 `text` | `true` 配 `regex:/^1$/`、`false` 或未声明 nullable 的 `null` 配 `regex:/^$/` 均通过；通用 text 将其转换为 `1` 或空串 | 上游 `validateRegex` 第 2135 行先拒绝既非字符串也非数值的类型，bool/null 均失败 | 在 regex 入口保持上游类型约束，不全局修改供其他规则使用的 text；补公开 Validator 的 bool/null/数字/字符串正反例，另验 `nullable|regex` 对 null 正常跳过 |

### 实际复现与证据边界

材料均在项目根 `target/tmp/laravel-fix-review-20261122/`。

- `upstream-probe.php` 在 PHP 8.5.12 容器内加载固定 Laravel 的真实 `ValidatesAttributes` trait 并执行原方法，退出 0。`upstream-probe.jsonl` 记录 `1e14` 浮点失败、bool/null regex 失败及数字/字符串正例。Unicode `/u` 反例已排除，不列为缺陷。
- `probe/` 是临时独立 Cargo 工程。原样提取当前 `integer()`、`text()`、`double_string()`，执行当前 regex 匹配表达式并使用仓库 pcre2。Rust 1.94.0、offline 编译及执行退出 0，`rust-probe.jsonl` 复现表中错误通过结果，`rust-probe.stderr` 保留编译日志。
- 这是源码片段的定向运行，不是完整框架集成回归。完整调用链已经静态核对：默认 integer 直接调用该函数；regex 使用该 text；null 只有声明 nullable 才提前跳过。应由返修 Agent 补公开入口集成用例。已有 `laravel_alignment_validation.rs` 未覆盖上述反例。
- 原验收缓存已被其他批次清理，本轮没有重新冷编译整个框架。独立探针的 TMPDIR 和 Cargo target 均在本批项目内目录。

复现 Rust 探针：

```sh
TMPDIR="$PWD/target/tmp/laravel-fix-review-20261122/tmp" \
CARGO_TARGET_DIR="$PWD/target/tmp/laravel-fix-review-20261122/cargo-target" \
cargo +1.94.0 run --offline \
  --manifest-path target/tmp/laravel-fix-review-20261122/probe/Cargo.toml
```

## L01–L22 核对表

“实现及证据已核对”指本轮检查代码与既有 Agent 日志，未声称重新执行该模块的完整测试。

| 原编号 | 内容 | 本轮判断 | 边界或后续 |
|---|---|---|---|
| L01 | HTTP client 请求头、multipart、重定向、sink | 实现及证据已核对 | sink 仍受 body_limit 限制并缓冲下载，三语已披露 |
| L02 | integer / strict | 需要返修 R1 | 大浮点转换边界遗漏 |
| L03 | min/max/size/between | 实现及证据已核对 | 已用 BigDecimal；不扩展为所有规则全集 |
| L04 | Request / prefetch / Session | 实现及证据已核对 | 全量批次 CSRF 失败仍需复验，见下节 |
| L05 | 配置默认 slug / cookie / prefix | 实现及证据已核对 | 默认名称改变不迁移旧会话与缓存 |
| L06 | Mail sendNow / always* | 实现及证据已核对 | 取消为 Option；真实供应商未验 |
| L07 | 标准 Validation 规则 | 需要返修 R2 | regex 输入类型遗漏 |
| L08 | Validator / FormRequest 钩子 | 实现及证据已核对 | 官方前端既有浏览器证据已检查；本轮未重跑 |
| L09 | Filesystem 契约及操作链 | 实现及证据已核对 | S3 为本地协议模拟，真实权限、签名、区域未验 |
| L10 | ApplicationBuilder 入口及生命周期 | 实现及证据已核对 | withBindings 按固定上游实际 registered 行为核对 |
| L11 | Auth 原名契约及消费者 | 实现及证据已核对 | Fortify 历史一次 Session 失败根因仍未定位 |
| L12 | Queue 原名契约及消费者 | 实现及证据已核对 | 全量批次 job_batches 有失败记录，需闭环 |
| L13 | Log 上下文 | 实现及证据已核对 | 全量批次存在 Unix socket 路径过长的夹具失败 |
| L14 | Events push/flush/forgetPushed | 实现及证据已核对 | flush 不自动消费清空；不以 defer 替代 |
| L15 | TestResponse 常用断言 | 实现及证据已核对 | 限于列明断言，不是 Laravel 断言全集 |
| L16 | Schedule 时长监听 | 实现及证据已核对 | Duration 适配与 Finished/Failed 路径有定向证据 |
| L17 | Query 回调 / toRawSql | 实现及证据已核对 | 三库 Query 与 MySQL/SQLite ORM 为已有实跑证据 |
| L18 | URL 策略入口 | 实现及证据已核对 | 真实代理、域名、HTTPS 部署未验 |
| L19 | View 条件、循环、共享数据 | 实现及证据已核对 | blocking 渲染作用域已接入 |
| L20 | Broadcast 用户认证 HTTP | 声明范围内已实现 | Reverb 完整 pusher:signin 客户端协议仍未实现，不算端到端通过 |
| L21 | Redis purge/extend | 实现及证据已核对 | Redis 8 原 ignored 定向实跑材料已检查 |
| L22 | Translator 原名及作用域 | 实现及证据已核对 | owned String / &self Result 属已记录 Rust 适配 |

## 另一个全量验收批次尚不能算通过

`target/tmp/clean-full-acceptance-20261122/full-test-result.json` 当前记载总退出码 1、67 批中 9 批退出非零。本轮读取其结果和具体失败日志，没有运行该批次，也不能将其未经归因的失败全部认定为生产缺陷。当前没有找到这些失败全部复验通过的汇总证据。

| 分组 | 读到的具体失败 | 当前判断 |
|---|---|---|
| 验收入口 | 根应用传 `--lib`，报 no library targets | 入口错误，应选真实 bin/test 目标，不能算生产故障 |
| 数据库环境 | Precognition、Passport 可选授权及安装测试缺 MySQL URL / 必需变量 | 应用项目 MySQL 夹具重跑，不能按跳过算成功 |
| 日志夹具 | 3 项 Unix socket 路径超过 SUN_LEN | 项目内选择足够短的临时路径后重跑 |
| CSRF | `origin_only_and_session_errors_are_application_and_request_scoped` 返回 500 | 相关 tests/csrf.rs 存在其他 Agent 的 WIP，保留并等待实际复验 |
| 资源发布 | `artisan_publishes_registered_package_assets_and_migrations` 文件名后缀断言失败 | 相关 tests/test_app.rs 存在其他 Agent 的 WIP，保留并等待实际复验 |
| HTTP | `registration_order_and_per_method_replacement_are_preserved` 204 / 200 不符 | 待核对当前契约并复验 |
| 队列 | SQLite `dispatches_and_tracks_batches_on_sqlite` 子进程 queue storage operation failed | 待定位与复验 |
| 迁移 | SQLite ledger 表数断言实际 11、预期 4 | 待核对启用扩展后的夹具预期与实际登记链 |
| Faker | `creditCardExpirationDateString(true, Some("%Q"))` 未返回预期错误 | 待对照固定依赖契约及复验 |

这些是额外验收缺口，不新增为 L01–L22 的确定代码缺陷编号。旧定向通过不能覆盖当前全量失败，当前全量失败也不能抹去旧定向通过的证据。

## 本轮实际验证

- `cargo +1.94.0 fmt --all --check`：通过。
- `node docs/tests/check_content.mjs`：通过；每语 109 个导航章节、111 个 Markdown 文件。
- `git diff --check`：通过。
- 固定 PHP 真实方法探针及 Rust 源码片段探针：均运行成功，观察到 R1/R2 差异。
- 2,330 个受版本控制的 Rust/manifest/lock 文件与原修复隔离源码对比：仅其他 Agent 正在修改的 csrf.rs、test_app.rs 不同；此结论不包含全部文档文件。
- 原 Agent 的 alignment、query/Redis、ORM、扩展、生成应用、Clippy、三语构建及浏览器日志已核对；这些是历史实跑材料，本轮未重新执行全套数据库、Clippy、浏览器和供应商验收。

关闭本轮复核需先修复 R1/R2、补公开入口集成用例，并给全量失败清单逐项提供修复后通过或明确环境阻塞。真实第三方服务、Windows、正式部署以及已声明的 Reverb 完整客户端协议仍按现行待办处理。
