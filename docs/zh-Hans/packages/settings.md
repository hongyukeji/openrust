# 数据库设置（扩展）

`openrust-settings` 将类型化应用设置存入数据库，可用于网站信息、服务配置及业务策略等设置组。命名参考 Spatie框架Settings，复用 OpenRust 的数据库、加密、迁移和容器作用域。

## 启用与迁移

通过 `settings` Cargo feature 启用可选包。包发现登记自身 Provider 和默认表迁移，不注册应用业务设置组：

```sh
cargo run --features settings -- artisan migrate
```

包自身提供默认配置，默认应用不包含 `config/settings.rs`。需要定制时运行 `./artisan vendor:publish --tag=settings-config`，在 `config/mod.rs` 的模块声明和配置映射中分别加入 `mod settings;` 与 `("settings".to_owned(), settings::load(env)?),`，然后重新构建。修改表名或连接时保留自己的对应迁移。

其他应用添加 `openrust-settings` 依赖并使用 OpenRust 包发现。声明自己的设置类型，在包注册完成后，由该应用的 Provider 调用 `SettingsServiceProvider::register_settings::<YourSettings>(app).await?`。默认应用没有预注册的 `SiteSettings` 业务组。首次读取前运行迁移；Provider 注册和启动不查询设置表。

`settings.repository` 默认为 `database`；`settings.repositories.database.connection` 默认使用应用数据库连接，`table` 默认为 `settings`。自定义连接或表时，登记相应的 `CreateSettingsTable` 迁移并在该连接运行，替代默认迁移。迁移仍使用已有台账和执行器。

## 声明与读写

为派生 `Clone`、`Serialize`、`Deserialize` 的结构体实现 `SettingsGroup`。`group()` 声明唯一组名，`defaults(config)` 返回完整默认对象，`validate()` 校验业务约束，`encrypted()` 声明敏感的顶层属性。序列化字段集合须稳定，不要按值省略字段。组名与属性名使用不带限定符的 ASCII 标识符，最多 128 字节；属性名前缀 `__settings_` 保留给内部使用。

在请求或任务作用域中，从应用／容器异步解析 `Settings<YourSettings>`。可信应用代码可这样操作：

```rust,ignore
let settings = app.make::<Settings<SiteSettings>>().await?;
let current = settings.get().await;
settings.fill(serde_json::json!({"title": "我的网站"})).await?;
settings.save().await?;
settings.reset("title").await?;
settings.save().await?;
```

`get()` 返回类型化副本，通过 `fill` 修改后显式 `save`。`fill` 拒绝未声明字段和错误类型，完整候选值验证成功后才接受改动。嵌套对象、数组整项替换；后台须显式合并允许修改的嵌套字段。`reset` 删除数据库覆盖并使用声明默认值。未提交字段保持原值；类型允许时，`null`、空字符串、零和 `false` 各自保留。`refresh()` 丢弃本地编辑并重新加载。

`lock`／`unlock` 操作已持久化的属性，调用前须保存待提交编辑。修改锁定属性明确报错，不会静默成功。每组保留一条内部 `__settings_version` 记录，串行化写入并检测旧编辑，覆盖首次创建和恢复默认；这是对 Spatie 表结构的明确扩展。发生 `Conflict` 后须重新读取并处理冲突，不能盲目重试旧快照。旧版设置类型更新时保留新版增加的数据库属性。

模块设置使用相同接口。将类型放在模块内，在设置 Provider 注册之后，由模块 Provider 调用 `SettingsServiceProvider::register_settings::<DeliverySettings>(app).await?`。该组共用设置表，与 `site` 相互独立，无需修改仓储。

```rust
use openrust::contracts::config::Repository;
use openrust_settings::{SettingsGroup, SettingsResult};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct DeliverySettings { enabled: bool }
impl SettingsGroup for DeliverySettings {
    fn group() -> &'static str { "module_delivery_options" }
    fn defaults(_: &dyn Repository) -> SettingsResult<Self> {
        Ok(Self { enabled: true })
    }
}
```

## 事务与生命周期

`save()` 成功提交后才更新实例的持久快照。`save_in(&mut transaction)` 在调用方事务内使用保存点，不发布新的持久快照；传入目标数据库的事务，外层提交后调用 `refresh`。外层回滚后仍可重试待提交编辑。取消操作时，事务所有者须回滚或释放事务。

每组通过进程内版本快照供现有请求／任务作用域复用。使用 `SettingsServiceProvider` 时，下个作用域在对账后看到新值（默认六十秒），不再每次请求查询完整属性。已有作用域及未保存编辑保留原快照；`refresh()` 可绕过间隔并丢弃本地编辑。长命令、调度轮次和流式连接须明确刷新边界；多个组不保证全局同一快照。数据库故障及损坏数据在检查时明确失败，只有缺失的覆盖值回退默认值。

默认仓储每组限制 256 个属性、1 MiB 持久 JSON，内部少量元数据单独计入限制。无需共享缓存即可使用；静态 `Config` 契约保持，动态设置不会写进 `config:cache`。

## 单机与集群

单台服务器与多个应用／Worker 进程使用相同的 `Settings<T>` 接口、Provider 和配置。各节点共用权威设置数据库及表，不需要集群开关、Redis 或额外守护命令。首次实际读取启动轻量进程内检查器；仅注册设置组不查询设置表。

发布的 `config/settings.rs` 包含以下可选配置：

```rust,ignore
"namespace": null,
"refresh_interval": 60,
"cache": {
    "enabled": true,
    "store": null,
    "ttl": 3600,
},
```

