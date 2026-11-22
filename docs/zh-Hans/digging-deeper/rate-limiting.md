# 限流

## 基本用法

用 `RateLimiter::attempt` 检查计数键、运行异步操作并记录成功结果。`Attempt::Throttled` 表示回调未执行。最后一个参数是窗口秒数。

```rust
use openrust::{
    cache::{CacheResult, rate_limiting::Attempt},
    support::facades::RateLimiter,
};

async fn send_message(key: &str) -> CacheResult<Attempt<&'static str>> {
    RateLimiter::attempt(key, 5, async { Ok("sent") }, 60).await
}
```

用 `hit` 直接消耗一次额度，`attempts` 和 `remaining` 查询额度，`availableIn` 查询重置时间，`clear` 清除计数。限流器使用 `cache.limiter` 指定的缓存存储，未指定时使用默认存储。所选存储需要支持原子锁。
