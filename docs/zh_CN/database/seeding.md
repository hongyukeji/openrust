# 数据填充

## 编写 Seeder

用 `make:seeder` 在 `database/seeders` 生成 Rust Seeder。在 `database/seeders/mod.rs` 注册它的 `SeederEntry`，然后重新构建应用。`run` 接收 `SeederContext`，可以获取所选连接和应用服务。

Artisan 执行 Seeder 及其子 Seeder 时会临时关闭 Eloquent 的批量赋值保护。`Model::create` 和 `fill` 等方法可以设置受保护的属性，仍会应用修改器和类型转换。命令成功或失败时都会恢复保护。独立的 Tokio 任务（包括通过 `tokio::spawn` 创建的任务）保留各自的保护。

```sh
./artisan make:seeder CatalogSeeder
```

### 调用其他 Seeder

`DatabaseSeeder` 是默认入口。用 `call` 按顺序运行子 Seeder。`callSilent` 隐藏子项的进度输出。`callOnce` 跳过本次执行中已经成功的 Seeder。`callWith` 可传入参数，子项用 `parameters()` 读取。

用 `callWithMany(names, parameters)` 按顺序给多个 Seeder 传入相同的值。用 `callSilentWith(names, parameters)` 隐藏这些 Seeder 的进度行，或用 `callOnceWith(names, silent, parameters)` 跳过已成功的 Seeder 并选择是否显示进度。各 Seeder 通过 `parameters()` 读取参数。发生错误时停止执行列表，失败的 Seeder 可以重试。

以下示例要求 `plans` 表有唯一的 `code` 列和一个 `label` 列。重复运行时，upsert 更新同一行。

```rust
use openrust::database::{
    DatabaseFuture, DatabaseResult, query::Record,
    seeder::{Seeder, SeederContext, SeederEntry},
};

pub struct CatalogSeeder;

impl Seeder for CatalogSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let label = context.parameters().get("label").cloned()
                .unwrap_or_else(|| "Standard".into());
            let row = Record::from([
                ("code".into(), "standard".into()),
                ("label".into(), label),
            ]);
            context.connection().table("plans")
                .upsert(&[row], &["code"], Some(&["label"]))
                .await?;
            Ok(())
        })
    }
}

pub struct DatabaseSeeder;

impl Seeder for DatabaseSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(context.callOnceWith(
            &["CatalogSeeder"],
            true,
            Record::from([("label".into(), "Standard plan".into())]),
        ))
    }
}

pub const ENTRIES: &[SeederEntry] = &[
    SeederEntry::new("DatabaseSeeder", &DatabaseSeeder),
    SeederEntry::new("CatalogSeeder", &CatalogSeeder),
];
```

## 运行 Seeder

用 `db:seed` 运行默认 Seeder。用 `--class` 或位置参数指定类，用 `--database` 选择连接。Seeder 内通过 `DB` 门面访问时默认使用所选连接，其他任务各自保留自己的连接选择。

```sh
./artisan db:seed
./artisan db:seed --class=CatalogSeeder
./artisan migrate:fresh --seed --seeder=CatalogSeeder
```

也可以在 `migrate --seed` 或 `migrate:refresh --seed` 后填充数据。用 `--seeder` 选择指定类。Seeder 失败会停止后续调用，并返回失败退出码。

### 在生产环境中强制运行 Seeder

生产环境会提示确认。部署流程已经确认数据变更时，使用 `--force`。

```sh
./artisan db:seed --force
```

## 使用模型工厂 {#using-model-factories}

模型绑定、状态、回调和关联工厂见 [Eloquent 工厂](../eloquent/factories.md#factory-model-binding)。

应用在 `database/factories` 提供默认 `UserFactory`。迁移空数据库后，`./artisan db:seed` 使用它创建已验证的示例账号 `test@example.com`，密码为 `password`。模型按应用配置哈希密码。默认 Seeder 使用当前选定的数据库，运行期间静默模型事件。对同一账号重复执行默认 Seeder 会收到邮箱重复错误。

将 Seeder 选中的连接传给 `createOn`，即可沿用 `db:seed --database=…` 的连接选择。可在 Seeder 的 `run` 中调用下面的函数。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, factories::Factory},
    seeder::SeederContext,
};

async fn seed_flights<M: Model>(context: &SeederContext<'_>) -> DatabaseResult<()> {
    Factory::<M>::new(|| Attributes::from([("name".into(), "Flight".into())]))
        .sequence(|index| Attributes::from([("name".into(), format!("Flight {index}").into())]))
        .count(3)
        .createOn(context.connection())
        .await?;
    Ok(())
}
```

## 静默模型事件 {#muting-model-events}

注册 Seeder 时调用 `withoutModelEvents()`，可在执行期间静默 Eloquent 模型事件。它调用的子 Seeder 继承该范围。成功返回或报错后都会恢复模型事件。Seeder 进度输出与工厂回调照常执行。

```rust
use openrust::database::seeder::{Seeder, SeederEntry};

const fn muted_entry(seeder: &'static dyn Seeder) -> SeederEntry {
    SeederEntry::new("DatabaseSeeder", seeder).withoutModelEvents()
}
```
