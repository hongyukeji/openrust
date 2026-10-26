# 缓存

`CachePlugin` 按 `[cache]` 配置初始化默认与命名 store。缓存能力为可选项：不安装插件时不会连接 Redis；安装后必须显式配置 store，未配置、driver 不支持、初始化失败或缓存 I/O 失败均返回错误，不会回退到本地内存。

```toml
[cache]
default_store = "default"

[cache.stores.default]
driver = "redis"
namespace = "app"
dsn = "redis://127.0.0.1:6379/"

[cache.stores.local]
driver = "memory"
namespace = "local"
```

启用 Redis 时，在 OpenRust 依赖中选择 `cache-redis` feature。`REDIS_URL` 与应用命名空间变量（默认 `APP_REDIS_URL`）均覆盖默认 Redis store 的 `dsn`；命名空间变量优先。

通过 `Inject<Cache>` 或应用状态取得默认服务，`Cache::store("local")` 显式选择命名实例。`get` 返回 JSON 类型化值或 `None`，`set` 支持可选 TTL，`remove` 删除键。零 TTL、空 key 和长度超过 1024 字节的 key 会被拒绝；值编解码、驱动初始化和 I/O 失败都不会伪装成未命中。

内置 `memory` 仅在进程内共享；`file` 使用 SHA-256 文件名、毫秒 TTL 元数据及同目录 rename 替换，仅在节点本地适用（原子替换语义由目标操作系统的 rename 保证）；Redis 标记为共享后端。tag generation 仅由实现可选原子能力的 backend 支持；缓存目前不提供通用原子增量、锁、SWR 或同键加载合并，这些边界由后续功能簇处理。缓存不是权限、会话撤销等权威状态的替代品。

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

扩展包可以实现 `CacheDriver` 与 `CacheBackend`，并在插件 `build` 阶段通过 `AppBuilder::extension_mut::<CacheDrivers>()` 注册；`CachePlugin::finish` 会在所有插件完成注册后解析实例。自定义选项使用 `[cache.stores.<name>]` 下除 `driver`、`namespace` 以外的字段，配置调试输出只显示选项名称。
