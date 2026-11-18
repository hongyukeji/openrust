# 日志

## 配置

通过 `LOG_CHANNEL` 选择默认渠道，`LOG_STACK` 设置默认栈的成员顺序，`LOG_LEVEL` 设置最低级别。默认 `stack` 包含 `single`，向 `storage/logs/openrust.log` 追加记录。相对路径以应用根目录为基准。渠道可用 `name` 自定义记录标签，默认标签取应用环境。

当前驱动包括文件 `single`、标准错误 `stderr`、丢弃记录的 `null` 和组合渠道的 `stack`。记录达到最低级别时才打开文件。`single` 支持 `locking`、`bubble` 和整数 `permission`，如 Rust 的 `0o600`。权限设置在 Unix 生效。记录使用 UTC 时间。

## 构建日志栈

`channels` 支持列表或逗号分隔字符串。成员按顺序执行。`bubble: false` 的处理器接收记录后停止后续处理。`null` 丢弃达到阈值的记录并停止后续处理。设置 `ignore_exceptions: true` 后，组内尝试每个处理器并抑制写入错误。

`Log::stack(&["single", "stderr"], None)` 创建临时日志栈。命名渠道按应用惰性解析和缓存。更新命名渠道配置后，可用 `forgetChannel(Some(name))` 重新解析。

## 写入日志

在应用生命周期中调用 `Log::info("Order {id} created", &context)`。`Log::channel(Some("stderr"))?` 可选择命名渠道。上下文使用结构化 JSON，`replace_placeholders` 启用占位符替换。Rust 日志接口同步写入。

无效渠道回退到 `logging.channels.emergency.path`。写入失败时，紧急渠道写入诊断和原记录。紧急文件也无法写入时，改用标准错误。

```rust
use openrust::{config::Repository, log::{LogContext, LogManager}};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let logger = LogManager::new(Arc::new(Repository::new(Default::default())), root);
    let context = LogContext::from_iter([("id".into(), 42.into())]);
    logger.info("Order {id} created", &context)?;
    Ok(())
}
```

## 轮转文件渠道

使用 `daily` 或 `monthly` 生成如 `openrust-2026-11-11.log` 和 `openrust-2026-11.log` 的文件。每个 UTC 周期内，首条达到阈值的记录打开当期文件。`max_files` 包含当前文件，设为 0 时不限保留数量。应用默认保留 14 个日文件和 3 个月文件。清理仅处理基本文件名、扩展名和周期格式均匹配的普通文件。

## 临时渠道与自定义渠道

`Log::build(&settings)?` 创建临时日志渠道。使用 `Log::stackChannels` 及 `StackChannel::Named`、`StackChannel::Logger` 可组合配置渠道和临时渠道。组合后的日志栈有自己的标签。

配置 `driver: "custom"` 时，将 `via` 指向通过 `LogManager::registerFactory` 登记的工厂。使用 `LogManager::extend` 可注册具名驱动。工厂是编译后的 Rust 闭包，接收管理器和渠道配置，返回实现日志契约的对象。首次并发解析时可能同时调用工厂，初始化应可安全重复。成功解析的具名渠道共用发布后的实例。工厂失败时使用紧急日志，后续调用可重试。

```rust
use openrust::{config::Repository, log::{LogContext, LogManager, StackChannel}};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let manager = LogManager::new(Arc::new(Repository::new(Default::default())), root);
    manager.registerFactory("audit", |manager, config| {
        let logger = manager.build(&json!({"driver":"monthly", "path":config["path"]}))?;
        Ok(logger)
    });
    let audit = manager.build(&json!({"driver":"custom", "via":"audit", "path":"audit.log"}))?;
    let stack = manager.stackChannels(
        &[StackChannel::Named("single"), StackChannel::Logger(&audit)],
        Some("orders"),
    )?;
    stack.info("Audit ready", &LogContext::new())?;
    Ok(())
}
```

