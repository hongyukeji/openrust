# Helpers

## Data paths {#data-paths}

Import `Arr` and the data helpers from `openrust::support` to access JSON arrays and objects. `Arr::get` checks the exact key before traversing a dotted path. `data_get` traverses the path and supports `*`, `{first}`, and `{last}`. Missing paths return `None`. A stored null returns `Some(Value::Null)`.

```rust
use openrust::support::{Arr, data_fill, data_get, data_has};
use serde_json::json;

let mut users = json!([{"active": null}, {}]);
data_fill(&mut users, "*.active", json!(true))?;
assert!(data_has(&users, "0.active"));
assert_eq!(data_get(&users, Some("*.active")), Some(json!([null, true])));
assert_eq!(Arr::get(&users, "1.active"), Some(&json!(true)));
```

Use `data_has` to check a literal path, `data_set` to overwrite, `data_fill` to fill missing entries, and `data_forget` to remove values. Mutations validate nesting and return `DataPathError` on failure. Use `Arr::dot` and `Arr::undot` to flatten and expand paths. For typed arrays use Rust slices and iterators. For collection pipelines use `Collection` or `LazyCollection`. See [Strings](./strings.md).
