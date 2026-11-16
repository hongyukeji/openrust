# 01-04 Syslog 渠道

固定 Laravel 以应用名生成标识，组合 facility 和级别，`formatter` 缺省用 Laravel 格式，值为 `default` 时保留 SyslogHandler 自身的正文格式。沿用 v0.x 的应用级渠道目录、级别过滤和紧急回退；v0.x 日志工厂无 syslog 传输。Goravel 作为应用级编译装配参考。 [Laravel LogManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/LogManager.php#L399) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Log/LogManagerTest.php#L449) · [Goravel 日志装配](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/log/application.go)

Rust 使用每渠道独立的本地 Unix 数据报发送端，保留应用标识及进程 ID。macOS 默认套接字为 `/var/run/syslog`，Linux 为 `/dev/log`。可显式指定 socket。发送使用非阻塞套接字，发送失败走现有紧急日志路径且下一条消息可再送。缺少 Unix 数据报的平台走紧急回退，Windows 事件日志接线另行跟踪。 [Monolog SyslogHandler](https://github.com/Seldaek/monolog/blob/3.10.0/src/Monolog/Handler/SyslogHandler.php)

隔离基线 `48af9dfe0`，macOS arm64、Rust 1.94.0。16 项定向回归通过。独立 Unix 接收端逐条核对 facility 字段、级别、标识、格式化器和低级别过滤。实际 HTTP 的两个应用分别使用独立套接字，实际 Artisan 子进程验证命令消息。无接收端、无效 facility 触发紧急回退，接收端恢复后同一渠道重新发送。系统应用根目录规范化可能增加 Unix 套接字路径长度，实测采用短路径。严格 Clippy、Rust 格式及实际 `main artisan inspire` 通过。

三语示例编译运行并完成真实数据报接收。VitePress 构建、12 项通用浏览器检查和 6 项 Syslog 锚点检查通过。 [英文](../../../../docs/en/the-basics/logging.md#syslog-channel) · [简体中文](../../../../docs/zh_CN/the-basics/logging.md#syslog-channel) · [繁體中文](../../../../docs/zh_TW/the-basics/logging.md#syslog-channel)

01-04 整单元仍跟踪固定 Laravel 的 JsonFormatter 异常上下文及 Windows syslog 对应行为。
