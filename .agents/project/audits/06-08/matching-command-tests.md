# 06-08 Artisan 命令配套测试

- 对照固定 Laravel [CreatesMatchingTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Concerns/CreatesMatchingTest.php)、[GeneratorCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/GeneratorCommand.php) 和 [Generating Commands](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md#generating-commands)。固定 [Goravel MakeCommand](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/console/console/make_command.go) 提供编译型命令接线参考。复核旧版 `v0.x`（`6d1a8192`）的命令帮助和测试目录，未发现配套生成器。
- `make:command --test` 先写命令，再通过既有 `make:test` 定义与实现生成默认功能测试。配套名称按应用目录相对路径构造，保留子目录并追加 `Test`。`--force` 转交配套生成器。
- Rust 采用同一进程内的生成器及显式模块装配。配套测试失败时返回状态 1，保留已生成命令并给出单独补建命令。已存在文件遵循各生成器的覆盖契约。PHP 专用 Pest 和 PHPUnit 选项不映射到 Cargo。
- 基于 `bae3ff46` 的隔离候选。Rust 1.94.0 锁定离线工作区串行 563 项通过，5 项既有夹具忽略，格式及全目标严格 Clippy 通过。实际 Artisan 验证嵌套名称和 `.rs` 后缀、损坏模板后的单独重试、命令及配套测试重复保护和强制覆盖。
- 实际执行三语正文的生成示例，生成的两份源码显式接入后，Cargo 实际运行配套测试，Artisan 实际运行 `app:send-report`。三语站点构建、12 项通用浏览器检查和 6 项小节及跨页导航检查通过。
