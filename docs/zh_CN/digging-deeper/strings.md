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

## 字面量与 Unicode 编辑 {#literal-unicode-editing}

`chopStart` 和 `chopEnd` 删除候选中第一个匹配的前缀或后缀。`replaceStart` 和 `replaceEnd` 只替换对应边界。`position` 返回 Unicode 标量索引或 `None`。`mask` 和 `take` 支持负偏移或负长度限制。用 `deduplicate` 折叠指定字符的连续重复，用 `trim`、`ltrim` 或 `rtrim` 裁剪边界，用 `squish` 合并空白。各编辑操作也有链式入口。

```rust
use openrust::support::Str;

assert_eq!(Str::position("你🙂好🙂", "🙂", -2), Some(3));
assert_eq!(Str::of("你好世界🙂").mask("*#", -4, Some(-1)).value(), "你*#*🙂");
assert_eq!(Str::of("  你好   🙂 ").squish().value(), "你好 🙂");
```

## 随机字符串与标识符 {#random-and-identifiers}

`Str::random` 使用系统熵种子的随机生成器创建字母数字串。`Str::password` 接收显式字符池，长度足够时包含各启用字符池中的字符。`uuid`、`uuid7` 和 `ulid` 返回类型化标识符。用 `isUuid` 配合可选的数字版本，或用 `isUlid` 验证文本。UUID 和 ULID 使用 Rust 随机源及当前时钟。

```rust
use openrust::support::{PasswordOptions, Str};

let password = Str::password(24, PasswordOptions::default())?;
assert_eq!(password.len(), 24);
assert!(Str::isUuid(&Str::uuid7().to_string(), Some(7)));
assert!(Str::isUlid(&Str::ulid().to_string()));
```
