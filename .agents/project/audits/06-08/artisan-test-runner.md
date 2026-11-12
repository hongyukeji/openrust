# 06-08 Artisan 测试运行入口

- 对照固定 Laravel 文档 [Running Tests](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/testing.md#running-tests) 与 [应用骨架依赖](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/composer.json)。核对骨架依赖约束对应的 [Collision v8.6.0 TestCommand](https://github.com/nunomaduro/collision/blob/v8.6.0/src/Adapters/Laravel/Commands/TestCommand.php) 的子进程、流式输出、参数和状态。固定 Goravel 的框架树提供测试生成器，实际测试使用编译语言的测试工具。旧版 `v0.x`（`6d1a8192`）未发现对应 Artisan 运行器。
- 复用 07-03 进程 API。将原生参数交给应用根目录中的 Cargo，不经过 shell。使用 `CARGO` 或 PATH 上的 `cargo`，继承环境，无框架默认超时。Artisan 终端模式传递 stdout、stderr 和退出码，程序化输出继续捕获。终端 Ctrl-C 返回 130 并通过既有进程组生命周期清理。
- Rust 的测试选项、编译并行与测试线程交给 Cargo 和标准测试运行器。PHPUnit／Pest 专用覆盖率、分析器、TTY 和并行数据库选项仍属于各自测试运行器的契约。初始 `--` 可消除 Cargo 与 Artisan 同名选项的歧义。
- 基于 `469046df` 的隔离候选，Rust 1.94.0 锁定离线工作区 564 项通过，5 项既有夹具忽略，格式及严格 Clippy 通过。应用实际子进程覆盖帮助不构建、Cargo 选项、两层参数分隔、stdout／stderr、静默和原始失败状态 101。真实阻塞测试验证输出在退出前可读、SIGINT 返回 130 且测试进程已终止。
- 首次全量复验把串行设置放在环境变量，gRPC 子进程的串行 libtest 前缀干扰就绪识别。改为父测试运行器参数后通过。既有夹具源码保留。
- 实际执行三语源码 `./artisan test` 示例，每次精确运行 1 项。三语站点构建、12 项通用浏览器检查及 6 项运行测试小节检查通过。
