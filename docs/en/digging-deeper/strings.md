# Strings

## Unicode strings {#unicode-strings}

Import `Str` from `openrust::support`. `Str::of` creates a chainable `Stringable`. Length, indexing, slicing, and reversal operate on Unicode scalar values. A combined grapheme can span several positions. Negative offsets count from the end.

```rust
use openrust::support::Str;

assert_eq!(Str::substr("你好🙂", -2, None), "好🙂");
assert_eq!(Str::of("Open Rust").snake("_").to_string(), "open_rust");
assert!(Str::is(&["api/*"], "api/users", false));
assert_eq!(Str::transliterate("München"), "Munchen");
```

`Str::is` matches the whole string, with `*` matching any sequence. `before`, `after`, `between`, and their variants borrow a section of the original string. Use `studly`, `camel`, `snake`, and `kebab` to convert names. Transliteration uses the portable-ascii table. Base64 decoding validates strictly and returns bytes or a decoding error.

## Markdown and English inflection {#markdown-and-inflection}

`Str::markdown` renders GitHub-flavored Markdown, including tables, strikethrough and task lists. For inline text, use `inlineMarkdown` to omit paragraph wrappers. The default escapes input HTML and clears unsafe link and image URLs. `markdownWith` and `inlineMarkdownWith` take `MarkdownOptions` to strip HTML or allow HTML from a trusted source.

`plural`, `singular`, `pluralStudly` and `pluralPascal` use English inflection. `counted` adds an integer count with English digit grouping. The Rust renderer uses pulldown-cmark. Word inflection uses Doctrine rules and defaults to English.

```rust
use openrust::support::Str;

assert_eq!(Str::of("**你好**").inlineMarkdown().value(), "<strong>你好</strong>");
assert_eq!(Str::of("UserChild").pluralStudly(2).value(), "UserChildren");
assert_eq!(Str::counted("person", 1_234), "1,234 people");
```

## Literal and Unicode editing {#literal-unicode-editing}

`chopStart` and `chopEnd` remove the first matching prefix or suffix from the supplied candidates. `replaceStart` and `replaceEnd` edit only the selected edge. `position` returns a Unicode scalar index or `None`. Both `mask` and `take` support negative offsets or limits. Use `deduplicate` with literal characters, `trim`, `ltrim`, or `rtrim` for edges, and `squish` to collapse whitespace. Each editing operation also has a fluent counterpart.

```rust
use openrust::support::Str;

assert_eq!(Str::position("你🙂好🙂", "🙂", -2), Some(3));
assert_eq!(Str::of("你好世界🙂").mask("*#", -4, Some(-1)).value(), "你*#*🙂");
assert_eq!(Str::of("  你好   🙂 ").squish().value(), "你好 🙂");
```

## Random strings and identifiers {#random-and-identifiers}

`Str::random` generates an alphanumeric string using the system-seeded random generator. `Str::password` accepts explicit character pools and includes one character from each enabled pool when the requested length permits. `uuid`, `uuid7`, and `ulid` return typed identifiers. Use `isUuid` with an optional numeric version, or `isUlid`, to validate text. UUID and ULID generation uses Rust randomness and the current clock.

```rust
use openrust::support::{PasswordOptions, Str};

let password = Str::password(24, PasswordOptions::default())?;
assert_eq!(password.len(), 24);
assert!(Str::isUuid(&Str::uuid7().to_string(), Some(7)));
assert!(Str::isUlid(&Str::ulid().to_string()));
```

## Regular expressions and text layout {#regex-and-text-layout}

`match` returns the first capture when it participates, then falls back to the whole match. `matchAll` returns the first capture for each match when the pattern declares one, using an empty string for an unmatched group. Supply Rust regex syntax, such as `(?i)` for case insensitivity. Replacements use `$1` or named captures. Use `replaceMatchesWith` for a Rust callback. Invalid patterns return `regex::Error`.

`width` and `limit` use the PHP 8.5 East Asian width table. `wordWrap` follows scalar-counting wrap and breaks at spaces or cuts long words on request. `limit` can preserve words, trimming basic HTML markup before truncating.

```rust
use openrust::support::Str;

assert_eq!(Str::matchAll(r"(\d+)", "甲12 乙34")?, ["12", "34"]);
assert_eq!(Str::replaceMatches(r"(\d+)", "[$1]", "甲12 乙34", Some(1))?, "甲[12] 乙34");
assert_eq!(Str::limit("中文🙂", 4, "…", false), "中文…");
assert_eq!(Str::wordWrap("中a 🙂b 中c", 3, "|", false)?, "中a|🙂b|中c");
```

