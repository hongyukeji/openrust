# 集合

## 建立集合

用 `Collection::make` 建立循序集合，用 `Collection::fromEntries` 指定鍵。集合保留迭代順序。整數鍵與 `"3"` 這類標準十進位字串對應同一筆資料。`"03"` 這類字串保留原樣。

## 可用方法

`map`、`filter` 和 `sortBy` 保留原鍵。呼叫 `values()` 可將鍵重排為從零開始的連續整數。用 `groupBy` 分組，第二個參數傳入 `true` 可在組內保留原鍵。

```rust
use openrust::support::Collection;

fn main() {
    let amounts = Collection::fromEntries([(0, 1), (3, 2), (7, 3)]);
    let mapped = amounts.map(|value, _| value * 10);
    let selected = mapped.filter(|value, _| *value >= 20);

    assert_eq!(selected.toJson().unwrap(), r#"{"3":20,"7":30}"#);
    assert_eq!(selected.values().toJson().unwrap(), "[20,30]");

    let groups = Collection::make(["apple", "apricot", "pear"])
        .groupBy(|word, _| word.starts_with('a'), false);
    assert_eq!(groups.toJson().unwrap(), r#"{"1":["apple","apricot"],"0":["pear"]}"#);
}
```

回呼接收具型別的 Rust 值，可直接存取欄位。`sortBy` 使用回呼傳回值的型別排序，相等時保留輸入順序。`avg` 和 `median` 傳回 `Option<f64>`。

## 保留鍵的比較與位置操作 {#collection-key-operations}

`diff`、`intersect` 及其關聯鍵變體和 `duplicates` 保留傳回項目的原鍵。`shift` 和 `splice` 重新編號被修改集合的整數鍵。`split` 平衡各組長度，`splitIn` 優先填滿固定長度的組並保留鍵。`chunkBy` 按相鄰值分組。

```rust
use openrust::support::Collection;

let mut rows = Collection::fromEntries([("4", "甲"), ("keep", "乙"), ("9", "丙")]);
assert_eq!(rows.shift(), Some("甲"));
assert_eq!(rows.toJson()?, r#"{"keep":"乙","0":"丙"}"#);
let existing = Collection::make(["乙"]);
let remaining = rows.diff(&existing).toArray()?;
assert_eq!(remaining, serde_json::json!(["丙"]));
```

## 型別化轉換與巢狀 JSON {#collection-projections}

用型別化投影完成欄位比較、分組和執行期型別篩選。`mapToGroups` 傳回分組集合，`mapToDictionary` 傳回分組向量。`Collection<Value>` 支援點路徑、遞迴合併及取代。`random` 不放回抽樣，可保留鍵。

集合擴充使用 Rust 擴充 trait，高階訊息對應直接回呼，診斷可用 `dbg!(&collection.all())`。`lazy()` 的迭代器配接器提供快取和基於時間的走訪。

```rust
use openrust::support::Collection;
use serde_json::json;

let first = Collection::fromValue(json!({"team":{"tags":["甲"]}}));
let second = Collection::fromValue(json!({"team":{"tags":["乙"]}}));
let merged = first.mergeRecursive(second)?.dotWithDepth(Some(1))?.toArray()?;
assert_eq!(merged, json!({"team.tags":["甲","乙"]}));
```

## JSON 序列化

當鍵依迭代順序是從零開始的連續整數時，集合序列化為 JSON 陣列。其他鍵產生 JSON 物件。`toJson()` 傳回 JSON 字串或序列化錯誤，非有限浮點數也會回報錯誤。集合可直接傳給 `ApiResponse::success`。直接反序列化為 `Collection<T>` 可保留 JSON 物件成員順序。`fromValue` 使用傳入的 `serde_json::Value` 的順序。

## 惰性集合

