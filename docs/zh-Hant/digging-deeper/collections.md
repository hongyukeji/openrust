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
