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

## Pail 会话捕获

使用 `cargo build --features pail` 启用可选的 `openrust-pail` 包，现有包发现会装配 Provider。它将任意日志驱动接受的消息复制到当前应用的 `storage/pail/*.pail` 活跃会话。没有会话文件时不写捕获文件。实际命令和筛选接口见[下文](#tailing-log-messages-using-pail)。

包公开 `File`、`Files`、`Handler` 和 `PailServiceProvider`。`File::create()` 创建会话，保留已有文件内容；`destroy()` 删除会话。跨进程文件锁保护 JSON 行写入，保留级别和结构化异常。超过一小时未写入的会话会被清除。日志不会重建已删除的会话，文件错误沿原日志结果传播。

`context.__pail.origin` 包含 HTTP 方法与路径、命令名或队列与任务来源。公开 Context 覆盖消息 context，隐藏数据不进入捕获。Rust 以绑定应用的异步作用域隔离同进程并发请求、嵌套命令及并发任务；没有日志监听器时跳过来源收集。

HTTP 身份读取当前 guard 已解析的用户，后续切换 guard 也会更新。异步认证尚未解析时身份为 null。PHP Pail 可以在同步日志调用中解析认证，这项差异单独跟踪；本节不代表 Pail 命令和认证行为已经全部对齐。

### 读取与筛选捕获记录

`openrust_pail::value_objects::MessageLogged::fromJson` 读取一行捕获数据，可获取消息、级别、日期／时间、异常类与文件、trace，以及类型化的 HTTP、命令或队列来源。解析、日期转换和来源解码的失败通过 `Result` 返回。`context()` 排除保留字段 `__pail`、`exception` 和 `userId`。

`Options::accepts` 要求所有已设置条件同时匹配。级别比较和子串搜索使用 ASCII 大小写折叠。message 条件只搜索消息文本；通用 filter 搜索包含来源和 context 的四字段 JSON 表示。该表示跟随 PHP 的 Unicode 转义、对象顺序和浮点格式，因此通用 Unicode 搜索可能需要使用 `\u` 表示。认证 ID 按字符串精确比较，`12` 与 `012` 不同。`fromCommand(&Input)` 保留 auth 优先于 user 的规则，包括显式空值，以及 PHP 的 timeout 转换；Rust 的命令输入对象与命令实现分开传入。

```rust
use openrust_pail::{Options, value_objects::MessageLogged};

fn select(line: &str) -> Result<bool, serde_json::Error> {
    let message = MessageLogged::fromJson(line)?;
    let options = Options::new(3600, None, Some("ERROR".into()), None, Some("payment".into()));
    Ok(options.accepts(&message))
}
```

以上解析与筛选接口也用于[实时 Pail 命令](#tailing-log-messages-using-pail)。认证自动解析保留上文所述限制。

### Provider 启用条件

禁用捕获时仍注册 Files 和 Handler 绑定。启动时，若容器环境名精确为 `testing`，或 `VAPOR_SSM_PATH` 为非空且非 `0` 的字符串，就跳过捕获监听。`PAIL_TESTS=true` 用于 Pail 自身验收，可以覆盖这两个条件。该旗标按 Laravel Env 规则处理 false、empty、null 和引号值；Vapor 标记按原始 PHP 字符串真假判断。

旗标来自当前应用不可变的环境快照，进程变量优先于选中的环境文件。配置缓存生效时不加载环境文件，因此文件中的 `PAIL_TESTS` 不生效。Handler 在第一条被接受的日志或队列生命周期事件到来时才解析；构造失败在该时机传播，原日志驱动已接受的写入仍保留。

Pail 通过共享路径服务解析 `Application::storagePath("pail")`。在首次捕获前设置 `useStoragePath` 或 `APP_STORAGE_PATH`，参见[应用存储路径](../getting-started/configuration.md#application-storage-path)。

默认文件日志渠道和应急回退使用应用存储路径。应在加载配置或解析渠道前配置路径；已加载的渠道路径保留其配置值，参见[应用存储路径](../getting-started/configuration.md#application-storage-path)。

## Pail CLI 打印器 {#pail-cli-printer}

可选 Pail 包提供 `openrust_pail::contracts::Printer` 和 `printers::CliPrinter`。在 Artisan 命令中构造 `CliPrinter::new(output, basePath)`，调用 `print(&messageLogged)?`。打印器读取当前输出的详细级别与颜色状态。普通模式显示时间并缩略类型、文件和消息；`-v` 显示完整日期与正文，`-vv` 或 `-vvv` 再显示捕获的堆栈帧。

底部显示 HTTP 方法／路径和认证信息，或队列／任务／命令，以及公开 Context。数字键与 null、PHP 风格值导出、HTML 文本和多行消息按固定 Pail 规则处理。边框宽度上限为 150 个终端列；原始消息和长底部信息可能像上游一样溢出。Rust 的 `::` 类型命名空间及 Windows 本机路径分隔符适配到缩略和相对来源显示。

Rust JSON 无符号整数保留原始精度；超过 PHP 有符号整数范围的值，其显示可能与 PHP 的浮点近似不同。Rust 直接生成消息框，通过 `Output::writeRaw` 写入已渲染文本。真实 Artisan 输出立即写出，捕获输出保留在缓冲区。`--ansi`、`--no-ansi`、`--quiet` 和 `--silent` 生效，写入错误沿调用链传播；格式错误在写入消息框前返回。输出详细级别随调用作用域保存和恢复。`PAIL_TESTS` 用于捕获测试，显示的日期、文件和堆栈保留实际记录值，不采用 PHP 测试夹具中的替换值。

[实时 Pail 命令](#tailing-log-messages-using-pail)使用此打印器，加载 Provider 后也会向 [Artisan dev](../digging-deeper/artisan.md#the-dev-command)添加默认 `logs` 进程。

## 使用 Pail 跟踪日志 {#tailing-log-messages-using-pail}

把可选包编入应用后运行实际 Artisan 命令：

```sh
cargo build --locked --features pail
./target/debug/main artisan pail --timeout=1
./target/debug/main artisan pail --timeout=0 --level=error --message=payment -vv
```

`pail` 跟随应用日志事件，不依赖日志渠道的存储后端。每次调用在当前应用存储路径的 `pail/` 目录创建独立会话，并发调用分别接收日志。`--filter`、`--message`、`--level`、`--auth` 和 `--user` 使用上文的筛选规则，`--auth` 优先于别名 `--user`。执行超时默认 3600 秒，零表示关闭超时；超时打印“Maximum execution time exceeded.”后成功退出。Ctrl+C 或 SIGTERM 停止跟随并删除本次会话。无效记录或输出错误令命令失败，同样清理会话；`--quiet` 和 `--silent` 抑制输出。

Rust 原生跟随文件，支持分段 UTF-8 行、截断和按名称重新打开轮换文件，无需 PHP、PCNTL 或 `tail` 子进程。初次读取已有内容遵循末十行规则。在检测到截断或替换时丢弃未完成行，避免拼接不同文件代际；完整 JSON 记录立即筛选并打印。Windows 使用框架原生取消路径，Pail Windows 实机验收仍列入发布矩阵。

Provider 通过所属应用的 ConsoleKernel 注册命令，沿用捕获的 testing／Vapor 门禁。Rust 没有 PHP 全局 SAPI 标志：仅启动 HTTP 不执行命令注册回调，独立和程序化 Artisan 使用同一回调。取消捕获调用会清理会话并恢复调用方输出状态。本实现不增加心跳：与上游一样，即使 `--timeout=0`，超过一小时没有写入的会话也可能被下一次日志生产清理。清理保留 `.gitignore`。加载 Provider 后也会向 [Artisan dev](../digging-deeper/artisan.md#the-dev-command)添加默认 `logs` 进程。

### 开发进程注册

以 `--features pail` 构建时，包发现加载 Provider，`./target/debug/main artisan dev:list --json` 会列出执行 `pail --timeout=0` 的 `logs`。用 `./target/debug/main artisan dev --inline` 启动，监督器复用同一二进制，不调用 Cargo，也不要求 PHP／PCNTL。未编译 Pail，或应用未加载其 Provider 时，没有默认 `logs` 条目。查询列表不会创建捕获会话。

注册按 Provider 身份判断，独立于捕获门禁。在 testing 或 Vapor 环境中，已加载的 Provider 仍提供默认 `logs`，但 `pail` 命令被门禁禁用，除非设置 PAIL_TESTS 例外。此类应用可用 `DevCommands::except(&["logs"])?` 排除它。原生实现可在 Windows 注册该进程，Pail Windows 实机验收仍列入发布矩阵。

每次解析进程列表都会检查 Provider 的加载状态，因此在 Pail 注册之前查询列表，不会导致后续永久漏掉 `logs`。