資料庫列可透過查詢建構器的 [cursor](../database/queries#streaming-results) 非同步讀取。這個入口支援提早停止讀取。

### 簡介

`LazyCollection` 按需從 Rust 迭代器讀取項目，適合逐步處理大量資料。`take(2)` 只讀取產生兩筆結果所需的項目。負數限制會迭代到結尾，只保留指定大小的尾端。

### 建立惰性集合

將迭代器工廠傳給 `make`，帶鍵來源使用 `fromEntries`。可能失敗的來源使用 `tryMake` 或 `tryFromEntries`。普通迭代每次建立自己的迭代器，並在第一次錯誤時終止。組合後的迭代器在首次取值時才開啟來源。可透過 `getIterator()` 或迭代 `&collection` 逐項讀取結果。

```rust
use std::{cell::Cell, io::{BufRead, BufReader}};

use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let reads = Cell::new(0);
    let lines = LazyCollection::tryMake(|| {
        BufReader::new(&b"INFO ready\nWARN slow\nWARN retry\n"[..])
            .lines()
            .inspect(|_| reads.set(reads.get() + 1))
            .map(|line| line.map_err(|error| CollectionError::Source(Box::new(error))))
    })
    .filter(|line, _| line.starts_with("WARN"))
    .values()
    .remember();

    assert_eq!(reads.get(), 0);
    assert_eq!(lines.clone().take(1).collect()?.toJson()?, r#"["WARN slow"]"#);
    assert_eq!(reads.get(), 2);
    assert_eq!(lines.collect()?.toJson()?, r#"["WARN slow","WARN retry"]"#);
    assert_eq!(reads.get(), 3);
    Ok(())
}
```

### Enumerable 契約

`Collection` 和 `LazyCollection` 都實作 `Enumerable`，提供 `map`、`filter`、`values`、`take`、`all` 和 `collect`。`all` 或 `collect` 收集迭代結果時，同一個鍵保留最後一個值。需要保留每次出現的項目時，先呼叫 `values`。`Collection::lazy()` 可將已收集的集合用作來源。

### 惰性集合方法

`tapEach` 在讀取項目時執行副作用。`takeUntilTimeout` 在兩次讀取之間檢查單調時鐘的截止時間。`takeUntilTimeoutWith` 還可傳回最後一個項目，若起初就逾時則傳回空值。`throttle` 在呼叫執行緒等待剩餘間隔。`withHeartbeat` 在迭代時檢查間隔。同步讀取可能在截止時間之後才傳回。

`chunk` 每次產生一個已收集的 `Collection`，可選擇保留組內鍵。惰性集合在擁有它的執行緒執行，可以借用執行緒內的區域狀態。

#### 分組與滑動視窗 {#lazy-grouping-and-sliding}

`chunkWhile` 對連續項目分組。回呼收到下一個值、它的鍵和目前的群組。回傳 false 時，該項目成為新群組的第一項。`sliding(size, step)` 只產出完整視窗，視窗大小和步長都須為正數。分組和視窗保留原始鍵，每組以實體化的 `Collection` 回傳。滑動視窗要求值實作 `Clone`，讓各視窗獨立擁有值。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::make(|| [4, 4, 8, 8, 12]);
    let groups = readings
        .clone()
        .chunkWhile(|reading, _, group| group.last() == Some(reading));
    assert_eq!(groups.count()?, 3);

    let windows = readings.sliding(2, 2)?;
    assert_eq!(windows.count()?, 2);
    assert_eq!(windows.first()?.unwrap().toJson()?, "[4,4]");
    Ok(())
}
```

#### 選取與附加項目 {#lazy-selection-and-append}

`slice(offset, length)` 保留原始鍵。偏移或長度為負數時，先實體化來源並套用集合的鍵規則。`nth(step, offset)` 從偏移位置開始每隔指定步長選取一項，並重新編號。`keys()` 按讀取順序產出鍵，包括重複鍵。`concat` 讀完第一個惰性集合後才開啟第二個，並統一重排索引。`chunkBy` 依回呼傳回的型別化值對連續項目分組。

```rust
use openrust::support::{ArrayKey, CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("a", 4), ("b", 7), ("c", 11)]);
    assert_eq!(
        readings.clone().slice(1, Some(1)).all()?,
        vec![(ArrayKey::from("b"), 7)]
    );
    assert_eq!(readings.clone().keys().count()?, 3);
    assert_eq!(readings.clone().chunkBy(|value, _| value / 10).count()?, 2);

    let combined = readings.nth(2, 0)?.concat(LazyCollection::make(|| [15]));
    assert_eq!(combined.collect()?.toJson()?, "[4,11,15]");
    Ok(())
}
```

#### 展開巢狀項目 {#lazy-expanding-nested-items}

`collapse` 展開一層巢狀的惰性集合、實體集合、向量或陣列，並重新編號。`collapseWithKeys` 保留內部鍵，實體化時同鍵的最後一個值生效。`flatMap` 將每個項目映射成一組，再展開。對於 JSON 值，`flatten(Some(depth))` 按指定深度展開陣列與物件並保留純量。用 `None` 展開所有層級。

```rust
use openrust::support::{Collection, CollectionError, LazyCollection};
use serde_json::json;

