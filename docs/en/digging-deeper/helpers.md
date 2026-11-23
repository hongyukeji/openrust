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

Use `Uri::of` to parse an absolute or relative reference. Component setters return a new, validated URI. `query()` reads decoded parameters while preserving literal plus signs and dotted keys. Use `with_query`, `with_query_if_missing`, `replace_query`, `push_onto_query`, and `without_query` to edit the query. Pass a JSON map for named parameters. Application code can generate an address with `Uri::to`, `route`, `action`, `signed_route`, or `temporary_signed_route`, then call `redirect` to build a validated redirect response.

```rust
use openrust::support::Uri;
use serde_json::json;

let uri = Uri::of("/items?label=a+b")?
    .with_query(&json!({"page": 2}))?
    .with_fragment("summary")?;
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

Use `Number::default()` for English and USD defaults. Set locale and currency on an instance with `use_locale` and `use_currency`. Use `with_locale` and `with_currency` to pass scoped settings to a callback. These settings belong to the instance and can be used concurrently across requests.

`format`, `percentage`, and `currency` use CLDR data. `parse_int` preserves 64-bit integers and `parse_float` reads a localized numeric prefix. `file_size`, `abbreviate`, `for_humans`, `clamp`, `pairs`, and `trim` follow numeric conventions.

```rust
use openrust::support::Number;

let number = Number::default();
let localized = number.with_locale("de-DE", |de| de.format(1234.5, Some(2), None))??;
assert_eq!(localized, "1.234,50");
assert_eq!(number.currency(1234.5, None, None)?, "$1,234.50");
assert_eq!(number.spell_ordinal(21)?, "twenty-first");
assert_eq!(number.parse_int("9,223,372,036,854,775,807")?, i64::MAX);
```

### Localized number spellout {#localized-number-spellout}

Use `spell` and `spell_ordinal` for localized integers and fractions. The Rust implementation uses the bundled ICU 76.1 / CLDR 46 rules. When a locale lacks a named spellout ordinal, `spell_ordinal` uses the locale’s default spellout rules. The optional `after` and `until` bounds are inclusive and return formatted digits. For finite floats outside the signed 64-bit range, decimal fallback uses the input’s round-trip decimal digits.

```rust
use openrust::support::Number;

let number = Number::default();
let german = number.with_locale("de-DE", |number| number.spell(1.25, None, None))??;
let russian = number.with_locale("ru", |number| number.spell(1.25, None, None))??;
let chinese = number.with_locale("zh-TW", |number| number.spell(1.25, None, None))??;
assert_eq!(german, "eins Komma zwei fünf");
assert_eq!(russian, "одна целый двадцать пять сотых");
assert_eq!(chinese, "一點二五");
```

## Timing tools {#timing-tools}

`Benchmark::value` returns the result and elapsed milliseconds. `measure` averages a nonzero number of runs, and `measure_many` reports named callbacks in order. Use `value_async` to measure a future.

`Sleep` uses `Duration` and waits when awaited. Use `and` to add time, `when` or `unless` to control eligibility, and `while` to repeat the interval. `Sleep::fake()` creates an instance-local log. Pass it to a delay with `using` and check the recorded durations.

`Timebox::call` runs an async callback and pads its result to a minimum duration, including a returned error. `return_early` lets the same instance return without padding until `dont_return_early` resets it.

```rust
use openrust::support::{Benchmark, Sleep, Timebox};
use std::time::Duration;

let (value, milliseconds) = Benchmark::value(|| 6 * 7);
assert_eq!(value, 42);
assert!(milliseconds >= 0.0);

let duration = Duration::from_millis(5);
let fake = Sleep::fake();
Sleep::r#for(duration).using(&fake).wait().await;
fake.assert_sequence(&[Some(duration)]);

let mut timebox = Timebox::default();
assert_eq!(timebox.call(async |_| Ok::<_, ()>(42), duration).await, Ok(42));
```

## Array transforms and selection {#array-transforms}

Use `select` for literal fields and `pluck` for dot paths. Filtering, mapping, partitioning, and sorting preserve keys. `take` reindexes numeric keys. `flatten` and `collapse` differ in depth and key handling.

```rust
use openrust::support::Arr;
use serde_json::json;

