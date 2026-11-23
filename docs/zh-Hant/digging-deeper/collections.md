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

    assert_eq!(selected.to_json().unwrap(), r#"{"3":20,"7":30}"#);
    assert_eq!(selected.values().to_json().unwrap(), "[20,30]");

    let groups = Collection::make(["apple", "apricot", "pear"])
        .groupBy(|word, _| word.starts_with('a'), false);
    assert_eq!(groups.to_json().unwrap(), r#"{"1":["apple","apricot"],"0":["pear"]}"#);
}
```

回呼接收具型別的 Rust 值，可直接存取欄位。`sortBy` 使用回呼傳回值的型別排序，相等時保留輸入順序。`avg` 和 `median` 傳回 `Option<f64>`。

## 保留鍵的比較與位置操作 {#collection-key-operations}

`diff`、`intersect` 及其關聯鍵變體和 `duplicates` 保留傳回項目的原鍵。`shift` 和 `splice` 重新編號被修改集合的整數鍵。`split` 平衡各組長度，`splitIn` 優先填滿固定長度的組並保留鍵。`chunkBy` 按相鄰值分組。

```rust
use openrust::support::Collection;

let mut rows = Collection::fromEntries([("4", "甲"), ("keep", "乙"), ("9", "丙")]);
assert_eq!(rows.shift(), Some("甲"));
assert_eq!(rows.to_json()?, r#"{"keep":"乙","0":"丙"}"#);
let existing = Collection::make(["乙"]);
let remaining = rows.diff(&existing).to_array()?;
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
let merged = first.mergeRecursive(second)?.dotWithDepth(Some(1))?.to_array()?;
assert_eq!(merged, json!({"team.tags":["甲","乙"]}));
```

## JSON 序列化

當鍵依迭代順序是從零開始的連續整數時，集合序列化為 JSON 陣列。其他鍵產生 JSON 物件。`to_json()` 傳回 JSON 字串或序列化錯誤，非有限浮點數也會回報錯誤。集合可直接傳給 `ApiResponse::success`。直接反序列化為 `Collection<T>` 可保留 JSON 物件成員順序。`fromValue` 使用傳入的 `serde_json::Value` 的順序。

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
    assert_eq!(lines.clone().take(1).collect()?.to_json()?, r#"["WARN slow"]"#);
    assert_eq!(reads.get(), 2);
    assert_eq!(lines.collect()?.to_json()?, r#"["WARN slow","WARN retry"]"#);
    assert_eq!(reads.get(), 3);
    Ok(())
}
```

### Enumerable 契約

`Collection` 和 `LazyCollection` 都實作 `Enumerable`，提供 `map`、`filter`、`values`、`take`、`all` 和 `collect`。`all` 或 `collect` 收集迭代結果時，同一個鍵保留最後一個值。需要保留每次出現的項目時，先呼叫 `values`。`Collection::lazy()` 可將已收集的集合用作來源。

### 惰性集合方法

`tapEach` 在讀取項目時執行副作用。

`chunk` 每次產生一個已收集的 `Collection`，可選擇保留組內鍵。惰性集合在擁有它的執行緒執行，可以借用執行緒內的區域狀態。

#### 截止時間與逾時回呼 {#lazy-deadline}

給 `takeUntilTimeout` 傳入單調時鐘的 `Instant`。走訪開始時已到截止時間，來源不會開啟。已經開始的讀取可以在截止後產出一項，下一次取值直接結束走訪。

```rust
use std::{
    cell::RefCell,
    thread,
    time::{Duration, Instant},
};

use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let deadline = Instant::now() + Duration::from_millis(250);
    let notified = RefCell::new(None);
    let source = LazyCollection::fromEntries(|| {
        thread::sleep(deadline.saturating_duration_since(Instant::now()));
        [("first", 1), ("later", 2)]
    })
    .takeUntilTimeoutWith(deadline, |entry| {
        *notified.borrow_mut() = entry.map(|(value, key)| (*value, key.clone()));
    });

    let mut items = source.getIterator();
    assert_eq!(items.next().transpose()?, Some(("first".into(), 1)));
    assert_eq!(*notified.borrow(), None);
    assert!(items.next().is_none());
    assert_eq!(*notified.borrow(), Some((1, "first".into())));
    Ok(())
}
```

使用 `takeUntilTimeoutWith` 時，開始即逾時會用 `None` 呼叫回呼。走訪中達到截止時間時，下一次取值會把上一項的值和鍵傳給回呼。來源正常耗盡或讀取失敗時仍傳回各自的結果。

#### 節流與心跳 {#lazy-pacing}

`throttle` 在繼續迭代時讓呼叫執行緒等待，從間隔中扣除處理上一個項目已花費的時間。讀取到耗盡會在最後一個項目之後再推進一次迭代器。透過 `take` 提前停止時，最後一次等待尚未開始。

`withHeartbeat` 在成功讀取後檢查經過的時間，並在產出該項目前執行回呼。每次心跳後重新計時，每輪走訪也獨立計時。間隔為零時，每個成功讀取的項目都會觸發回呼。

```rust
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let beats = Cell::new(0);
    let interval = Duration::from_millis(2);
    let jobs = LazyCollection::make(|| ["import", "index"])
        .withHeartbeat(Duration::ZERO, || beats.set(beats.get() + 1))
        .throttle(interval);

    assert_eq!(beats.get(), 0);
    let started = Instant::now();
    assert_eq!(jobs.count()?, 2);
    assert!(started.elapsed() >= interval * 2);
    assert_eq!(beats.get(), 2);
    Ok(())
}
```

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
    assert_eq!(windows.first()?.unwrap().to_json()?, "[4,4]");
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
    assert_eq!(combined.collect()?.to_json()?, "[4,11,15]");
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
        groups.collapseWithKeys().collect()?.to_json()?,
        r#"{"first":1,"second":2}"#
    );

    let doubled = LazyCollection::make(|| [1, 3]).flatMap(|value, _| [*value, value * 2]);
    assert_eq!(doubled.collect()?.to_json()?, "[1,2,3,6]");

    let nested = LazyCollection::make(|| [json!([1, [2]]), json!(3)]);
    assert_eq!(nested.flatten(None).collect()?.to_json()?, "[1,2,3]");
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
    assert_eq!(rows.collect()?.to_json()?, r#"[[1,"one"],[2,null]]"#);

    let labels = LazyCollection::from(["first", "second"]);
    assert_eq!(
        labels.combine(numbers).collect()?.to_json()?,
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
    assert_eq!(distinct.collect()?.to_json()?, r#"{"a":10,"c":21}"#);
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
    assert_eq!(sorted.collect()?.to_json()?, r#"{"b":7,"a":12,"c":12}"#);
    let duplicates = readings.duplicatesBy(|value, _| value / 10);
    assert_eq!(duplicates.collect()?.to_json()?, r#"{"c":1}"#);
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
    assert_eq!(counts.collect()?.to_json()?, r#"{"high":2,"low":2}"#);
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
            .to_json()?,
        r#"{"a":1}"#
    );
    assert_eq!(
        current.replace(incoming).collect()?.to_json()?,
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
    assert_eq!(mapped.collect()?.to_json()?, r#"{"a":20,"b":14}"#);
    Ok(())
}
```

#### 尋找項目與相鄰值 {#lazy-looking-up-items}

`get` 傳回指定鍵第一次出現的項目。`has` 檢查單一鍵，`hasAll` 檢查所有指定鍵，`has_any` 檢查是否存在任一指定鍵。PHP 的寬鬆相等可以比對 `"08"` 和 `8`，Rust 依正規化後的 `ArrayKey` 比較，兩者是不同的鍵。`search` 傳回第一個符合的鍵。`before` 和 `after` 按來源順序傳回相鄰值。`last` 迭代到來源結束。

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

#### 缺失值回退 {#lazy-lookup-fallbacks}

用 `getOrElse`、`firstOrElse`、`lastOrElse` 或 `valueOrElse` 傳入回退閉包。需要依條件查找時，用 `firstWhereOrElse` 和 `lastWhereOrElse`。

```rust
use openrust::support::{CollectionError, LazyCollection};
use serde_json::{Value, json};

fn main() -> Result<(), CollectionError> {
    let rows = LazyCollection::fromEntries(|| {
        [
            ("draft", json!({"profile":{"name":null}})),
            ("published", json!({"profile":{"name":"Taylor"}})),
        ]
    });

    let row = rows.getOrElse("archived", || json!({"status":"missing"}))?;
    assert_eq!(row, json!({"status":"missing"}));
    assert_eq!(
        rows.valueOrElse("profile.name", || json!("anonymous"))?,
        Value::Null
    );
    let row = rows.lastWhereOrElse(
        |_, key| key == &"published".into(),
        || json!({"status":"missing"}),
    )?;
    assert_eq!(row["profile"]["name"], "Taylor");
    Ok(())
}
```

查找無結果時回退閉包執行一次。路徑上已有的 JSON null 仍是查找結果，來源錯誤會傳回呼叫端。也可以用 `|| default` 傳入已有值。

#### 篩選值與欄位 {#lazy-filtering-values}

`r#where` 和 `where_in` 比較型別化投影傳回的值。嚴格版本使用相同的 Rust 相等規則。`whereBetween` 包含兩個端點。`whereNotBetween` 比對低於下界或高於上界的值。對 JSON，`whereNullAt` 比對缺少的路徑或明確的空值，`whereNotNullAt` 保留存在且非空的值。這些篩選方法按需讀取，並保留原始鍵。

```rust
use openrust::support::{CollectionError, LazyCollection};
use serde_json::json;

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("low", 1), ("same", 3), ("same", 4)]);
    let selected = readings.whereBetween(|value| *value, 3..=4).values();
    assert_eq!(selected.collect()?.to_json()?, "[3,4]");

    let records = LazyCollection::from([json!({}), json!({"status": false})]);
    assert_eq!(records.whereNotNullAt("status").count()?, 1);
    Ok(())
}
```

#### 依值選擇走訪邊界 {#lazy-value-boundaries}

用 `takeUntilValue`、`takeWhileValue`、`skipUntilValue` 和 `skipWhileValue` 直接比較具型別的值。比較沿用 Rust 的 `PartialEq`。`takeUntilValue` 排除第一個相符項目，`skipUntilValue` 從第一個相符項目開始傳回。

```rust
use openrust::support::{CollectionError, LazyCollection};
use serde_json::json;

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| {
        [
            ("text", json!("2")),
            ("marker", json!(2)),
            ("hidden", json!("0")),
            ("tail", json!(3)),
        ]
    });

    let before = readings.clone().takeUntilValue(json!(2));
    let from = readings.skipUntilValue(json!(2)).filterTruthy();
    assert_eq!(before.to_json()?, r#"{"text":"2"}"#);
    assert_eq!(from.to_json()?, r#"{"marker":2,"tail":3}"#);
    Ok(())
}
```

JSON 值可用 `filterTruthy()` 依 PHP 真值規則篩選，JSON 物件依關聯陣列處理。它會略過 null、false、零、空字串、字串 `0`、空陣列和空映射。這些操作都按需讀取、保留原鍵並傳遞來源錯誤。

#### 分頁、拆分與填充 {#lazy-paging-and-partition}

`forPage` 的頁碼從 1 開始，保留來源鍵。`split` 在第一次取值時實體化來源並均衡分組。`splitIn` 呼叫時先計數，再按需迭代以填滿前面的群組。`partition` 單次迭代，傳回通過與未通過的兩個實體集合。正數 `pad` 在來源結束後從右側補齊，負數則先實體化並從左側補齊。右側填充無法分配整數鍵時，Rust 傳回索引溢位錯誤。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::from([10, 20, 30, 40, 50]);
    let page = readings.clone().forPage(2, 2).values();
    assert_eq!(page.collect()?.to_json()?, "[30,40]");

    let (passed, failed) = readings.partition(|value, _| *value >= 30)?;
    assert_eq!(passed.count(), 3);
    assert_eq!(failed.count(), 2);
    let padded = readings.pad(6, 0);
    assert_eq!(padded.collect()?.to_json()?, "[10,20,30,40,50,0]");
    Ok(())
}
```

#### 序列化值 {#lazy-serializing-values}

`to_array` 和 `jsonSerialize` 傳回 JSON 值。`to_json` 與 `to_pretty_json` 傳回編碼後的文字。每次呼叫都會實體化來源。從零開始的連續整數鍵產生陣列，稀疏鍵或具名鍵產生物件。重複鍵保留最後一個值。先呼叫 `values()` 可保留每次出現。`fromJson` 將陣列或物件解碼成可重複迭代的來源。巢狀的惰性集合支援 Serde 序列化。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("a", 10), ("a", 12), ("b", 7)]);
    assert_eq!(readings.to_json()?, r#"{"a":12,"b":7}"#);
    assert_eq!(readings.values().to_json()?, "[10,12,7]");

    let decoded = LazyCollection::<i32>::fromJson("[4,5]")?;
    assert_eq!(decoded.to_json()?, "[4,5]");
    Ok(())
}
```

#### 串接投影值 {#lazy-implode-by}

用 `implode(glue)` 串接可顯示的值。對記錄使用 `implodeBy(project, glue)`，可選取欄位或格式化每一項，回呼也能讀取項目的鍵。傳入空分隔符可直接串接。

```rust
use openrust::support::{CollectionError, LazyCollection};

struct Order {
    product: &'static str,
}

fn main() -> Result<(), CollectionError> {
    let orders = LazyCollection::fromEntries(|| {
        [("a", "Desk"), ("b", "Lamp"), ("a", "Chair")]
            .map(|(key, product)| (key, Order { product }))
    });

    assert_eq!(
        orders.implodeBy(|order, _| order.product, ", ")?,
        "Chair, Lamp"
    );
    assert_eq!(
        orders.implodeBy(|order, key| format!("{key}:{}", order.product), " / ")?,
        "a:Chair / b:Lamp"
    );
    Ok(())
}
```

惰性集合先具體化，再呼叫投影。重複鍵最後一次出現的項目生效，鍵保留首次出現的位置。讀取來源時發生錯誤會在執行投影前傳回。

#### 生成與包裝來源 {#lazy-generating-sources}

`range` 從起點朝終點生成整數。步長恰好到達終點時包含終點。步長以正數 `NonZeroUsize` 表示。`times` 將從 1 開始的位置傳給回呼，結果鍵從 0 開始。`take` 可限制生成數量。`wrap(Some(item))` 產出一項，`wrap(None)` 產生空來源。`fromValue` 接收 JSON 值，`unwrap` 實體化為帶鍵項目。

```rust
use std::num::NonZeroUsize;

use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let numbers = LazyCollection::range(9, 1, NonZeroUsize::new(2).unwrap());
    assert_eq!(numbers.take(3).to_json()?, "[9,7,5]");

    let readings = LazyCollection::times(10_000, |index| index * 5);
    assert_eq!(readings.take(2).to_json()?, "[5,10]");
    assert_eq!(
        LazyCollection::wrap(Some("ready")).to_json()?,
        r#"["ready"]"#
    );
    Ok(())
}
```

#### 條件分支與管線 {#lazy-conditionals-and-pipelines}

`when` 和 `unless` 立即選擇回呼。`when_empty` 與 `whenNotEmpty` 會探測第一項並可能傳回來源錯誤。`whenEmptyElse` 和 `whenNotEmptyElse` 接收備用回呼。`pipe` 可改變傳回型別，`pipeThrough` 串接傳回相同集合型別的步驟。`tap` 在呼叫處檢查集合。`ensure` 使用述詞逐項驗證，第一次不符合時停止。後續迭代會再次開啟來源，可使用 `remember()` 重播已讀項目。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::<i32>::empty()
        .when_empty(|_| LazyCollection::from([3, 5]))?
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
    assert_eq!(fields.flip().to_json()?, r#"{"status":"active"}"#);

    let records = LazyCollection::from([json!({"status":"active","internal":42})]);
    assert_eq!(
        records.select(&["status"]).to_json()?,
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
    let selected = sample.to_json()?;
    assert_eq!(sample.count()?, 2);
    assert_eq!(sample.to_json()?, selected);
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
        iterator.get_cache().and_then(|cache| cache.get("a")),
        Some(&1)
    );

    assert_eq!(iterator.next().transpose()?, Some(("a".into(), 2)));
    assert_eq!(
        iterator.get_cache().and_then(|cache| cache.get("a")),
        Some(&2)
    );
    Ok(())
}
```

用 `getCachingIteratorWithFullCache()` 啟用 完整快取行為。項目值需要能複製，每個鍵保留最近回傳的值。`get_cache()` 提供已回傳的項目。需要跨多次走訪重用同一來源時，請用 `remember()`。

舊名稱 `containsOneItem()` 和 `containsManyItems()` 分別呼叫 `hasSole()` 和 `hasMany()`。若要檢查是否恰有一項符合條件，請使用 `hasSoleWhere()`。

#### 跳脫字串表示 {#lazy-escaped-string}

`toString()` 把集合轉換成 JSON 文字。啟用 `escapeWhenCastingToString(true)` 後，會對文字執行 HTML 跳脫，現有實體也一併跳脫。明確呼叫 `toString()` 可接收來源與序列化錯誤。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let values = LazyCollection::from(["<tag> &lt;"]).escapeWhenCastingToString(true);
    assert_eq!(values.toString()?, "[&quot;&lt;tag&gt; &amp;lt;&quot;]");
    assert_eq!(values.to_json()?, r#"["<tag> &lt;"]"#);
    Ok(())
}
```

此開關作用於字串轉換。`to_json()` 和 `to_pretty_json()` 仍回傳 JSON。

#### 輸出惰性集合的除錯內容 {#lazy-dump}

用 `dump()` 把帶鍵的快照寫到標準輸出，再繼續鏈式呼叫。`dumpWith(&[...])` 在項目之後輸出額外的除錯值。Rust 用 `Debug` 格式輸出兩者。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let values = LazyCollection::from([1, 2, 3]).dumpWith(&[&"before mapping"])?;
    assert_eq!(values.map(|value, _| value * 2).first()?, Some(2));
    Ok(())
}
```

用 `dumpTo(writer, context)` 指定輸出。建立快照時會讀完整個來源，重複鍵保留最後一個值。後續走訪會重新開啟原始來源。來源或輸出錯誤會回傳給呼叫方。

#### 輸出並停止執行 {#lazy-dump-and-stop}

用 `dd()` 或 `ddWith(&[...])` 搭配 `?`，將除錯快照交給目前執行邊界。Artisan 命令回傳 `ConsoleError` 時，快照寫到標準輸出，命令以狀態碼 1 結束。

```rust
use openrust::{console::ConsoleError, support::LazyCollection};

fn inspect_import() -> Result<i32, ConsoleError> {
    let rows = LazyCollection::from(["pending", "imported"]);
    match rows.ddWith(&[&"import stopped"])? {}
}
```

HTTP 處理器回傳 `HttpError` 時，快照成為純文字 HTTP 500 回應。服務程序繼續處理其他請求。Rust 透過常規的結果傳遞結束目前命令或處理器。

#### 高階訊息 {#lazy-higher-order-proxy}

用 `proxy` 綁定集合操作，再用 `call` 傳入具型別的回呼，讀取各項的欄位或呼叫其方法。`proxy(LazyCollection::map).call(...)` 對應 `$tasks->map->name`。回呼可以擷取傳給方法的參數。

```rust
use openrust::support::LazyCollection;

struct Task {
    name: &'static str,
    attempts: u32,
}

impl Task {
    fn exhausted(&self, limit: u32) -> bool {
        self.attempts >= limit
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tasks = LazyCollection::make(|| {
        [
            Task {
                name: "ingest",
                attempts: 1,
            },
            Task {
                name: "notify",
                attempts: 3,
            },
        ]
    });
    let names = tasks
        .clone()
        .proxy(LazyCollection::map)
        .call(|task, _| task.name);
    let retry_limit = 3;
    let exhausted = tasks
        .proxy(LazyCollection::filter)
        .call(move |task, _| task.exhausted(retry_limit));

    assert_eq!(
        names.all()?,
        vec![(0.into(), "ingest"), (1.into(), "notify")]
    );
    assert_eq!(
        exhausted.map(|task, _| task.name).all()?,
        vec![(1.into(), "notify")]
    );
    Ok(())
}
```

Rust 在編譯期解析集合操作和資料項存取。自訂操作可將函式或閉包傳給 `proxy`。所選操作決定何時走訪來源、如何保留鍵及如何傳遞錯誤。

#### `remember()`

`remember()` 建立一個共享迭代器，按需保留已讀項目。每次迭代按位置重播，超過快取前綴時繼續從原來源讀取。來源錯誤也會重播。值須實作 `Clone`，讓每次迭代取得擁有所有權的值。快取屬於集合本身，會隨讀取量增長。
