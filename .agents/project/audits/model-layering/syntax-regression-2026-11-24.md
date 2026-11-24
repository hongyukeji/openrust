# 生成产物回归的 Rust 语法与隐藏字段定位

2026-11-24。修复[生成产物回归](generated-artifacts-2026-11-24.md)后续只读复核发现的两处测试缺口：正则分词误读字符／嵌套注释，以及双因素字段只按全文件片段匹配而未限定 hidden 声明位置。只修改测试与记录，保留模型、安装器和框架公开契约，不自动重构用户代码。

## 实现边界

- 新增 framework 集成测试 `model_layering_parser`，复用该 crate 已有的 `proc_macro2 1.0.107`／`syn 2.0.119` 及 span 支持，没有新增运行期依赖或修改 Cargo 清单。Python 验收通过测试二进制的私有 JSON 协议取得真实 Rust tokens；普通字符串、字符、字节字符、原始字符串和生命周期由 Rust 词法器处理，嵌套块注释不再通过正则剥离。无效语法拒绝继续检查。
- User 元数据检查使用 syn AST：限定 `impl Model for User` 的 `metadata`，追踪其实际返回的静态 `ModelMetadata`，识别构造链中最后生效的 `hidden(&[...])`。其他函数、其他模型、未返回静态项、fillable 和被后续 hidden 覆盖的声明不作为有效隐藏项。
- 安装差异只规范化上述 hidden 数组中的两个约定敏感字段；其他代码继续逐 token 比较。能力声明、Clone、原有用户方法、字符串值及旧存储定制的保留约束继续生效。解析器只检查输出，不改写任何应用源码；它针对当前支持的骨架，不是任意业务代码的语义证明器。
- 生成应用的共享测试服务设置非空双因素 secret／recovery 哨兵值，检查 `get_hidden()`、`to_array()` 和序列化内容。另生成刻意把字段放入 fillable 的映射模型作为反例，验证检查失败且两个非空值确实可被序列化；反例只存在于隔离应用，模型文件没有加入业务流程或数据库访问。

## 验收范围

- 正反契约：默认骨架／模型 stub／Fortify 发布路径；字符及转义字符后的真实数据库调用；嵌套注释内外代码；生命周期、普通／原始字符串；安装白名单与旧用户代码保留；正确元数据及错位／未生效声明。
- 原生 Rust 解析器测试及严格 Clippy。
- 重跑默认应用、make:model、自定义 stub、已有模型定制、Sanctum／Passport／Fortify 安装前后与重复安装；生成应用的真实 HTTP 和 Artisan 消费者均验证非空敏感字段的正反序列化行为，继续核对实际数据库写入、验证状态、回读、删除及迁移回滚。

## 本轮实际执行

基线 `e03be8accee84131133a1eecac7b8609d23c834c` 加本批四个测试文件，在 `target/tmp/model-layering-syntax-20261124/source` 隔离执行，未带入其他协作者的在途实现。复用 `tests/build_guard.py` 固定 Rust 1.94.0、项目内 TMPDIR 与工作流独占 Cargo target，保持正常增量和并发。

```sh
python3 -B tests/build_guard.py --workflow model-layering-syntax-20261124 -- python3 -B target/tmp/model-layering-syntax-20261124/source/tests/generated_model_layering_contract_test.py
# 以下 Cargo 命令在同一 guard 环境的 source 中依次运行
cargo test --locked -p openrust-framework --test model_layering_parser
cargo fmt --all --check
cargo clippy --locked -p openrust-framework --test model_layering_parser -- -D warnings
# 同一 guard 环境调用独占 MySQL 入口
python3 -B tests/mysql_acceptance.py -- python3 -B target/tmp/model-layering-syntax-20261124/source/tests/generated_model_layering.py
```

- 11项 Python 契约单测通过；原生解析器2项 Rust 测试通过，无失败或忽略。源码格式检查与解析器严格 Clippy 通过。Python 只是测试编排和断言消费者，Rust 语法由真实 proc_macro2／syn 解析。
- 本轮重新执行独占 MySQL 8.4.13 生成应用流程。默认 feature 下真实 `/up`、`/` HTTP 通过；普通、字段／具名连接、自定义 stub 的 make:model 及已有文件拒绝覆盖通过。Sanctum／Passport／Fortify 首次和重复安装的差异白名单、User 定制、自定义 stub、其他模型及 Action 定制保留均通过。
- 真实 Artisan 执行迁移、迁移状态、路由查询和 `layering:probe`；与 TCP HTTP `/layering-probe` 共同核对实际 User 创建、邮箱验证状态、回读和删除。两种消费者的 JSON 均返回 `sensitive_hidden: true`、`misplaced_hidden_rejected: true`，非空双因素字段正例隐藏、错放 fillable 的反例暴露值并被检查器拒绝；匿名 `/api/user` 返回401。
- 生成应用 `cargo fmt --all` 后的独立 `cargo fmt --all --check`、`cargo clippy --locked --features sanctum,passport,fortify --bin main -- -D warnings` 通过；包迁移回滚通过。

不复用上轮7项单测作为本轮结果，不将编排命令数累加为 Rust 测试数。本轮未新增 SQLite／PostgreSQL、完整认证或 Passkeys 协议的验收结论。

## 收尾

开始时可用空间低于项目32 GB停线，已通过 guard 核对占用并取得锁后安全清理本项目编译产物，释放67,201,376,256字节，清理后可用96,560,148,480字节；源码、日志、依赖下载、数据库及数据卷保留。详细验收日志位于本工作流 `contract-tests.log`、`parser-tests.log`、`fmt.log`、`parser-clippy.log` 和 `generated-layering-*.log`。收尾范围仅本批隔离源码、生成应用、临时脚本及运行临时目录，保留日志与可复用编译缓存。只提交本批测试及验收记录，不推送，不改变公开契约或自动改写用户代码。