`refresh_interval` 单位为秒（默认 60，Provider 支持 1～86400）。设为 **0** 可关闭后台检查及已加载快照的自动过期；首次加载后持续复用快照，直到显式 `refresh()`、`poll()`、`notify()` 或本进程保存更新它。处理器的自动重试也会停止，需要时主动调用 `poll()` 或 `notify()`；缓存是否启用独立控制。同组并发读取会复用已完成的版本检查，避免排队重复查询。对不频繁修改的设置，默认一分钟更保守，但不代表容量保证，仍应结合活动进程和设置组数量评估数据库负载。`cache.store = null` 沿用应用默认 Cache 存储；显式选择的存储必须存在，`cache.enabled = false` 则停用缓存加速。默认命名空间由环境、数据库驱动、数据库名、表前缀及表名组成，各节点须保持一致且稳定。独立应用共用 Cache 时，应使用不同命名空间及独立设置表或连接。命名空间隔离缓存与通知，**不隔离数据库行**；默认身份无法区分部署或数据库 Schema 时，需显式设置。

检查器仅对本进程已使用的组，从**写连接**批量读取持久版本；版本改变才重读完整属性。现有 Cache 保存带版本的原始快照和仅含元数据的变更提示，每个命名空间／组最多两个键，不随每次更新积累缓存文件。声明为加密的属性在共享快照中仍为密文。提示检查间隔最长一秒；没有共享缓存、使用本地缓存、提示丢失、缓存过期或 Redis 重启时，仍由数据库定期对账补齐。这是最终一致，不保证各节点同时切换；慢数据库或应用处理器可能延长正常间隔。

数据库提交是权威结果，缓存超时或处理器失败不会把已提交的保存伪装成回滚。缓存操作有超时限制，故障期间暂时绕过缓存，`cache_failures()` 提供进程故障计数。数据库故障使已检查状态失效，新加载明确失败直到恢复；已有作用域的值仍可用。新进程直接读取最新提交状态，无需重放通知。所有更新须走仓储写接口以更新版本；直接 SQL 修改但不更改版本时，须显式 `refresh()`。

`save_in` 仅在外层事务提交后发布提示，回滚不发布。提交与提示之间崩溃，由下次数据库检查恢复。每个处理器按本进程观察到的版本去重，失败处理器重试；连续更新的中间版本可能合并。处理器须幂等，进程重启不提供业务事件的持久或恰好一次投递保证。

### 刷新应用消费者

通过 `SettingsManager` 注册应用自己的失效／重建逻辑。包注册后使用 `app.make::<SettingsManager>().await?` 获取。例如，使应用自己的缓存投影失效：

```rust,ignore
use openrust::{cache::CacheManager, contracts::cache::Repository as _};
use openrust_settings::{SettingsError, SettingsManager};

let manager = app.make::<SettingsManager>().await?;
let cache = app.make::<CacheManager>().await?.store(None)?;
let handler = manager.on_change("site", move |_change| {
    let cache = cache.clone();
    Box::pin(async move {
        cache.forget("site:rendered").await
            .map_err(|_| SettingsError::Refresh)?;
        Ok(())
    })
}).await?;

let state = manager.status("site").await;
// state：observed_version、applied_version、pending_handlers、last_error
manager.forget_handler("site", handler).await;
```

处理器在独立异步任务中执行，不继承当前 HTTP／任务的门面作用域，须捕获所需依赖。需要管理器本身时，捕获 `manager.downgrade()` 并在处理器内 `upgrade()`，避免循环引用。处理器五秒超时会记录脱敏错误并保留待处理版本，开启自动检查时，没有新版本也继续重试。首次加载只建立基线，不触发变更处理器。`status()` 只表示本进程已注册处理器的状态，**不代表集群全部节点确认**，不能用作全局发布屏障或持久审计。

`poll().await?` 主动对账所有活动组；`notify(&SettingsNotification { namespace, group, version }).await?` 可接入应用可选的通知传输层，只接受元数据并向数据库核实，忽略其他命名空间及非活动组，不信任通知传入的值。两者都不依赖 Redis Pub/Sub。`shutdown().await` 停止本管理器后台任务，不关闭共享连接；之后仍可显式 load／refresh／poll。

不使用应用 Provider 时，可组合 `SettingsManager::new(source, SettingsOptions::default(), None)?`，包装为 `Arc` 后交给 `Settings::<T>::load`；按部署设置稳定命名空间即可。自定义 `SettingsRepository` 可覆写 `versions()` 进行批量检查，默认回退完整读取。非空快照须通过 `SettingsSnapshot::with_version` 提供非空、不透明且每次提交变化的版本；从未持久化的空组可没有版本。

本功能不分发 `.env`、不改写 `Config`、部署密钥、监听端口或数据库连接池。服务自己的刷新边界与滚动发布仍由应用显式控制。

## 凭据与后台接入

敏感属性使用应用加密器及旧密钥环；嵌套凭据可作为一个对象整体加密。`re_encrypt()` 用当前密钥重写已存在且未锁定的加密属性，完成重写与恢复验证后再移除旧密钥。备份恢复同时需要数据库数据及适用的部署密钥；密文损坏不静默回退默认值。

设置包装对象不能序列化，调试输出不包含属性值。`get()` 返回的类型化数据供可信内部使用，HTTP、日志与队列载荷须使用明确的公开或脱敏投影。后台授权、校验、保留未修改凭据的表单更新及持久审计由应用提供。

本扩展不注册后台路由，也不自动动态改写短信、邮件或存储管理器。这些消费者须通过显式适配重建缓存的服务实例。在途操作保留原快照；历史上传记录须保留磁盘标识。数据库翻译词条继续由[翻译加载器](../digging-deeper/localization.md)管理。
