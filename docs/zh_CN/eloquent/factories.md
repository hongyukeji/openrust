# Eloquent 工厂

## 定义模型工厂

用 `Factory::new` 定义模型的默认属性。若希望通过 `Flight::factory()` 调用，在模型上实现 `HasFactory`。运行下方命令在 `database/factories` 生成文件，再声明 Rust 模块并填写属性定义。

```sh
./artisan make:factory FlightFactory --model=Flight
```

### 本地化测试数据 {#localized-fake-data}

使用 `Factory::withFaker` 定义生成的模型属性。它读取所选连接所属应用的 `app.faker_locale`。使用 `fake(Some("fr_FR"))` 显式指定语言，或用 `fake(None)` 读取当前应用配置。Rust 生成器使用 fake-rs 的数据，提供 `name`、`safeEmail`、`city` 和 `sentence`。未支持的语言使用英文数据。

```rust
use openrust::database::eloquent::{Attributes, Model, factories::Factory};

pub fn profile_factory<M: Model>() -> Factory<M> {
    Factory::withFaker(|faker| {
        Attributes::from([
            ("name".into(), faker.name().into()),
            ("email".into(), faker.safeEmail().into()),
        ])
    })
}
```


用 `faker.unique(false)` 让同一个生成器按方法去重，用 `unique(true)` 清空历史。`Factory::tryWithFaker` 的同一批次共享生成器，重试耗尽会在该批模型写入前返回错误。独立工厂调用各有自己的历史。例如，可在工厂定义中使用 `faker.unique(false).safeEmail()?`。

内置生成方法还包括 `firstName`、`lastName`、`freeEmail`、`userName`、`streetName`、`postcode`、`phoneNumber`、`company` 和 `word`。这些方法也可通过 `unique` 调用。样本取自 fake-rs 中相应语言的数据集。

用 `numberBetween(first, second)` 生成整数，包含两个端点，端点可以任意排序。`randomDigit()` 返回 0 至 9，`randomDigitNotNull()` 返回 1 至 9。`dateTimeBetween(start, end)` 接收显式 UTC 日期时间，按整秒取样并包含两端，起始秒晚于结束秒时返回错误。四种方法都支持 `unique`。

```rust
use chrono::DateTime;
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, factories::Factory},
};

pub fn activity_factory<M: Model>() -> Factory<M> {
    Factory::tryWithFaker(|faker| -> DatabaseResult<Attributes> {
        let start = DateTime::UNIX_EPOCH;
        let end = DateTime::from_timestamp(86_400, 0).unwrap();
        let happened_at = faker.dateTimeBetween(start, end)?;
        Ok(Attributes::from([
            ("score".into(), faker.numberBetween(1, 100).into()),
            ("happened_at".into(), happened_at.to_rfc3339().into()),
        ]))
    })
}
```

## 使用工厂创建模型

`state` 按声明顺序叠加。`sequence` 接收批次内从零开始的索引。`make` 创建内存模型并运行 `afterMaking`。`create` 保存模型、创建已声明的子关系，然后运行 `afterCreating`。调用 `count` 后返回 Eloquent 集合。工厂属性绕过批量赋值过滤，并继续执行模型修改器与属性转换。

批量工厂先构造全部模型，再运行 `afterMaking`。`create` 逐个保存模型及其声明的关系，写入完成后才运行该批次的 `afterCreating`。`afterMaking` 失败时，该批模型尚未写入，构造期间解析的上级工厂可能已写入记录。

使用 `raw()` 或 `rawOn(connection)` 获取应用模型修改器和转换前的展开属性。指定数量时返回属性映射列表。原始属性展开跳过目标工厂的回调和子关系，仍可能保存声明的上级以解析关联键。使用 `createQuietly()` 或 `createQuietlyOn(connection)` 可保存模型及关系，同时抑制 Eloquent 模型事件。工厂回调照常执行。

用 `createOne(attributes)` 创建单个模型，即使工厂已指定批次数量。用 `createMany(records)` 逐条按顺序创建。传入 `FactoryRecords::Default` 使用工厂数量，未指定时创建一条。传入 `FactoryRecords::Count(n)` 指定数量，或用 `FactoryRecords::Attributes(vec![…])` 为每条记录指定覆盖属性。覆盖值在工厂状态之后应用。每条记录完成两个工厂回调后才开始下一条。后续记录失败时，先前写入可能保留。用 `createOneQuietly` 和 `createManyQuietly` 抑制模型事件，或用对应的 `On` 方法指定连接。

用 `makeOne(attributes)` 构造一个未保存的模型，或用 `makeMany(records)` 按顺序构造多个未保存的模型。两者都接受显式属性并执行 `afterMaking`。对应的 `On` 方法可指定连接。`lazy(attributes)` 返回异步回调，调用并等待后才解析连接并创建单个模型或指定数量的批次。重复调用会重新创建。

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

## 工厂关系

`has` 接收 `HasOne`、`HasMany`、`MorphOne` 或 `MorphMany`。用 `r#for` 创建 `BelongsTo` 或 `MorphTo` 的上级模型，用 `forExisting` 关联已有模型。每次批量调用只解析一次上级模型，`make` 也遵循这条规则。再次调用会生成新的上级。使用 `MorphTo` 时，描述符需要登记目标模型和映射别名。`hasAttached` 接收 `BelongsToMany` 或 `MorphToMany`，并写入中间表属性。写入按顺序执行。后续关联失败时，之前的写入可能保留。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, factories::Factory, relations::MorphTo},
    seeder::SeederContext,
};

pub async fn seed_comments<C: Model, P: Model>(
    context: &SeederContext<'_>,
    comments: Factory<C>,
    posts: Factory<P>,
    subject: &str,
) -> DatabaseResult<()> {
    comments.forNamed::<_, MorphTo<C>>(subject, posts).count(3)
        .createOn(context.connection()).await?;
    Ok(())
}
```

子工厂先读取工厂定义，再应用关系查询的默认属性和显式状态。直接及多态子关系随后补入外键和类型字段，再执行 `afterMaking`。附加模型工厂也会在应用显式状态前读取关系查询的默认属性。

用 `hasNamed`、`forNamed` 或 `hasAttachedNamed` 按名称解析关系。工厂使用 `Model::namedRelations` 或应用 `resolveRelationUsing` 登记的描述符。显式传入 Rust 描述符类型，例如 `forNamed::<_, MorphTo<Comment>>("subject", posts)`。名称不存在或类型不匹配时返回错误。

### 复用已有模型 {#recycling-existing-models}

用 `recycle` 传入一个已保存的模型，或用 `recycleMany` 传入同类型模型的迭代器。这两个方法可在 `count` 前后调用。模型池会传给嵌套关系工厂。解析上级时，工厂在同一连接和类型的候选模型中随机选取一个，缺少匹配项时创建新上级。同一批次复用已选中的上级。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, Model, factories::Factory, relations::BelongsTo},
    seeder::SeederContext,
};

pub async fn reuse_owner<C: Model, P: Model>(
    context: &SeederContext<'_>,
    children: Factory<C>,
    owner: P,
    relation: BelongsTo<C, P>,
    owners: Factory<P>,
) -> DatabaseResult<Collection<C>> {
    children.r#for(relation, owners).count(3).recycle(owner)
        .createOn(context.connection()).await
}
```
