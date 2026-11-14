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

## JSON 序列化

當鍵依迭代順序是從零開始的連續整數時，集合序列化為 JSON 陣列。其他鍵產生 JSON 物件。`toJson()` 傳回 JSON 字串或序列化錯誤，非有限浮點數也會回報錯誤。集合可直接傳給 `ApiResponse::success`。直接反序列化為 `Collection<T>` 可保留 JSON 物件成員順序。`fromValue` 使用傳入的 `serde_json::Value` 的順序。

## 惰性集合

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

#### `remember()`

`remember()` 建立一個共享迭代器，按需保留已讀項目。每次迭代按位置重播，超過快取前綴時繼續從原來源讀取。來源錯誤也會重播。值須實作 `Clone`，讓每次迭代取得擁有所有權的值。快取屬於集合本身，會隨讀取量增長。
