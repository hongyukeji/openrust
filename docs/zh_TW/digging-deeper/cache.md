# 快取

## 設定

在 `config/cache.rs` 透過 `CACHE_STORE` 選擇預設儲存。`array` 使用應用程式記憶體，`file` 使用本機處理程序可共用的目錄，`database` 使用共用的 SQLite 或 MySQL 資料庫。

工作階段資料可使用[工作階段快取](../the-basics/session.md#session-cache)。`session` 儲存庫需要有效的 HTTP 工作階段，不支援標籤和快取鎖。

## 資料庫快取資料表 {#database-cache-tables}

對設定的資料庫執行 `./artisan migrate`，套用隨附的 `cache` 和 `cache_locks` 遷移。若缺少該遷移，可用 `make:cache-table` 或別名 `cache:table` 產生。執行產生的 Rust 遷移前須重新建置應用程式。

```sh
./artisan make:cache-table
cargo build
./artisan migrate
```

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

### 讀取快取項目 {#retrieving-items-from-the-cache}

`getOr(key, default)` 接受預設值，`getOrElse(key, resolve)` 接受非同步後援閉包。鍵缺少或快取值為 JSON `null` 時使用預設值，命中時略過後援。後端錯誤仍回傳給呼叫端，後援計算的結果只回傳給呼叫端。

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn report_title() -> CacheResult<Value> {
    Cache::getOrElse("reports:title", || async { Ok(Value::from("Untitled")) }).await
}
```

### 型別化讀取 {#typed-cache-values}

用 `string`、`integer`、`float`、`boolean` 和 `array` 驗證讀取結果。預設值傳 `None` 或 `Some(value)`，鍵缺少或值為 null 時使用。延遲計算的預設值用對應的 `OrElse` 方法。快取值或預設值的型別不符時回傳 `CacheError::InvalidType`。字串和布林值按型別驗證，數字方法接受有效的數字字串，`array` 接受 JSON 陣列和物件。

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn report_limit() -> CacheResult<i64> {
    Cache::integer("reports:limit", Some(Value::from(10))).await
}
```

Redis 保留新寫入的數字字串及其原始寫法。既有整數項目可繼續按整數讀取。增減數字字串時會使用 Redis 指令碼把項目轉成整數，並保留到期時間。

### 批次操作 {#cache-batch-operations}

`getMultiple(keys, default)` 按輸入順序回傳對應表，可傳入統一的預設值。`manyOr` 為每個鍵接收預設值，`manyOrElse` 按鍵延遲計算預設值。批次讀取只呼叫一次儲存空間，缺少的鍵和 null 使用預設值。`set` 和 `setMultiple` 寫入，`delete` 和 `deleteMultiple` 移除項目，`clear` 清空所選儲存庫。批次刪除遇到缺少的鍵會繼續，並在任一鍵缺少時回傳 `false`。

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn report_state() -> CacheResult<Value> {
    let entries = Cache::manyOr(&[
        ("reports:title", Value::from("Untitled")),
        ("reports:limit", Value::from(10)),
    ]).await?;
    Ok(entries["reports:title"].clone())
}
```

### 讀取並回填 {#retrieve-store}

`remember` 回傳快取值，未命中時執行計算並寫入快取。`rememberWithWarmth` 回傳 `(value, warm)`，命中非空值時 `warm` 為 `true`。`rememberWithWarmthFor` 在未命中時根據計算結果設定 TTL。`rememberForever` 和別名 `sear` 將計算結果寫入快取且不設定到期時間。

```rust
use openrust::{cache::{CacheResult, Ttl, Value}, support::facades::Cache};

pub async fn load_report() -> CacheResult<(Value, bool)> {
    Cache::rememberWithWarmth("reports:latest", Some(Ttl::Seconds(60)), async {
        Ok(Value::from("ready"))
    }).await
}
```

### 到期後背景更新 {#swr}

`flexible(key, [fresh, expires], resolve)` 接收從同一次寫入開始計時的兩個秒數。在 `fresh` 之前直接讀取快取。在 `fresh` 和 `expires` 之間先回傳舊值，再於成功的回應、命令或工作結束後更新。快取未命中或已到期時，先計算再回傳。

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn refresh_report() -> CacheResult<Value> {
    Cache::flexible("reports:summary", [5, 10], || async {
        Ok(Value::from("ready"))
    }).await
}
```

背景更新使用快取原子鎖。如果其他呼叫端已更新同一代快取，等待執行的更新會略過計算。更新失敗時，舊值可以繼續使用到原定到期時間。`flexibleWith` 接受兩個 `Ttl` 和 `FlexibleOptions`，可設定鎖租期、擁有者權杖或失敗後仍更新的 `always_defer`。在應用程式生命週期之外直接呼叫儲存庫時，可讀取舊值直到到期。背景計算需要擷取擁有所有權的值，以便在請求處理函式回傳後執行。

### 取回並刪除 {#retrieve-delete}

`pullOr(key, default)` 和 `pullOrElse(key, resolve)` 支援預設值。兩者先執行儲存庫的原子 `pull`，項目缺少或值為 `null` 時再使用預設值。兩個呼叫同時取回同一非空項目時，只有一個能取得該值。

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

在支援標籤的儲存上，可用 `Cache::memo(Some("redis"))?.tags(&["reports", "daily"])?`。同一生命週期內，完全相同的有序標籤清單會共用命中和未命中，改變順序或重複項目會選取其他標籤群組。透過記憶化標籤儲存庫寫入會使對應鍵失效。呼叫標籤儲存庫的 `flush()` 會清空後端所選標籤，並使該儲存的所有標籤群組快照失效。呼叫 `clear()` 會清空整個儲存，包括一般快取值。呼叫 `flushMemoized()` 只清除所選標籤群組的記憶化結果。

在 Redis 上，`flushStale().await?` 會轉送給所選標籤，清理到期索引參照，並保留有效值及記憶化結果。透過獨立的一般 Redis 儲存庫清空標籤後，本生命週期仍可讀取已記憶化的舊結果，直到記憶化檢視使它們失效。

## cache 輔助工具 {#the-cache-helper}

`cache!()` 或 `openrust::support::cache()` 取得目前應用程式的快取管理員。呼叫 `.store(None)?` 取得預設儲存庫。`cache!(key).await?` 回傳 `Option<Value>`，`cache!(key, default).await?` 支援預設值。

寫入形式接受至少一組鍵值和 `Option<Ttl>`。與固定 Laravel 的輔助工具一致，它只寫入第一項。需要寫入全部項目時使用 `Cache::putMany`。

```rust
use openrust::{cache, cache::{CacheResult, Ttl, Value}};

pub async fn report() -> CacheResult<Value> {
    cache!({ "reports:latest" => Value::from("ready") }, Some(Ttl::Seconds(60))).await?;
    cache!("reports:latest", Value::from("pending")).await
}
```

## 儲存庫設定 {#cache-repository-settings}

`getName`、`getStore` 和 `getEventDispatcher` 分別讀取已設定的儲存庫名稱、底層儲存空間和事件分派器。具名儲存庫停用快取事件後仍可讀取名稱。建立標籤儲存庫時，會繼承來源儲存庫的名稱、事件分派器和預設有效時間。

`putWithDefault` 使用 `getDefaultCacheTime` 回傳的時間，初始值為 3600 秒。明確呼叫 `put(key, value, None)` 會永久保存。使用 `setDefaultCacheTime` 修改自有儲存庫的預設有效時間。`setStore` 和 `setEventDispatcher` 也用於自有儲存庫。複製後的儲存庫共用底層儲存空間，每份副本可以各自取代設定。

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn cache_report() -> CacheResult<bool> {
    let mut repository = Cache::store(Some("redis"))?.as_ref().clone();
    repository.setDefaultCacheTime(Some(60));
    let reports = repository.tags(&["reports"])?;
    reports.putWithDefault("latest", &Value::from("ready")).await
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

呼叫 `Cache::tags(&["daily"])?.flushStale().await?` 可清理指定 Redis 標籤的到期參照。它保留有效快取值和其他標籤索引。傳入空清單時不存取 Redis。

執行 `./artisan cache:prune-stale-tags redis` 可清理整個儲存。兩種方式都以 Redis 伺服器時間與有序集合記錄的到期時間判斷參照是否到期，也會移除值鍵已不存在的參照。清理只修改索引，快取值與仍存在的永久項目參照均會保留。可指定儲存名稱，或省略以使用預設儲存。

指定 `array` 等內建非 Redis 儲存時，命令會直接成功結束。例如，即使預設快取儲存是 Redis，執行 `./artisan cache:prune-stale-tags array` 也會保留 Redis 標籤索引。

在 Redis Cluster 中，儲存層級的清理也會在負責該儲存標籤插槽的主節點尋找標籤索引。若 Redis 淘汰了索引登記集合，清理可重新發現仍存在的索引、移除過期參照並保留有效值。若待回收世代的索引與登記都已遺失，清理也會移除對應的待回收記錄。清理也會移除格式無效的待回收世代識別碼。命令在同一插槽內每批最多檢查 128 個待回收世代。仍有有效索引參照的世代會保留，讓後續按標籤清空時可以繼續處理。

在 Redis Cluster 中，`flushStale()` 會尋找所選標籤各世代仍存在的索引，也會移除這些標籤中已無索引參照或格式無效的待回收記錄，包括索引已被淘汰的世代。所選標籤的索引遺失後，即使沒有待回收記錄，也會移除殘留的索引登記。仍有有效參照的待回收世代會保留，供後續按標籤清空時繼續處理。

即使標籤索引的登記已遺失，索引鍵型別異常也會使清理失敗。清理範圍內的待回收集合型別異常時，清理也會失敗。命令以狀態碼 1 結束，恢復正確的鍵型別後可重新清理。

Redis Cluster 插槽遷移期間，清理批次涉及缺少的鍵，或同一批次的鍵分布於不同節點時，Redis 可能回傳 `TRYAGAIN`。此時 `flushStale()` 回傳錯誤，`cache:prune-stale-tags` 以狀態碼 1 結束。插槽遷移完成後可重試同一清理。已完成的批次會保留結果，重試時會略過已移除的參照。

如果 Redis Cluster 中的標籤清空中斷，可再次執行 `./artisan cache:clear redis --tags=artists`，繼續刪除舊世代的值。重試時會分批清理已退役的世代，並移除格式無效的世代識別碼。如果 Redis 鍵型別異常，命令會以狀態碼 1 結束。恢復正確的鍵型別後即可重試。

在 Redis Cluster 上，框架依連線前綴和儲存前綴為標籤命名空間選擇一個雜湊槽，讓值與標籤索引可透過 Lua 原子更新。當 `forget` 或 `pull` 移除某個標籤索引的最後一個成員時，同一次原子更新也會移除該索引的登記。按標籤清空和到期清理都分批掃描標籤索引。遇到格式錯誤或指向目前儲存標籤命名空間之外的參照時，只移除索引參照，保留其指向的鍵。到期清理也會移除無效的索引登記。如果標籤清空中斷並留下待回收的舊世代，到期清理會在該世代的索引清空時移除對應記錄。一般快取鍵繼續按叢集規則分片。單槽成為瓶頸時，可使用不同儲存或前綴分散標籤快取負載。

## 原子鎖

用 `Cache::lock` 建立鎖的控制代碼。`get` 嘗試一次，`block` 在限定時間內等待。持有者可釋放或更新有效的鎖。跨處理程序釋放時，用持有者權杖呼叫 `Cache::restoreLock`。使用有限租期可在處理程序提前結束後復原。

### 鎖定狀態與重試間隔 {#lock-status}

`isLocked().await?` 查詢後端是否持有鎖。`isOwnedBy(token).await?` 比對指定權杖，`isOwnedByCurrentProcess().await?` 比對目前控制代碼的權杖。`isOwnedBy(None).await?` 檢查是否沒有擁有者。狀態查詢不會取得鎖或續期。`null` 儲存庫始終回報未鎖定，擁有者查詢使用目前控制代碼的權杖。

`betweenBlockedAttemptsSleepFor(milliseconds)` 設定傳回之控制代碼的 `block` 和 `blockWith` 重試間隔，預設為 250 毫秒。間隔為 0 時，每次重試前會讓出 Tokio 執行權。

```rust
use std::time::Duration;

use openrust::{cache::CacheResult, support::facades::Cache};

pub async fn ready_for_report() -> CacheResult<bool> {
    let mut lock = Cache::lock("report", Some(Duration::from_secs(10)))?
        .betweenBlockedAttemptsSleepFor(25)?;
    lock.block(Duration::from_secs(2)).await?;
    let owned = lock.isOwnedByCurrentProcess().await;
    lock.release().await?;
    owned
}
```

### 持鎖執行回呼 {#lock-callbacks}

用 `getWith` 嘗試一次，或用 `blockWith` 等待。兩者接管鎖的控制代碼，在回呼傳回時釋放鎖，包含回呼出錯的情況。鎖被占用時，`getWith` 傳回 `None`。等待逾時則由 `blockWith` 傳回 `CacheError::LockTimeout`。取得失敗時均不會呼叫回呼。

```rust
use std::time::Duration;

use openrust::{
    cache::{CacheResult, Ttl, Value},
    support::facades::Cache,
};

pub async fn cache_report() -> CacheResult<Option<bool>> {
    Cache::lock("report", Some(Duration::from_secs(10)))?
        .getWith(|| async {
            Cache::put("reports/latest", &Value::from("ready"), Some(Ttl::Seconds(60))).await
        })
        .await
}
```

Future 被取消或回呼 panic 時，Tokio 會排程清理。執行階段結束而無法清理時，有限租期可讓鎖到期。

### 並行限制 {#concurrency-limiting}

`Cache::withoutOverlapping(key, callback).await?` 對同一個鍵一次只執行一個回呼，最多等待 10 秒。`withoutOverlappingWith(key, callback, lock_for, wait_for, owner)` 以秒指定租期和等待時間，並接受選用的擁有者權杖。租期為零時使用驅動程式的預設值。無法取得鎖時回傳 `CacheError::LockTimeout`。

使用 `Cache::funnel` 允許多個回呼並行執行。各應用程式執行個體須使用同一個共用鎖儲存空間、快取前綴、限制器名稱和並行上限。租期應涵蓋回呼的執行時間，到期後其他呼叫端可以取得該名額。`null` 儲存庫允許所有呼叫通過。

```rust
use openrust::{cache::CacheResult, support::facades::Cache};

pub async fn generate_report() -> CacheResult<&'static str> {
    Cache::funnel("reports:generate:")?
        .limit(3)
        .releaseAfter(60)
        .block(10)
        .thenWith(|| async { Ok("generated") }, |_| async { Ok("busy") })
        .await
}
```

用 `limit` 設定並行名額。租期預設 60 秒，等待預設 3 秒，`sleep(milliseconds)` 預設 250 毫秒。`block(0)` 只掃描一次名額。`then(callback)` 回傳 `CacheError::LimiterTimeout`，`thenWith(callback, fallback)` 可在回呼中處理該錯誤。可用 `Cache::store(Some("redis"))?.funnel(name)?` 選取具名儲存庫。缺少原子鎖能力的儲存庫回傳 `CacheError::LocksUnsupported`。

### 清除快取鎖 {#clearing-cache-locks}

使用 `Cache::flushLocks().await?` 清除預設儲存的鎖，或用 `Cache::store(Some("database"))?.flushLocks().await?` 指定儲存。Artisan 也可以指定儲存名稱。

```sh
./artisan cache:clear database --locks
./artisan cache:clear redis --tags=reports,daily
```

清鎖命令會清除所選鎖儲存內的所有鎖，包括共用相同後端的其他前綴。陣列和檔案儲存分別保存鎖與一般項目。資料庫需要獨立的鎖資料表。Redis 會清空整個專用鎖資料庫。同一 Redis 執行個體應使用不同的邏輯資料庫編號。不同入口設定相同編號時，OpenRust 會先核對 Redis 執行個體識別碼，連線需允許 `INFO SERVER`。

僅清除鎖支援獨立的 Redis 邏輯資料庫或獨立的 Redis Cluster。Redis 會清空整個鎖後端。快取連線與鎖連線須指向不同的邏輯資料庫或叢集。涉及叢集時，需允許 `CLUSTER INFO`、`CLUSTER NODES`，直連的一端另需允許 `INFO CLUSTER`，以便框架在清除前辨識指向同一叢集的別名。 [Redis 叢集設定](../database/redis.md)

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
