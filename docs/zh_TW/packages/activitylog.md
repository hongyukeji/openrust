# 操作稽核（擴充）

## 安裝與設定 {#configuration}

啟用應用的 `activitylog` feature，或加入 `openrust-activitylog` 並使用套件發現。`ActivitylogServiceProvider` 登記服務與遷移，啟動時不存取資料庫。在應用 `Cargo.toml` 的 `[features].default` 加入 `activitylog`，保留既有功能，再執行 `./artisan migrate`。

`ACTIVITYLOG_ENABLED` 控制預設啟用狀態；`ACTIVITYLOG_BUFFER_ENABLED` 控制 `buffer.enabled`，預設關閉。`include_soft_deleted_subjects` 預設關閉。非法布林值使設定載入失敗；命中設定快取時保留快取值。

執行 `./artisan vendor:publish --tag=activitylog-config` 發布設定，在應用載入器登記模組與設定群組後重新編譯。重複發布保留定製，`--force` 覆寫。預設 `enabled: true`、`default_log_name: "default"`、`default_auth_driver: null`、`connection: null`、`clean_after_days: 365`；日誌名稱可為空。設定快取命中時使用已存快照，變更後重新啟動常駐程序。

指定 `activitylog.connection` 時，由應用在目標連線顯式登記 `ActivitylogServiceProvider::migrations()`；套件略過預設連線遷移。

## 記錄操作 {#recording}

`ActivityLogger` 提供流式記錄；`ActivityRepository` 負責查詢、交易寫入與清理。操作者及對象使用可信業務身分；顯式傳入的屬性預設保留，由應用選擇可記錄資料。

```rust
use openrust_activitylog::{ActivityLogger, CauserResolver, Subject};
use serde_json::json;

let mut logger = ActivityLogger::new(connection.clone());
logger.setCauserResolver(CauserResolver::new(Some(
    Subject::new("user", authenticated_user_id)?,
)));
let record = logger.on(Subject::new("settings", "1")?)
    .inLog("settings").event("updated")
    .withProperties(json!({"module": "site"}))
    .withChanges(json!({"old": {"enabled": false}, "attributes": {"enabled": true}}))
    .log("settings.updated").await?;
```

同一 Logger 成功記錄後清空主體、事件、屬性、時間和顯式操作者，下一筆恢復預設日誌名稱及操作者。`byAnonymous()` 指定匿名身分；`by(None)` 保留預設解析。`tap(callback, event)` 在解析預設操作者後執行，可修改待存資料。`createdAt` 接受 `jiff::Timestamp`。

`onModel(&model)`、`byModel(&model)?` 擷取模型真實 morph 身分及屬性，供 `:subject.name`、`:causer.name` 等預留位置使用。在應用作用域中，`byId(id).await?` 透過設定的認證 Provider 查找使用者；不存在的 ID 返回錯誤且不寫日誌。

## 作用域與模型事件 {#model-events}

`activity()` 與 `facades::Activitylog::logger()` 每次建立新記錄器。HTTP 請求、Artisan 命令與佇列工作透過容器原生執行作用域共用日誌狀態與操作者解析器。獨立呼叫者可用 `logger.scope(async { ... })` 將顯式建立的上下文共用給 helper 與模型事件。`setLogStatus`、`setCauserResolver` 接受顯式作用域物件，不共用程序級目前使用者。

`ActivityLogStatus` 提供開關；`withoutLogging(|| async { ... })` 原來啟用時暫時關閉，在結束、錯誤、panic 或取消後恢復。原來已關閉時保留回呼對狀態的操作。`CauserResolver::withCauser` 支援巢狀覆寫及恢復。預設操作者讀取目前請求已完成認證的身分，遵循 `default_auth_driver` 或選中／預設 guard，並使用模型真實 morph 映射。記錄前須透過認證中介層或 guard 完成認證；同步模型回呼無法發起尚未完成的非同步認證查詢。獨立呼叫者須顯式提供解析器。

模型實作 `models::concerns::LogsActivity`，在應用 Provider 登記 `ModelType::bootLogsActivity(app, repository)`。`getActivitylogOptions` 返回 `LogOptions`，支援 `logAll`、`logOnly`、`logExcept`、`logFillable`、`logUnguarded`、`logOnlyDirty`、`dontLogEmptyChanges`、`dontLogIfAttributesChangedOnly`、`useAttributeRawValues` 及描述回呼。預設不選屬性，但允許空變更日誌。使用原生 Eloquent 事件；顯式交易內同連線日誌與業務一起回滾，其他日誌連線在業務提交後寫入。

模型還須實作 `Clone`，儲存以 `Default::default()` 初始化的 `models::LoggingState`，並在 `activitylogState` 返回該欄位。`disableLogging()`、`enableLogging()` 僅影響該實例；複製會複製目前開關值。用已登記的關聯路徑如 `owner.name` 選擇關聯屬性；舊快照按原外鍵讀取，顯式交易也能讀取交易內關聯。應用採不同名稱時，在原生命名關聯中顯式登記別名。

