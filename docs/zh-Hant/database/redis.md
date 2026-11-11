# Redis

## 設定

在 `config/database.rs` 的 `database.redis` 下定義連線。應用程式提供 `default` 和 `cache` 連線。透過 `REDIS_HOST`、`REDIS_PORT`、`REDIS_USERNAME` 和 `REDIS_PASSWORD` 設定端點及憑證。兩個連線預設分別使用邏輯資料庫 0 和 1，可用 `REDIS_DB` 和 `REDIS_CACHE_DB` 修改。

可透過 `REDIS_URL` 設定端點。URL 內的資料庫編號優先於設定的編號。若要讓兩個連線使用同一 Redis 執行個體的不同邏輯資料庫，請省略 URL 中的資料庫路徑。

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