fn main() -> Result<(), CollectionError> {
    let groups = LazyCollection::make(|| {
        [
            Collection::fromEntries([("first", 1)]),
            Collection::fromEntries([("second", 2)]),
        ]
    });
    assert_eq!(
        groups.collapseWithKeys().collect()?.toJson()?,
        r#"{"first":1,"second":2}"#
    );

    let doubled = LazyCollection::make(|| [1, 3]).flatMap(|value, _| [*value, value * 2]);
    assert_eq!(doubled.collect()?.toJson()?, "[1,2,3,6]");

    let nested = LazyCollection::make(|| [json!([1, [2]]), json!(3)]);
    assert_eq!(nested.flatten(None).collect()?.toJson()?, "[1,2,3]");
    Ok(())
}
```

#### 配對多個來源 {#lazy-pairing-sources}

`zip` 按位置配對值。用 `zipMany` 將第一個來源與多個後續來源配對。兩者都讀取到最長來源結束，缺少的項目以 `None` 表示。`combine` 將第一個來源的值用作鍵，與第二個來源的值配對。迭代發現長度不同時回傳 `CollectionError::InvalidArgument`。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let numbers = LazyCollection::from([1, 2]);
    let rows = numbers.clone().zip(["one"]);
    assert_eq!(rows.collect()?.toJson()?, r#"[[1,"one"],[2,null]]"#);

    let labels = LazyCollection::from(["first", "second"]);
    assert_eq!(
        labels.combine(numbers).collect()?.toJson()?,
        r#"{"first":1,"second":2}"#
    );
    Ok(())
}
```

#### 去重與尋找符合項目 {#lazy-deduplicating-and-finding}

`unique` 和 `uniqueStrict` 使用型別化相等比較。`uniqueBy` 依回呼傳回的值去重。這些方法保留第一次出現的項目及其原始鍵。`contains` 和 `firstOrFail` 在第一次符合時停止。`hasSole` 和 `hasMany` 最多讀取兩個符合項目。`sole` 先依鍵實體化前兩個符合項目，再檢查是否只剩一個值。用帶 `Where` 的方法傳入述詞。得出結果前遇到的來源錯誤會原樣傳回。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("a", 10), ("b", 11), ("c", 21)]);
    let distinct = readings.clone().uniqueBy(|value, _| value / 10);
    assert_eq!(distinct.collect()?.toJson()?, r#"{"a":10,"c":21}"#);
    assert!(readings.contains(&11)?);
    assert!(readings.hasSoleWhere(|value, _| *value > 20)?);
    assert_eq!(readings.firstOrFailWhere(|value, _| *value > 10)?, 11);
    Ok(())
}
```

#### 排序與尋找重複項目 {#lazy-sorting-and-duplicates}

`sort`、`sortBy`、`sortKeys` 及其降冪版本、`reverse` 和 `duplicates` 都會在第一次取值時實體化來源。同一來源鍵重複出現時使用最後一個值。排序和反轉保留鍵。`duplicatesBy` 傳回重複的投影值，並保留它們的原始鍵。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("a", 12), ("b", 7), ("c", 12)]);
    let sorted = readings.clone().sort();
    assert_eq!(sorted.collect()?.toJson()?, r#"{"b":7,"a":12,"c":12}"#);
    let duplicates = readings.duplicatesBy(|value, _| value / 10);
    assert_eq!(duplicates.collect()?.toJson()?, r#"{"c":1}"#);
    Ok(())
}
```

#### 彙總值 {#lazy-aggregating-values}

