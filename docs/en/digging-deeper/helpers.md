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

### Date facade and test clock {#date-facade}

`support::facades::Date` delegates to the application’s `support::DateFactory`; `support::Date` remains the date value type. `now`, `today`, `tomorrow` and `yesterday` take `Option<DateTimezone>`; `None` reads the owning application's current timezone. Civil midnights account for daylight-saving transitions. A default UTC factory is available without an application.

`set_test_now(Some(value))`, `get_test_now` and `has_test_now` use the same clock as test travel, HTTP, Artisan and model timestamps. `set_test_now(None)` clears it. `with_test_now` accepts a native future and clears the test time on exit, including errors, panic and cancellation; it does not restore a previous freeze. Concurrent executions keep scoped values separate and application clocks are isolated. Use the test application's travel helpers when nested restoration is needed.

```rust
use openrust::support::{Date as DateValue, DateTimezone, facades::Date};

let frozen = DateValue::from_utc("2024-03-10T12:00:00Z".parse()?, DateTimezone::UTC);
Date::with_test_now(Some(frozen), async {
    let today = Date::today(None)?;
    let tomorrow = Date::tomorrow(None)?;
    Ok::<_, openrust::support::DateError>((today, tomorrow))
}).await??;
```

`parse` and `raw_parse` accept text, integer or floating-point timestamps, date values and `DateInput::Now`. Relative expressions use the same application test clock. `DateInput` also accepts JSON strings, numbers and null; booleans, arrays and objects return `DateError::InvalidExpression`. Non-finite floats and values outside the native date range fail; inputs over 20 MiB are rejected.

```rust
use openrust::support::facades::Date;

let next = Date::parse("tomorrow", None)?;
let text = next.format("Y-m-d H:i:sP")?;
let epoch = Date::parse(0, None)?;
assert_eq!(epoch.format("U.u")?, "0.000000");
```

`format` uses PHP date-format tokens. `timezone_name` and `format("e T")` preserve parsed names, offsets and abbreviations through civil arithmetic. `as_date_time` remains the Jiff view, where a parsed abbreviation uses a fixed offset. Explicit native timezones keep their own transition rules for parsing and formatting. The returned date value retains Jiff's −9999…9999 year range; request `InputDate` keeps the existing wider parser range.

`create_from_timestamp` and `create_from_timestamp_ms` create dates from seconds or milliseconds. Pass `None` for UTC, independently of the application timezone; pass `Some(zone)` to convert the resulting instant. `create_from_timestamp_utc` and `create_from_timestamp_ms_utc` select UTC explicitly. All four accept the numeric and text forms of `DateInput`.

```rust
let seconds = Date::create_from_timestamp_utc("1601735792.198956")?;
let milliseconds = Date::create_from_timestamp_ms_utc("1601735792198.956")?;
assert_eq!(seconds.format("U.u")?, milliseconds.format("U.u")?);
```

Timestamp strings follow the fixed numeric-chunk compatibility rules, rather than validating date expressions. Negative milliseconds retain the comparison version's historical split behavior: `-1` becomes `1970-01-01 00:00:00.001000+00:00`. Non-finite numbers and dates outside the native range return errors.

`has_format` checks the fixed field patterns and widths. `has_format_with_modifiers` also interprets modifiers such as `#`, `!` and `|`. These are pattern checks, not calendar validity checks: `2024-02-30` matches `Y-m-d`. `has_relative_keywords` checks whether an expression depends on its reference date; null, empty and invalid input return false.

```rust
assert!(!Date::has_format("2024-2-9", "Y-m-d")?);
assert!(Date::has_format_with_modifiers("31/08/2015", "d#m#Y")?);
assert!(Date::has_relative_keywords("tomorrow")?);
assert!(!Date::has_relative_keywords(None)?);
```

`create_from_format` and `raw_create_from_format` return `Result<Option<Date>, DateError>`. Strict mode defaults to true: a format mismatch returns an error. With `use_strict_mode(false)`, a mismatch returns `None`. Timezone errors, NUL input, size limits and native range errors remain errors. `use_strict_mode(None)` restores the true default. This setting belongs to the application factory and is shared by its HTTP requests and Workers; separate applications and standalone factories keep separate settings.

```rust
Date::use_strict_mode(false)?;
assert!(Date::create_from_format("Y-m-d", "wrong", None)?.is_none());
Date::use_strict_mode(None)?;
let date = Date::raw_create_from_format("!Y-m-d", "2024-01-02", None)?.unwrap();
assert_eq!(date.format("H:i:s")?, "00:00:00");
```

