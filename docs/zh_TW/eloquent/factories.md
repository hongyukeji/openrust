# Eloquent 工廠

## 定義模型工廠

用 `Factory::new` 定義模型的預設屬性。若希望透過 `Flight::factory()` 呼叫，在模型上實作 `HasFactory`。執行下方指令在 `database/factories` 產生檔案，再宣告 Rust 模組並填寫屬性定義。

```sh
./artisan make:factory FlightFactory --model=Flight
```

### 本地化測試資料 {#localized-fake-data}

使用 `Factory::withFaker` 定義產生的模型屬性。它讀取所選連線所屬應用程式的 `app.faker_locale`。使用 `fake(Some("fr_FR"))` 明確指定語言，或用 `fake(None)` 讀取目前應用程式的設定。Rust 產生器使用 fake-rs 的資料，提供 `name`、`safeEmail`、`city` 和 `sentence`。尚未支援的語言使用英文資料。

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


## 使用工廠建立模型

`state` 按宣告順序疊加。`sequence` 接收批次內從零開始的索引。`make` 建立記憶體模型並執行 `afterMaking`。`create` 儲存模型、建立已宣告的子關聯，再執行 `afterCreating`。呼叫 `count` 後回傳 Eloquent 集合。工廠屬性略過大量賦值篩選，並繼續執行模型修改器與屬性轉換。

批次工廠先建構所有模型，再執行 `afterMaking`。`create` 逐一儲存模型及其宣告的關聯，寫入完成後才執行該批次的 `afterCreating`。`afterMaking` 失敗時，該批模型尚未寫入，建構期間解析的上層工廠可能已寫入記錄。

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

`has` 接受 `HasOne`、`HasMany`、`MorphOne` 或 `MorphMany`。用 `r#for` 建立 `BelongsTo` 或 `MorphTo` 的上層模型，用 `forExisting` 關聯既有模型。每次批次呼叫只解析一次上層模型，`make` 也遵循這項規則。再次呼叫會產生新的上層模型。`MorphTo` 描述元需要登錄目標模型與對應別名。`hasAttached` 接受 `BelongsToMany` 或 `MorphToMany` 並寫入中介表屬性。寫入依序執行。後續關聯失敗時，先前的寫入可能保留。

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

子工廠先讀取工廠定義，再套用關聯查詢的預設屬性和明確狀態。直接及多型子關聯隨後補入外鍵和類型欄位，再執行 `afterMaking`。附加模型工廠也會在套用明確狀態前讀取關聯查詢的預設屬性。

### 重用既有模型 {#recycling-existing-models}

用 `recycle` 傳入一個已儲存的模型，或用 `recycleMany` 傳入同類型模型的迭代器。這兩個方法可在 `count` 前後呼叫。模型池會傳給巢狀關聯工廠。解析上層模型時，工廠在同一連線和類型的候選模型中隨機選取一個，缺少符合項目時建立新上層模型。同一批次重用已選中的上層模型。

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
