# Blade 标量算术

## 固定对照与旧版复用

承接 `eadfa8c57` 和 [存储消费者验收发现](storage-consumers.md)的 `{{ 1 + 2 }}` 缺口。正文对应 03-07 [Blade Templates / Displaying Data](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/blade.md#displaying-data)，标量转换复用 07-01 的基础能力。

固定 Framework 13.40.0 `1449bf241017087fbd38c118ecaabef3f6797fdc`：核对 CompilesEchos、CompilesConditionals、CompilesComponents、CompilesIncludes、CompilerEngine 和 [HandleExceptions::handleError / handleDeprecationError](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Bootstrap/HandleExceptions.php)。PHP 运算结果以实际 PHP 8.4.14 与全部固定为 13.40.0 的 Illuminate 包生成，不以 Rust 实现生成预期。

旧版 `../openrust-v0.x` `6d1a81924` 的 `vendor/openrust-framework/src/view.rs` 使用 MiniJinja，不具备可直接复用的 Blade 算术 AST。旧 `support/scalars.rs` 的 PHP 数值解析、浮点整数转换及精度 14 输出已移植到当前 input_scalars。本批复用该实现和边界意图，只扩大内部可见性，不引入第二份转换算法。入口、装配与 Goravel 参考关系保持原样，未改 main、bootstrap 或 Runner。

## 实现与适配

- AST 支持加、减、乘、除、取模、幂和正负号；按 PHP 优先级、左右结合、短路与严格标量类型求值。整数检查溢出并转为 f64；二进制、八进制和十六进制大字面量以 guard / sticky 位一次舍入。浮点缓存保存 bits，保留非有限数和负零；编译版本 29 → 30 自动使旧缓存失效。
- 对数值字符串采用 PHP 完整数值判定。整数字符串取模保留字符串封顶转换，与直接浮点值的环绕转换区别；非法数值、除零、缺失变量和键产生 ViewError 并走现有异常报告 / 响应。`??`、isset / empty 允许缺失，不能吞掉表达式自身的类型或索引溢出错误。
- 取模的精度损失和零底数负幂属于 deprecation，继续渲染；按当前应用 logging.deprecations 解析渠道，默认 null，testing 按快照变量 LOG_DEPRECATIONS_WHILE_TESTING 决定是否记录。日志解析或写入错误不改变渲染结果。trace 复用原生 logException 快照并按需捕获 Rust backtrace；无 trace 的位置指向 Rust 求值器，不伪造 PHP 编译文件 / 行号。
- 将求值错误传播到条件、循环、includes、属性、组件、授权和扩展回调。PHP 会先求值的 include 参数与 props / aware 默认表达式保持先求值；false / missing include 不会执行目标视图。
- 标量输出、运算、比较和真假判断保留 INF、NAN 和负零。当前公开 ViewData 仍为 JSON，非有限数跨入数组、子视图、组件和 JSON 扩展参数时报错。这是明确的视图数据边界，不等于完整 PHP zval 或任意 PHP 执行器。引用、展开、任意函数调用和 PHP 对象重载不在本批实现范围。
- 原回归夹具中的缺失普通变量改为显式 false / 空列表，隔离子视图用 coalesce 或显式 null 表达可选输入，必需命名插槽由调用方明确提供；无 iterable formatter 的数组输出按 PHP 错误处理。非法数组键 / 溢出测试移入独立 PHP 比对失败用例，成功用例继续覆盖缺失键的 coalesce 回退。

## 验收状态

Ubuntu 原生 amd64、Rust 1.94.0 Debug，在 `eadfa8c57` 的隔离源码快照中仅覆盖本批源码与测试。最终统一验收全部通过：

```sh
cargo test --locked -p openrust -p openrust-framework \
  --test blade_arithmetic --test view_finder \
  --test console --test feature --test artisan_entry
cargo build --locked --bin main
python3 tests/blade_arithmetic_http.py
cargo clippy --locked --workspace --all-targets --features pail -- -D warnings
cargo +1.94.0 fmt --all -- --check
npm --prefix docs run build
```

- 共 33 项测试函数通过：5 项算术专题、5 项既有视图、7 项根 Console、14 项 Feature、2 项 Artisan 入口。962 组 PHP 输出 / 错误逐项对照通过，同一语料再经真实 HTTP 验证 523 次成功与 439 次 JSON 500，补验普通 HTML 500 与恢复。
- 覆盖数值与非数值转换、溢出、结合 / 优先级、负零、INF / NAN、短路、数组错误、条件 / 循环、includes / 组件默认值、编译缓存失效、JSON 边界。10 个并存应用验证弃用渠道隔离、保留值规范化的 testing 门禁、原生 backtrace / 来源、默认 null 及无效日志渠道不中断请求。
- 既有布局、组件、迭代器、授权、视图服务与并发 HTTP 回归通过；旧的缺失输入夹具修正为显式可选或必需输入，未静默吞掉求值错误。
- 实际默认根二进制验证 `7|9|512|INF|-0`、HEAD、/up、浮点取模与零底负幂继续输出并记录弃用、除零 / 缺失变量 / 非数值 / JSON 边界的 JSON 和 HTML 500；修复视图后恢复 200。
- 实际 `view:cache`、`view:clear`、`inspire`、原始 `./artisan inspire`、SIGINT 退出 0 和端口重新绑定通过。脚本只修改其临时应用目录，没有覆盖项目欢迎页或路由。
- 最终全工作区、全目标并启用 Pail 的严格 Clippy 退出 0；全仓格式、差异检查及 Python AST 检查通过。
- 三语正文、站点构建、12 项通用浏览器矩阵与 6 项算术章节 / 页面宽度检查通过。正文的整数输出与订单条件示例纳入 PHP 语料 / HTTP 验收。
- 16 份源码 / 测试 / 参考文件与隔离副本 SHA-256 一致；Cargo.toml / Cargo.lock、main、bootstrap、config、routes 共 32 份基线文件与主工作区逐字节一致。
- 本批不改数据库、缓存驱动或 Worker，没有运行数据库验收，不将历史 MySQL 结果算作本批验收。跨平台完整矩阵仍按发布前计划执行。

参考脚本在真实 Illuminate View Factory / CompilerEngine 上执行。仅为匿名组件命名空间提供 getNamespace 的最小容器对象；错误处理器复现固定 HandleExceptions 的错误 / 弃用分流，没有运行 PHP 完整 HTTP 应用。脚本逐例记录成功输出或捕获的异常，不能把基础设施异常当作成功预期。常量非法数组键可导致 PHP 编译期致命退出，所以非法键和溢出采用变量输入验证运行时失败。包含 views、版本和 962 例（523 项成功输出、439 项错误）结果的 JSON 固定在测试夹具中；PHP 仅用于参考生成，不进入产品运行时或 Cargo 依赖。

## 交付范围

本批仅将下列 24 个核定文件提交到本地 Git，不推送。Pail 认证自动解析、Windows 发布矩阵及视图 JSON / PHP 语言差异保持独立跟踪；核心 66/66 仅为既有验收范围。

## 最小文件范围

```text
tests/
├── console.rs
└── blade_arithmetic_http.py
vendor/openrust-framework/
├── src/support/input_scalars.rs
├── src/view/compilers/
│   ├── blade_compiler.rs
│   ├── template.rs
│   └── template/
│       ├── arithmetic.rs
│       ├── attributes.rs
│       ├── authorization.rs
│       ├── expressions.rs
│       ├── fonts.rs
│       └── includes.rs
└── tests/
    ├── blade_arithmetic.rs
    ├── view_finder.rs
    ├── fixtures/blade_arithmetic.json
    └── reference/
        ├── blade_arithmetic.php
        └── blade_arithmetic.composer.json
docs/{en,zh_CN,zh_TW}/the-basics/blade.md
.agents/project/
├── audits/07-01/{blade-arithmetic,storage-consumers}.md
├── audits/core-closure-ledger.md
├── current.md
└── progress.md
```

## 参考夹具复现

在独立临时目录中把 `tests/reference/blade_arithmetic.composer.json` 复制为 composer.json，安装固定依赖后，以 `BLADE_ORACLE_VENDOR=/绝对路径/vendor/autoload.php php tests/reference/blade_arithmetic.php` 执行。实际验收使用 composer:2.8.12（PHP 8.4.14）；所有 Illuminate 包版本由生成器核验为 v13.40.0。Composer 配置仅供历史参考夹具，不进入产品依赖。Rust 测试无需 PHP 或网络。

已使用同一固定环境连续两次生成，结果逐字节一致，SHA-256 `b3d4416f93fe78873cf19fe676e96650878b1016acb4c03fa770ca037f2c4dbb`。初始语料曾错误地将 deprecation 当异常、遗漏 bus / pipeline 精确版本及使用编译期不可捕获非法数组键；均修正后重新生成。条件语料用 HTML 标签界定指令，避免把不在本批范围的词边界和空白编译差异混入算术比较。

## 独立后续边界

- 原生 ViewData 的非有限数集合 / 参数边界见三语正文；不冒充完整 PHP zval。若后续扩展为原生 ViewValue，需单独设计兼容与序列化，不在当前算术中悄悄编码特殊字符串。
- PHP Blade 指令的词边界（如裸单词紧邻 @endif）及无参数指令后空白吞并规则尚未逐项收口。已记录为 03-07 后续编译器专题；本批不宣称原生编译器能执行任意 PHP 或所有 PHP Blade 文本形式。
- Pail 认证自动解析与 Windows 发布矩阵仍沿原记录跟踪。

## 资源与证据

已清理当批远程快照、编译、下载、Composer 依赖约 5.73 GB 和站点产物约 58.05 MB，另回收首次拉取且摘要匹配的临时 Composer 镜像。Docker 数据卷前后一致；保留共享 Rust 镜像及文档依赖。原始日志、参考 Composer 锁、SHA 清单、浏览器结果与提交映射保留在 `/private/tmp/openrust-blade-arithmetic-20261119`。项目磁盘可用 54.89 GB（51.12 GiB），周额度剩余 13%，第二窗口无读数。沿用用户忽略本轮额度限制的授权，磁盘停线继续生效；未启用 credits、重置额度或推送。


## 2026-11-19 指令空白后续

前述词边界／引擎空白差异由 [03-07 专项](../03-07/blade-statements.md)完成范围验收，包括片段缓冲区与完整视图回退；编译格式现为 31。原 962 组算术／HTTP 回归保留通过。全源预处理与式内注释、JSON 数据边界及 Pail／平台余项仍按专项记录推进，本条不改变此前历史验收的范围。