With a frozen clock and no explicit timezone, format creation follows the mock's original timezone unless the format specifies one. Unformatted parsing still uses the application timezone. `!` and `|` reset unspecified fields. Formatted `Request::date` and its facade use the same rules, including lenient `None` results. Default input normalization runs first, so trailing NULs removed by trimming do not reach the date parser; direct format creation rejects NUL input.

`get_days()` returns independent English weekday names from Sunday through Saturday. `singular_unit` and `plural_unit` normalize date unit names; `is_modifiable_unit` checks the exact singular name and case. `get_time_format_by_precision` returns the time pattern for minutes, seconds, milliseconds or microseconds, or `DateError::InvalidPrecision`.

```rust
assert_eq!(Date::singular_unit("CENTURIES")?, "century");
assert_eq!(Date::get_time_format_by_precision("milliseconds")?, "H:i:s.v");
assert!(Date::can_be_created_from_format("2024-02-29", "Y-m-d")?);
assert!(!Date::can_be_created_from_format(None, "Y-m-d")?);
```

`can_be_created_from_format` combines actual format creation with the field-pattern check. NUL input remains an error. Dates outside the native year range return false even when a wider calendar can represent them.

`Date::sleep(seconds).await` advances the shared test clock when time is frozen, preserving the mock timezone and the original date value. Otherwise it waits asynchronously with microsecond precision; zero and negative durations do not wait. Non-finite or unrepresentable durations return an error.

```rust
Date::sleep(0.25).await?;
```

`create` and `create_safe` accept `DateComponents`; `create_strict` accepts `StrictDateComponents` with integer seconds. `Default` uses year 0, January 1 at midnight; `current()` uses `None` for every component to read the current clock. Explicit `None` components inherit the mock's original timezone when no timezone is supplied. A supplied timezone converts the mock first.

```rust
use openrust::support::{DateComponents, StrictDateComponents};
let value = Date::create(DateComponents {
    year: Some(2024), month: Some(2), day: Some(30),
    ..Default::default()
}, None)?.unwrap(); // 2024-03-01
let safe = Date::create_safe(DateComponents {
    year: Some(2024), month: Some(2), day: Some(29),
    ..DateComponents::current()
}, None)?;
let strict = Date::create_strict(StrictDateComponents::default(), None)?;
```

`create_safe` rejects invalid calendar components and DST gaps instead of normalizing them; invalid values return an error in strict mode or `None` in lenient mode. `create_strict` always validates, leaving the application's strict-mode setting unchanged. Its integer components prevent PHP's implicit fractional-to-integer coercion. Years outside -9999 through 9999 return `DateError::OutOfRange`. `create_from_date(year, month, day, timezone)` keeps the clock time; `create_midnight_date` sets midnight. `create_from_time(hour, minute, second, timezone)` keeps the clock date; use `Some(0)` for midnight, and `None` to inherit a component.

`make(input, timezone)` returns a cloned date object or parses a date-like string. Null, numbers, empty text, intervals starting with `P` and recurrences starting with `R` return `None`; invalid date-like text returns an error. Passing an existing date preserves its original timezone, even when a timezone argument is supplied.

`create_from_interface` and `create_from_immutable` accept an owned or borrowed date value or `jiff::Zoned`, preserving the represented instant and timezone. Other input shapes are rejected.

```rust
let copied = Date::create_from_interface(&value)?;
let maybe_date = Date::make("2024-01-02", None)?;
assert!(Date::make("P1D", None)?.is_none());
let from_id = Date::create_from_id("01ARZ3NDEKTSV4RRFFQ69G5FAV")?;
```

`create_from_id` accepts ULID, time-based UUID v1/v2/v6/v7 text, or the framework's native UUID/ULID values. Non-time UUIDs and malformed or overflowing identifiers return an error. UUID v2 loses the low 32 timestamp bits by design. Dates use UTC offsets and keep the source converter's microsecond rounding and error behavior; years outside the native date range return `DateError::OutOfRange`.

`create_from_time_string(time, timezone)` modifies today at midnight using the existing date-expression parser. A time without a colon receives `:0`; `"12"` therefore means noon. The resulting date keeps the supplied or application timezone, even if the text mentions a different timezone. Normalization and relative/full-date expressions follow the date modifier rules; invalid expressions return an error in both strict and lenient modes.

```rust
let noon = Date::create_from_time_string("12", None)?;
let precise = Date::create_from_time_string("12:34:56.123456", None)?;
```

