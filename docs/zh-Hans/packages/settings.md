# 数据库设置（扩展）

`openrust-settings` 将类型化应用设置存入数据库，可用于网站信息、服务配置及业务策略等设置组。命名参考 Spatie框架Settings，复用 OpenRust 的数据库、加密、迁移和容器作用域。

## 启用与迁移

通过 `settings` Cargo feature 启用可选包。包发现登记自身 Provider 和默认表迁移，不注册应用业务设置组：

```sh
cargo run --features settings -- artisan migrate
```

包自身提供默认配置，默认应用不包含 `config/settings.rs`。需要定制时运行 `./artisan vendor:publish --tag=settings-config`，在 `config/mod.rs` 的模块声明和配置映射中分别加入 `mod settings;` 与 `("settings".to_owned(), settings::load(env)?),`，然后重新构建。修改表名或连接时保留自己的对应迁移。

其他应用添加 `openrust-settings` 依赖并使用 OpenRust 包发现。声明自己的设置类型，在包注册完成后，由该应用的 Provider 调用 `SettingsServiceProvider::registerSettings::<YourSettings>(app).await?`。默认应用没有预注册的 `SiteSettings` 业务组。首次读取前运行迁移；Provider 注册和启动不查询设置表。

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

模块设置使用相同接口。将类型放在模块内，在设置 Provider 注册之后，由模块 Provider 调用 `SettingsServiceProvider::registerSettings::<DeliverySettings>(app).await?`。该组共用设置表，与 `site` 相互独立，无需修改仓储。

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

`save()` 成功提交后才更新实例的持久快照。`saveIn(&mut transaction)` 在调用方事务内使用保存点，不发布新的持久快照；传入目标数据库的事务，外层提交后调用 `refresh`。外层回滚后仍可重试待提交编辑。取消操作时，事务所有者须回滚或释放事务。

每组从写连接一次查询，当前请求／任务内复用，下一作用域读取数据库新值。不要把设置实例保存在永久单例里。长命令、调度轮次和流式连接须明确刷新边界；多个组不保证全局同一快照。数据库故障及损坏数据明确失败，只有缺失的覆盖值回退默认值。

默认仓储每组限制 256 个属性、1 MiB 持久 JSON，内部少量元数据单独计入限制。无需共享缓存即可使用；静态 `Config` 契约保持，动态设置不会写进 `config:cache`。

## 凭据与后台接入

敏感属性使用应用加密器及旧密钥环；嵌套凭据可作为一个对象整体加密。`reEncrypt()` 用当前密钥重写已存在且未锁定的加密属性，完成重写与恢复验证后再移除旧密钥。备份恢复同时需要数据库数据及适用的部署密钥；密文损坏不静默回退默认值。

设置包装对象不能序列化，调试输出不包含属性值。`get()` 返回的类型化数据供可信内部使用，HTTP、日志与队列载荷须使用明确的公开或脱敏投影。后台授权、校验、保留未修改凭据的表单更新及持久审计由应用提供。

本扩展不注册后台路由，也不自动动态改写短信、邮件或存储管理器。这些消费者须通过显式适配重建缓存的服务实例。在途操作保留原快照；历史上传记录须保留磁盘标识。数据库翻译词条继续由[翻译加载器](../digging-deeper/localization.md)管理。