`sum`、`avg`、`min`、`max` 和 `reduce` 單次迭代來源，重複鍵的每次出現都參與計算。`every` 遇到第一個不符合條件的項目就停止。`countBy` 逐項計數，讀完來源後產出各組計數。`median` 和 `mode` 先實體化來源，同鍵以最後一個值為準。空來源的平均值、極值、中位數和眾數傳回 `None`。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("x", 90), ("y", 5), ("x", 7), ("z", 5)]);
    assert_eq!(readings.sum()?, 107);
    assert_eq!(readings.median()?, Some(5.0));
    assert_eq!(readings.mode()?, Some(vec![5]));

    let counts = readings.countBy(|value, _| if *value == 5 { "low" } else { "high" });
    assert_eq!(counts.collect()?.toJson()?, r#"{"high":2,"low":2}"#);
    Ok(())
}
```

#### 計算符合條件的百分比 {#lazy-percentage}

`percentage(predicate, precision)` 計算符合判定條件的項目百分比，並按指定小數位捨入。判定函式依來源順序收到每個項目的值與鍵。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("a", 95), ("b", 82), ("c", 70)]);
    let passing = readings.percentage(|value, _| *value >= 80, 2)?;
    assert_eq!(passing, Some(66.67));
    Ok(())
}
```

該方法只走訪來源一次。空來源回傳 `None`，來源錯誤會結束走訪。精度使用非負整數。

#### 比較與更新集合 {#lazy-comparing-and-updating}

`diff`、`intersect` 及其依鍵和關聯比較的變體，在第一次取值時實體化兩個來源。`union` 保留既有鍵對應的值。`merge` 覆蓋同名字串鍵，並重排數字鍵。`replace` 先讀完替換來源，再替換主要來源中每個鍵第一次出現的值，最後附加主要來源未出現的替換鍵。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let current = LazyCollection::fromEntries(|| [("a", 1), ("b", 2)]);
    let incoming = LazyCollection::fromEntries(|| [("b", 2), ("c", 3)]);
    assert_eq!(
        current
            .clone()
            .diffAssoc(incoming.clone())
            .collect()?
            .toJson()?,
        r#"{"a":1}"#
    );
    assert_eq!(
        current.replace(incoming).collect()?.toJson()?,
        r#"{"a":1,"b":2,"c":3}"#
    );
    Ok(())
}
```

#### 依鍵篩選與投影 {#lazy-keys-and-projection}

`only` 按來源順序傳回每個指定鍵第一次出現的項目，全部找到後停止。`except` 先實體化來源，再排除指定鍵。`keyBy`、`pluckWithKeys` 和 `mapWithKeys` 可多次產出同一個鍵，結果實體化時以最後一個值為準。`groupBy`、`mapToDictionary` 和 `mapToGroups` 在第一次取值時實體化來源，再傳回完整分組。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let records = LazyCollection::fromEntries(|| [("a", 10), ("a", 12), ("b", 7)]);
    assert_eq!(records.clone().only(["a"]).first()?, Some(10));
    assert_eq!(records.except(["b"]).first()?, Some(12));

    let readings = LazyCollection::from([("a", 10), ("b", 7)]);
    let mapped = readings.mapWithKeys(|record, _| [(record.0, record.1 * 2)]);
    assert_eq!(mapped.collect()?.toJson()?, r#"{"a":20,"b":14}"#);
    Ok(())
}
```

#### 尋找項目與相鄰值 {#lazy-looking-up-items}

`get` 傳回指定鍵第一次出現的項目。`has` 檢查單一鍵，`hasAll` 檢查所有指定鍵，`hasAny` 檢查是否存在任一指定鍵。PHP 的寬鬆相等可以比對 `"08"` 和 `8`，Rust 依正規化後的 `ArrayKey` 比較，兩者是不同的鍵。`search` 傳回第一個符合的鍵。`before` 和 `after` 按來源順序傳回相鄰值。`last` 迭代到來源結束。

```rust
use openrust::support::{ArrayKey, CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("08", 10), ("8", 12), ("end", 7)]);
    assert_eq!(readings.get(8)?, Some(12));
    assert!(readings.hasAll(["08", "end"])?);
    assert_eq!(readings.search(&12)?, Some(ArrayKey::from(8)));
    assert_eq!(readings.before(&12)?, Some(10));
    assert_eq!(readings.after(&12)?, Some(7));
    assert_eq!(readings.last()?, Some(7));
    Ok(())
}
```

#### 篩選值與欄位 {#lazy-filtering-values}

