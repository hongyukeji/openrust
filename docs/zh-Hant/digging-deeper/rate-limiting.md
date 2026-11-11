# 限流

## 基本用法

用 `RateLimiter::attempt` 檢查計數鍵、執行非同步操作並記錄成功結果。`Attempt::Throttled` 表示回呼未執行。最後一個參數是視窗秒數。

```rust
use openrust::{
    cache::{CacheResult, rate_limiting::Attempt},
    support::facades::RateLimiter,
};

async fn send_message(key: &str) -> CacheResult<Attempt<&'static str>> {
    RateLimiter::attempt(key, 5, async { Ok("sent") }, 60).await
}
```

用 `hit` 直接消耗一次額度，`attempts` 和 `remaining` 查詢額度，`availableIn` 查詢重設時間，`clear` 清除計數。限流器使用 `cache.limiter` 指定的快取儲存區，未指定時使用預設儲存區。所選儲存區須支援原子鎖。
