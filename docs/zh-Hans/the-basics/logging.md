# 日志

Slack 渠道的默认显示名称为应用名称加 ` Log`，例如 `ExampleStore Log`。可以在 `.env` 中设置 `LOG_SLACK_USERNAME` 覆盖。

## 配置

通过 `LOG_CHANNEL` 选择默认渠道，`LOG_STACK` 设置默认栈的成员顺序，`LOG_LEVEL` 设置最低级别。默认 `stack` 包含 `single`，向 `storage/logs/app.log` 追加记录。相对路径以应用根目录为基准。渠道可用 `name` 自定义记录标签，默认标签取应用环境。

支持 `single`、`daily`、`monthly`、`stderr`、`syslog`、`errorlog`、`slack`、`papertrail`、`null`、`stack`、`monolog` 以及注册的 `custom` 工厂。记录达到最低级别时才打开文件。`single` 支持 `locking`、`bubble` 和整数 `permission`，如 Rust 的 `0o600`。权限设置在 Unix 生效。记录使用 UTC 时间。

## 记录弃用警告 {#logging-deprecation-warnings}

`LOG_DEPRECATIONS_CHANNEL` 选择受支持的运行期兼容警告渠道，默认 `null` 丢弃记录。`LOG_DEPRECATIONS_TRACE=true` 附带位置和回溯，通过 `logging.deprecations` 配置。已定义的 `logging.channels.deprecations` 具名渠道始终优先于所选渠道。`APP_ENV=testing` 时默认跳过警告，`LOG_DEPRECATIONS_WHILE_TESTING=true` 可启用。日志失败不会使弃用警告变成失败响应。

这里处理模板求值中有损数值转换等运行期兼容警告。Rust 编译器诊断仍属于构建输出，不会全局拦截所有依赖库的警告。应用自身的运行期警告可显式写入配置的 deprecations 渠道。

## 构建日志栈

`channels` 支持列表或逗号分隔字符串。成员按顺序执行。`bubble: false` 的处理器接收记录后停止后续处理。`null` 丢弃达到阈值的记录并停止后续处理。设置 `ignore_exceptions: true` 后，组内尝试每个处理器并抑制写入错误。

`Log::stack(&["single", "stderr"], None)` 创建临时日志栈。命名渠道按应用惰性解析和缓存。更新命名渠道配置后，可用 `forget_channel(Some(name))` 重新解析。

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

使用 `daily` 或 `monthly` 生成如 `app-2026-11-11.log` 和 `app-2026-11.log` 的文件。每个 UTC 周期内，首条达到阈值的记录打开当期文件。`max_files` 包含当前文件，设为 0 时不限保留数量。应用默认保留 14 个日文件和 3 个月文件。清理仅处理基本文件名、扩展名和周期格式均匹配的普通文件。

## 临时渠道与自定义渠道

`Log::build(&settings)?` 创建临时日志渠道。使用 `Log::stack_channels` 及 `StackChannel::Named`、`StackChannel::Logger` 可组合配置渠道和临时渠道。组合后的日志栈有自己的标签。

配置 `driver: "custom"` 时，将 `via` 指向通过 `LogManager::register_factory` 登记的工厂。使用 `LogManager::extend` 可注册具名驱动。工厂是编译后的 Rust 闭包，接收管理器和渠道配置，返回实现日志契约的对象。首次并发解析时可能同时调用工厂，初始化应可安全重复。成功解析的具名渠道共用发布后的实例。工厂失败时使用紧急日志，后续调用可重试。

```rust
use openrust::{config::Repository, log::{LogContext, LogManager, StackChannel}};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let manager = LogManager::new(Arc::new(Repository::new(Default::default())), root);
    manager.register_factory("audit", |manager, config| {
        let logger = manager.build(&json!({"driver":"monthly", "path":config["path"]}))?;
        Ok(logger)
    });
    let audit = manager.build(&json!({"driver":"custom", "via":"audit", "path":"audit.log"}))?;
    let stack = manager.stack_channels(
        &[StackChannel::Named("single"), StackChannel::Logger(&audit)],
        Some("orders"),
    )?;
    stack.info("Audit ready", &LogContext::new())?;
    Ok(())
}
```