## 定制日志记录 {#customizing-channel-records}

在服务 Provider 中用 `LogManager::registerFormatter`、`registerProcessor` 和 `registerTap` 注册名称。通过 `formatter` 选择格式化器，用 `formatter_with` 传入选项。将 `formatter` 设为 `"default"` 使用内置格式。文件、轮转文件及标准错误处理器按格式化器返回的字节写入，记录分隔符也由格式化器提供。自定义日志工厂管理自己的输出。

`processors` 的成员可以是名称，也可以是带 `processor` 和 `with` 的对象。处理器按配置顺序在格式化前执行，可通过 `LogRecord` 修改消息、记录上下文和公开作用域元数据。日志栈按成员顺序合并处理器，各成员沿用自己的格式化器。

`"audit:orders,prod"` 这样的 tap 会收到日志器及两个参数。tap 在缓存渠道前按列表顺序执行。用 `setFormatter` 替换渠道格式化器，用 `pushProcessor` 在队首插入处理器。

```rust
use openrust::{
    config::Repository,
    log::{LogContext, LogManager},
};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = LogManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    manager.registerFormatter("message", |_| {
        Ok(Arc::new(|record| Ok(format!("{}\n", record.message()))))
    });
    let channel = manager.build(&json!({
        "driver": "single",
        "path": "storage/logs/audit.log",
        "formatter": "message",
    }))?;
    channel.info("Order recorded", &LogContext::new())?;
    Ok(())
}
```

## 处理器渠道 {#handler-channels}

通过 `driver: "monolog"` 指定具名 Rust 处理器。内置 `stream` 处理器接受文件路径、`stderr` 或 `stdout`。用 `LogManager::registerHandler` 注册其他处理器。工厂收到合并后的设置，返回接收结构化记录和格式化文本的回调。设置按渠道 `level`、`with`、`handler_with` 的顺序合并，后者覆盖前者。

设置 `action_level` 后，渠道累积记录，达到指定级别时写出整批。底层处理器仍按自己的最低 `level` 逐条过滤。第一次写出后，后续记录直接进入底层处理器。设置 `stop_buffering: false` 可在每次写出后开始累积下一批。缓冲状态属于解析后的渠道，渠道释放时丢弃尚未触发的批次。

```rust
use openrust::{config::Repository, log::{LogContext, LogManager}};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = LogManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    let audit = manager.build(&json!({
        "driver": "monolog",
        "handler": "stream",
        "with": {"stream": "storage/logs/audit.log"},
        "handler_with": {"level": "info"},
        "action_level": "error",
        "stop_buffering": false,
    }))?;
    audit.info("Import started", &LogContext::new())?;
    audit.error("Import failed", &LogContext::new())?;
    Ok(())
}
```

## Syslog 渠道 {#syslog-channel}

设置 `driver: "syslog"` 后，将记录发送到本机系统日志。在 Unix 上，通过本地数据报套接字发送。macOS 默认使用 `/var/run/syslog`，Linux 默认使用 `/dev/log`。可通过 `socket` 指定接收端，相对路径从应用根目录解析。发送失败写入紧急日志，同一渠道在下一条记录到达时可重新发送。

`facility` 默认是 `user`，可填写 `local4` 等名称或对应的 syslog 数值常量。数据报优先级由 facility 和记录级别组成，标签取自应用名与进程 ID。省略格式化器时，消息正文采用框架行格式。设置 `formatter: "default"` 可省略正文中的时间戳，也可以指定已注册的格式化器。缺少 Unix 数据报支持的平台使用紧急日志。

