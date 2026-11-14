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

`randomDigitNot(except)` 排除指定數字。`randomNumber(digits, strict)` 的位數傳入 `None` 時隨機選擇一至九位。嚴格模式產生確切位數，非嚴格模式也接受零位並回傳零。明確位數超過九位或嚴格模式指定零位時回傳錯誤。兩種方法皆支援 `unique`。

`randomFloat(decimals, min, max)` 先在區間取樣，再依指定小數位捨入，中點遠離零。傳入 `None` 可隨機選擇零至九位精度或產生上界。上下界可逆序，捨入結果可能超出原區間。Rust 對非有限邊界或捨入溢位回傳錯誤。`unique` 將正零與負零視為同一個值。

用 `unixTime(max)` 在 Unix 紀元與指定端點之間取樣整秒，或用 `dateTime(max)` 回傳 UTC 日期時間。傳入 `None` 時使用目前時間。`date(format, max)` 和 `time(format, max)` 接收 Chrono 格式，預設分別為 `%Y-%m-%d` 和 `%H:%M:%S`，格式無效時回傳錯誤。`iso8601(max)` 包含實際使用的時區偏移量。五種方法皆支援 `unique`。

用 `dateTimeInInterval(date, interval)` 在 UTC 起點和該時間加上帶正負號的 Chrono `TimeDelta` 之間按整秒取樣。負時長向過去取樣。起點和時長傳入 `None` 時，分別預設為 30 年前和往後五天。`dateTimeThisCentury(max)`、`dateTimeThisDecade(max)` 和 `dateTimeThisMonth(max)` 分別從目前時間往前 100 年、10 年和一個月開始。`dateTimeThisYear(max)` 從應用程式時區當年 1 月 1 日零點開始。其他日曆回溯和 `dateTimeInInterval` 的預設起點也採用應用程式時區。兩個區間參數都省略時，預設五天依本地日曆計算。明確傳入的 `TimeDelta` 表示實際經過的時長。這四個方法在 `max` 為 `None` 時以目前時間為終點，拒絕早於起點的終點，日曆溢出依 FakerPHP 規則正規化。五個方法皆支援 `unique`。Rust 使用具型別的日期和時長。需要自訂日曆區間時，可計算終點後呼叫 `dateTimeBetween`。

用 `amPm(max)`、`dayOfMonth(max)`、`dayOfWeek(max)`、`month(max)`、`monthName(max)` 和 `year(max)` 依實際使用的時區格式化隨機日期的各個部分。日和月的數字以補零字串回傳，星期和月份名稱沿用 FakerPHP 的英文日期格式。`dateTimeAD(max)` 從 FakerPHP 的 64 位元公元日期下界開始取樣。這些方法在 `max` 為 `None` 時使用目前時間。`century()` 隨機回傳 I 至 XXI 的羅馬數字。所有方法皆支援 `unique`。

用 `timezone(countryCode)` 隨機取得 IANA 時區識別碼。傳入 `None` 從完整清單取樣，也可傳入 `Some("TW")` 等國家或地區代碼來限定範圍。代碼區分大小寫，未知代碼回傳 `None`。框架內建與 `chrono-tz` 同版的 IANA 資料。此方法支援 `unique`。回傳的識別碼可用於將 UTC 日期轉換到目標時區。

日期字串優先使用 `setDefaultTimezone(Some(zone))` 為目前產生器指定的時區，其次讀取應用程式或工廠連線所屬應用程式的 `app.timezone`，預設為 UTC。傳入 `None` 可清除覆寫，`getDefaultTimezone()` 可查看覆寫值。`dateTimeInTimezone(max, timezone)` 回傳帶時區的日期時間，傳入 `Some(zone)` 可覆寫本次呼叫。日期時間方法仍回傳 UTC 時刻。產生器的時區覆寫用於輸出格式化，日曆邊界繼續依應用程式時區計算。

用 `dateTimeBetweenExpressions(start, end)` 傳入 PHP 風格的日期表達式，例如 `"-30 years"` 和 `"now"`。兩個端點共用同一參考時刻，並依應用程式時區解析。表達式中的明確時區優先。數字終點字串依 Unix 時間戳解析，`"@-1"` 則是明確的時間戳表達式。端點解析失敗時依 FakerPHP 的數值轉換規則使用 Unix 零秒。起點晚於終點仍回傳錯誤。此方法與 `dateTimeBetween` 共用 `unique` 歷史。

