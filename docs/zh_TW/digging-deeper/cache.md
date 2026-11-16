# 快取

## 設定

在 `config/cache.rs` 透過 `CACHE_STORE` 選擇預設儲存。`array` 使用應用程式記憶體，`file` 使用本機處理程序可共用的目錄，`database` 使用共用的 SQLite 或 MySQL 資料庫。預設遷移建立 `cache` 和 `cache_locks` 資料表。啟用資料庫儲存前，請在設定的資料庫執行遷移。

```sh
./artisan migrate
```

## 快取用法

用 `Cache::store(Some("file"))` 選擇具名儲存，也可直接呼叫門面使用預設儲存。快取值支援 JSON 和位元組。正數 `Ttl::Seconds` 設定有效期，`None` 表示永久儲存。傳給 `put` 的 TTL 為零或負數時會刪除該項。`add` 原子插入缺少的項目，`touch` 延長或調整有效期，`pull` 原子讀取並移除。

`cache:forget` 移除一個快取項目。`cache:clear` 清空所選資料儲存，包括共用同一資料表的所有前綴。鎖使用獨立命名空間。檔案及資料庫快取使用 OpenRust 編碼。

```sh
./artisan cache:forget reports/latest file
./artisan cache:clear file
```

## 快取記憶化 {#cache-memoization}

用 `Cache::memo(None)` 選擇預設儲存庫，或用 `Cache::memo(Some("redis"))` 選擇具名儲存庫。首次查詢會存取後端，同一 HTTP 請求或佇列工作內的重複查詢會重用命中及未命中結果。透過記憶化儲存庫修改項目會先清除對應的記憶體結果，再寫入後端。一般儲存庫的寫入在記憶體結果失效後或下一個請求、工作中可見。 在框架管理的生命週期之外，每次呼叫都會建立獨立的記憶化儲存庫。從已結束的生命週期保留的儲存庫會直接讀取後端。

```rust
use openrust::{
    cache::{CacheResult, Value},
    contracts::cache::Repository as _,
    support::facades::Cache,
};

pub async fn report() -> CacheResult<Option<Value>> {
    let cache = Cache::memo(Some("redis"))?;
    let _ = cache.get("reports/latest").await?;
    Cache::memo(Some("redis"))?.get("reports/latest").await
}
```

## 快取標籤 {#cache-tags}

使用 `Cache::tags` 操作預設儲存，或透過 `Cache::store(Some("array"))?.tags(...)` 指定儲存。`array`、`redis` 和 `failover` 支援標籤。目前直接選擇其他儲存時會回傳不支援標籤的錯誤。

```rust
use openrust::{
    cache::{CacheError, Ttl, Value},
    contracts::cache::Repository as _,
    support::facades::Cache,
};

async fn store_report() -> Result<bool, CacheError> {
    Cache::tags(&["reports", "daily"])?
        .put("latest", &Value::from("ready"), Some(Ttl::Seconds(60)))
        .await
}
```

讀取時傳入完全相同的標籤順序，包括重複項目。呼叫 `Cache::tags(&["daily"])?.flush().await?` 可使所有帶有 `daily` 標籤的項目失效。一般鍵及僅含無關標籤的項目會保留。標籤透過切換世代失效。`array` 和 `failover` 的舊項目無法再依標籤讀取，依原 TTL 到期。直接選擇 Redis 儲存時，還會分批刪除舊世代的實際鍵。Redis 在同一指令碼內寫入項目並更新索引，清理舊世代期間仍可寫入新世代。

執行 `./artisan cache:prune-stale-tags redis` 可清除已到期或已刪除值的索引參照，仍有效的值會保留。可指定儲存名稱，或省略以使用預設儲存。

## 原子鎖

用 `Cache::lock` 建立鎖的控制代碼。`get` 嘗試一次，`block` 在限定時間內等待。持有者可釋放或更新有效的鎖。跨處理程序釋放時，用持有者權杖呼叫 `Cache::restoreLock`。使用有限租期可在處理程序提前結束後復原。

```rust
use std::time::Duration;

use openrust::{
    cache::{CacheResult, Ttl, Value},
    support::facades::Cache,
};

pub async fn cache_report() -> CacheResult<bool> {
    let mut lock = Cache::lock("report", Some(Duration::from_secs(10)))?;
    if !lock.get().await? {
        return Ok(false);
    }

    let written = Cache::put("reports/latest", &Value::from("ready"), Some(Ttl::Seconds(60))).await;
    let released = lock.release().await;
    let written = written?;
    released?;
    Ok(written)
}
```

### 清除快取鎖 {#clearing-cache-locks}

使用 `Cache::flushLocks().await?` 清除預設儲存的鎖，或用 `Cache::store(Some("database"))?.flushLocks().await?` 指定儲存。Artisan 也可以指定儲存名稱。

```sh
./artisan cache:clear database --locks
./artisan cache:clear redis --tags=reports,daily
```

清鎖命令會清除所選鎖儲存內的所有鎖，包括共用相同後端的其他前綴。陣列和檔案儲存分別保存鎖與一般項目。資料庫需要獨立的鎖資料表。Redis 會清空整個專用鎖資料庫。同一 Redis 執行個體應使用不同的邏輯資料庫編號。不同入口設定相同編號時，OpenRust 會先核對 Redis 執行個體識別碼，連線需允許 `INFO SERVER`。

`--tags` 會清除符合標籤的項目。同時使用非空的 `--tags` 與 `--locks` 會失敗。

## Redis 儲存

設定 `CACHE_STORE=redis`，使用 [Redis 連線](../database/redis.md)儲存快取。原子取出使用 Redis `GETDEL`，此操作需要 Redis 6.2 或更新版本。

`cache:clear redis` 清空快取儲存所用的整個邏輯資料庫，包括同一資料庫內其他前綴的鍵。使用獨立邏輯資料庫的鎖連線仍保留其鎖。

## 快取容錯移轉 {#cache-failover}

設定 `CACHE_STORE=failover` 可將 `failover` 設為預設快取儲存。`config/cache.rs` 中該驅動程式的 `stores` 清單決定嘗試順序。應用程式預設先嘗試 `database`，再嘗試 `array`。需要優先使用 Redis 時，可設定為 `["redis", "database", "array"]`。

每次操作都從第一個儲存開始。儲存回傳錯誤時才嘗試下一個，快取未命中或成功回傳 `false` 時直接結束。主要儲存恢復後會再次優先讀取它，故障期間寫入備援儲存的項目此時可能讀不到。所有儲存均失敗時，操作會回傳最後一個錯誤。

在應用程式事件分派器中監聽 `openrust::cache::events::CacheFailedOver`，可得知新發生的儲存錯誤。事件包含 `storeName` 和 `exception` 錯誤訊息。同一儲存連續回傳錯誤只會產生一次事件，經過一次該儲存未回傳錯誤的操作後，再次回傳錯誤才會重新觸發。鎖控制代碼由第一個能建立它的儲存提供，後續取得、續期與釋放均使用該儲存。

```rust
use openrust::{
    cache::{CacheManager, CacheResult, Value},
    contracts::cache::Repository as _,
};

pub async fn cache_status(manager: &CacheManager) -> CacheResult<Option<Value>> {
    manager.store(Some("failover"))?.get("status").await
}
```