```rust
use openrust::{config::Repository, log::{LogContext, LogManager}};
use serde_json::json;
use std::{os::unix::net::UnixDatagram, sync::Arc, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let receiver = UnixDatagram::bind("audit.sock")?;
    receiver.set_read_timeout(Some(Duration::from_secs(2)))?;
    let manager = LogManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    let audit = manager.build(&json!({
        "driver": "syslog",
        "socket": "audit.sock",
        "facility": "local4",
        "formatter": "default",
    }))?;
    audit.notice("Import complete", &LogContext::new())?;
    let mut bytes = [0; 1024];
    let received = receiver.recv(&mut bytes)?;
    let packet = std::str::from_utf8(&bytes[..received])?;
    assert!(packet.starts_with("<165>"));
    assert!(packet.ends_with("Import complete"));
    Ok(())
}
```
在 Windows 上，渠道写入 Application 事件日志，Provider 名取自应用名。事件详细信息中的两个插入字符串分别保存带进程 ID 的应用标识和格式化消息，类别保存 syslog 级别编号。与 PHP 的 Windows 接线一致，`alert` 映射为 Error，`info` 映射为 Information，其余级别映射为 Warning。Windows 会校验 `facility`，事件本身没有 facility 字段。`socket` 仅适用于 Unix，Windows 配置应省略该项。

在 PowerShell 中将 `my-app` 换成从应用名派生的 Provider 名，即可读取原始消息。

```powershell
$filter = "*[System[Provider[@Name='my-app']]]"
Get-WinEvent -LogName Application -FilterXPath $filter -MaxEvents 1 |
    ForEach-Object { $_.Properties[1].Value }
```


## JSON 异常日志 {#json-exception-logs}

设置 `formatter: "json"` 输出结构化记录。可通过 `formatter_with` 设置 `append_newline`、`ignore_empty_context_and_extra` 和 `max_normalize_depth`（默认 9）。

使用 `Log::logException(level, message, &error, &context)` 将错误写入 `context.exception`，来源链记录在 `previous` 中。在 `withExceptions` 中注册 `exceptionContext::<E>`，为匹配的错误类型添加字段，来源错误也适用。现有 `context` 回调会补充直接记录的根错误。应用通过报告机制记录时，根错误的字段只出现在记录的上下文中，来源错误的字段仍留在对应节点。Rust 会保留直接传入和已注册错误的类型名，类型被擦除的来源可能显示为 `Error`。标准错误接口不提供原始文件或调用栈。

```rust
use openrust::{config::Repository, log::{Level, LogContext, LogManager}};
use serde_json::json;
use std::{io, sync::Arc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = LogManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    let logger = manager.build(&json!({
        "driver": "single",
        "path": "storage/logs/errors.jsonl",
        "formatter": "json",
    }))?;
    let error = io::Error::other("storage offline");
    logger.logException(Level::Error, "Import failed", &error, &LogContext::new())?;
    Ok(())
}
```

## 上下文信息

[Context Facade](../digging-deeper/context.md) 捕获的公开字段以独立 JSON 元数据写在单条日志字段之后。自定义写入端可以实现 `logWithContext` 接收两组字段。隐藏字段保存在上下文仓库中。

## 监听日志消息

在服务提供者的 `boot` 方法中注册 `Log::listen`。渠道接受并写入记录后，同步执行监听闭包，闭包返回 `LogResult<()>`。`LogManager::listen` 和 `Logger::listen` 共享当前应用的监听器，覆盖命名渠道、日志栈、临时渠道和重命名 Logger。

`MessageLogged` 包含原消息、级别和 context。占位符插值及渠道处理器作用于写出的记录。`logException` 还提供规范化的 `context["exception"]`。低于所有处理器阈值的记录跳过通知。应急回退记录和诊断也会通知监听器。

闭包按注册顺序执行。回调错误在日志写入后向调用者传播，并停止后续回调。闭包可以再次写日志或注册监听器，新增注册在下一次通知生效。同步日志监听在调用者的应用作用域内执行，通用异步事件分发采用显式调用。

```rust
use openrust::{log::LogError, support::facades::Log};

fn register_log_listener() -> Result<(), LogError> {
    Log::listen(|event| {
        eprintln!("{}", event.message);
        Ok(())
    })
}
```
