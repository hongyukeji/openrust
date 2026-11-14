# 数据填充

## 编写 Seeder

用 `make:seeder` 在 `database/seeders` 生成 Rust Seeder。在 `database/seeders/mod.rs` 注册它的 `SeederEntry`，然后重新构建应用。`run` 接收 `SeederContext`，可以获取所选连接和应用服务。

```sh
./artisan make:seeder CatalogSeeder
```

### 调用其他 Seeder

`DatabaseSeeder` 是默认入口。用 `call` 按顺序运行子 Seeder。`callSilent` 隐藏子项的进度输出。`callOnce` 跳过本次执行中已经成功的 Seeder。`callWith` 可传入参数，子项用 `parameters()` 读取。

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
            let row = Record::from([
                ("code".into(), "standard".into()),
                ("label".into(), "Standard plan".into()),
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
        Box::pin(context.call(&["CatalogSeeder"]))
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

## 使用模型工厂

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
