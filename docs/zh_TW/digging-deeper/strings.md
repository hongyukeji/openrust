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

## 常值與 Unicode 編輯 {#literal-unicode-editing}

`chopStart` 和 `chopEnd` 移除候選中第一個相符的前綴或後綴。`replaceStart` 和 `replaceEnd` 只取代相應邊界。`position` 傳回 Unicode 純量索引或 `None`。`mask` 和 `take` 支援負位移或負長度限制。用 `deduplicate` 合併指定字元的連續重複，用 `trim`、`ltrim` 或 `rtrim` 裁剪邊界，用 `squish` 合併空白。各編輯操作也有鏈式入口。

```rust
use openrust::support::Str;

assert_eq!(Str::position("你🙂好🙂", "🙂", -2), Some(3));
assert_eq!(Str::of("你好世界🙂").mask("*#", -4, Some(-1)).value(), "你*#*🙂");
assert_eq!(Str::of("  你好   🙂 ").squish().value(), "你好 🙂");
```

## 隨機字串與識別碼 {#random-and-identifiers}

`Str::random` 使用系統熵種子的隨機產生器建立英數字串。`Str::password` 接收明確的字元池，長度足夠時包含各啟用字元池的字元。`uuid`、`uuid7` 和 `ulid` 傳回型別化識別碼。用 `isUuid` 搭配選用的數字版本，或用 `isUlid` 驗證文字。UUID 和 ULID 使用 Rust 隨機來源及目前時鐘。

```rust
use openrust::support::{PasswordOptions, Str};

let password = Str::password(24, PasswordOptions::default())?;
assert_eq!(password.len(), 24);
assert!(Str::isUuid(&Str::uuid7().to_string(), Some(7)));
assert!(Str::isUlid(&Str::ulid().to_string()));
```

## 正規表示式與文字排版 {#regex-and-text-layout}

`match` 優先傳回有比對結果的第一個擷取群組，否則傳回完整比對。`matchAll` 在宣告擷取群組時傳回每筆的第一組，未比對的群組使用空字串。正規表示式採用 Rust 語法，例如 `(?i)` 表示忽略大小寫，替換支援 `$1` 或具名擷取。`replaceMatchesWith` 接收 Rust 回呼，無效模式傳回 `regex::Error`。

`width` 和 `limit` 使用 PHP 8.5 的東亞字元寬度表。`wordWrap` 依 Laravel 的 Unicode 純量規則在空格處換行，也可強制拆分長字。`limit` 的保留整字模式會處理基本 HTML 標籤後截斷。

```rust
use openrust::support::Str;

assert_eq!(Str::matchAll(r"(\d+)", "甲12 乙34")?, ["12", "34"]);
assert_eq!(Str::replaceMatches(r"(\d+)", "[$1]", "甲12 乙34", Some(1))?, "甲[12] 乙34");
assert_eq!(Str::limit("中文🙂", 4, "…", false), "中文…");
assert_eq!(Str::wordWrap("中a 🙂b 中c", 3, "|", false)?, "中a|🙂b|中c");
```