用 `dateTimeInIntervalExpressions(date, interval)` 傳入 PHP 風格的相對區間，例如 `Some("+1 month")` 或 `Some("5 days ago")`。終點依起始日期所在時區的本地時間計算，跨越日光節約時間時也遵循此規則。取樣結果回傳 UTC。省略日期時從 30 年前開始，省略區間時使用五個日曆日。區間字串接受相對表達式，拒絕絕對日期、時間和時區識別碼。此方法與 `dateTimeInInterval` 共用 `unique` 歷史。

用 `resolveDateMax(expression)` 解析 PHP 風格的日期上界，再以 `Some(max)` 傳給任何接受 `max` 的日期產生方法。解析一次即可在多個呼叫中共用同一上界。小數和科學記號的數字字串向零截斷為 Unix 秒。空字串使用目前時間。日期文字預設依應用程式時區解讀，文字中的明確時區優先。無效或超出範圍的值回傳錯誤。

用 `unixTimeExpression(max)` 從字串上界直接取樣 Unix 秒，支援 Chrono 日期範圍之外的時間戳和延伸年份。它與 `unixTime` 共用 `unique` 歷史。語法無效的上界使用 Unix 零秒。需要在取樣前驗證輸入時，用 `resolveDateMax`。超過解析限制或數值範圍時仍回傳錯誤。有限數字字串超出有號 64 位元範圍時飽和至端點，溢位為無限大時依 PHP 整數轉換規則取零。回傳日期的介面仍要求上界可由 Chrono 表示。

```rust
use chrono::{DateTime, TimeDelta};
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, factories::Factory},
};

pub fn activity_factory<M: Model>() -> Factory<M> {
    Factory::tryWithFaker(|faker| -> DatabaseResult<Attributes> {
        let start = DateTime::UNIX_EPOCH;
        let end = DateTime::from_timestamp(86_400, 0).unwrap();
        let happened_at = faker.dateTimeBetween(start, end)?;
        let reviewed_at = faker.dateTimeInInterval(Some(happened_at), Some(TimeDelta::days(5)))?;
        let sampled_this_year = faker.dateTimeThisYear(None)?;
        let historical = faker.dateTimeBetweenExpressions("-30 years", "now")?;
        let archived_at = faker.dateTimeInIntervalExpressions(Some("2024-01-31"), Some("+1 month"))?;
        let cutoff = faker.resolveDateMax("2024-12-31 23:59:59 UTC")?;
        let published_at = faker.iso8601(Some(cutoff));
        let epoch_seconds = faker.unixTimeExpression("+1000000-01-01 UTC")?;
        Ok(Attributes::from([
            ("score".into(), faker.numberBetween(1, 100).into()),
            ("happened_at".into(), happened_at.to_rfc3339().into()),
            ("reviewed_at".into(), reviewed_at.to_rfc3339().into()),
            ("sampled_this_year".into(), sampled_this_year.to_rfc3339().into()),
            ("historical".into(), historical.to_rfc3339().into()),
            ("archived_at".into(), archived_at.to_rfc3339().into()),
            ("published_at".into(), published_at.into()),
            ("epoch_seconds".into(), epoch_seconds.into()),
            ("weekday".into(), faker.dateTimeInTimezone(Some(DateTime::UNIX_EPOCH), None).format("%A").to_string().into()),
            ("timezone".into(), faker.timezone(Some("TW")).unwrap_or("UTC").into()),
        ]))
    })
}
```

## 使用工廠建立模型

`state` 按宣告順序疊加。`sequence` 接收批次內從零開始的索引。`make` 建立記憶體模型並執行 `afterMaking`。`create` 儲存模型、建立已宣告的子關聯，再執行 `afterCreating`。呼叫 `count` 後回傳 Eloquent 集合。工廠屬性略過大量賦值篩選，並繼續執行模型修改器與屬性轉換。

用 `prependState` 將轉換放到目前所有已登記狀態之前。每個轉換都能讀取先前產生的屬性。用 `set(name, value)` 在末尾附加單一屬性覆寫。兩種方法皆可在 `count` 前後使用。直接傳入建立方法的屬性會在已登記狀態之後套用。

批次工廠先建構所有模型，再執行 `afterMaking`。`create` 逐一儲存模型及其宣告的關聯，寫入完成後才執行該批次的 `afterCreating`。`afterMaking` 失敗時，該批模型尚未寫入，建構期間解析的上層工廠可能已寫入記錄。

