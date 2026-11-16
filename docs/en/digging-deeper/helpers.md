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

## Fluent URIs {#fluent-uris}

Use `Uri::of` to parse an absolute or relative reference. Component setters return a new, validated URI. `query()` reads decoded parameters while preserving literal plus signs and dotted keys. Use `withQuery`, `withQueryIfMissing`, `replaceQuery`, `pushOntoQuery`, and `withoutQuery` to edit the query. Pass a JSON map for named parameters. Application code can generate an address with `Uri::to`, `route`, `action`, `signedRoute`, or `temporarySignedRoute`, then call `redirect` to build a validated redirect response.

```rust
use openrust::support::Uri;
use serde_json::json;

let uri = Uri::of("/items?label=a+b")?
    .withQuery(&json!({"page": 2}))?
    .withFragment("summary")?;
assert_eq!(uri.value(), "/items?label=a%2Bb&page=2#summary");
```

## Typed array reads and query encoding {#typed-arrays}

`Arr::array`, `boolean`, `float`, `integer`, and `string` check the stored JSON type. Missing paths and wrong types return distinct errors. `Arr::add` fills a missing or null path. `pull` returns the value and removes it. `wrap` keeps arrays and objects, turns null into an empty list, and wraps a scalar in a one-element list. `Arr::query` builds an RFC 3986 query string in insertion order.

```rust
use openrust::support::Arr;
use serde_json::json;

let mut data = json!({"filter": {"label": "a+b", "enabled": true}});
assert!(Arr::boolean(&data, "filter.enabled")?);
assert_eq!(Arr::pull(&mut data, "filter.enabled")?, Some(json!(true)));
assert_eq!(Arr::query(&data)?, "filter%5Blabel%5D=a%2Bb");
```
