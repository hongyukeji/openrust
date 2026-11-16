# 01-04 日志格式化器、处理器和 tap

与固定 Laravel 的 LogManager 对照，Rust 通过已编译、应用级的工厂注册公开配置名称。复用 v0.x 的具名工厂、缓存失效和失败重试模式。固定 Goravel 作为应用级创建渠道和合并日志栈的参考。 [Laravel LogManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/LogManager.php) · [Laravel 日志定制测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Log/LogManagerTest.php) · [Goravel 日志应用](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/log/application.go)

- `formatter` 与 `formatter_with` 适用于文件、轮转文件和标准错误。自定义格式化器返回完整记录，默认输出沿用原有行格式。一个栈的成员保留各自格式，自定义日志工厂管理其自身输出。
- `processors` 支持名称与带 `with` 的配置对象。处理器按顺序读写 `LogRecord`，上下文在发生修改时才复制。栈按成员顺序收集处理器，低于所有处理器对应输出阈值的记录不执行回调。
- `tap` 在命名渠道缓存前按列表顺序执行，支持名称及冒号分隔的逗号参数。用 `setFormatter` 替换格式化器，用 `pushProcessor` 前置处理。配置错误和运行时写入错误沿用现有紧急日志路径。

隔离基线 `c570cf91d`，Rust 1.94.0。13 项日志、Context 和管理器属性定向测试通过。真实 TCP HTTP 验证两个应用的并发落盘、独立目录、插值和处理器，Artisan 子进程通过 tap 格式化月文件。测试还覆盖栈内不同格式、处理器顺序、tap 覆盖、过滤时跳过处理器、显式默认格式和无效格式的紧急回退。格式检查、框架库和日志测试的严格 Clippy、实际 `main artisan inspire` 通过。

三语新增示例分别编译与执行，核对实际文件内容。VitePress 构建、12 项通用浏览器检查及 6 项定制章节锚点检查通过。 [英文](../../../../docs/en/the-basics/logging.md#customizing-channel-records) · [简体中文](../../../../docs/zh_CN/the-basics/logging.md#customizing-channel-records) · [繁體中文](../../../../docs/zh_TW/the-basics/logging.md#customizing-channel-records)

01-04 整单元仍跟踪配置型通用 handler（含参数覆盖）、`action_level`／`stop_buffering`、固定基线的 syslog 格式化及 JSON 异常上下文边界。
