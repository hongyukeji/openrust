# 06-08 Artisan 命令生成

- 对照固定 Laravel 的 [Generating Commands](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md#generating-commands)、[ConsoleMakeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ConsoleMakeCommand.php)、[命令生成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Generators/ConsoleMakeCommandTest.php) 和 [通用生成器测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Console/GeneratorCommandTest.php)。参考固定 [Goravel MakeCommand](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/console/console/make_command.go) 的编译型注册及覆盖用例。检查旧版 `v0.x`（`6d1a8192`）的命令目录和帮助测试，未找到同名生成器。
- `make:command` 复用现有 `GeneratorCommand`、文件写入函数和 `Str::kebab`。支持子目录、可省略的 `.rs` 后缀、自定义命令签名、应用模板和 `-f`／`--force`。默认命令签名取类型的末段并加 `app:`。
- Rust 文件使用蛇形路径与具体 `Command` 实现。应用开发者声明模块并经现有 `withCommands` 接线后重新编译。无效名称和生成冲突采用现有 Artisan 失败退出码 1。Laravel 的配套测试生成选项依赖独立测试生成器，留待对应子单元。
- 在 `dffbb632` 的隔离候选上，Rust 1.94.0 锁定离线工作区串行 561 项通过、5 项既有夹具忽略，格式和严格 Clippy 通过。首次运行触发既有 SQLite 登录测试的随机密文子串断言，最终串行回归通过。
- 实际 Artisan 子进程验证并发同路径生成、扩展名归一化、文件保护、强制覆盖、命令签名覆盖、保留字和路径拒绝、自定义模板及损坏模板的错误退出。执行三语的六条生成命令，按文档接入应用，再用 Rust 1.94.0 通过源码 `./artisan` 实际运行两条生成命令、帮助和 `inspire`。
- 三语站点构建、12 项通用浏览器检查和 6 项生成小节定向检查通过。采用主工作区的空模板占位路径，其他在途文件保留。