`r#where` 和 `whereIn` 比較型別化投影傳回的值。嚴格版本使用相同的 Rust 相等規則。`whereBetween` 包含兩個端點。`whereNotBetween` 比對低於下界或高於上界的值。對 JSON，`whereNullAt` 比對缺少的路徑或明確的空值，`whereNotNullAt` 保留存在且非空的值。這些篩選方法按需讀取，並保留原始鍵。

```rust
use openrust::support::{CollectionError, LazyCollection};
use serde_json::json;

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("low", 1), ("same", 3), ("same", 4)]);
    let selected = readings.whereBetween(|value| *value, 3..=4).values();
    assert_eq!(selected.collect()?.toJson()?, "[3,4]");

    let records = LazyCollection::from([json!({}), json!({"status": false})]);
    assert_eq!(records.whereNotNullAt("status").count()?, 1);
    Ok(())
}
```

#### 分頁、拆分與填充 {#lazy-paging-and-partition}

`forPage` 的頁碼從 1 開始，保留來源鍵。`split` 在第一次取值時實體化來源並均衡分組。`splitIn` 呼叫時先計數，再按需迭代以填滿前面的群組。`partition` 單次迭代，傳回通過與未通過的兩個實體集合。正數 `pad` 在來源結束後從右側補齊，負數則先實體化並從左側補齊。右側填充無法分配整數鍵時，Rust 傳回索引溢位錯誤。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::from([10, 20, 30, 40, 50]);
    let page = readings.clone().forPage(2, 2).values();
    assert_eq!(page.collect()?.toJson()?, "[30,40]");

    let (passed, failed) = readings.partition(|value, _| *value >= 30)?;
    assert_eq!(passed.count(), 3);
    assert_eq!(failed.count(), 2);
    let padded = readings.pad(6, 0);
    assert_eq!(padded.collect()?.toJson()?, "[10,20,30,40,50,0]");
    Ok(())
}
```

#### 序列化值 {#lazy-serializing-values}

`toArray` 和 `jsonSerialize` 傳回 JSON 值。`toJson` 與 `toPrettyJson` 傳回編碼後的文字。每次呼叫都會實體化來源。從零開始的連續整數鍵產生陣列，稀疏鍵或具名鍵產生物件。重複鍵保留最後一個值。先呼叫 `values()` 可保留每次出現。`fromJson` 將陣列或物件解碼成可重複迭代的來源。巢狀的惰性集合支援 Serde 序列化。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("a", 10), ("a", 12), ("b", 7)]);
    assert_eq!(readings.toJson()?, r#"{"a":12,"b":7}"#);
    assert_eq!(readings.values().toJson()?, "[10,12,7]");

    let decoded = LazyCollection::<i32>::fromJson("[4,5]")?;
    assert_eq!(decoded.toJson()?, "[4,5]");
    Ok(())
}
```

#### 生成與包裝來源 {#lazy-generating-sources}

`range` 從起點朝終點生成整數。步長恰好到達終點時包含終點。步長以正數 `NonZeroUsize` 表示。`times` 將從 1 開始的位置傳給回呼，結果鍵從 0 開始。`take` 可限制生成數量。`wrap(Some(item))` 產出一項，`wrap(None)` 產生空來源。`fromValue` 接收 JSON 值，`unwrap` 實體化為帶鍵項目。

```rust
use std::num::NonZeroUsize;

use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let numbers = LazyCollection::range(9, 1, NonZeroUsize::new(2).unwrap());
    assert_eq!(numbers.take(3).toJson()?, "[9,7,5]");

    let readings = LazyCollection::times(10_000, |index| index * 5);
    assert_eq!(readings.take(2).toJson()?, "[5,10]");
    assert_eq!(
        LazyCollection::wrap(Some("ready")).toJson()?,
        r#"["ready"]"#
    );
    Ok(())
}
```

#### 條件分支與管線 {#lazy-conditionals-and-pipelines}

