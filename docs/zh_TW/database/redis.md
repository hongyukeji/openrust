# Redis

## 設定

在 `config/database.rs` 的 `database.redis` 下定義連線。應用程式提供 `default` 和 `cache` 連線。透過 `REDIS_HOST`、`REDIS_PORT`、`REDIS_USERNAME` 和 `REDIS_PASSWORD` 設定端點及憑證。兩個連線預設共用 `REDIS_DB` 指定的邏輯資料庫，未設定時使用資料庫 0。可用 `REDIS_CACHE_DB` 為快取資料指定獨立的邏輯資料庫。執行 `cache:clear redis` 會清空快取邏輯資料庫的所有鍵，包括同庫的其他用途鍵。

可透過 `REDIS_URL` 設定端點。URL 內的資料庫編號優先於設定的編號。若要讓兩個連線使用同一 Redis 執行個體的不同邏輯資料庫，請省略 URL 中的資料庫路徑。

### 叢集 {#clusters}

在 `config/database.rs` 的 `database.redis.clusters` 中定義具名的種子節點清單。同名直連和叢集同時存在時，優先使用直連。`options.cluster` 使用 `redis` 選取原生 Redis Cluster。叢集使用資料庫 0，並透過種子節點探索其他節點。

```json
{
  "clusters": {
    "sharded": [
      { "host": "127.0.0.1", "port": 7000 },
      { "url": "redis://127.0.0.1:7001/0" }
    ]
  }
}
```

透過 `Redis::connection(Some("sharded"))` 選取叢集。原生命令接受 Redis 參數。需要多個鍵位於同一槽位時，使用共同的雜湊標籤，例如 `{reports}`。

```rust
use openrust::{redis::RedisResult, support::facades::Redis};

pub async fn cluster_report() -> RedisResult<Option<Vec<u8>>> {
    let connection = Redis::connection(Some("sharded"))?;
    connection.set("{reports}:latest", b"ready").await?;
    connection.get("{reports}:latest").await
}
```

Redis 回傳 `MOVED` 或 `ASK` 時，OpenRust 會為內建單鍵操作跟隨重新導向，包括快取鎖。重新導向次數由第一個種子節點的 `max_retries` 限制，設為零時直接回傳重新導向錯誤。連線中斷時 Redis 可能已執行指令，框架會回傳錯誤並讓後續請求重新連線。主節點切換後，後續請求可以找到新的主節點。未涵蓋的原始指令可能回傳重新導向錯誤。佇列 Worker 的專用阻塞連線使用直連。

## 與 Redis 互動

在應用程式作用域內使用 `Redis` 門面。`Redis::get` 回傳可選的位元組值，`Redis::set` 寫入位元組，兩者都會套用 `REDIS_PREFIX`。用 `Redis::connection(Some("cache"))` 選擇其他連線。

```rust
use openrust::{redis::RedisResult, support::facades::Redis};

pub async fn redis_report() -> RedisResult<Option<Vec<u8>>> {
    Redis::set("reports/latest", b"ready").await?;
    Redis::get("reports/latest").await
}
```

使用 `command` 執行原始 Redis 命令。需要為原始命令的鍵加上前綴時，用 `connection.key("name")` 建立參數。Pub/Sub 使用獨立連線。透過 Redis 連線契約呼叫 `subscribe` 或 `psubscribe`，再用 `next` 讀取訊息。

命令連線中斷後，連線管理器會為後續請求重新連線。結果尚未確認的命令可能回傳錯誤，應用程式應只重試可安全重複的操作。連線重試次數與延遲上下限透過 `REDIS_MAX_RETRIES`、`REDIS_BACKOFF_BASE` 和 `REDIS_BACKOFF_CAP` 設定。

## Redis 快取

設定 `CACHE_STORE=redis` 可選擇 Redis 作為預設快取。`REDIS_CACHE_CONNECTION` 指定資料連線，`REDIS_CACHE_LOCK_CONNECTION` 指定鎖連線，預設分別為 `cache` 和 `default`。到期及鎖操作請參閱[快取](../digging-deeper/cache.md)。

將 `REDIS_CACHE_CONNECTION` 或 `REDIS_CACHE_LOCK_CONNECTION` 設定為叢集名稱，可將快取值或個別鎖放在叢集上。跨槽批次讀取維持輸入順序，批次寫入逐項路由。`cache:clear` 清空所有主節點。快取標籤目前使用直連。收到明確的槽重新導向時，快取批次讀取會逐鍵查詢。

僅清除鎖支援獨立的 Redis 邏輯資料庫或獨立的 Redis Cluster。Redis 會清空整個鎖後端。快取連線與鎖連線須指向不同的邏輯資料庫或叢集。涉及叢集時，需允許 `CLUSTER INFO`、`CLUSTER NODES`，直連的一端另需允許 `INFO CLUSTER`，以便框架在清除前辨識指向同一叢集的別名。
