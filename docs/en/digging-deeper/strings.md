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
