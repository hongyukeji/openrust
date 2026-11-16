# 01-04 JSON 异常上下文

对照固定 Laravel JsonFormatter 及异常测试。直接记录时把根错误上下文放入 `context.exception`，通过报告链路时根上下文只进入记录的 `context`。每个 source 以 `previous` 表达，保留通过 `exceptionContext::<E>` 注册的自身字段，结构字段具有优先权。Rust 把结构化快照与格式化分离，因此可跨栈和触发缓冲传递。普通文本日志继续使用原有上下文字段。旧版无 JSON 异常规范化器，本批复用已在应用级装配的日志管理器和异常报告契约，沿用固定 Goravel 的编译式服务装配。 [固定 Laravel formatter](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/Formatters/JsonFormatter.php) · [固定 Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Log/JsonFormatterTest.php) · [Goravel 日志](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/log/application.go)

内置 `json` 支持空字段、换行及最大规范化深度设置。达到深度边界时保留该层的类名及消息并停止写入该层的附加上下文。Rust 稳定 `Error` 不提供原始异常文件、调用栈或通用代码字段，未知的擦除来源标记为 `Error`，显式传入和注册类型保留具体名称。

隔离基线 `b318216ca`，Rust 1.94.0。27 项日志、Context、容器属性、异常报告与渲染定向回归通过。真实 HTTP 验证直接记录、报告去重、来源独立上下文、有限深度及隐藏字段不进入日志。Artisan 子进程写出结构化异常记录。验证无处理器时的直接记录和缓冲后写入。框架库及日志测试严格 Clippy、Rust 格式和实际 `main artisan inspire` 通过。

三语示例编译执行并解析实际 JSONL 文件。VitePress 构建、12 项通用浏览器检查和 6 项 JSON 小节锚点检查通过。 [英文](../../../../docs/en/the-basics/logging.md#json-exception-logs) · [简体中文](../../../../docs/zh_CN/the-basics/logging.md#json-exception-logs) · [繁體中文](../../../../docs/zh_TW/the-basics/logging.md#json-exception-logs)

01-04 的 Windows syslog 对应行为继续跟踪。