## 定制日志记录 {#customizing-channel-records}

在服务 Provider 中用 `LogManager::register_formatter`、`register_processor` 和 `register_tap` 注册名称。通过 `formatter` 选择格式化器，用 `formatter_with` 传入选项。将 `formatter` 设为 `"default"` 保留处理器自己的格式器；省略时采用渠道默认格式器。文件、轮转文件及标准错误处理器按格式化器返回的字节写入，记录分隔符也由格式化器提供。自定义日志工厂管理自己的输出。

`processors` 的成员可以是名称，也可以是带 `processor` 和 `with` 的对象。处理器按配置顺序在格式化前执行，可通过 `LogRecord` 修改消息、记录上下文和公开作用域元数据。日志栈按成员顺序合并处理器，各成员沿用自己的格式化器。

`"audit:orders,prod"` 这样的 tap 会收到日志器及两个参数。tap 在缓存渠道前按列表顺序执行。用 `set_formatter` 替换渠道格式化器，用 `push_processor` 在队首插入处理器。

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
    manager.register_formatter("message", |_| {
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

通过 `driver: "monolog"` 指定具名 Rust 处理器。内置 `stream` 处理器接受文件路径、`stderr` 或 `stdout`。用 `LogManager::register_handler` 注册其他处理器。工厂收到合并后的设置，返回接收结构化记录和格式化文本的回调。设置按渠道 `level`、`with`、`handler_with` 的顺序合并，后者覆盖前者。

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
    assert!(packet.ends_with("Import complete [] []"));
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

使用 `Log::log_exception(level, message, &error, &context)` 将错误写入 `context.exception`，来源链记录在 `previous` 中。在 `with_exceptions` 中注册 `exception_context::<E>`，为匹配的错误类型添加字段，来源错误也适用。现有 `context` 回调会补充直接记录的根错误。应用通过报告机制记录时，根错误的字段只出现在记录的上下文中，来源错误的字段仍留在对应节点。Rust 会保留直接传入和已注册错误的类型名，类型被擦除的来源可能显示为 `Error`。标准错误接口不提供原始文件或调用栈。

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
    logger.log_exception(Level::Error, "Import failed", &error, &LogContext::new())?;
    Ok(())
}
```

## 上下文信息

[Context Facade](../digging-deeper/context.md) 捕获的公开字段以独立 JSON 元数据写在单条日志字段之后。自定义写入端可以实现 `log_with_context` 接收两组字段。隐藏字段保存在上下文仓库中。

## 监听日志消息

在服务提供者的 `boot` 方法中注册 `Log::listen`。渠道接受并写入记录后，同步执行监听闭包，闭包返回 `LogResult<()>`。`LogManager::listen` 和 `Logger::listen` 共享当前应用的监听器，覆盖命名渠道、日志栈、临时渠道和重命名 Logger。

`MessageLogged` 包含原消息、级别和 context。占位符插值及渠道处理器作用于写出的记录。`log_exception` 还提供规范化的 `context["exception"]`。低于所有处理器阈值的记录跳过通知。应急回退记录和诊断也会通知监听器。

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

### 日志调用时的请求认证 {#request-log-authentication}

同步 `Log::info` 等既有方法捕获当前 guard 已解析的身份，未解析用户为 null。在异步 HTTP 处理器中，需要本次日志解析认证时，使用请求日志入口：

```rust
Log::log_with_request(request, openrust::log::Level::Info, "Order placed", &LogContext::new()).await?;
```

`LogManager` 和已解析的 `Logger` 渠道也提供同名方法。渠道先写日志，再按注册顺序通知监听器。存在活跃 Pail 会话时，Pail 等待当前选中 guard，在本次调用返回前捕获 ID 和邮箱。已缓存用户直接复用；登录、登出和 guard 切换影响后续日志。没有活跃会话、级别未被渠道接受或被抑制的弃用警告不会触发认证。内置 HTTP 异常报告也使用请求日志入口。

显式 await 对应 PHP 在同步日志监听器中解析 guard 的能力。容器回调等同步代码继续使用身份快照。`LogManager::listen_for_request` 注册同步监听器及其异步请求版本，普通 `listen` 回调仍同步运行。执行回调时不持有监听器注册表锁。guard 解析期间产生的日志读取已有快照，不再递归解析认证。

认证或监听器错误在普通渠道完成写入后沿日志结果返回。取消 future 因而可能留下已有渠道记录而没有 Pail 捕获。取消会释放请求借用与解析作用域，后续调用可以重试。请求和应用分别保存认证状态；HTTP 中程序化调用 Artisan 保留该 HTTP 来源，独立 Artisan 使用命令来源。

认证回调本身可以写日志。同一请求、同名 Guard 的嵌套读取只返回已知身份，不重新启动尚未完成的认证。完成、错误和取消均释放解析标记；外层认证错误仍正常返回，后续请求或重试可重新解析。

### 读取与筛选捕获记录

`openrust_pail::value_objects::MessageLogged::from_json` 读取一行捕获数据，可获取消息、级别、日期／时间、异常类与文件、trace，以及类型化的 HTTP、命令或队列来源。解析、日期转换和来源解码的失败通过 `Result` 返回。`context()` 排除保留字段 `__pail`、`exception` 和 `user_id`。

`Options::accepts` 要求所有已设置条件同时匹配。级别比较和子串搜索使用 ASCII 大小写折叠。message 条件只搜索消息文本；通用 filter 搜索包含来源和 context 的四字段 JSON 表示。该表示跟随 PHP 的 Unicode 转义、对象顺序和浮点格式，因此通用 Unicode 搜索可能需要使用 `\u` 表示。认证 ID 按字符串精确比较，`12` 与 `012` 不同。`from_command(&Input)` 保留 auth 优先于 user 的规则，包括显式空值，以及 PHP 的 timeout 转换；Rust 的命令输入对象与命令实现分开传入。

```rust
use openrust_pail::{Options, value_objects::MessageLogged};

fn select(line: &str) -> Result<bool, serde_json::Error> {
    let message = MessageLogged::from_json(line)?;
    let options = Options::new(3600, None, Some("ERROR".into()), None, Some("payment".into()));
    Ok(options.accepts(&message))
}
```

以上解析与筛选接口也用于[实时 Pail 命令](#tailing-log-messages-using-pail)。请求认证遵循上文的显式 await 与快照规则。

### Provider 启用条件

禁用捕获时仍注册 Files 和 Handler 绑定。启动时，若容器环境名精确为 `testing`，或 `VAPOR_SSM_PATH` 为非空且非 `0` 的字符串，就跳过捕获监听。`PAIL_TESTS=true` 用于 Pail 自身验收，可以覆盖这两个条件。该旗标按框架Env 规则处理 false、empty、null 和引号值；Vapor 标记按原始 PHP 字符串真假判断。

旗标来自当前应用不可变的环境快照，进程变量优先于选中的环境文件。配置缓存生效时不加载环境文件，因此文件中的 `PAIL_TESTS` 不生效。Handler 在第一条被接受的日志或队列生命周期事件到来时才解析；构造失败在该时机传播，原日志驱动已接受的写入仍保留。

Pail 通过共享路径服务解析 `Application::storage_path("pail")`。在首次捕获前设置 `use_storage_path` 或 `APP_STORAGE_PATH`，参见[应用存储路径](../getting-started/configuration.md#application-storage-path)。

默认文件日志渠道和应急回退使用应用存储路径。应在加载配置或解析渠道前配置路径；已加载的渠道路径保留其配置值，参见[应用存储路径](../getting-started/configuration.md#application-storage-path)。

## Pail CLI 打印器 {#pail-cli-printer}

可选 Pail 包提供 `openrust_pail::contracts::Printer` 和 `printers::CliPrinter`。在 Artisan 命令中构造 `CliPrinter::new(output, base_path)`，调用 `print(&message_logged)?`。打印器读取当前输出的详细级别与颜色状态。普通模式显示时间并缩略类型、文件和消息；`-v` 显示完整日期与正文，`-vv` 或 `-vvv` 再显示捕获的堆栈帧。

底部显示 HTTP 方法／路径和认证信息，或队列／任务／命令，以及公开 Context。数字键与 null、PHP 风格值导出、HTML 文本和多行消息按固定 Pail 规则处理。边框宽度上限为 150 个终端列；原始消息和长底部信息可能像上游一样溢出。Rust 的 `::` 类型命名空间及 Windows 本机路径分隔符适配到缩略和相对来源显示。

Rust JSON 无符号整数保留原始精度；超过 PHP 有符号整数范围的值，其显示可能与 PHP 的浮点近似不同。Rust 直接生成消息框，通过 `Output::write_raw` 写入已渲染文本。真实 Artisan 输出立即写出，捕获输出保留在缓冲区。`--ansi`、`--no-ansi`、`--quiet` 和 `--silent` 生效，写入错误沿调用链传播；格式错误在写入消息框前返回。输出详细级别随调用作用域保存和恢复。`PAIL_TESTS` 用于捕获测试，显示的日期、文件和堆栈保留实际记录值，不采用 PHP 测试夹具中的替换值。

[实时 Pail 命令](#tailing-log-messages-using-pail)使用此打印器，加载 Provider 后也会向 [Artisan dev](../digging-deeper/artisan.md#the-dev-command)添加默认 `logs` 进程。

## 使用 Pail 跟踪日志 {#tailing-log-messages-using-pail}

在应用 `Cargo.toml` 的 `[features].default` 列表中加入 `pail`，保留已有功能，然后使用源码入口：

```sh
./artisan pail --timeout=1
./artisan pail --timeout=0 --level=error --message=payment -vv
```

`pail` 跟随应用日志事件，不依赖日志渠道的存储后端。每次调用在当前应用存储路径的 `pail/` 目录创建独立会话，并发调用分别接收日志。`--filter`、`--message`、`--level`、`--auth` 和 `--user` 使用上文的筛选规则，`--auth` 优先于别名 `--user`。执行超时默认 3600 秒，零表示关闭超时；超时打印“Maximum execution time exceeded.”后成功退出。Ctrl+C 或 SIGTERM 停止跟随并删除本次会话。无效记录或输出错误令命令失败，同样清理会话；`--quiet` 和 `--silent` 抑制输出。

Rust 原生跟随文件，支持分段 UTF-8 行、截断和按名称重新打开轮换文件，无需 PHP、PCNTL 或 `tail` 子进程。初次读取已有内容遵循末十行规则。在检测到截断或替换时丢弃未完成行，避免拼接不同文件代际；完整 JSON 记录立即筛选并打印。Windows 上使用 `.\artisan.bat pail --timeout=0`。原生控制台 Ctrl+C 和正常超时均会停止命令并删除本次捕获会话。

Provider 通过所属应用的 ConsoleKernel 注册命令，沿用捕获的 testing／Vapor 门禁。Rust 没有 PHP 全局 SAPI 标志：仅启动 HTTP 不执行命令注册回调，独立和程序化 Artisan 使用同一回调。取消捕获调用会清理会话并恢复调用方输出状态。本实现不增加心跳：与上游一样，即使 `--timeout=0`，超过一小时没有写入的会话也可能被下一次日志生产清理。清理保留 `.gitignore`。加载 Provider 后也会向 [Artisan dev](../digging-deeper/artisan.md#the-dev-command)添加默认 `logs` 进程。

### 开发进程注册

启用 `pail` 后，包发现加载 Provider，`./artisan dev:list --json` 会列出执行 `pail --timeout=0` 的 `logs`。用 `./artisan dev --inline` 启动，监督器复用同一二进制，不调用 Cargo，也不要求 PHP／PCNTL。未编译 Pail，或应用未加载其 Provider 时，没有默认 `logs` 条目。查询列表不会创建捕获会话。

注册按 Provider 身份判断，独立于捕获门禁。在 testing 或 Vapor 环境中，已加载的 Provider 仍提供默认 `logs`，但 `pail` 命令被门禁禁用，除非设置 PAIL_TESTS 例外。此类应用可用 `DevCommands::except(&["logs"])?` 排除它。Windows 监督器向默认 `logs` 进程发送控制台关停信号，并通过 Windows 作业回收进程树。正常 Ctrl+C 关停会删除本次 Pail 会话。

每次解析进程列表都会检查 Provider 的加载状态，因此在 Pail 注册之前查询列表，不会导致后续永久漏掉 `logs`。
## 频道上下文

使用 `with_context`、`without_context` 设置或移除频道上下文。`share_context` 合并到已解析及未来的频道，每条日志的同名字段优先。`shared_context` 读取未来频道的默认值。`flush_shared_context` 只清空这些默认值，不删除已合并到频道的字段；移除频道字段使用 `without_context`。启动阶段默认值由请求或队列任务继承，动态上下文按作用域隔离，复用频道句柄也不会跨请求共享。


## 读取已解析通道

`LogManager::get_channels()` 与 `Log::get_channels()` 返回已解析通道名称到 `Arc<Logger>` 句柄的快照，不创建尚未使用的通道。`forget_channel` 移除通道后，影响后续解析；先前返回的快照和日志句柄仍可使用。

## 访问原生日志器

`Logger::get_logger()` 返回通道实际使用的 `Arc<NativeLogger>` 句柄。`LogManager::get_logger()` 与 `Log::get_logger()` 解析默认通道，通过 `Result` 返回同一个原生实例。读取已解析句柄不会创建其他通道；`forget_channel` 后，已持有的句柄仍可使用。

```rust
use openrust::{log::LogContext, support::facades::Log};

let channel = Log::channel(None)?;
let native = channel.get_logger();
native.info("Worker started", &LogContext::new())?;
let default_native = Log::get_logger()?;
```

原生写入复用配置的级别过滤、处理器、格式器和输出目标，只接收调用时传入的上下文，跳过应用频道上下文、Context 元数据、MessageLogged 监听器及 Pail 捕获。原生写入或处理器失败会直接返回错误。需要应用上下文、事件和应急恢复时，使用通道或 Log Facade 的级别写入方法。原生句柄实现现有 Rust 日志契约；共享通道的处理器和格式器继续通过既有注册与 tap 接口配置。

## 进程错误日志 {#process-error-log}

使用 `driver: "errorlog"` 将记录写入进程错误流。支持 `type: 0`（默认）和 `type: 4`；原生可执行程序没有 PHP SAPI，两者均写标准错误。目标额外追加一个换行，即使格式化文本已有末尾换行。级别过滤、占位符、处理器和格式化沿用普通渠道规则。该驱动按构造器契约始终继续下一个堆栈成员，`bubble` 配置不改变此行为；不支持的类型进入既有应急回退。

## 远程日志渠道 {#remote-log-channels}

`slack` 驱动将 JSON 发送到配置的 incoming Webhook。设置 `LOG_SLACK_WEBHOOK_URL`，应用配置以 `LOG_LEVEL` 或 `critical` 作为最低级别。通过 `url`、`username`、`channel`、`emoji`、`attachment`（默认 `true`）、`short`（默认 `false`）、`context`（默认 `true`）和 `exclude_fields` 配置报文。附件保留结构化字段；`attachment: false` 发送格式化文本。占位符替换和处理器先于发送执行。

`exclude_fields` 沿用外部处理器的点分路径规则：空值保留，路径中末段重复出现时停在第一个同名片段。需要不同脱敏策略时使用处理器；不要在日志上下文中放入不应发送到配置目标的密钥。

原生同步日志会等待传输结果。渠道内部线程持有既有 HTTP 客户端和运行时，在异步请求中记录日志也不会嵌套运行时。`timeout` 为每次尝试的秒数（默认 `10`）；连接、超时和正文传输失败最多尝试五次，不跟随重定向。与外部处理器相同，单凭 HTTP 状态码不判定传输错误，收到响应不表示远程服务已接受消息。传输失败走既有应急回退，渠道恢复后可继续发送。

`papertrail` 渠道读取 `PAPERTRAIL_URL` 和 `PAPERTRAIL_PORT`，发送远程 Syslog 数据报。保留现有 `driver: "papertrail"` 原生别名，通用处理器形式为：

```rust
let settings = serde_json::json!({
    "driver": "monolog",
    "handler": "syslog_udp",
    "handler_with": {"host": "127.0.0.1", "port": "10000", "ident": "orders"},
    "replace_placeholders": true,
});
```

处理器接受整数或数字字符串端口，以及 `facility`、`ident`、`rfc`：`0` 为 RFC 3164，`1`（默认）为 RFC 5424，`2` 带毫秒精度。主机名取自系统，程序标识默认使用应用名称。自定义格式化输出多行时逐行发送数据报；含报文头的名义长度上限为 65,023 字节，同时受平台 Socket 限制。Unix 下端口 `0` 选择本地数据报 Socket，相对路径从应用根目录解析。UDP 不提供送达确认；渠道继续沿用普通堆栈过滤、格式化与应急处理。

## 内置格式器与处理器 {#native-log-components}

`openrust::log::formatters::HtmlFormatter` 和 `openrust::log::processors::{MemoryUsageProcessor, PsrLogMessageProcessor}` 是可直接调用的公开类型，也可通过渠道配置选择：

```json
{"driver":"single","path":"storage/logs/formatted.html","formatter":"html","formatter_with":{"dateFormat":"Y-m-d H:i:s"},"processors":[{"processor":"memory_usage","with":{"realUsage":true,"useFormatting":false}},{"processor":"psr_log_message","with":{"removeUsedContextFields":true}}]}
```

`html` 转义消息和字段，`dateFormat` 使用现有的 PHP 风格日期格式。`psr_log_message` 替换 JSON 上下文中的占位符，可移除已使用字段；JSON 上下文不包含不透明 PHP 对象及其日期格式钩子。`memory_usage` 将原生进程驻留内存写入 `extra.memory_usage`，指标是操作系统内存，不是 PHP 分配器内存。`useFormatting` 默认 `true`，设为 `false` 保留数字字节数。`realUsage: false` 会被拒绝，不返回不同含义的替代指标。同名编译注册优先于内置工厂。

## 浏览器控制台输出 {#browser-log-output}

开发时可配置渠道：
```json
{"driver":"monolog","handler":"browser_console","level":"debug"}
```

`browser_console` 向 HTML 或 JavaScript 响应追加脚本，保留级别、`[[文本]]{color: blue}` 样式标记、渠道标签、上下文和附加字段。JSON、HEAD、无正文状态、已编码内容、分段文件和二进制下载保持原样。CLI 不输出浏览器脚本。记录按请求和应用隔离。对应公开类型为 `openrust::log::BrowserConsoleHandler`。

流式正文在 trailers 或 EOF 前追加控制台输出。原始正文流在请求作用域关闭后消费，应提前用 `Log::channel(None)?` 取得自有日志句柄并移入流内，不延长请求级 Facade 生命周期。动态浏览器渠道应在返回响应前创建，或预先声明在 `logging.channels`，以便安装正文包装。

## 从容器解析 Tap {#container-resolved-taps}

在服务提供者的 `register` 中调用以下函数，并将渠道的 `tap` 配置为 `["customize:orders,prod"]`。首次创建渠道时，注册闭包才从所属应用容器解析具体定制类及其构造依赖，不在启动时提前构造。编译注册替代运行时类名加载，依赖仍由容器提供。`forget_channel` 后重新创建渠道及其 tap。

```rust
use openrust::{
    container::{Container, ClassDescriptor, ConstructorParam},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    log::{Formatter, LogError, LogManager, LogResult, Logger, formatters::LineFormatter},
    support::ProviderResult,
};
use std::sync::Arc;

struct FormatterLabel(String);
struct CustomizeFormatter { label: Arc<FormatterLabel> }
impl CustomizeFormatter {
    fn invoke(&self, logger: &mut Logger, arguments: &[&str]) -> LogResult<()> {
        let pattern = format!("{}:{}:%message%\n", self.label.0, arguments.join(","));
        let formatter = LineFormatter::new(Some(&pattern), None);
        let formatter: Arc<Formatter> = Arc::new(move |record| formatter.format(record));
        for mut handler in logger.get_handlers() {
            handler.set_formatter(formatter.clone());
        }
        Ok(())
    }
}
fn register_formatter_tap(app: &Application) -> ProviderResult {
    let container = app.container();
    container.bind::<FormatterLabel, _>(|container| {
        let config = container.get::<dyn openrust::contracts::config::Repository>("config")?;
        let label = config.get("app.name").and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "App".into());
        Ok(Arc::new(FormatterLabel(label)))
    })?;
    container.register_class(ClassDescriptor::concrete::<CustomizeFormatter, _>(
        vec![ConstructorParam::class::<FormatterLabel>("label")],
        |arguments| Ok(Arc::new(CustomizeFormatter { label: arguments.service(0)? })),
    )?)?;
    let owner = Arc::downgrade(&Container::get_instance());
    container.make::<LogManager>()?.register_tap("customize", move |logger, arguments| {
        owner.upgrade().ok_or(LogError::InvalidConfiguration)?
            .make::<CustomizeFormatter>()?.invoke(logger, arguments)
    });
    Ok(())
}
```

`openrust::log::formatters::LineFormatter` 与 `formatter: "line"` 支持 `%datetime%`、`%channel%`、`%level_name%`、`%level%`、`%message%`、`%context%`、`%extra%` 及单个 `%context.key%`／`%extra.key%`。`formatter_with` 接受 `format`、`dateFormat`、`allowInlineLineBreaks` 和 `ignoreEmptyContextAndExtra`，后两项默认 `false`。公开 setter 和 `format_batch` 使用同一实现，返回的分隔符原样写入。原生错误保留现有结构化快照，不虚构 PHP Throwable 栈或运行时类反射。

在 tap 中，`get_handlers()` 通过 `&mut Logger` 借用实际处理链，每个句柄可调用 `set_formatter`。配置采用写时复制，其他已共享日志器保留原有设置，普通写入路径不增加格式配置锁。应急回退不提供可配置的渠道处理链。

Log 门面还提供 `driver`、`get_default_driver`、`set_default_driver`、`forget_channel` 和 `extend`。`driver(None)` 选择当前默认渠道，`forget_channel(None)` 清除该渠道缓存，已经返回的日志器仍保留原有输出目标。这些操作需在有效应用作用域内执行。

## 声明处理器自带格式器 {#formattable-handlers}

自定义文字处理器可实现 `FormattableHandler`，在 Provider 中通过 `LogManager::register_formattable_handler` 注册，继续使用现有处理器注册表和写入管线。

```rust
use openrust::log::{FormattableHandler, Formatter, LogManager, LogRecord, LogResult};
use std::{io::Write as _, sync::Arc};

