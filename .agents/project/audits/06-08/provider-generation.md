# 06-08 Artisan 服务提供者生成

- 对照固定 Laravel [Registering Providers](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/providers.md#registering-providers)、[ProviderMakeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ProviderMakeCommand.php) 和 [生成器测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Generators/ProviderMakeCommandTest.php)。固定 [Goravel ProviderMakeCommand](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/console/provider_make_command.go) 提供编译型注册参考。复核旧版 `6d1a8192` 的 generated-provider fixture 及安装脚本，沿用先核对装配点、显式模块和 Provider 列表、再编译验证的做法。
- `make:provider` 支持嵌套 PascalCase 名称、`.rs`、`-f` 和可发布的 `provider.rs.stub`。生成到 `app/providers/`，声明模块并追加到 `bootstrap/providers.rs`。Rust 源码修改复用安装器的 Cargo 清单锁、上下文检查和原子单文件替换。
- 标准骨架可自动接线。无法安全定位的自定义装配在生成前返回错误及手动指引。重复生成保护已有源码，`--force` 只允许替换生成源码。已有源码仍与模板一致且登记缺失时可重试补齐。Provider 类型在编译后参与真实的 register 与 boot 生命周期。
- 基于 `1d855b1f` 的隔离候选。Rust 1.94.0 锁定离线工作区 567 项通过，5 项既有夹具忽略，格式及严格 Clippy 通过。实际 Artisan 覆盖嵌套生成、重复和并发保护、自定义 bootstrap 拒绝后重试、缺失注册补齐、格式化的注册保留、发布模板和强制覆盖。
- 三语命令各实际执行。生成的 Provider 按自动写入的模块和列表编译，实际 `artisan inspire` 与 `GET /up` 验证 `register` 在 `boot` 前执行，HTTP 返回 200。三语站点构建、12 项通用浏览器检查和 6 项生成小节定向检查通过。