`when` 和 `unless` 立即選擇回呼。`whenEmpty` 與 `whenNotEmpty` 會探測第一項並可能傳回來源錯誤。`whenEmptyElse` 和 `whenNotEmptyElse` 接收備用回呼。`pipe` 可改變傳回型別，`pipeThrough` 串接傳回相同集合型別的步驟。`tap` 在呼叫處檢查集合。`ensure` 使用述詞逐項驗證，第一次不符合時停止。後續迭代會再次開啟來源，可使用 `remember()` 重播已讀項目。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::<i32>::empty()
        .whenEmpty(|_| LazyCollection::from([3, 5]))?
        .when(true, |items| items.map(|value, _| value * 2))
        .ensure(|value, _| *value <= 10)?;

    let total = readings.pipe(|items| items.reduceWithKeys(0, |sum, value, _| sum + value))?;
    assert_eq!(total, 16);
    Ok(())
}
```

#### 轉換記錄與巢狀列 {#lazy-transforming-records}

`select` 從每筆記錄中擷取指定 JSON 欄位，並重新編號。`flip` 交換型別為 `ArrayKey` 的值與鍵。`mapSpread` 和 `eachSpread` 把列切片及其鍵傳給回呼。`mapInto` 透過 `From` 轉換每個值。`mergeRecursive`、`replaceRecursive` 和 `multiply` 在第一次取值時實體化輸入。

```rust
use openrust::support::{ArrayKey, CollectionError, LazyCollection};
use serde_json::json;

fn main() -> Result<(), CollectionError> {
    let fields = LazyCollection::fromEntries(|| [("active", ArrayKey::from("status"))]);
    assert_eq!(fields.flip().toJson()?, r#"{"status":"active"}"#);

    let records = LazyCollection::from([json!({"status":"active","internal":42})]);
    assert_eq!(
        records.select(&["status"]).toJson()?,
        r#"[{"status":"active"}]"#
    );
    let rows = LazyCollection::from([vec![2_u8, 3]]);
    assert_eq!(
        rows.mapSpread(|row, _| row.iter().sum::<u8>())
            .mapInto::<u16>()
            .first()?,
        Some(5)
    );
    Ok(())
}
```

#### 組合與抽樣 {#lazy-combining-and-sampling}

`crossJoin` 與傳入的切片計算笛卡兒積，在第一次取值時實體化主要來源。`shuffle` 同樣在第一次取值時實體化，並為打亂後的值重新編號。`randItem` 和 `random` 在呼叫時抽樣。`random(count, preserve_keys)` 不放回抽樣，傳回的集合每次迭代都重播已選項目。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let pairs = LazyCollection::from([1, 2]).crossJoin(&[&[10, 20]]);
    assert_eq!(pairs.first()?, Some(vec![1, 10]));

    let names = LazyCollection::fromEntries(|| [("x", 4), ("y", 7), ("z", 9)]);
    let sample = names.random(2, true)?;
    let selected = sample.toJson()?;
    assert_eq!(sample.count()?, 2);
    assert_eq!(sample.toJson()?, selected);
    Ok(())
}
```

#### 單次走訪的快取 {#lazy-caching-iterator}

`getCachingIterator()` 會預先讀取一項。用 `hasNext()` 檢查是否還有結果，再用 `next()` 前進。預先讀取發現的來源錯誤會在前進到該項時回傳。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let numbers = LazyCollection::fromEntries(|| [("a", 1), ("a", 2), ("b", 3)]);
    let mut iterator = numbers.getCachingIteratorWithFullCache();

    assert_eq!(iterator.next().transpose()?, Some(("a".into(), 1)));
    assert!(iterator.hasNext());
    assert_eq!(
        iterator.getCache().and_then(|cache| cache.get("a")),
        Some(&1)
    );

    assert_eq!(iterator.next().transpose()?, Some(("a".into(), 2)));
    assert_eq!(
        iterator.getCache().and_then(|cache| cache.get("a")),
        Some(&2)
    );
    Ok(())
}
```

用 `getCachingIteratorWithFullCache()` 啟用 Laravel 的完整快取行為。項目值需要能複製，每個鍵保留最近回傳的值。`getCache()` 提供已回傳的項目。需要跨多次走訪重用同一來源時，請用 `remember()`。

舊名稱 `containsOneItem()` 和 `containsManyItems()` 分別呼叫 `hasSole()` 和 `hasMany()`。若要檢查是否恰有一項符合條件，請使用 `hasSoleWhere()`。

#### `remember()`

`remember()` 建立一個共享迭代器，按需保留已讀項目。每次迭代按位置重播，超過快取前綴時繼續從原來源讀取。來源錯誤也會重播。值須實作 `Clone`，讓每次迭代取得擁有所有權的值。快取屬於集合本身，會隨讀取量增長。
