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

## Markdown 與英語詞形變化 {#markdown-and-inflection}

`Str::markdown` 呈現 GitHub 風格 Markdown，支援表格、刪除線和工作清單。用 `inlineMarkdown` 呈現行內文字並省略段落標籤。預設會跳脫輸入 HTML，並清除不安全的連結和圖片 URL。`markdownWith` 和 `inlineMarkdownWith` 接收 `MarkdownOptions`，可移除 HTML 或允許可信任來源的 HTML。

`plural`、`singular`、`pluralStudly` 和 `pluralPascal` 使用英語詞形。`counted` 加上依英語分組的整數數量。Rust 使用 pulldown-cmark 與 pluralizer，Laravel 使用 PHP CommonMark 擴充，並支援依語系格式化及 Doctrine 詞形變化。

```rust
use openrust::support::Str;

assert_eq!(Str::of("**你好**").inlineMarkdown().value(), "<strong>你好</strong>");
assert_eq!(Str::of("UserChild").pluralStudly(2).value(), "UserChildren");
assert_eq!(Str::counted("person", 1_234), "1,234 people");
```
