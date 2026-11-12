# 06-08 Artisan 测试生成

- 对照固定 Laravel 的 [Creating Tests](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/testing.md#creating-tests)、[TestMakeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/TestMakeCommand.php) 和 [生成器集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Generators/TestMakeCommandTest.php)。参考固定 [Goravel 测试生成器](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/console/test_make_command.go) 的子目录、覆盖选项和编译型测试套件。复核旧版 `v0.x`（`6d1a8192`）的命令和测试设施。旧版无同名生成器。复用 T-01 的测试应用与现有源码生成逻辑。
- 默认生成 `tests/feature/` 下的测试，`-u`／`--unit` 生成 `tests/unit/` 下的测试。支持嵌套 PascalCase 名称、可选 `.rs` 后缀、自定义模板及 `-f`／`--force`。保留测试套件入口 `main.rs`。命令生成与测试生成共用名称及模板读取规则。
- Rust 功能测试通过 `TestApp` 检查临时应用的健康路由，单元测试使用标准 `#[test]`。两类测试均由 Cargo 运行，生成的文件经显式模块声明接入。Laravel 的 Pest 与 PHPUnit 选项及匹配测试生成属于各自的工具链或后续接线。
- 基于 `42228b31` 隔离候选。Rust 1.94.0 锁定离线工作区串行 562 项通过，5 项既有夹具忽略。格式和全目标严格 Clippy 通过。真实 Artisan 子进程验证后缀归一化、并发同路径写入、两个应用目录隔离、保留已有文件、强制覆盖、自定义及损坏模板、非法名称和测试入口保护。
- 从三语正文抽取六条生成命令和六条 Cargo 命令。生成源码显式注册后，每种语言各运行一个功能测试和一个单元测试，生成后的测试目标也通过严格 Clippy。三语 VitePress 构建、12 项通用浏览器检查和 6 项定向检查通过。
- 使用主工作区的空生成器占位路径，原样纳入单元测试入口骨架。下一批在 `make:command` 中接配套测试生成。
