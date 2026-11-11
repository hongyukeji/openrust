# 字串

## Unicode 字串 {#unicode-strings}

從 `openrust::support` 匯入 `Str`。`Str::of` 回傳可鏈式操作的 `Stringable`。長度、索引、截取和反轉以 Unicode 純量值為單位。一個組合字素可能占多個位置。負位移從末尾計算。

```rust
use openrust::support::Str;

assert_eq!(Str::substr("你好🙂", -2, None), "好🙂");
assert_eq!(Str::of("Open Rust").snake("_").to_string(), "open_rust");
assert!(Str::is(&["api/*"], "api/users", false));
assert_eq!(Str::transliterate("München"), "Munchen");
```

`Str::is` 比對完整輸入，`*` 比對任意序列。`before`、`after`、`between` 及其變體借用原字串的片段。使用 `studly`、`camel`、`snake` 和 `kebab` 轉換名稱。轉寫使用 portable-ascii 字元表。Base64 解碼採嚴格驗證，回傳位元組或解碼錯誤。