Date macros use the application's factory and clock. Register a typed callback, invoke it through `macro_call`, and remove it with `None`. Arguments and results may be owned Rust values, including values that are not `Send`; the stored callback itself must be `Send + Sync`.

```rust
use openrust::support::DateMacro;
Date::r#macro("stamp", DateMacro::new(|factory, prefix: String| {
    Ok(format!("{prefix}{}", factory.now(None)?.format("Y-m-d")?))
}))?;
let text: String = Date::macro_call("stamp", "date:".to_owned())?;
assert!(Date::has_macro("stamp")?);
Date::r#macro("stamp", None)?;
Date::reset_macros()?;
```

`get_macro` returns a retained callback handle. `mixin` registers an iterator of `(name, DateMacro)` entries, replacing existing names; `mixin_using` accepts a fallible registration provider. Rust mixins explicitly register compiled callbacks rather than reflecting over methods or evaluating trait names.

`generic_macro(DateGenericMacro::new(callback), priority)` handles unknown dynamic names. Higher priorities run first; equal priorities keep registration order. Return `DateMacroError::UnknownMethod(name)` to try the next callback; other errors stop dispatch. Named macros take precedence. `reset_macros` clears both registries. The default date factory converts a generic macro's result into a date, so generic callbacks must produce a date or `jiff::Zoned`; scalar/empty results return `InvalidGeneratedDate`. An unknown name in lenient mode also fails that conversion. Named macros can return arbitrary types. Built-in methods keep their explicit public entry points.

`instance(&value)` clones a date value. Localized creation, custom factories and error diagnostics are not currently exposed.


## Pipeline {#pipeline}

`Pipeline::send` starts a typed async pipeline. `through` replaces the stages and `pipe` appends a stage. Call `next.run` to continue or return a result to short-circuit. Use `then` for a final projection, `then_return` to return the carried value, and `finally` for a callback that sees the original input.

For database work, use `Pipeline<T, DatabaseError, R, Transaction>` and `within_transaction`. Stages use `next.context()` to query the same transaction. Return an error to roll it back. Pass typed callables or resolve services explicitly.

Use `.within_transaction(None)` for the default connection or `.within_transaction(Some("audit"))` for a named connection. This only configures the pipeline; `.then(...)` or `.then_return()` opens the transaction and executes it. The destination receives `(value, &mut Transaction)`. `finally` runs once after commit or rollback, and also when execution panics or is cancelled.

```rust
use openrust::support::facades::Pipeline;
use std::convert::Infallible;

let result = Pipeline::<_, Infallible>::send(20)
    .pipe(|value, next| Box::pin(async move { next.run(value + 1).await }))
    .pipe(|value, next| Box::pin(async move { next.run(value * 2).await }))
    .then_return()
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
]).without_additional_properties();
let encoded = schema.to_array()?;
let restored = JsonSchema::from_array(encoded.clone())?;
assert_eq!(restored.to_array()?, encoded);
assert!(JsonSchema::from_array(json!({"type": "unknown"})).is_err());
```

Serialization returns a `Result`: non-finite numbers fail. `from_array` supports local `$ref` pointers, required properties, nullable unions and anyOf compositions. Remote and circular references, boolean schemas, tuple items, ambiguous types, type-specific constraints on multi-type unions and null defaults fail. Unknown metadata outside the supported subset is omitted; this API constructs schemas and does not validate application data. Expansion is limited to 20,000 fragments and 128 nesting or reference levels.


## Probability callbacks

`Lottery::odds(1, 100)?` selects one winning integer opportunity out of 100. For a fractional probability use `Lottery::odds(0.02, None::<u64>)?`. Chain `winner(|| ...)` and `loser(|| ...)` to return a common Rust type from the selected callback; both default to booleans. `choose()` returns one result and `choose_times(n)` returns a vector. Values above the denominator always win; negative values always lose. Nonfinite probabilities are rejected.

For deterministic tests use `always_win`, `always_lose`, `fix(sequence)`, or `force_result_with_sequence(sequence, missing_callback)`. Exhausted sequences sample normally unless a fallback is supplied. `set_result_factory` accepts a custom sampler. Reset with `determine_result_normally()` or its plural alias. Run asynchronous controls inside `Lottery::scope(async { ... }).await`; controls outside a scope in an async runtime panic to prevent thread state leaking into another request. `scope_sync(|| ...)` provides a synchronous isolated scope. Scopes restore parent controls even after panic or cancellation. Callbacks capture inputs through closures; dynamic variadic invocation is not provided.