定製稽核模型時，在應用 Provider 用 `app.container().instanceNamed("application.activity", value)` 登記 `Arc::new(models::ActivityModel::of::<CustomActivity>())`，並將 `activitylog.activity_model` 設為 `"application.activity"`。保留稽核欄位與 JSON casts；模型中繼資料選擇表及可選連線，顯式 `activitylog.connection` 優先。一般寫入執行原生 Eloquent 儲存事件。定製動作時，以原生容器名稱登記 `LogActivityAction` 或 `CleanActivityLogAction`，再透過 `actions.log_activity`、`actions.clean_log` 選擇；用 `beforeLogging` 修改日誌，用 `CleanActivityLogAction::using` 定製清理。

## 交易邊界 {#transactions}

顯式交易先呼叫 `repository.prepare(activity)`，再用 `PreparedActivity::logIn` 寫入業務交易。傳入交易選擇資料庫，該庫須已有稽核表。交易失敗或取消會回滾；`repository.logIn(tx, activity)` 合併準備與寫入。獨立 `repository.log(activity)` 不屬於業務交易；外部通知及出貨由應用負責。

```rust
use openrust_activitylog::{Activity, ActivityRepository};

let repository = ActivityRepository::new(connection.clone());
let prepared = repository.prepare(Activity::new("settings.updated"))?;
connection.transaction(move |tx| {
    let prepared = prepared.clone();
    Box::pin(async move {
        // Apply the business mutation through tx.table(...).
        if let Some(record) = prepared { record.logIn(tx).await?; }
        Ok(())
    })
}, 1).await?;
```

## 脫敏與儲存邊界 {#redaction}

預設保留顯式屬性，不自動遮蔽。需要脫敏時使用 `withRedaction(Redaction::new().logProperties(&["module"]).logOnly(&["name"]))`，或啟用 `activitylog.redaction.enabled` 並設定 `properties`、`attributes`、`except`。策略只保留允許的頂層欄位，遞迴遮蔽密碼、token、金鑰名稱；`logExcept` 增加遮蔽名稱。只有啟用時才限制16 KiB及16層；無法辨識一般字串內任意秘密。

預設遷移採自動遞增主鍵、可空數值型 subject／causer morph 標識、可空 JSON `properties`／`attribute_changes` 及原生 `created_at`／`updated_at` 時間戳。`Subject` 在 API 邊界以字串表示 ID；應用主鍵為 UUID 或字串時，須在遷移前定製兩個 morph ID 欄位。`ActivityRecord::created_at` 是從儲存解碼的 UTC Unix 毫秒；預設 SQL 時間戳精度會捨棄秒以下部分。額外的可空 `request_id` 用於請求關聯。查詢不自動篩選租戶，應用返回記錄前須授權。Debug 隱藏承載資料及身分。

## 查詢與保留 {#queries}

`models::Activity` 是帶 JSON casts 的 Eloquent 模型；`getProperty("order.number", default)` 讀取巢狀屬性，`ActivityRecord` 也提供此方法。`subject::<Order>()`、`causer::<User>()` 返回原生型別化 morph 關聯，在應用作用域中使用。設 `include_soft_deleted_subjects: true` 後可讀取軟刪除對象；操作者保留正常軟刪除篩選。

```rust
let page = repository.query().inLog("settings")
    .forSubject(&Subject::new("settings", "1")?)
    .limit(50).get().await?;
// Pass the final record's id to .before(id) for the next page.
```

支援 `inLog`、`inLogs`、`forEvent`、`forSubject`、`causedBy`、`forRequest`，身分以精確文字比對。每頁1–1000筆，預設50筆；不自動提供後台或 HTTP 介面。

`./artisan activitylog:clean accounts --days=365` 清理指定日誌群組的過期記錄；省略 `accounts` 清理所有群組。省略天數使用 `clean_after_days`，天數須為正整數。正式環境需互動確認或使用 `--force`。`cleanBefore(utc_milliseconds, limit)` 拒絕未來截止時間；不提供防竄改儲存。

啟用 `activitylog.buffer.enabled` 後，每個 HTTP 請求、Artisan 命令或佇列執行嘗試中的 helper 與模型事件共用一個緩衝區。生命週期結束後刷新原緩衝區，失敗回應、命令及執行嘗試也刷新。緩衝記錄持久化前沒有 ID。刷新採原子批次插入，略過逐筆 Activity 模型儲存事件；模型事件日誌在業務交易提交後才入列，交易或儲存點回滾會捨棄對應日誌，巢狀提交保留原記錄順序。未傳交易控制代碼的顯式 Logger 呼叫仍獨立寫入；顯式日誌要求加入業務交易時使用 `PreparedActivity::logIn`。

回應前取消、程序終止或強制結束不能保證刷新。硬逾時 Worker 子程序只在原有截止時間內執行本地刷新。獨立程序使用 `ActivityBuffer::new(repository)`、`logger.withBuffer(buffer)` 並顯式等待 `buffer.flush()`，沒有非同步程序退出鉤子。刷新失敗保留待寫記錄，可顯式重試。
