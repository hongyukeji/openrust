# 进程

## 运行进程 {#running-processes}

在应用内使用 `openrust::support::facades` 中的 `Process`。字符串命令通过平台 Shell 运行。参数需要按原样传入时，使用参数数组。应用外使用 `openrust::process::Factory`。下面的示例使用 Unix Shell。

```rust
use openrust::process::Factory;
use std::time::Duration;

let processes = Factory::new();
let result = processes.command(["/bin/sh", "-c", r#"printf '%s' "$MODE""#])
    .env([("MODE", "test")])
    .timeout(Duration::from_secs(2))
    .run()
    .await?
    .throw()?;
assert_eq!(result.output(), b"test");
```

用 `path` 设置工作目录，用 `env` 和 `removeEnv` 增删环境变量，用 `input` 发送字节。默认超时为 60 秒。`idleTimeout` 限制无输出的时间，`forever` 取消总超时。`exit_code` 可读取非零退出码，`throw` 可将其转换为错误。

## 异步运行 {#running-asynchronously}

用 `start` 获取运行中的进程句柄。可读取 `output` 和 `errorOutput`，或通过 `latestOutput`、`latestErrorOutput` 取得未读取的字节。`startWithOutput` 对每个可用的 stdout 或 stderr 数据块调用回调。用 `wait` 等待结果，或用 `stop` 请求终止，宽限期结束后强制终止。macOS 和 Linux 会在完成、超时或取消时清理进程组。

用 `pool` 和 `concurrently` 并行运行命名子进程。`pipe` 把每一步成功进程的 stdout 作为下一步输入，遇到失败时停止。

### 等待输出 {#waiting-for-output}

对捕获输出的进程调用 `waitUntil`，会逐块接收 stdout 和 stderr，直到谓词匹配。匹配时返回 `true`，进程先退出时返回 `false`。再次等待时从未读片段继续，匹配可能跨块的标记时应累积字节。用 `wait` 获取最终退出码，或用 `waitWithOutput` 观察剩余片段。超时可通过 `Error::result()` 读取已捕获的结果。

```rust
use openrust::{process::OutputStream, support::facades::Process};
use std::time::Duration;

let mut process = Process::command("printf 'ready\n' >&2; sleep 0.1; printf 'done'")?
    .timeout(Duration::from_secs(3))
    .start()?;
let mut diagnostic = Vec::new();
let ready = process.waitUntil(|stream, chunk| {
    if stream == OutputStream::Stderr {
        diagnostic.extend_from_slice(chunk);
    }
    diagnostic.ends_with(b"ready\n")
}).await?;
assert!(ready);
assert_eq!(process.wait().await?.output(), b"done");
```

### 连接控制终端 {#tty}

在 Unix 上，`tty(true)` 将三个标准流连接到控制终端。输入和输出直接经过终端，进程没有输出缓冲，也不调用输出回调。应用没有控制终端时返回 `Error::TtyUnavailable`。`tty(false)` 恢复普通捕获模式。

附着进程加入终端的前台进程组。终止操作作用于直接子进程。Unix 捕获模式的进程继续使用独立进程组并按组清理。

```rust
use openrust::support::facades::Process;

let result = Process::command("test -t 0 && test -t 1 && test -t 2 && printf 'terminal ready\n'")?
    .tty(true)
    .run()
    .await?
    .throw()?;
assert!(result.output().is_empty());
```

### 数值信号 {#numeric-signals}

在 Unix 上可通过 `Signal::Raw` 向运行中的进程发送平台信号编号。运行下例前，将 `PROCESS_EXAMPLE_SIGNAL` 设为本机 `SIGUSR1` 的数值。

```rust
use openrust::{process::Signal, support::facades::Process};
use std::time::Duration;

let number: i32 = std::env::var("PROCESS_EXAMPLE_SIGNAL")?.parse()?;
let mut process = Process::command(
    "trap 'printf caught; exit 6' USR1; printf ready; while :; do sleep 0.02; done",
)?
.timeout(Duration::from_secs(4))
.start()?;
assert!(process.waitUntil(|_, bytes| bytes == b"ready").await?);
process.signal(Signal::Raw(number)).await?;
let result = process.wait().await?;
assert_eq!(result.exit_code(), 6);
assert_eq!(result.output(), b"readycaught");
```

## 测试 {#testing}

fake 和调用记录属于各自的工厂。用 `preventStrayProcesses(true)` 使未匹配的调用在启动可执行文件前失败。

```rust
use openrust::process::Factory;

let processes = Factory::new();
processes.fakeCommand("deploy *", Factory::result("deployed", "", 0));
processes.preventStrayProcesses(true);
let result = processes.run(["deploy", "preview"]).await?;
assert!(result.successful());
processes.assertRan(|process, _| process.command == ["deploy", "preview"].into());
```

### 模拟进程脚本 {#scripted-fakes}

用 `Factory::describe()` 描述模拟进程的 ID、按顺序出现的输出行、`running()` 返回 true 的检查次数及退出码。将构建后的结果注册到 `fakeCommand` 或模拟序列。等待时按顺序播放输出，信号记录在模拟进程本地。

```rust
use openrust::process::{Factory, OutputStream, Signal};

let processes = Factory::new();
processes.fakeCommand("worker", Factory::describe()
    .id(321)
    .runsFor(2)
    .errorOutput("ready")
    .output("finished")
    .exit_code(0)
    .result());
processes.preventStrayProcesses(true);

let mut worker = processes.start("worker")?;
assert_eq!(worker.id(), Some(321));
assert!(worker.waitUntil(|stream, bytes| stream == OutputStream::Stderr && bytes == b"ready\n").await?);
worker.signal(Signal::Terminate).await?;
assert!(worker.hasReceivedSignal(Signal::Terminate));
assert_eq!(worker.wait().await?.output(), b"finished\n");
```


## 顺序断言

调用 `assertNothingRan()` 要求当前没有调用记录。`assertNotRan(|process, result| ...)` 要求没有记录符合条件。`assertRanInOrder(&[&first, &second])` 同时检查调用总数与顺序；每个谓词接收 `&RecordedProcess` 和 `Option<&ProcessResult>`，失败的模拟操作可能没有结果。预期不符时断言 panic。在应用作用域内也可通过 `Process` Facade 调用这些方法。
