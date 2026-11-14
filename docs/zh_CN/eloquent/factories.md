# Eloquent 工厂

## 定义模型工厂

用 `Factory::new` 定义模型的默认属性。若希望通过 `Flight::factory()` 调用，在模型上实现 `HasFactory`。运行下方命令在 `database/factories` 生成文件，再声明 Rust 模块并填写属性定义。

```sh
./artisan make:factory FlightFactory --model=Flight
```

## 使用工厂创建模型

`state` 按声明顺序叠加。`sequence` 接收批次内从零开始的索引。`make` 创建内存模型并运行 `afterMaking`。`create` 保存模型、创建已声明的子关系，然后运行 `afterCreating`。调用 `count` 后返回 Eloquent 集合。工厂属性绕过批量赋值过滤，并继续执行模型修改器与属性转换。

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
    subject: MorphTo<C>,
) -> DatabaseResult<()> {
    comments.r#for(subject, posts).count(3).createOn(context.connection()).await?;
    Ok(())
}
```

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