let items = json!([{"id":"a","meta":{"name":"甲"}}, {"id":"b","meta":{"name":"乙"}}]);
let names = Arr::pluck(&items, "meta.name", Some("id"))?;
assert_eq!(names, json!({"a":"甲","b":"乙"}));
assert_eq!(Arr::take(&items, -1), json!([items[1]]));
assert_eq!(Arr::flatten(&json!([1,[2,[3]]]), Some(1))?, [json!(1),json!(2),json!([3])]);
```

## Dates and intervals {#dates-and-intervals}

`now` and `today` read the application timezone when passed `None`. Pass a `DateTimezone` to choose an IANA timezone explicitly. `today` starts at local midnight.

Use `plus` or `minus` with a signed `DateInterval`. `MonthOverflow::Carry` carries extra days into the next month and `Clamp` stops at the last day. Helpers from `microseconds` to `years` construct intervals.

`Date::timezone(name)` resolves IANA identifiers from the framework’s bundled 2025b database, independent of the operating system’s timezone data. `DateTimezone` is Jiff’s native `TimeZone`; passing a timezone obtained elsewhere uses that source. `Date::from_utc` takes a `jiff::Timestamp`, `from_local` takes a `jiff::civil::DateTime`, and `as_date_time` borrows a `jiff::Zoned`. Dates and timezones can be cloned when reused. These native types support years −9999 through 9999; the PHP date-expression parser retains its wider supported year range.

```rust
use openrust::support::{Date, DateTimezone, MonthOverflow, months, now};

let zone = now(None)?.as_date_time().time_zone().clone();
assert_eq!(now(Some(zone.clone()))?.as_date_time().time_zone().clone(), zone);
let start = Date::from_local("2024-01-31T09:00:00".parse()?, DateTimezone::UTC)?;
let end = start.plus(months(1), MonthOverflow::Clamp)?;
let formatted = end.as_date_time().strftime("%Y-%m-%dT%H:%M:%S%:z").to_string();
assert_eq!(formatted, "2024-02-29T09:00:00+00:00");
```

## Pipeline {#pipeline}

`Pipeline::send` starts a typed async pipeline. `through` replaces the stages and `pipe` appends a stage. Call `next.run` to continue or return a result to short-circuit. Use `then` for a final projection, `thenReturn` to return the carried value, and `finally` for a callback that sees the original input.

For database work, use `Pipeline<T, DatabaseError, R, Transaction>` and `within_transaction`. Stages use `next.context()` to query the same transaction. Return an error to roll it back. Pass typed callables or resolve services explicitly.

Use `.within_transaction(None)` for the default connection or `.within_transaction(Some("audit"))` for a named connection. This only configures the pipeline; `.then(...)` or `.thenReturn()` opens the transaction and executes it. The destination receives `(value, &mut Transaction)`. `finally` runs once after commit or rollback, and also when execution panics or is cancelled.

```rust
use openrust::support::facades::Pipeline;
use std::convert::Infallible;

let result = Pipeline::<_, Infallible>::send(20)
    .pipe(|value, next| Box::pin(async move { next.run(value + 1).await }))
    .pipe(|value, next| Box::pin(async move { next.run(value * 2).await }))
    .thenReturn()
    .await?;
assert_eq!(result, 42);
```

## Blank values and retries {#blank-and-retry}

`blank` and `filled` classify JSON values. Zero and false count as filled. `Retry::backoff` supplies the delays between attempts and `Retry::times` sets a fixed delay and attempt limit. `run_when` retries only errors accepted by its predicate.

Use `Option` for optional values, `Result::unwrap_or_else` for a fallback, and a scoped `OnceLock` to memoize a value.

```rust
use openrust::support::{Retry, blank, filled};
use serde_json::json;
use std::time::Duration;

