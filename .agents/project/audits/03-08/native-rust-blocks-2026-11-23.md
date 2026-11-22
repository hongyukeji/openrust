# Blade 原生 Rust 代码块

日期：2026-11-23。对应[计划](../../plans/blade-native-rust-blocks.md)。本次授权开发与验收，不授权暂存、提交、推送或发布；保留其他协作者工作。

## 方案定稿

保留 Blade 指令树、视图查找器、作用域、转义与缓存。在既有 `openrust_package_discovery::generate()` 中扫描应用及当前目标 normal 依赖的 `resources/views/**/*.blade.html`，包括应用发布覆盖目录；包可用 `[package.metadata.openrust.blade].paths` 增加包内相对目录。运行期 namespace 和覆盖优先级继续由现有 Provider／Finder 管理。默认应用 build.rs、bootstrap 和 Provider 无新增示例业务。

共享轻量 `openrust-blade-compiler` 只负责原生块词法边界；构建 feature 使用 syn 解析函数体，生成真实 Rust 函数，应用 rustc 执行类型、所有权、可见性和同步上下文检查。运行期不包含 syn 解析路径，也不启动 Cargo／rustc。已有包清单携带不可变函数表，无新全局可变注册中心。

最小闭环先通过实际应用构建及 Factory 渲染，再确定公开契约：`NativeContext::get<T>` 返回拥有的值，`set` 明确写回；`NativeResult` 是 `Result<(), Box<dyn Error + Send + Sync>>`。转换复用 ViewValue 数字访问器，整数范围检查、浮点读取可能舍入；缺键与类型错误在渲染期失败，Option 仅接收已有 null。不是动态模板的完整静态类型检查。

Rust 局部变量及 use 不跨函数导出。写回影响当前指令块的后续节点；分支、循环每次迭代、include、组件和嵌套渲染恢复调用方数据。循环内写回不更新下一轮或外层 while 条件。return Ok(()) 只结束该原生函数，输出继续通过 Blade 转义；无异步函数上下文。

构建与运行复用同一扫描器，保护 Rust 字符串、原始／字节／C 字符串、字符、生命周期、行／嵌套注释及宏分隔符，兼顾 Blade 注释、verbatim、转义、表达式及指令参数。源模板完整有序代码块正文必须与二进制中某个已构建序列逐字匹配，缓存命中也先检查。相同序列允许复用；动态 inline 不能引入新 Rust 源码。普通 HTML 更新仍可刷新，块新增／修改／删除／重排须重建，缓存格式 39 → 40。语法错误包含原模板路径与块起始行；rustc 诊断指向带模板名／块号／起始行的生成文件，不保证精确原行列映射。

原生函数生成在应用 crate：可访问应用 crate 符号及依赖公开 API，不能访问包私有实现；相对 include!、file!、line! 使用生成文件位置。模板是可信应用源码，拥有应用代码权限，非沙箱。

## 调研与复用

联网读取固定源码及测试，下载记录为 `target/tmp/blade-native-20261123/research.json`，来源位于同目录 `upstream/`。部分探测 URL 的 404 不作证据。下列候选均未作为完整引擎接入或运行其完整测试套件，不据此宣称性能收益。

