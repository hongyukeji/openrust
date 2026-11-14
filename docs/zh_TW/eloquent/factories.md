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


用 `faker.unique(false)` 讓同一個產生器按方法去重，用 `unique(true)` 清空歷史。`Factory::tryWithFaker` 的同一批次共用產生器，重試耗盡會在該批模型寫入前回傳錯誤。各次獨立呼叫擁有自己的歷史。例如，可在工廠定義中使用 `faker.unique(false).safeEmail()?`。

內建產生方法還包括 `firstName`、`lastName`、`freeEmail`、`userName`、`streetName`、`postcode`、`phoneNumber`、`company` 和 `word`。這些方法也可透過 `unique` 呼叫。樣本取自 fake-rs 中對應語言的資料集。

用 `numberBetween(first, second)` 產生整數，包含兩個端點，端點可以任意排序。`randomDigit()` 回傳 0 至 9，`randomDigitNotNull()` 回傳 1 至 9。`dateTimeBetween(start, end)` 接收明確的 UTC 日期時間，以整秒取樣並包含兩端，起始秒晚於結束秒時回傳錯誤。四種方法均支援 `unique`。

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

## 使用工廠建立模型

`state` 按宣告順序疊加。`sequence` 接收批次內從零開始的索引。`make` 建立記憶體模型並執行 `afterMaking`。`create` 儲存模型、建立已宣告的子關聯，再執行 `afterCreating`。呼叫 `count` 後回傳 Eloquent 集合。工廠屬性略過大量賦值篩選，並繼續執行模型修改器與屬性轉換。

批次工廠先建構所有模型，再執行 `afterMaking`。`create` 逐一儲存模型及其宣告的關聯，寫入完成後才執行該批次的 `afterCreating`。`afterMaking` 失敗時，該批模型尚未寫入，建構期間解析的上層工廠可能已寫入記錄。

使用 `raw()` 或 `rawOn(connection)` 取得套用模型修改器和轉換前的展開屬性。指定數量時回傳屬性映射清單。原始屬性展開會略過目標工廠的回呼和子關聯，仍可能儲存宣告的上層模型以解析關聯鍵。使用 `createQuietly()` 或 `createQuietlyOn(connection)` 可儲存模型及關聯，同時抑制 Eloquent 模型事件。工廠回呼照常執行。

用 `createOne(attributes)` 建立單一模型，即使工廠已指定批次數量。用 `createMany(records)` 逐筆依序建立。傳入 `FactoryRecords::Default` 使用工廠數量，未指定時建立一筆。傳入 `FactoryRecords::Count(n)` 指定數量，或用 `FactoryRecords::Attributes(vec![…])` 為每筆記錄指定覆寫屬性。覆寫值在工廠狀態之後套用。每筆記錄完成兩個工廠回呼後才開始下一筆。後續記錄失敗時，先前寫入可能保留。用 `createOneQuietly` 和 `createManyQuietly` 抑制模型事件，或用對應的 `On` 方法指定連線。

用 `makeOne(attributes)` 建構一個未儲存的模型，或用 `makeMany(records)` 依序建構多個未儲存的模型。兩者皆接受明確屬性並執行 `afterMaking`。對應的 `On` 方法可指定連線。`lazy(attributes)` 回傳非同步回呼，呼叫並等待後才解析連線並建立單一模型或指定數量的批次。重複呼叫會重新建立。

用 `forEachSequence(states)` 為每個模型套用一組屬性，並依屬性組數決定批次數量。`crossJoinSequence(sequences)` 依宣告順序組合各序列，最後一個序列變化最快，同名屬性以後面的序列為準。另行設定批次數量，超過組合數時從頭循環。傳入空序列會回傳錯誤。

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
    subject: &str,
) -> DatabaseResult<()> {
    comments.forNamed::<_, MorphTo<C>>(subject, posts).count(3)
        .createOn(context.connection()).await?;
    Ok(())
}
```

子工廠先讀取工廠定義，再套用關聯查詢的預設屬性和明確狀態。直接及多型子關聯隨後補入外鍵和類型欄位，再執行 `afterMaking`。附加模型工廠也會在套用明確狀態前讀取關聯查詢的預設屬性。

用 `hasNamed`、`forNamed` 或 `hasAttachedNamed` 按名稱解析關聯。工廠使用 `Model::namedRelations` 或應用程式 `resolveRelationUsing` 登錄的描述元。明確傳入 Rust 描述元型別，例如 `forNamed::<_, MorphTo<Comment>>("subject", posts)`。名稱不存在或型別不符時回傳錯誤。

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
