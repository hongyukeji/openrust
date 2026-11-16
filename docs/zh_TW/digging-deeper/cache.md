# 快取

## 設定

在 `config/cache.rs` 透過 `CACHE_STORE` 選擇預設儲存。`array` 使用應用程式記憶體，`file` 使用本機處理程序可共用的目錄，`database` 使用共用的 SQLite 或 MySQL 資料庫。預設遷移建立 `cache` 和 `cache_locks` 資料表。啟用資料庫儲存前，請在設定的資料庫執行遷移。

```sh
./artisan migrate
```

工作階段資料可使用[工作階段快取](../the-basics/session.md#session-cache)。`session` 儲存庫需要有效的 HTTP 工作階段，不支援標籤和快取鎖。

## Memcached {#memcached}

設定 `CACHE_STORE=memcached` 啟用 Memcached。預設位址為 `127.0.0.1:11211`，可用 `MEMCACHED_HOST` 和 `MEMCACHED_PORT` 修改。伺服器啟用 SASL PLAIN 時，一併設定 `MEMCACHED_USERNAME` 和 `MEMCACHED_PASSWORD`。 在 Unix 上，可將 `MEMCACHED_HOST` 設為通訊端的絕對路徑，並設定 `MEMCACHED_PORT=0`。

```dotenv
CACHE_STORE=memcached
MEMCACHED_HOST=127.0.0.1
MEMCACHED_PORT=11211
```

在 `config/cache.rs` 的 `cache.stores.memcached.servers` 設定多個節點。每個節點填寫 `host`、`port` 和選用的正數 `weight`（預設 100）。共用快取的應用程式程序應採用相同的節點及權重。`options` 支援 `connect_timeout_ms`、`timeout_ms`、`max_connections` 和 `max_item_bytes`。設定相同且使用同一個 `MEMCACHED_PERSISTENT_ID` 的儲存庫，會在應用程式中共用連線集區。

快取值使用 OpenRust 編碼。計數器不存在時會回傳 `CacheError::MissingCounter`，遞減最低到零。支援標籤失效與驗證擁有者的鎖定。Memcached 記憶體吃緊時可能淘汰項目，包含鎖定的擁有者記錄。`./artisan cache:clear memcached` 會清空所有已設定節點的資料，涵蓋所有快取前綴。需要限制清除範圍時，可使用專用節點。

## DynamoDB {#dynamodb}

設定 `CACHE_STORE=dynamodb`，並建立以字串欄位 `key` 為分割區索引鍵的 DynamoDB 資料表。預設表名為 `cache`。為數值欄位 `expires_at` 啟用 DynamoDB TTL，服務端即可回收過期資料列。快取讀取會檢查到期時間，即使 DynamoDB 尚未刪除該列。

用 `DYNAMODB_CACHE_TABLE` 指定表名，以 `AWS_DEFAULT_REGION` 指定區域。可以設定 `AWS_ACCESS_KEY_ID`、`AWS_SECRET_ACCESS_KEY` 和選用的 `AWS_SESSION_TOKEN`，也可使用 AWS SDK 憑證鏈。DynamoDB Local 等自訂端點使用 `DYNAMODB_ENDPOINT`。

`config/cache.rs` 中的 `attributes` 支援自訂 `key`、`value` 和 `expiration` 欄位名稱。分割區索引鍵和 TTL 設定需要與資料表一致。OpenRust 使用 JSON、位元組和 DynamoDB 原生數值儲存快取值。`forever` 與省略 TTL 的原子 `add` 會將到期時間設在五年後。

此驅動程式支援驗證擁有者的鎖定。省略租期或設定為零時使用 24 小時。快取標籤和整表清除不可用。可透過 `./artisan cache:forget <key> dynamodb` 刪除單一項目。

## 磁碟快取 {#storage-cache}

透過 `CACHE_STORE=storage` 將快取記錄寫入已設定的檔案系統磁碟。`CACHE_STORAGE_DISK` 指定磁碟，省略時使用應用程式預設磁碟。`CACHE_STORAGE_PATH` 是相對於該磁碟的快取目錄。

```dotenv
CACHE_STORE=storage
CACHE_STORAGE_DISK=local
CACHE_STORAGE_PATH=framework/cache/items
```

本機磁碟與 S3 共用相同的快取介面。到期或無法讀取的記錄視為未命中，後續寫入可以覆蓋到期檔案，清除也可刪除這些檔案。和 Laravel 的磁碟驅動程式相同，讀取後修改並寫回的操作沒有跨程序原子性。此驅動程式沒有鎖與標籤。

`./artisan cache:clear storage` 會清除快取目錄中所有前綴的檔案。快取路徑為空時，會清除所選磁碟內的檔案。磁碟同時保存一般檔案時，請使用專用快取目錄。

## 空快取儲存 {#null-store}

設定 `CACHE_STORE=null` 即可選擇 `null` 儲存，也可在單次操作用 `Cache::store(Some("null"))` 指定。應用程式未設定預設儲存時，快取管理器也會使用 `null`。讀取一律未命中，寫入及修改 TTL 回傳 `false`，計數回傳 `CacheError::MissingCounter`。刪除、清除項目與清鎖會回傳成功。兩個同名鎖控制代碼都能取得鎖，`null` 儲存沒有互斥能力。

```sh
./artisan cache:forget reports/latest null
./artisan cache:clear null --tags=reports
./artisan cache:clear null --locks
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

使用 `Cache::tags` 操作預設儲存，或透過 `Cache::store(Some("array"))?.tags(...)` 指定儲存。`array`、`redis`、`failover` 和 `null` 接受標籤。目前直接選擇其他儲存時會回傳不支援標籤的錯誤。

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

## 新增自訂快取驅動程式 {#adding-custom-cache-drivers}

為後端實作 `openrust::contracts::cache::Store`，在服務提供者的 `boot` 階段註冊驅動程式，並確保註冊早於首次解析儲存。下例使用獨立記憶體儲存示範註冊方式。

```rust
use std::sync::Arc;

use openrust::{cache::ArrayStore, support::facades::Cache};

Cache::extend("project", |manager, config| {
    manager.repository(Arc::new(ArrayStore::new()), config)
})?;
```

在 `config/cache.rs` 中將具名儲存的 `driver` 設為 `project`。工廠會收到快取管理器和完整儲存設定。透過 `manager.getContainer()` 取得應用程式服務，透過 `manager.repository(..., config)` 保留儲存名稱與事件設定。儲存解析為同步操作，回呼負責建立後端，非同步 I/O 放在個別快取操作中。

具名儲存庫會在首次存取時建立，後續繼續沿用。替換工廠後，可呼叫 `Cache::forgetDriver(Some("reports"))?` 或 `Cache::purge(Some("reports"))?`，下次解析就會使用新工廠。`Cache::build(config)?` 會依指定設定即時建立獨立儲存庫。

## 快取事件 {#cache-events}

透過應用程式事件分派器監聽 `CacheHit::NAME`、`KeyWritten::NAME` 和 `CacheLocksFlushed::NAME` 等事件。`openrust::cache::events` 中的事件帶有所選儲存名稱。讀寫事件提供呼叫端傳入的鍵和適用的有序標籤。批次讀取先分派 `RetrievingManyKeys`，再逐一分派命中或未命中事件。

在 `config/cache.rs` 的個別儲存設定加上 `events: false` 可停用該儲存庫的事件。容錯移轉儲存庫預設停用，底層各儲存仍依自身設定分派。記憶化儲存庫存取底層時會分派事件，命中記憶化快照的重複讀取沒有額外事件。

同步監聽器失敗時，操作會回傳錯誤。寫入前的監聽器失敗會阻止寫入，寫入後的監聽器失敗會保留已寫入的值。原子 `pull` 的讀取及刪除前置事件均在儲存操作前分派。

測試快取命中時，先寫入 `reports/latest`，再透過已啟動的 `TestApp` 讀取並確認：

```rust
use openrust::{cache::events::CacheHit, contracts::cache::Repository as _};

let events = app.fakeEvents(&[CacheHit::NAME]).await?;
let _ = cache.get("reports/latest").await?;
events.assertDispatched(CacheHit::NAME);
```
