# Eloquent 工廠

## 定義模型工廠

用 `Factory::new` 定義模型的預設屬性。若希望透過 `Flight::factory()` 呼叫，在模型上實作 `HasFactory`。執行下方指令在 `database/factories` 產生檔案，再宣告 Rust 模組並填寫屬性定義。

```sh
./artisan make:factory FlightFactory --model=Flight
```

## 使用工廠建立模型

`state` 按宣告順序疊加。`sequence` 接收批次內從零開始的索引。`make` 建立記憶體模型並執行 `afterMaking`。`create` 儲存模型、建立已宣告的子關聯，再執行 `afterCreating`。呼叫 `count` 後回傳 Eloquent 集合。工廠屬性略過大量賦值篩選，並繼續執行模型修改器與屬性轉換。

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

## 工廠關聯

`has` 接收型別化的 `HasMany`，`r#for` 建立 `BelongsTo` 的上層模型，`forExisting` 重用已有的上層模型，`hasAttached` 建立多對多關聯並填入中介表屬性。每個子模型各自建立新的上層模型，`make` 也遵循這條規則。關聯按順序寫入。後續關聯失敗時，先前的寫入可能保留。
