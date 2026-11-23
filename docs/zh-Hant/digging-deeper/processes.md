# 程序

## 執行程序 {#running-processes}

在應用程式內使用 `openrust::support::facades` 中的 `Process`。字串命令透過平台 Shell 執行。參數需要原樣傳入時，使用參數陣列。應用程式外使用 `openrust::process::Factory`。下方範例使用 Unix Shell。

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

用 `path` 設定工作目錄，用 `env` 和 `removeEnv` 增刪環境變數，用 `input` 傳送位元組。預設逾時為 60 秒。`idleTimeout` 限制沒有輸出的時間，`forever` 取消總逾時。`exit_code` 可讀取非零結束代碼，`throw` 可將其轉成錯誤。

## 非同步執行 {#running-asynchronously}

用 `start` 取得執行中的程序控制代碼。可讀取 `output` 和 `errorOutput`，或透過 `latestOutput`、`latestErrorOutput` 取得未讀取的位元組。`startWithOutput` 對每個可用的 stdout 或 stderr 區塊呼叫回呼。用 `wait` 等待結果，或用 `stop` 要求終止，寬限期結束後強制終止。macOS 和 Linux 會在完成、逾時或取消時清理程序群組。

用 `pool` 和 `concurrently` 並行執行具名子程序。`pipe` 把每一步成功程序的 stdout 當作下一步輸入，遇到失敗時停止。

### 等待輸出 {#waiting-for-output}

對擷取輸出的程序呼叫 `waitUntil`，會逐區塊接收 stdout 和 stderr，直到述詞符合。符合時傳回 `true`，程序先結束時傳回 `false`。再次等待時會從未讀區塊繼續，若標記可能跨區塊，應累積位元組後再比對。用 `wait` 取得最終結束碼，或用 `waitWithOutput` 觀察剩餘區塊。逾時可透過 `Error::result()` 讀取已擷取的結果。

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

### 連接控制終端 {#tty}

在 Unix 上，`tty(true)` 將三個標準串流連接到控制終端。輸入和輸出直接經過終端，程序沒有輸出緩衝，也不呼叫輸出回呼。應用程式沒有控制終端時會傳回 `Error::TtyUnavailable`。`tty(false)` 恢復一般擷取模式。

附加程序加入終端的前景程序群組。終止操作作用於直接子程序。Unix 擷取模式的程序繼續使用獨立程序群組並按群組清理。

```rust
use openrust::support::facades::Process;

let result = Process::command("test -t 0 && test -t 1 && test -t 2 && printf 'terminal ready\n'")?
    .tty(true)
    .run()
    .await?
    .throw()?;
assert!(result.output().is_empty());
```

### 數值訊號 {#numeric-signals}

在 Unix 上可透過 `Signal::Raw` 向執行中的程序傳送平台訊號編號。執行下例前，將 `PROCESS_EXAMPLE_SIGNAL` 設為本機 `SIGUSR1` 的數值。

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

## 測試 {#testing}

fake 與呼叫記錄屬於各自的工廠。用 `preventStrayProcesses(true)` 讓未比對的呼叫在啟動可執行檔之前失敗。

```rust
use openrust::process::Factory;

let processes = Factory::new();
processes.fakeCommand("deploy *", Factory::result("deployed", "", 0));
processes.preventStrayProcesses(true);
let result = processes.run(["deploy", "preview"]).await?;
assert!(result.successful());
processes.assertRan(|process, _| process.command == ["deploy", "preview"].into());
```

### 模擬程序腳本 {#scripted-fakes}

用 `Factory::describe()` 描述模擬程序的 ID、依順序出現的輸出行、`running()` 傳回 true 的檢查次數及結束碼。將建構後的結果註冊到 `fakeCommand` 或模擬序列。等待時依序播放輸出，訊號記錄在模擬程序本機。

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


## 順序斷言

呼叫 `assertNothingRan()` 要求目前沒有呼叫紀錄。`assertNotRan(|process, result| ...)` 要求沒有紀錄符合條件。`assertRanInOrder(&[&first, &second])` 同時檢查呼叫總數與順序；每個謂詞接收 `&RecordedProcess` 和 `Option<&ProcessResult>`，失敗的模擬操作可能沒有結果。預期不符時斷言 panic。在應用作用域內也可透過 `Process` Facade 呼叫這些方法。
