# Rate Limiting

## Basic Usage

Use `RateLimiter::attempt` to check a key, run the asynchronous operation and count a successful result. `Attempt::Throttled` means the callback did not run. The final argument is the decay period in seconds.

```rust
use openrust::{
    cache::{CacheResult, rate_limiting::Attempt},
    support::facades::RateLimiter,
};

async fn send_message(key: &str) -> CacheResult<Attempt<&'static str>> {
    RateLimiter::attempt(key, 5, async { Ok("sent") }, 60).await
}
```

Use `hit` to consume an attempt directly, `attempts` and `remaining` to inspect the budget, `availableIn` to get the reset delay, and `clear` to remove the counter. The limiter uses the cache store selected by `cache.limiter`, or the default store. Select a store with atomic locks.
