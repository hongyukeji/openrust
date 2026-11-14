# 輔助函式

## 資料路徑 {#data-paths}

從 `openrust::support` 匯入 `Arr` 和資料輔助函式以存取 JSON 陣列與物件。`Arr::get` 優先查找完整鍵，再走訪點路徑。`data_get` 直接走訪路徑，支援 `*`、`{first}` 和 `{last}`。路徑缺少時回傳 `None`，明確空值回傳 `Some(Value::Null)`。

```rust
use openrust::support::{Arr, data_fill, data_get, data_has};
use serde_json::json;

let mut users = json!([{"active": null}, {}]);
data_fill(&mut users, "*.active", json!(true))?;
assert!(data_has(&users, "0.active"));
assert_eq!(data_get(&users, Some("*.active")), Some(json!([null, true])));
assert_eq!(Arr::get(&users, "1.active"), Some(&json!(true)));
```

`data_has` 檢查字面路徑，`data_set` 覆寫值，`data_fill` 填補缺少的值，`data_forget` 移除值。修改會驗證巢狀深度，失敗回傳 `DataPathError`。用 `Arr::dot` 和 `Arr::undot` 展開及還原路徑。型別化陣列使用 Rust 切片和迭代器，集合管線使用 `Collection` 或 `LazyCollection`。字串操作見[字串](./strings.md)。
