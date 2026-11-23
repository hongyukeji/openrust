# Pennant

## 安装 {#installation}

在应用 Cargo.toml 中启用 `pennant` feature。通过 `./artisan` 运行时，将 `"pennant"` 加入 `[features].default` 并保留已有功能。包发现自动加载 `PennantServiceProvider`，默认应用无需追加 Provider 注册。

## 配置 {#configuration}

默认存储为 `database`，通过 `PENNANT_STORE` 选择；`array` 仅在当前生命周期保存状态。数据库默认表为 `features`，特性名称与作用域构成唯一键。需要定制时发布配置：

```sh
./artisan vendor:publish --tag=pennant-config
./artisan migrate
```

配置归属扩展包。发布的 Rust 配置需要接入编译型配置加载器并重新构建；仅发布不会执行新的 Rust 文件。命中配置缓存时使用缓存值，跳过包环境读取。Provider 登记默认表迁移，启动时不查询数据库。使用自定义表或连接时，由应用提供匹配的迁移。

## 定义特性 {#defining-features}

在 Pennant 启动后的应用 Provider 中登记定义。解析器接收拥有所有权的 `Scope`，返回 JSON 值；`defineValue` 定义常量。定义在应用内共享，解析值归属于当前 HTTP 请求、Artisan 调用或队列任务。

```rust
let store = app.make::<openrust_pennant::FeatureManager>().await?.store(None)?;
store.defineValue("new-api", serde_json::json!(true))?;
store.define("theme", std::sync::Arc::new(|scope| Box::pin(async move {
    Ok(serde_json::json!(if scope == openrust_pennant::Scope::Null { "default" } else { "blue" }))
})))?;
```

## 判断特性 {#checking-features}

只有 JSON `false` 表示关闭。`null`、零、空字符串和数组均为开启。未知特性返回 false 并发出 `UnknownFeatureResolved`，不会持久化此回退值。`allAreActive` 要求每个作用域的全部特性开启；`someAreActive` 要求每个作用域至少有一个匹配特性。

```rust
let user = store.r#for("user-42");
let enabled = user.active("new-api").await?;
let theme = user.value("theme").await?;
user.load_missing(&["new-api", "theme"]).await?;
let both = user.allAreActive(&["new-api", "theme"]).await?;
```

## 作用域 {#scopes}

使用 `for` 显式选择作用域，`Scope::from_model` 转换 Eloquent 模型，`globally` 选择单一全局作用域。全局与匿名作用域彼此独立。模型默认以具体 Rust 类和主键标识；`useMorphMap(true)` 将类部分切换为配置的 morph 别名。`FeatureScopeSerializeable` 配合 `Scope::fromSerializable` 自定义序列化；`FeatureScopeable` 配合 `forScopeable` 按所选 store 转换标识。

默认作用域读取原生认证已解析的身份。使用懒加载 guard 时，先调用 `Feature::for_request(request).await?` 再判断，或先运行认证中间件。`EnsureFeaturesAreActive` 会主动解析默认 guard。非 HTTP 场景请显式指定作用域，或配置 `FeatureManager::resolveScopeUsing`。

## 类式特性 {#feature-classes}

实现 `FeatureDefinition`，用 `defineClass` 登记异步工厂。工厂可以解析容器依赖。`name` 指定自定义名称；`acceptsNullScope` 显式替代 PHP 参数反射。`before` 返回非空结果时覆盖已存储值，但不会持久化覆盖值。已有缓存优先；`flushCache` 后重新运行钩子。`before` 返回 `None` 时交由驱动处理。

```rust
use openrust_pennant::{FeatureDefinition, FeatureFuture, Scope};
use serde_json::Value;
use std::sync::Arc;

struct NewApi;
impl FeatureDefinition for NewApi {
    fn name() -> &'static str { "new-api" }
    fn resolve<'a>(&'a self, _scope: &'a Scope) -> FeatureFuture<'a, Value> {
        Box::pin(async { Ok(Value::Bool(true)) })
    }
}
// Application Provider:
store.defineClass(|| Box::pin(async { Ok(Arc::new(NewApi)) }))?;
```

Rust 模块和工厂在编译时注册，不提供 PHP 运行时类扫描、反射和宏。枚举通过明确的字符串表示传入。工厂延迟执行，定义特性不会解析依赖。

`store.discover` 接收编译后的注册函数。通过 `store.instance::<NewApi>().await?` 取得类型明确的类实例，调用应用自定义方法。`isValidForScope` 控制 `all` 与 `loadAll` 包含哪些定义。Eloquent 模型导入 `openrust_pennant::concerns::HasFeatures` 后可调用 `features(store)`。`defineLottery` 复用原生 `Lottery`，与其他定义一样保存首次解析结果。

## 修改状态 {#updating-values}

激活操作写入所选作用域。`activateForEveryone` 更新已存储作用域；首次遇到的新作用域仍使用解析器。`forget` 删除状态，使其重新解析。`flushCache` 会清除 array 驱动已解析状态，数据库驱动保留持久值。

```rust
user.activate("new-api").await?;
user.activateWithValue(&["theme"], serde_json::json!("green")).await?;
user.deactivate("new-api").await?;
user.forget(&["theme"]).await?;
store.activateForEveryone("new-api", serde_json::json!(true)).await?;
```

## 中间件 {#middleware}

在原生路由组中用 `Arc::new(...)` 添加 `EnsureFeaturesAreActive::using(["new-api"])`。特性关闭时返回 HTTP 400。`whenInactive` 接收当前中间件实例的异步响应回调。按路由需要配置认证／会话中间件。

## 命令 {#commands}

`pennant:feature` 在 `app/features` 创建 Rust 类；请声明其模块并在 Provider 注册工厂。`stubs/feature.rs.stub` 可覆盖模板。除非传入 `--force`，已有文件会保留。清理命令删除存储值并保留定义；`pennant:clear` 是 `pennant:purge` 的别名。部署后通过 `./main artisan` 执行。

```sh
./artisan pennant:feature NewApi
./artisan pennant:purge new-api theme
./artisan pennant:purge --except=new-api --except=theme
./artisan pennant:purge --except-registered --store=database
./artisan pennant:clear
```

## 自定义驱动与事件 {#drivers-and-events}

通过 `FeatureManager::extend` 为配置驱动登记创建器，实现包含批量读取的原生 `Driver` 契约。`stored` 是可选能力，默认报告不支持；提供批量写入实现后才启用 `supportsSetAll`，否则逐项写入并在每项后发出事件。创建器不得递归解析自身 store。事件使用原生 Dispatcher：`FeatureResolved`、`FeatureRetrieved`、`UnknownFeatureResolved`、`UnexpectedNullScopeEncountered`、`FeatureUpdated`、`FeatureUpdatedForAllScopes`、`FeatureDeleted`、`FeaturesPurged` 和 `AllFeaturesPurged`。缓存命中同样发出读取事件；监听失败向调用方传播。
