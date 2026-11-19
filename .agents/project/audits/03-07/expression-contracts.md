# V03 Blade 表达式契约验收

2026-11-20，本批关闭 V03；不代表 V04–V18 或整个框架发布门禁通过。固定原文见[Laravel 基线](../../plans/laravel-rebuild-baseline.md)，三语正文的对应入口为 Blade `#expression-runtime` 与 `#array-expressions`。

## 契约与实现

以固定 Laravel Framework v13.40.0 的 BladeCompiler、CompilesIncludes、CompilesEchos、CompilesComponents 和 PHP 8.4.14 实际结果核定。旧版 `v0.x:vendor/openrust/framework/src/view.rs` 使用 MiniJinja，无可直接移植的 Blade 原生数组表达式；保留当前指令树、注册器与原生 ViewValue 结构。

| PHP 能力 | 本批 Rust 契约与验证 |
| --- | --- |
| 数组展开 | 新增 ArrayEntry::Unpack，支持列表及有序 ViewValue::Object。整数键重新编号，字符串键替换而保留位置，负整数起始索引与最大索引溢出遵循固定 PHP 结果。克隆值，不修改源数据；INF、NAN、负零保持原生类型。子视图数据也接受展开，空展开保留继承数据；数字数组不引入具名变量。 |
| 函数、静态方法、动态 callable | 通用 PHP 表达式调用不受支持；处理器、Factory::creator／composer 计算值，BladeCompiler::directive 接收原生参数并返回文本或可信 HTML，r#if／check 处理条件。测试实际注册、转义及缓存模板采用替换后的回调。 |
| 引用、引用遍历、变量变量 | Rust 侧先完成修改和借用关系，再提供拥有值的 ViewData。模板不建立 PHP 引用；对应语法在编译时拒绝。 |
| Traversable、ArrayAccess、魔术属性与对象方法 | 在 Rust 中物化数组／有序对象或投影字段；foreach 的 withIterator 不等于展开源。服务使用 registerViewService／@inject，组件使用 Component::data，文本投影使用 stringable／withStringable。现有接口沿用，不新增通用 PHP 调用解释器。 |
| PHP 语句及导入 | 应用 Rust 代码和模块导入承担职责。未注册的 @php、@endphp、@use 明确编译失败，避免原样泄漏；@@ 和 @verbatim 保留字面量，未知指令仍按既有规则保留。 |
| 编译与渲染阶段 | 预编译解析所有分支，不执行 creator、composer、组件工厂或指令回调；测试计数器核实。未选中分支的非法语法仍编译失败；合法表达式只在被求值时展开，短路跳过标量展开。渲染失败不污染缓存及下一次数据。 |

缓存版本由 33 升为 34，原数组节点序列化结构改变后重新编译。旧版本失效测试同步更新。语言适配的边界已在英文、简体和繁体正文明确说明，没有将任意 PHP 执行或对象重载列为已实现。

## 固定对照与实际验收

证据目录 `/private/tmp/openrust-v03-20261120/`，独立源码由 baseline.txt 的提交建立，仅叠加本批 14 份源码／测试／正文；[SHA 清单](expression-contracts-sha256.json)逐份核对一致。同期 Passport／Eloquent 及 Cargo.lock 改动未进入快照，不将本批验收扩展到这些后续工作。

参考 `vendor/openrust/framework/tests/reference/blade_unpacking.php`，使用既有 Composer 锁定夹具及镜像 `composer@sha256:5248900ab8b5f7f880c2d62180e40960cd87f60149ec9a1abfd62ac72a02577c`，`--network none`。脚本核对 PHP 8.4.14 与全部 Illuminate v13.40.0；96 例最终连续两次逐字节一致，56 成功、40 标量展开／索引溢出错误。覆盖顺序、覆盖位置、数字键、字符串键、负索引、嵌套、偏移、JS、组件、include、CSS 与短路。Laravel 错误类被视图包装，Rust 断言具体 RenderFailed 原因而非接受任意错误。

初轮 Rust 对照发现 include 仍走旧具名参数解析而编译失败，修复后全部匹配；初轮日志保留在 tests-isolated.log。PHP 字面量标量展开可能导致不可捕获的编译 fatal，夹具用变量输入验证可捕获的运行期失败。没有将这种 PHP 与 Rust 编译阶段差异隐藏为等价错误。

| 验收 | 实际结果 |
| --- | --- |
| Rust 1.94.0 定向测试 | blade_expression_contracts、view_native_values、blade_arithmetic、blade_statements、view_finder 共 27 个不同函数通过，tests-final.log；补强生命周期计数后 5 个表达式测试再次通过，tests-lifecycle-final.log。 |
| 根应用 HTTP／Artisan | root-build.log、root-http.log：真实 main 验证展开、include、PHP 语法失败、JSON／HTML 500、恢复、HEAD、健康、view:cache／clear、inspire 及 ./artisan；SIGINT 退出 0、端口释放。 |
| 应用隔离 | 实际双 TCP 应用依次验证 INF／NAN／负零展开、渲染500／编译500后有限数恢复、再展开及各自共享数据；关停后端口重新绑定。 |
| 严格 Clippy | cargo +1.94.0 clippy --offline --locked --workspace --all-targets --features pail,passport -- -D warnings 通过，clippy.log／clippy-final.log。 |
| 格式 | cargo +1.94.0 fmt --all --check 通过，fmt-final.log。 |
| 三语文档 | npm run build 通过，docs-build.log；check_browser.mjs 的 12 个根路径／子目录／三语／桌面移动组合，check_view_contracts.mjs 的 6 个表达式锚点与 View 互链组合均通过，docs-browser.log／docs-view.log。正文展开示例实际渲染为 red。 |

本批不触及数据库、邮件或分页消费者，不重复 V02 的 MySQL／邮件验收。Windows、Linux 目标和其他发布门禁仍由各自条目验收。未运行无筛选整工作区测试。

## 收尾

本地按本批范围提交，不推送。源码、夹具、日志和隔离源码保留；仅回收已闲置的本批 target、Playwright 安装及文档 dist，保留共享依赖缓存与数据库。实际回收读数见下方补记。

本批回收前可用 56,258,416 KiB，回收后 64,767,832 KiB（66.32 GB／61.77 GiB），实测增加 8,509,416 KiB。此读数包含同时进行的其他工作影响。远程仅移除本批 blade_unpacking_v03.php，既有 Composer 夹具、镜像和缓存保留。
