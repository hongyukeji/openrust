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

## Numbers {#numbers}

Use `Number::default()` for English and USD defaults. Set locale and currency on an instance with `useLocale` and `useCurrency`. Use `withLocale` and `withCurrency` to pass scoped settings to a callback. These settings belong to the instance and can be used concurrently across requests.

`format`, `percentage`, and `currency` use CLDR data. `parseInt` preserves 64-bit integers and `parseFloat` reads a localized numeric prefix. `fileSize`, `abbreviate`, `forHumans`, `clamp`, `pairs`, and `trim` follow Laravel’s numeric conventions. Spellout uses the vendored CLDR rules for its supported languages. Fractional spellout is available in English.

```rust
use openrust::support::Number;

let number = Number::default();
let localized = number.withLocale("de-DE", |de| de.format(1234.5, Some(2), None))??;
assert_eq!(localized, "1.234,50");
assert_eq!(number.currency(1234.5, None, None)?, "$1,234.50");
assert_eq!(number.spellOrdinal(21)?, "twenty-first");
assert_eq!(number.parseInt("9,223,372,036,854,775,807")?, i64::MAX);
```

## Timing tools {#timing-tools}

`Benchmark::value` returns the result and elapsed milliseconds. `measure` averages a nonzero number of runs, and `measureMany` reports named callbacks in order. Use `valueAsync` to measure a future.

`Sleep` uses `Duration` and waits when awaited. Use `and` to add time, `when` or `unless` to control eligibility, and `while` to repeat the interval. `Sleep::fake()` creates an instance-local log. Pass it to a delay with `using` and check the recorded durations.

`Timebox::call` runs an async callback and pads its result to a minimum duration, including a returned error. `returnEarly` lets the same instance return without padding until `dontReturnEarly` resets it.

```rust
use openrust::support::{Benchmark, Sleep, Timebox};
use std::time::Duration;

let (value, milliseconds) = Benchmark::value(|| 6 * 7);
assert_eq!(value, 42);
assert!(milliseconds >= 0.0);

let duration = Duration::from_millis(5);
let fake = Sleep::fake();
Sleep::r#for(duration).using(&fake).wait().await;
fake.assertSequence(&[Some(duration)]);

let mut timebox = Timebox::default();
assert_eq!(timebox.call(async |_| Ok::<_, ()>(42), duration).await, Ok(42));
```
