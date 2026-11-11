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

`has` 接收类型化的 `HasMany`，`r#for` 创建 `BelongsTo` 的上级模型，`forExisting` 复用已有上级模型，`hasAttached` 建立多对多关系并填充中间表属性。每个子模型单独创建其新上级，`make` 也遵循这条规则。关联按顺序写入。后续关联失败时，之前的写入可能保留。