使用 `raw()` 或 `rawOn(connection)` 取得套用模型修改器和轉換前的展開屬性。指定數量時回傳屬性映射清單。原始屬性展開會略過目標工廠的回呼和子關聯，仍可能儲存宣告的上層模型以解析關聯鍵。使用 `createQuietly()` 或 `createQuietlyOn(connection)` 可儲存模型及關聯，同時抑制 Eloquent 模型事件。工廠回呼照常執行。

用 `createOne(attributes)` 建立單一模型，即使工廠已指定批次數量。用 `createMany(records)` 逐筆依序建立。傳入 `FactoryRecords::Default` 使用工廠數量，未指定時建立一筆。傳入 `FactoryRecords::Count(n)` 指定數量，或用 `FactoryRecords::Attributes(vec![…])` 為每筆記錄指定覆寫屬性。覆寫值在工廠狀態之後套用。每筆記錄完成兩個工廠回呼後才開始下一筆。後續記錄失敗時，先前寫入可能保留。用 `createOneQuietly` 和 `createManyQuietly` 抑制模型事件，或用對應的 `On` 方法指定連線。

用 `makeOne(attributes)` 建構一個未儲存的模型，或用 `makeMany(records)` 依序建構多個未儲存的模型。兩者皆接受明確屬性並執行 `afterMaking`。`makeMany` 使用相同的 `FactoryRecords` 選項，呼叫後保留已設定數量，數量為零時回傳空集合。兩種方法皆不儲存回傳的模型，解析已宣告的上層關聯時仍可能寫入上層模型。對應的 `On` 方法可指定連線。`lazy(attributes)` 回傳非同步回呼，呼叫並等待後才解析連線並建立單一模型或指定數量的批次。重複呼叫會重新建立。

用 `forEachSequence(states)` 為每個模型套用一組屬性，並依屬性組數決定批次數量。`crossJoinSequence(sequences)` 依宣告順序組合各序列，最後一個序列變化最快，同名屬性以後面的序列為準。另行設定批次數量，超過組合數時從頭循環。傳入空序列會回傳錯誤。

在單一模型或批次工廠上使用 `insert(attributes)` 批次寫入產生的列，或用 `insertOn(connection, attributes)` 指定連線。工廠會執行 `afterMaking` 並解析所需的上層鍵。批次寫入略過目標模型的持久化事件、子關聯建立及 `afterCreating`。寫入使用儲存屬性，包含隱藏欄位，略過追加欄位和輸出存取器。

在單一模型或批次工廠上使用 `withoutAfterMaking()` 或 `withoutAfterCreating()`，清除該工廠已登記的對應回呼。之後可以重新登記回呼。另一階段的回呼及 Eloquent 模型事件照常執行。

在單一模型或批次工廠上使用 `connection(Some("archive"))` 指定命名連線。`getConnectionName()` 回傳工廠設定的覆寫值，`connection(None)` 清除該值。執行時依序查找工廠覆寫、模型連線和目前應用或 Seeder 的預設連線。`On` 方法對目標工廠使用明確傳入的連線。巢狀工廠優先使用自身覆寫或模型連線，再繼承外層連線。關聯連線驗證照常執行。

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

用 `parentAttribute("owner_id", owners)` 宣告由另一工廠產生的外鍵屬性。它會展開為已儲存模型的鍵，也可重用相符的模型。定義、狀態或呼叫傳入的明確值優先。`withoutParents(vec![])` 停用所有工廠外鍵屬性。傳入 `FactoryParentSelector::attribute("owner_id")` 或 `FactoryParentSelector::model::<Owner>()` 可依屬性名稱或模型型別停用。停用的屬性展開為 null，明確的 `for` 和 `forExisting` 關聯仍會解析。

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

向 `hasAttached` 或 `hasAttachedNamed` 傳入屬性映射清單，即可依關聯模型順序設定中介資料表屬性。關聯工廠數量超過清單長度時會循環。用 `FactoryPivot::resolve` 根據已儲存的父模型計算每次附加的屬性。單一屬性映射會套用到所有關聯。

可傳入既有模型、模型清單或 Eloquent 集合，直接附加這些模型。模型必須已持久化、具有關聯所需的鍵，並屬於該關聯使用的連線。目標無效時，會在寫入這次附加的中介資料表記錄前回傳錯誤。
