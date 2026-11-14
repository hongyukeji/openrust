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
