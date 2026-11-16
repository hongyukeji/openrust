# 字符串

## Unicode 字符串 {#unicode-strings}

从 `openrust::support` 导入 `Str`。`Str::of` 返回可链式操作的 `Stringable`。长度、索引、截取和反转以 Unicode 标量值为单位。一个组合字素可能占多个位置。负偏移从末尾计算。

```rust
use openrust::support::Str;

assert_eq!(Str::substr("你好🙂", -2, None), "好🙂");
assert_eq!(Str::of("Open Rust").snake("_").to_string(), "open_rust");
assert!(Str::is(&["api/*"], "api/users", false));
assert_eq!(Str::transliterate("München"), "Munchen");
```

`Str::is` 匹配完整输入，`*` 匹配任意序列。`before`、`after`、`between` 及其变体借用原字符串的片段。使用 `studly`、`camel`、`snake` 和 `kebab` 转换名称。转写使用 portable-ascii 字符表。Base64 解码采用严格校验，返回字节或解码错误。

## Markdown 与英语词形变化 {#markdown-and-inflection}

`Str::markdown` 渲染 GitHub 风格 Markdown，支持表格、删除线和任务列表。用 `inlineMarkdown` 渲染行内文本并省去段落标签。默认转义输入 HTML，并清除不安全的链接和图片 URL。`markdownWith` 和 `inlineMarkdownWith` 接收 `MarkdownOptions`，可移除 HTML 或允许可信来源的 HTML。

`plural`、`singular`、`pluralStudly` 和 `pluralPascal` 使用英语词形。`counted` 加上按英语分组的整数数量。Rust 使用 pulldown-cmark 与 pluralizer，Laravel 使用 PHP CommonMark 扩展，并支持按语言环境格式化和 Doctrine 词形变化。

```rust
use openrust::support::Str;

assert_eq!(Str::of("**你好**").inlineMarkdown().value(), "<strong>你好</strong>");
assert_eq!(Str::of("UserChild").pluralStudly(2).value(), "UserChildren");
assert_eq!(Str::counted("person", 1_234), "1,234 people");
```
