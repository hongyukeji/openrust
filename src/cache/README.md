# 缓存

Provider 装配的缓存 store 按名称异步构造并缓存；`store/default_store` 需要 await。driver 的 `validate` 默认不执行专属检查，自定义 driver 应覆盖它，才能在最终配置校验时拒绝无效选项。

`CacheServiceProvider` 支持在统一容器中装配内置和自定义 driver。

`CacheServiceProvider` 按 `[cache]` 配置启动默认 store 并按需初始化命名 store。缓存能力为可选项：不安装 Provider 时不会连接 Redis；安装后必须显式配置 store，未配置、driver 不支持、初始化失败或缓存 I/O 失败均返回错误，不会回退到本地内存。容器中注册了共享 `Readiness` 时，每个已解析 store 会注册只读依赖探针；其 I/O 失败会令 `/ready` 返回 503，错误细节不会出现在健康响应中。

```toml
[cache]
default = "redis"

[cache.stores.redis]
driver = "redis"
namespace = "app"
dsn = "redis://127.0.0.1:6379/"

[cache.stores.local]
driver = "memory"
namespace = "local"

[cache.singleflight]
max_in_flight_keys = 128
max_waiters_per_key = 64
loader_timeout_secs = 30

[cache.swr]
max_in_flight_refreshes = 64
```

启用 Redis 时，在 OpenRust 依赖中选择 `cache-redis` feature。常规配置使用 `REDIS_HOST`、`REDIS_USERNAME`、`REDIS_PASSWORD`、`REDIS_PORT` 和 `REDIS_DB` 分项变量；缓存与队列共用同一 Redis 数据库编号。`CACHE_STORE` 映射到 `cache.default`，选择的名称必须在 `cache.stores` 中登记；旧 `REDIS_DEFAULT_STORE` 与 `CACHE_DEFAULT_STORE` 均不再选择 store。具名 `redis` 是框架默认，业务可登记名为 `default` 的 memory 或自定义 driver，并用 `CACHE_STORE=default` 选择。需要单独 store 或连接时，在 `config/cache.toml` 或对应的 `APP_CACHE_STORES_<NAME>_DSN` 中配置。

通过 `Inject<Cache>` 或应用状态取得默认服务，`Cache::store("local")` 显式选择命名实例。`get` 返回 JSON 类型化值或 `None`，`set` 支持可选 TTL，`remove` 删除键。零 TTL、空 key 和长度超过 1024 字节的 key 会被拒绝；值编解码、驱动初始化和 I/O 失败都不会伪装成未命中。

若调用发生在 `TenantContext` 可信作用域内，缓存 key、singleflight 合并键及 tag generation 均自动加入租户隔离；无租户作用域时沿用原有 key 行为。task-local 不会自动跨 `tokio::spawn`；SWR entry 会固定其创建时的租户物理键，后台 loader 若要读取租户业务数据仍须显式恢复可信作用域。

内置 `memory` 仅在进程内共享；`file` 使用 SHA-256 文件名、毫秒 TTL 元数据及同目录 rename 替换，仅在节点本地适用（原子替换语义由目标操作系统的 rename 保证）；Redis 标记为共享后端。tag generation 仅由实现可选原子能力的 backend 支持；支持 tag 的 driver 可使用 stale-while-revalidate 与有界的进程内同键加载合并。跨应用 SWR 刷新锁需要配置共享 `SharedLocks`；缓存不是权限、会话撤销等权威状态的替代品。

支持可选 `CacheTagBackend` 的驱动可使用 tag generation。先捕获 entry 再进行慢加载，写入会校验 generation 未变化：

```rust,ignore
let tagged = store.tagged(&["users"])?;
let entry = tagged.entry("profile:42").await?;
let value = match entry.get::<Profile>().await? {
    Some(value) => value,
    None => {
        let value = load_profile().await?;
        entry.set(&value, Duration::from_secs(300)).await?;
        value
    }
};
store.invalidate_tags(&["users"]).await?;
```

