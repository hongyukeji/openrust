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