struct PrefixHandler { prefix: String }
impl FormattableHandler for PrefixHandler {
    fn get_default_formatter(&self) -> LogResult<Arc<Formatter>> {
        let prefix = self.prefix.clone();
        Ok(Arc::new(move |record| Ok(format!("{prefix}:{}\n", record.message()))))
    }
    fn write(&self, _record: &LogRecord<'_>, formatted: &str) -> LogResult<()> {
        std::io::stderr().lock().write_all(formatted.as_bytes())?;
        Ok(())
    }
}
fn register_own_formatter(manager: &LogManager) {
    manager.register_formattable_handler("own_text", |arguments| {
        let prefix = arguments.get("prefix").and_then(|v| v.as_str())
            .unwrap_or("APP").to_owned();
        Ok(Arc::new(PrefixHandler { prefix }))
    });
}
```

配置 `driver: "monolog"`、`handler: "own_text"`、`handler_with: {"prefix":"APP"}` 和 `formatter: "default"` 使用自带格式。省略 `formatter` 采用框架格式，指定 `line`、`html` 或注册名称则覆盖自带格式。普通 `register_handler` 回调保持可用，选 `default` 时采用标准行格式器。

默认 getter 仅在第一条通过级别过滤的记录需要时执行。失败后可在同一渠道重试，成功格式器会缓存。原生调用返回原错误链，框架写入保留应急回退。执行用户代码时不持注册锁。并发首次解析可能多次调用 getter，最终保留同一个成功格式器，因此 getter 应保持幂等。此能力输出文字，不代表任意 PHP 扩展或结构化 APM 格式器类型均已支持。

核心不包含 New Relic APM Agent。通用处理器回调不等于已内置该供应商集成，原生 SDK 和结构化 APM 适配需单独实现。