assert!(blank(&json!("  ")));
assert!(filled(&json!(false)));
let attempts = Retry::backoff([Duration::from_millis(2), Duration::from_millis(3)])
    .run(|attempt| async move {
        if attempt == 3 { Ok(attempt) } else { Err("transient") }
    })
    .await?;
assert_eq!(attempts, 3);
```

## Deferred callbacks {#deferred-callbacks}

Register an owned future with `defer(callback, name, always)` inside an HTTP handler, Artisan command or queue job. It runs in the current process at the end of that lifecycle. A named callback replaces the previous callback with that name in the same lifecycle; use `None` for independent callbacks.

```rust
use openrust::support::defer;

let _ = defer(async move {
    std::fs::write("/tmp/report-ready", b"ready")?;
    Ok::<_, std::io::Error>(())
}, Some("report.ready"), false)?;
```

By default, callbacks run for HTTP statuses below 400, commands exiting with zero and successful job attempts. Set `always` to `true`, or call `.always()` on the returned handle, to include failures. Errors are reported through the application exception handler and later callbacks continue. Callbacks registered while callbacks run are also drained.

Capture an `Arc` of a scoped service when registering; resolving it again after completion may produce a new instance. HTTP callbacks run when the transport releases the response body, including a disconnected stream. Nested Artisan calls and synchronous jobs share the enclosing lifecycle. Local callbacks in a hard-timeout job run in that job process before its outcome returns, within its timeout budget. Cancellation before completion, process termination or a hard timeout cannot guarantee execution. Use a durable queue for work that must survive these failures. Registering outside an active lifecycle returns `concurrency::Error::OutsideRequest`.

## JSON Schema {#json-schema}

OpenRust provides `JsonSchema` static constructors and the instance-based `JsonSchemaTypeFactory`. String, integer, number, boolean, object, array, union and anyOf types compose through `Type`. Constraints use builder methods; explicitly pass booleans to `required`, `nullable` and `unique`. Convert heterogeneous properties with `.into()`.

```rust
use openrust::json_schema::{JsonSchema, JsonSchemaTypeFactory};
use serde_json::json;

let factory = JsonSchemaTypeFactory;
let schema = factory.object([
    ("name", factory.string().min(1).required(true).into()),
    ("active", factory.boolean().default(true).into()),
]).withoutAdditionalProperties();
let encoded = schema.to_array()?;
let restored = JsonSchema::fromArray(encoded.clone())?;
assert_eq!(restored.to_array()?, encoded);
assert!(JsonSchema::fromArray(json!({"type": "unknown"})).is_err());
```

Serialization returns a `Result`: non-finite numbers fail. `fromArray` supports local `$ref` pointers, required properties, nullable unions and anyOf compositions. Remote and circular references, boolean schemas, tuple items, ambiguous types, type-specific constraints on multi-type unions and null defaults fail. Unknown metadata outside the supported subset is omitted; this API constructs schemas and does not validate application data. Expansion is limited to 20,000 fragments and 128 nesting or reference levels.


## Probability callbacks

`Lottery::odds(1, 100)?` selects one winning integer opportunity out of 100. For a fractional probability use `Lottery::odds(0.02, None::<u64>)?`. Chain `winner(|| ...)` and `loser(|| ...)` to return a common Rust type from the selected callback; both default to booleans. `choose()` returns one result and `choose_times(n)` returns a vector. Values above the denominator always win; negative values always lose. Nonfinite probabilities are rejected.

For deterministic tests use `always_win`, `always_lose`, `fix(sequence)`, or `force_result_with_sequence(sequence, missing_callback)`. Exhausted sequences sample normally unless a fallback is supplied. `set_result_factory` accepts a custom sampler. Reset with `determine_result_normally()` or its plural alias. Run asynchronous controls inside `Lottery::scope(async { ... }).await`; controls outside a scope in an async runtime panic to prevent thread state leaking into another request. `scope_sync(|| ...)` provides a synchronous isolated scope. Scopes restore parent controls even after panic or cancellation. Callbacks capture inputs through closures; dynamic variadic invocation is not provided.
