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

## Redis 儲存

設定 `CACHE_STORE=redis`，使用 [Redis 連線](../database/redis.md)儲存快取。原子取出使用 Redis `GETDEL`，此操作需要 Redis 6.2 或更新版本。

`cache:clear redis` 清空快取儲存所用的整個邏輯資料庫，包括同一資料庫內其他前綴的鍵。使用獨立邏輯資料庫的鎖連線仍保留其鎖。
