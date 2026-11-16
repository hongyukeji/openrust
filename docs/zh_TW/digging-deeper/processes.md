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

用 `path` 設定工作目錄，用 `env` 和 `removeEnv` 增刪環境變數，用 `input` 傳送位元組。預設逾時為 60 秒。`idleTimeout` 限制沒有輸出的時間，`forever` 取消總逾時。`exitCode` 可讀取非零結束代碼，`throw` 可將其轉成錯誤。

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

## 測試 {#testing}

fake 與呼叫記錄屬於各自的工廠。用 `preventStrayProcesses(true)` 讓未比對的呼叫在啟動可執行檔之前失敗。

```rust
use openrust::process::Factory;

let processes = Factory::new();
processes.fakeCommand("deploy *", Factory::result("deployed", "", 0));
processes.preventStrayProcesses(true);
let result = processes.run(["deploy", "preview"]).await?;
assert!(result.successful());
processes.assertRan(|process| process.command == ["deploy", "preview"].into());
```
