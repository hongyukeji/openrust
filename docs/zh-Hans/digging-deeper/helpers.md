# 辅助函数

## 数据路径 {#data-paths}

从 `openrust::support` 导入 `Arr` 和数据辅助函数访问 JSON 数组与对象。`Arr::get` 优先查找完整键，再遍历点路径。`data_get` 直接遍历路径，支持 `*`、`{first}` 和 `{last}`。路径缺失返回 `None`，显式空值返回 `Some(Value::Null)`。

```rust
use openrust::support::{Arr, data_fill, data_get, data_has};
use serde_json::json;

let mut users = json!([{"active": null}, {}]);
data_fill(&mut users, "*.active", json!(true))?;
assert!(data_has(&users, "0.active"));
assert_eq!(data_get(&users, Some("*.active")), Some(json!([null, true])));
assert_eq!(Arr::get(&users, "1.active"), Some(&json!(true)));
```

`data_has` 检查字面路径，`data_set` 覆盖值，`data_fill` 填补缺失值，`data_forget` 移除值。修改会校验嵌套深度，失败返回 `DataPathError`。用 `Arr::dot` 和 `Arr::undot` 展开和还原路径。类型化数组使用 Rust 切片和迭代器，集合流水线使用 `Collection` 或 `LazyCollection`。字符串操作见[字符串](./strings.md)。
