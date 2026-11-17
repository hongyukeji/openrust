# 会话

## 配置

用 `SESSION_DRIVER=database` 将会话保存在配置的数据库和 `sessions` 表中。使用会话路由前先运行应用迁移。`array` 驱动将数据保存在当前应用进程中。用 `SESSION_ENCRYPT=true` 加密存储载荷。

配置认证守卫后，数据库驱动在请求结束时把当前守卫的用户 ID 写入 `user_id`。访客请求写入 `NULL`。自定义会话表的 `user_id` 列类型应能保存应用使用的用户标识。

用 `SESSION_DRIVER=file` 将会话保存在 `storage/framework/sessions`。可通过应用配置的 `session.files` 指定其他目录。`native` 是 `file` 的别名。用 `SESSION_DRIVER=null` 在当前请求中使用会话，请求结束后丢弃会话状态。

用 `SESSION_DRIVER=redis`、`memcached` 或 `dynamodb` 将会话保存在缓存后端。`SESSION_STORE` 选择具名缓存存储，默认使用驱动名。Redis 的 `SESSION_CONNECTION` 选择连接名，默认为 `default`。应用配置中的 `session.prefix` 可覆盖 Redis 会话使用的缓存前缀。会话使用独立的 Redis 仓库，普通缓存保留自己的连接和前缀。后端在 `SESSION_LIFETIME` 分钟后使载荷过期。

用 `SESSION_DRIVER=cookie` 将会话保存在两个加密 Cookie 中。配置的会话 Cookie 保存 ID，以该 ID 命名的 Cookie 保存载荷和有效期。`SESSION_EXPIRE_ON_CLOSE` 使浏览器在关闭时丢弃 Cookie，载荷仍受有效期约束。加密后的载荷 Cookie 响应头上限为 4096 字节。轮换并删除旧会话时，当前浏览器会收到旧载荷 Cookie 的过期指令。已复制的一对 Cookie 在有效期内仍可重放。会话阻塞需要服务端驱动。

配置会话驱动后，默认 `web` 组会在处理器运行前启动会话。自定义组需要把 `EncryptCookies` 放在 `StartSession` 前面。 Cookie 驱动还需要在两者之间放置 `AddQueuedCookiesToResponse`。

## 使用会话

在路由中间件栈中按顺序配置 `EncryptCookies` 和 `StartSession`，再用 `request.session()` 或 `request.session_mut()` 访问已加载的会话。路由返回后，中间件保存会话并附加会话 Cookie。导入 `Session` trait 即可调用会话方法。

```rust
use openrust::{
    contracts::session::Session,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};

fn remember(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let session = request.session_mut()?;
        session.put("profile.name", "Ada".into())?;
        session.flash("message", "Saved".into())?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
```

`only` 筛选顶层键，`except` 从副本中移除指定键或点号路径。`exists` 和 `missing` 区分显式空值与缺失键，`has` 和 `hasAny` 检查非空值。`remember` 接收返回 `Result` 的闭包，只在键缺失或值为空时计算并保存。

用 `push` 向数组或关联数组追加值。`increment` 和 `decrement` 接收显式整数步长，返回更新后的 JSON 值。输入无效时返回错误并保留原值。

## 闪存数据

`flash` 写入的值在当前请求和下一次请求可用。用 `keep` 延长指定字段的可用期，或用 `reflash` 将所有闪存数据保留到再下一次请求。

## 重新生成会话 ID

`regenerate(true)` 删除存储中的旧 ID、保留会话属性并轮换 CSRF 令牌。用 `invalidate()` 清空会话属性并轮换 ID。

## 会话缓存 {#session-cache}

在已启动会话的路由中调用 `request.session()?.cache()?`。它与 `Cache::store(Some("session"))` 取得同一个仓库。缓存条目随当前会话保存，支持常用缓存操作和过期时间。`flush()` 只清除当前缓存命名空间，保留普通会话属性和其他缓存命名空间。重新生成会话 ID 会保留缓存，使会话失效则会清空缓存。

```rust
use openrust::{
    cache::Ttl,
    contracts::cache::Repository as _,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};

fn cache_banner(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let cache = request.session()?.cache()?;
        cache.put("dashboard.banner", &"ready".into(), Some(Ttl::Seconds(300))).await?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
```

`SESSION_CACHE_KEY` 配置命名空间，默认是 `_cache`。会话驱动随会话数据持久化缓存。操作需要活跃的 HTTP 会话，因此 `./artisan cache:clear session` 无法清除这些条目。同一会话的请求需要串行写入时，可使用支持锁的缓存仓库来启用会话阻塞。

## 会话阻塞

默认情况下，使用同一个会话的请求可以并发执行。注册会话路由时调用 `block(lock_seconds, wait_seconds)`，可以让使用同一个会话 ID 的请求依次执行。

```rust
use openrust::support::facades::Route;

Route::getWith("/profile", remember, |route| {
    route.block(10, 10);
    Ok(())
})?;
```

第一个参数是锁的有效期，第二个参数是等待已被占用的锁的最长时间。中间件在读取会话前获取锁，保存后释放。等待超时交由异常处理器处理，默认返回 HTTP 500。

用 `SESSION_BLOCK=true` 为所有启动会话的路由启用阻塞。用 `SESSION_BLOCK_LOCK_SECONDS` 和 `SESSION_BLOCK_WAIT_SECONDS` 设置默认值，两者均默认 10 秒。路由上的显式设置覆盖默认值。`withoutBlocking()` 清除路由级设置。开启全局阻塞后，这条路由仍使用全局设置。

用 `SESSION_BLOCK_STORE` 选择支持原子锁的缓存存储，例如 `redis`。省略该配置时使用默认缓存存储。多个应用进程需要使用共享存储，`array` 缓存只协调同一进程中的请求。
