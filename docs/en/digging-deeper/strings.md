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

`plural`, `singular`, `pluralStudly` and `pluralPascal` use English inflection. `counted` adds an integer count with English digit grouping. The Rust renderer uses pulldown-cmark and the inflector uses pluralizer. Laravel uses PHP CommonMark extensions and supports locale selection and Doctrine inflection.

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

`width` and `limit` use the PHP 8.5 East Asian width table. `wordWrap` follows Laravel’s scalar-counting wrap and breaks at spaces or cuts long words on request. `limit` can preserve words, trimming basic HTML markup before truncating.

```rust
use openrust::support::Str;

assert_eq!(Str::matchAll(r"(\d+)", "甲12 乙34")?, ["12", "34"]);
assert_eq!(Str::replaceMatches(r"(\d+)", "[$1]", "甲12 乙34", Some(1))?, "甲[12] 乙34");
assert_eq!(Str::limit("中文🙂", 4, "…", false), "中文…");
assert_eq!(Str::wordWrap("中a 🙂b 中c", 3, "|", false)?, "中a|🙂b|中c");
```