memory 提供进程内原子 generation；Redis 通过 Lua 比较代际后写入，并在多个 Redis 连接之间共享失效。file 与未实现可选能力的 custom driver 明确返回不支持。tagged 值必须带有限 TTL；失效不扫描或删除旧 key，generation 元数据不设过期以防旧代复活，因此 tag 应使用有界、稳定名称，不能直接使用用户输入。多 tag 失效逐个原子执行，整体不保证全有或全无。

`CacheStore::get_or_load_with(key, load_id, ttl, loader)` 在一个进程中的同一 store 上合并相同 key、返回类型和稳定 `load_id` 的并发 miss。默认限制为 128 个在途 key、每个 key 64 个等待者、loader 最长 30 秒；可在 `[cache.singleflight]` 调整，边界分别为 1–4096 个 key、1–4096 个等待者和 1–300 秒。容量耗尽会报错；leader 失败时等待者收到 `cache_loader_failed`，leader 被取消时单个等待者会重新竞争。该机制不提供跨进程恰好一次，外部副作用仍需业务幂等。

`load_id` 是调用方提供的稳定加载语义版本，用于区分并发 loader；同一 store/key 应长期使用相同类型与 load ID。示例：

```rust,ignore
let user = store
    .get_or_load_with("user:42", "user-profile-v1", Some(Duration::from_secs(300)), || async {
        load_user_profile(42).await
    })
    .await?;
```

`TaggedCacheStore::get_stale_while_revalidate(key, load_id, fresh_ttl, stale_ttl, loader)` 只对 tag store 开放。首次 miss 与超过 `fresh_ttl + stale_ttl` 的条目会等待 loader；fresh 窗口返回新值；stale 窗口立即返回旧值并安排后台刷新。后台刷新受 `[cache.swr].max_in_flight_refreshes`（1–256，默认 64）、singleflight 在途 key 上限与 loader timeout 限制。tag generation 在返回 stale 前再次核验，刷新写入也以捕获的 generation 条件提交；tag 失效优先于旧值。到达 stale 窗口末尾后，后端 TTL 删除条目，不会继续提供陈旧值。

应用持有后台刷新任务。`App::run_command` 结束、HTTP `serve` 收到停机信号或 listener 异常时会合作取消并 join；超过 `[server].shutdown_timeout_secs` 后 abort 剩余 task 并返回有界停机错误。loader future 被取消可能已产生外部副作用，仍需业务幂等。若应用显式注册 `SharedLocks`，刷新前会尝试数据库共享租约，减少多实例重复刷新；租约仅 best-effort，过期或节点暂停仍可能重复执行，不保证恰好一次。未注册共享锁时仅在当前进程合并。Redis 使用 Redis `TIME` 计算跨连接 fresh 边界；memory/custom tag driver 使用其后端时钟，分布式 custom driver 应覆盖该时钟契约。

```rust,ignore
let profile = store
    .tagged(&["profiles"])?
    .get_stale_while_revalidate(
        "user:42",
        "profile-v1",
        Duration::from_secs(30),
        Duration::from_secs(120),
        || async { load_user_profile(42).await },
    )
    .await?;
```

扩展包可实现 `CacheDriver` 与 `CacheBackend`，通过 `CacheServiceProvider::new().with_driver(driver)` 在构造时登记，或在后续 Provider boot 中异步解析 `Cache` 并调用 `extend(driver)`。默认 store 在 boot 时解析；其他 named store 按需异步构造并缓存。同名 creator 替换不会改变已解析的 store；`forget_driver` / `purge` 后重新解析会使用当前配置和 creator。driver 可实现 `validate` 以在最终配置校验时检查专属选项；未实现时框架无法验证其自定义选项。自定义选项使用 `[cache.stores.<name>]` 下除 `driver`、`namespace` 以外的字段。