## Slug and strict transliteration {#slug-and-transliteration}

`Str::slug` uses the English portable-ascii map, a hyphen separator and the default `@` → `at` dictionary. Use `slugWith` to select a separator, pass `None` to retain Unicode letters and numbers, or supply dictionary entries in replacement order. `ascii` exposes the same locale-specific mapping directly.

`transliterateWithOptions` accepts the unknown-character replacement and a strict flag. Strict mode normalizes the text, removes nonspacing marks, then applies ICU4X script-to-Latin and Latin-to-ASCII rules before portable-ascii fallback. Script detection and the version of the CLDR data can affect results relative to PHP Intl.

```rust
use openrust::support::Str;

assert_eq!(Str::slug("Über Café @ Home"), "uber-cafe-at-home");
assert_eq!(Str::slugWith("Köln", "-", Some("de"), &[]), "koeln");
assert_eq!(
    Str::of("Привет 你好")
        .transliterateWithOptions(Some("?"), true)
        .to_string(),
    "Privet ni hao"
);
```

## Word inflection by language {#word-inflection}

Pass an `InflectionLanguage` to `pluralIn`, `singularIn`, `pluralStudlyIn`, `pluralPascalIn`, or `countedIn`. Rules cover English, Esperanto, French, Italian, Norwegian Bokmål, Portuguese, Spanish, and Turkish. Both `Str` and `Stringable` accept the language per call.

```rust
use openrust::support::{InflectionLanguage, Str};

let french = Str::of("cheval").pluralIn(2, InflectionLanguage::French).to_string();
let turkish = Str::pluralIn("araba", 2, InflectionLanguage::Turkish);
assert_eq!(french, "chevaux");
assert_eq!(turkish, "arabalar");
assert_eq!(Str::plural("child", 2), "children");
```

## Timestamp-first COMB UUIDs {#ordered-comb-uuid}

`Str::orderedUuid` creates a UUID v4 with the first six bytes set to Unix time in 10-microsecond ticks. `orderedUuidAt` accepts an explicit `SystemTime`. Identifiers from different ticks sort by timestamp. Identifiers in the same tick have random ordering.

```rust
use openrust::support::Str;
use std::time::{Duration, UNIX_EPOCH};

let first = Str::orderedUuidAt(UNIX_EPOCH + Duration::from_micros(10))?;
let second = Str::orderedUuidAt(UNIX_EPOCH + Duration::from_micros(20))?;
assert_eq!(first.get_version_num(), 4);
assert!(first < second);
let prefix = first.to_string()[..13].to_owned();
assert_eq!(prefix, "00000000-0001");
```

## Title case and literal editing {#title-and-literal-editing}

Use `title`, `headline`, `apa` and `initials` for casing and labels. The padding and substring methods count Unicode scalar values. `replace` applies ordered literal pairs, while `swap` chooses the longest match without reprocessing replacements. `excerpt` takes a radius around the first match.

`isUrl` validates an absolute URL with a host and a permitted scheme using the Rust URL parser. Fluent conditions take typed closures, regex splitting uses Rust regex syntax, and Base64 decoding and decryption return bytes. Select a Rust digest type with `hash::<D>()` to get lowercase hexadecimal text.

```rust
use openrust::support::Str;

let title = Str::of("helloWorld__École")
    .whenContains(&["World"], false, |value| value.headline())
    .swap(&[("World", "世界")])
    .to_string();

assert_eq!(title, "Hello 世界 École");
assert_eq!(Str::padBoth("中", 6, "é🙂"), "é🙂中é🙂é");
assert_eq!(Str::excerpt("甲乙中心丙丁", "中心", 1, "…").as_deref(), Some("…乙中心丙…"));
```

## Format scanning {#format-scanning}

`Stringable::scan` reads fields from a format string. Use width, assignment suppression, numbered fields, and character sets. A mismatch leaves unmatched slots as `None`, while end of input before any conversion returns an empty collection. Invalid formats return `ScanError`. Field widths and `%n` count bytes. A field cut inside UTF-8 is retained as `ScanValue::Bytes`.

```rust
use openrust::support::Str;
use serde_json::json;

let fields = Str::of("42 甲").scan("%2$d %1$s%n");
assert!(fields.is_err()); // Positional and sequential output cannot be mixed.
let fields = Str::of("42 甲").scan("%2$d %1$s")?.toArray()?;
assert_eq!(fields, json!(["甲", 42]));
assert_eq!(Str::of("17").scan("%d %s")?.toArray()?, json!([17, null]));
```
