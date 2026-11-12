# 任務排程

## 定義排程

在 `routes/console.rs` 的 Artisan 註冊作用域中登記排程任務。在應用程式既有的 `register()` 函式中呼叫 `Schedule::configure`。

```rust
use openrust::{console::ConsoleError, support::facades::Schedule};

pub(crate) fn register() -> Result<(), ConsoleError> {
    Schedule::configure(|schedule| {
        schedule
            .command("inspire", [])
            .dailyAt("08:30")?
            .timezone(chrono_tz::Asia::Shanghai)
            .withoutOverlapping();

        Ok(())
    })
}
```

用 `call` 登記非同步回呼，用 `command` 呼叫已註冊的 Artisan 命令，用 `exec` 執行外部程式，用 `job` 派送佇列酬載。程式與參數分開傳入。`exec` 從應用程式根目錄啟動。回呼接收應用程式與命令輸出物件。每次排程派送都會產生新的任務 ID。

用 `cron` 設定五欄位運算式，用 `dailyAt` 設定每日的本地時間，用 `everyMinute` 或 `everyTenSeconds` 等子分鐘頻率執行重複任務。排程時區依序取 `app.schedule_timezone`、`app.timezone`、UTC。用 `timezone` 覆寫單個任務的時區。用 `environments`、`when` 和 `skip` 限制執行條件。日光節約時間切換時，同一本地時間可能出現兩次，也可能被跳過。

### 維護模式 {#maintenance-mode}

應用程式進入[維護模式](../getting-started/configuration.md#maintenance-mode)後，預設略過排程工作。需要在維護期間執行的工作，可在 `Schedule::configure` 中呼叫 `evenInMaintenanceMode()`。

```rust
schedule.command("inspire", []).evenInMaintenanceMode();
```

子分鐘排程偵測到維護模式後，會在本次執行剩餘的時間繼續略過一般工作。執行 `./artisan up` 後，下一次 `./artisan schedule:run` 可以重新執行一般工作。

### 暫停排程工作 {#pausing-scheduled-tasks}

執行 `./artisan schedule:pause` 暫停排程器，執行 `./artisan schedule:resume` 或別名 `./artisan schedule:continue` 恢復。暫停狀態儲存在應用程式的預設快取。多個程序需要共用暫停狀態時，請使用相同的預設快取與命名空間。排程互斥鎖可透過 `schedule.useCache(...)` 選擇快取。

在 `Schedule::configure` 中呼叫 `evenWhenPaused()`，可讓指定工作在暫停期間仍符合執行條件。

```rust
schedule.command("inspire", []).evenWhenPaused();
```

子分鐘排程在同一輪中可感知暫停與恢復，已開始的工作會繼續執行。可在 `Schedule::configure` 中呼叫 `schedule.doNotAllowPausing()` 停用應用程式的暫停功能。可監聽 `openrust::console::events` 中的 `SchedulePaused`、`ScheduleResumed` 與 `ScheduledTaskSkipped`。

### 排程群組 {#schedule-groups}

在 `Schedule::configure` 中用 `group` 為多個任務設定共用的排程屬性。建立排程群組前至少設定一個屬性。

```rust
schedule
    .timezone(chrono_tz::Asia::Tokyo)
    .dailyAt("17:00")?
    .group(|schedule| {
        schedule.command("inspire", []);
        Ok(())
    })?;
```

巢狀群組繼承外層設定。巢狀群組和個別任務都可覆寫時區、頻率等設定。篩選條件會累積。暫時設定的屬性僅套用於下一次任務註冊。離開排程群組後恢復外層設定。

## 防止重疊與單機執行

`withoutOverlapping()` 在執行期間持有快取鎖，任務完成或發生錯誤後釋放。預設租期為 1,440 分鐘，可用 `withoutOverlappingFor(minutes)` 調整。`onOneServer()` 為每個事件在每分鐘選出一個執行者。回呼或佇列任務啟用互斥前，需要用 `name` 設定穩定且唯一的名稱。

跨執行個體互斥要求所有執行者使用相同的共用快取儲存及命名空間。可使用應用程式預設的共用儲存，或用 `schedule.useCache("redis")` 指定。

## 執行排程器

在系統排程器中每分鐘呼叫 `./artisan schedule:run`，也可以用 `./artisan schedule:work` 常駐執行。常駐程序收到 SIGTERM 或 Ctrl+C 後會完成目前的任務。用 `./artisan schedule:list` 查看運算式和下一次執行時間。`schedule:run --whisper` 隱藏沒有任務執行時的提示。

有子分鐘任務時，`schedule:run` 持續到當前分鐘結束。`./artisan schedule:interrupt` 透過所選快取儲存通知當前分鐘的重複迴圈結束。任務錯誤交由例外報告器處理，後續到期任務繼續執行，排程命令處理完後成功結束。排程基礎設施發生錯誤時，命令以失敗狀態結束。

## 任務鉤子 {#task-hooks}

可為個別任務或排程群組註冊鉤子。範例中的 `hook` 建立接收應用程式與主控台輸出的非同步回呼。

```rust
schedule
    .command("fixture:hello", [])
    .name("hook-success")
    .before(hook("task-before"))
    .then(hook("task-then"));
```

排程器在執行任務前執行 `before`。`after` 與 `then` 等價。`onSuccess` 和 `onFailure` 依任務結果執行。任務結束後的鉤子依註冊順序執行。巢狀群組繼承外層鉤子，並可追加自己的鉤子。鉤子例外交由應用程式例外處理器回報。

## 事件 {#events}

從 `openrust::console::events` 匯入排程事件型別，並在應用程式組裝作用域註冊監聽器。範例使用自訂的 `ScheduleEvents` 監聽器，以及將註冊錯誤轉為 `ConsoleError` 的 `console_error`。

```rust
let listener = Arc::new(ScheduleEvents::default());
for name in [
    "console.scheduled_task_starting",
    "console.scheduled_task_finished",
    "console.scheduled_task_failed",
    "console.scheduled_task_skipped",
] {
    Event::listen(name, listener.clone(), Delivery::Sync).map_err(console_error)?;
}
```

前景任務執行前派送 `ScheduledTaskStarting`。命令執行結束時派送 `ScheduledTaskFinished`，結束碼非零時接著派送 `ScheduledTaskFailed`。執行拋錯時，開始事件之後派送失敗事件。完成和失敗事件攜帶耗時 `Duration`，失敗事件也會攜帶例外。

任務因篩選條件、暫停、單機選舉或重疊鎖定而跳過時派送 `ScheduledTaskSkipped`。