| 来源 | 固定版本／提交及许可 | 采用与差异 |
| --- | --- | --- |
| [固定 BladeCompiler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/BladeCompiler.php)、[RawPhp](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/Concerns/CompilesRawPhp.php)、同提交 BladePhpStatementsTest／BladeVerbatimTest／BladeCommentsTest | v13.40.0，`1449bf241017087fbd38c118ecaabef3f6797fdc`，MIT | 保护原始代码后继续现有模板编译链；Rust 的编译时机改为应用构建，不模拟 PHP 执行器。 |
| [Sailfish](https://github.com/rust-sailfish/sailfish/tree/efcd80adc8f668fc295b4ac0f52d59830c81a817) | 0.11.9，`efcd80adc8f668fc295b4ac0f52d59830c81a817`，MIT | 编译器以 syn::Block 解析 Rust 并生成宿主代码；借鉴机制，不替换 Blade 语法。 |
| [Askama](https://github.com/askama-rs/askama/tree/a7715696d0d8c13f576e170600869a8226fee394) | 0.16.1，`a7715696d0d8c13f576e170600869a8226fee394`，MSRV 1.88，MIT OR Apache-2.0 | 核对 derive 生成机制；其结构体上下文静态保证不能套用于动态 ViewValue。 |
| [Maud](https://github.com/lambda-fairy/maud/tree/b95c8494760c3bb640d2a973c4f69d29671e11db) | `b95c8494760c3bb640d2a973c4f69d29671e11db`，MIT OR Apache-2.0 | 核对宏生成及转义边界；不引入另一套 HTML DSL。 |
| [syn](https://github.com/dtolnay/syn/tree/2.0.119) | **实际依赖固定 =2.0.119**，MSRV 1.71，MIT OR Apache-2.0 | 仅 build feature 启用 full／parsing，无直接 quote 依赖；Rust 1.94.0 实际编译。保留原正文到生成文件，由 rustc 检查语义。 |

重新核对 `v0.x:vendor/openrust/framework/src/view.rs`、框架 Cargo.toml：旧版使用 MiniJinja 的环境与 namespace 管理，没有可复用的原生 Blade 代码生成管线。复用当前构建发现、PackageManifest、Factory、Finder、RenderContext 与 ViewValue，新增共同扫描器避免构建／渲染边界漂移。维护成本集中在扫描边界与生成文件；由同一实际消费者覆盖，避免引入整套可选模板引擎。

## 实现完成

- `vendor/openrust/blade-compiler/`：共同扫描器、构建生成器及词法反例。
- `vendor/openrust/package-discovery/`：normal 图及模板根发现、已有 packages.rs 装配。
- `vendor/openrust/framework/src/view/native.rs`：类型转换、当前作用域写回及原生错误来源。
- `view/compilers/{blade_compiler,template}.rs`：缓存前一致性校验、原生节点、局部写回；`view/console.rs` 保留重建错误提示。
- `foundation/package_manifest.rs`／`view_service_provider.rs`：应用级不可变构建产物装配。
- `tests/fixtures/blade-native/`／`tests/blade_native_acceptance.py`：真实应用、依赖包、模板及独立部署消费者。
- 三语 `docs/{en,zh-Hans,zh-Hant}/the-basics/blade.md`：实际用法、作用域、同步／可见性、数值转换、构建、部署和保留指令迁移。

## 验收状态

**方案定稿、实现完成、下列范围验收通过。** macOS arm64，Rust 1.94.0。最终质量快照为 `292418a396f62497ab7d1d6c295b8a20aa6524a8` 加本次 41 个源码／夹具文件，位于 `target/tmp/blade-native-20261123/quality-source/`；逐字比较一致。三语正文另与 `source/docs/` 比较，真实消费者使用 `acceptance-quality/`。没有把并行在途源码的未完成质量结果算入本次通过结论。

全部证据位于 `target/tmp/blade-native-20261123/`。最终 Cargo 经 `tests/build_guard.py --workflow blade-native-20261123 --tmp-dir <项目>/target/tmp/blade-native-20261123/temp -- …`，使用独占同目录 `cargo-target/`，保留增量编译与正常并发。设置 dev/test debug=0、strip=symbols 控制产物大小，未关闭增量。以下以 `Q=…/quality-source/Cargo.toml`、`W=…/blade-native-20261123` 缩写绝对项目内路径：

| 命令／实际入口 | 结果及证据 |
| --- | --- |
| `cargo +1.94.0 fmt --manifest-path $Q --all --check`，另对新 helper／独立 fixture 执行 rustfmt --check | 通过，`fmt-scope.log`、`fmt-fixture.log`。 |
| `cargo +1.94.0 clippy --manifest-path $Q --locked --workspace --all-targets -- -D warnings` | 通过，`clippy-scope.log`。未屏蔽本次 lint；仓库既有第三方补丁 crate 的 time／rav1e／imgalifier 警告另见日志。 |
| `cargo +1.94.0 test --manifest-path $Q --locked -p openrust-blade-compiler --all-features` | 3 项通过、0 ignored，`scanner-final.log`。 |
| `cargo +1.94.0 test --manifest-path $Q --locked -p openrust-framework --test blade_arithmetic --test blade_statements --test blade_expression_contracts --test blade_query_helpers --test blade_translations --test view_finder --test view_native_values` | 39 项通过、0 ignored，`regressions-scope.log`。 |
| `cargo +1.94.0 test --manifest-path $Q --locked -p openrust --test application_skeleton` | 2 项通过、0 ignored，`skeleton-final.log`。 |
| `python3 tests/blade_native_acceptance.py --work $W/acceptance-quality --source $W/quality-source` | **29 个实际消费者阶段通过**，`consumer-quality.log` 及 `acceptance-quality/*.log`。消费者内部固定工具链 1.94.0、锁定离线构建。 |
| `cargo +1.94.0 clippy --manifest-path $W/acceptance-quality/consumer/Cargo.toml --locked --features standalone --all-targets -- -D warnings` | 通过，包含真实生成 Rust，`consumer-clippy-final.log`。 |
| 隔离 `source/docs` 的 `npm run build` | 内容／命令入口正反例／VitePress 构建通过，`docs-build-final.log`。 |
| `node tests/check_browser.mjs`、`node tests/check_scope.mjs`，专项 native 页面脚本 | 三语、`/` 与 `/docs/`、桌面／手机共 12 个通用及 12 个专项组合通过；全部导航通过，`docs-browser.log`、`docs-scope.log`、`docs-native.log`。专项截图检查无页面横向溢出，人工查看简中手机及英文桌面布局。 |

实际消费者不是源码关键词检查：

- 标准库 BTreeSet 求和及应用函数调用 → 显式写回 → 默认转义／显式原始输出；三语正文 Alice 示例实际渲染。
- rustc 拒绝类型 E0308、所有权 E0382、包私有符号 E0603、await E0728、字符串返回值；syn 拒绝非法语句；未闭合块构建失败。诊断实际包含模板文件名／块信息。
- 缺键、错误类型、已有 null、业务错误传播、Rust let 不自动导出；分支不进入不执行、提前 return 仅结束当前块。
- foreach 每轮读取变量及 loop 元数据、for 计数、while 加显式 break；分支／循环／include／组件局部修改恢复；子模板继承及布局写回。
- 包公开函数、包视图、发布覆盖优先、移走覆盖后新进程回退包默认视图；自定义目录经既有 namespace API 消费。
- 12 个并发线程、第二应用实例、3 个真实 SyncQueue 任务通过 Facade 渲染；HTTP 原生块嵌套 Factory.render 不污染外层数据。
- 构建及 view:cache 不生成业务副作用标记；view:clear 保留已有标记且再次渲染可执行原生函数。未知 inline 原生源码明确拒绝。
- 热缓存修改／新增／重排代码块明确拒绝；Cargo 对模板新增／修改／删除实际重建；旧二进制＋新模板、新二进制＋已删除旧源码／旧缓存拒绝，匹配组合通过。拒绝检查断言真实重建提示，不仅检查非零退出。
- 使用标准 standalone feature 与 Application::inferBasePath；复制二进制和资源到独立目录，隐藏消费者源码，移除 APP_BASE_PATH，PATH 不含构建工具，真实 TCP HTTP 24 组并发不同语言／数据请求通过；运行期改原生代码返回 500，普通 HTML 变更正常刷新，恢复后再次成功。

## 过程中的失败与修正

早期一条 Cargo 命令没有显式 manifest，因 build_guard 固定 cwd 而针对活动根目录运行；发现缓存版本断言后改为显式隔离 manifest，不把该命令计为隔离验收。早期复用空闲构建缓存后发现另一流程接续，随后切换本任务独占 target；其他流程后续执行的安全产物清理不记成本任务成果。

初始含其他在途改动的 `source/` 全工作区 fmt／Clippy 分别被欢迎页／语言相关格式及路由两处 needless_borrow 阻断，保留 `fmt-final.log`／`clippy-final.log`。没有修改协作者文件，也不据旧混合快照判断当前其他模块质量。该组合仍通过当时 39 项视图回归及 28 阶段消费者；最终结论使用上述干净提交基线加本任务范围的 44 项测试、29 阶段消费者与严格检查。

新增夹具曾因缺少 trait 导入、无参数 @break 后紧接 @endwhile 的既有单词边界、生命周期示例 needless_lifetimes，以及更换示例时漏补一个参数而失败，均修复并重跑。生命周期使用必要的双参数借用，不增加 lint 豁免。旧二进制反例放在部署目录，避免 standalone 根目录差异造成假阳性。临时源码与记录继续保留；不以先前成功覆盖后续失败。

## 范围与剩余边界

本计划范围无剩余实现阻塞。保留明确语言边界：同步原生函数、动态数据运行期转换、每指令块局部写回、生成文件诊断、可信源码权限；不承诺完整 PHP 运行时或原模板精确 Rust span。相同源码序列的构建产物可复用，默认部署仍须二进制与资源匹配。旧字面 @rust／@endrust 须迁移到 @@ 或 verbatim。

未验 Windows／其他架构、release／Docker 发布、完整上游 PHPUnit／候选模板引擎套件、性能基准、持久化异步队列驱动；任务隔离本次使用真实同步队列，外部数据库／队列服务不纳入通过结论。全部在途模块组合不是本次完整验收范围。

最终验收阶段未触及磁盘停线；收尾可用 40,058,088 KiB，约 41.02 GB／38.20 GiB，见 `target/tmp/blade-native-20261123/final-check.json`。保留编译缓存、日志、独立交付文件和其他协作者改动，没有暂存、提交、推送、发布或操作开发数据库。


验收完成后用户明确授权本次本地提交；仅提交本主题源码、测试、三语正文及对应记录，不包含其他在途改动，不推送或发布。
