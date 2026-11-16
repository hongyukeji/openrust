# 01-04 配置型日志处理器与级别缓冲

固定 Laravel 的 `monolog` 渠道按 `level`、`with`、`handler_with` 合并处理器参数。OpenRust 从应用级编译工厂解析具名 `RecordHandler`，提供默认的文件及标准流处理器。回调收到结构化记录和格式化文本，并服从配置的过滤与栈传播。旧版有应用级具名日志工厂及共享处理器栈，未实现按级别触发的缓冲。本批沿用现有工厂、缓存、异常回退与组装方式。 [固定 Laravel LogManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/LogManager.php#L433) · [固定 Laravel 处理器文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#creating-monolog-handler-channels) · [固定 Goravel 日志应用](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/log/application.go)

`action_level` 累积记录，达到触发级别时按该批顺序交给底层处理器。底层仍执行自身的级别过滤。默认触发后直写，`stop_buffering: false` 每次触发后继续缓冲。缓冲阶段以 Rust 自有数据保留记录的时间、公开上下文和格式化器，写入发生在状态锁外。状态随已解析渠道持有，释放时丢弃尚未触发的记录。 [固定 Monolog 缓冲处理器](https://github.com/Seldaek/monolog/blob/3.10.0/src/Monolog/Handler/FingersCrossedHandler.php)

隔离基线 `e3df2a317`。Rust 1.94.0 通过 15 项日志、Context 和管理器属性回归。测试包括参数覆盖、配置级别与传播、工厂失败的紧急回退和重新解析、首次触发、循环触发和原始记录上下文。两个应用的真实 TCP HTTP 并发写入各自的缓冲文件。真实 Artisan 子进程将触发前记录和触发记录写到标准错误。格式检查、框架库与日志测试严格 Clippy、实际 `main artisan inspire` 通过。

三语示例分别构建运行并检查两条实际落盘记录。VitePress 构建及 18 项浏览器检查通过，包含根路径与 `/docs/` 下三语处理器章节的锚点。 [英文](../../../../docs/en/the-basics/logging.md#handler-channels) · [简体中文](../../../../docs/zh_CN/the-basics/logging.md#handler-channels) · [繁體中文](../../../../docs/zh_TW/the-basics/logging.md#handler-channels)

01-04 仍开放，继续处理固定基线的 syslog 格式化和 JSON 异常上下文。
