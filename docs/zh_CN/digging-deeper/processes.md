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

用 `path` 设置工作目录，用 `env` 和 `removeEnv` 增删环境变量，用 `input` 发送字节。默认超时为 60 秒。`idleTimeout` 限制无输出的时间，`forever` 取消总超时。`exitCode` 可读取非零退出码，`throw` 可将其转换为错误。

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

## 测试 {#testing}

fake 和调用记录属于各自的工厂。用 `preventStrayProcesses(true)` 使未匹配的调用在启动可执行文件前失败。

```rust
use openrust::process::Factory;

let processes = Factory::new();
processes.fakeCommand("deploy *", Factory::result("deployed", "", 0));
processes.preventStrayProcesses(true);
let result = processes.run(["deploy", "preview"]).await?;
assert!(result.successful());
processes.assertRan(|process| process.command == ["deploy", "preview"].into());
```
