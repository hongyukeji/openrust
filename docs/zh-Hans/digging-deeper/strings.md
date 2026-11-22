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

`plural`、`singular`、`pluralStudly` 和 `pluralPascal` 使用英语词形。`counted` 加上按英语分组的整数数量。Rust 渲染使用 pulldown-cmark。词形变化采用 Doctrine 规则，默认使用英语。

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

## 正则表达式与文本排版 {#regex-and-text-layout}

`match` 优先返回参与匹配的第一捕获组，否则返回完整匹配。`matchAll` 在声明了捕获组时返回每项的第一组，未匹配的组使用空字符串。正则采用 Rust 语法，例如 `(?i)` 表示忽略大小写，替换支持 `$1` 或具名捕获。`replaceMatchesWith` 接收 Rust 回调，无效模式返回 `regex::Error`。

`width` 和 `limit` 使用 PHP 8.5 的东亚字符宽度表。`wordWrap` 按 Laravel 的 Unicode 标量规则在空格处换行，也可强制拆分长词。`limit` 的保留整词模式会处理基本 HTML 标签后截断。

```rust
use openrust::support::Str;

assert_eq!(Str::matchAll(r"(\d+)", "甲12 乙34")?, ["12", "34"]);
assert_eq!(Str::replaceMatches(r"(\d+)", "[$1]", "甲12 乙34", Some(1))?, "甲[12] 乙34");
assert_eq!(Str::limit("中文🙂", 4, "…", false), "中文…");
assert_eq!(Str::wordWrap("中a 🙂b 中c", 3, "|", false)?, "中a|🙂b|中c");
```

## Slug 和严格转写 {#slug-and-transliteration}

`Str::slug` 使用 portable-ascii 英语映射、连字符以及默认的 `@` → `at` 字典。`slugWith` 可设置分隔符、用 `None` 保留 Unicode 字母与数字，也可按替换顺序传入字典。`ascii` 提供相同的语言映射。

`transliterateWithOptions` 接收未知字符的替换文本和严格模式标记。严格模式先规范化文本并去除非占位组合符号，再应用 ICU4X 的文字转拉丁及拉丁转 ASCII 规则，最后回退到 portable-ascii。文字识别及 CLDR 数据版本会影响与 PHP Intl 的结果一致性。

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

## 按语言转换词形 {#word-inflection}

`pluralIn`、`singularIn`、`pluralStudlyIn`、`pluralPascalIn` 和 `countedIn` 接收 `InflectionLanguage`。规则覆盖英语、世界语、法语、意大利语、书面挪威语、葡萄牙语、西班牙语和土耳其语。`Str` 和 `Stringable` 均可逐次指定语言。

```rust
use openrust::support::{InflectionLanguage, Str};

let french = Str::of("cheval").pluralIn(2, InflectionLanguage::French).to_string();
let turkish = Str::pluralIn("araba", 2, InflectionLanguage::Turkish);
assert_eq!(french, "chevaux");
assert_eq!(turkish, "arabalar");
assert_eq!(Str::plural("child", 2), "children");
```

## 时间戳前置的 COMB UUID {#ordered-comb-uuid}

`Str::orderedUuid` 创建 UUID v4，前六字节保存以 10 微秒为单位的 Unix 时间戳。`orderedUuidAt` 接收显式的 `SystemTime`。跨时间刻度按时间戳排序，同一刻度内的顺序由随机字节决定。

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

## 标题大小写与字面量编辑 {#title-and-literal-editing}

使用 `title`、`headline`、`apa` 和 `initials` 生成标题或首字母。填充和截取以 Unicode 标量计数。`replace` 按顺序替换字面量，`swap` 选最长匹配且每段只替换一次。`excerpt` 从首次匹配处向两侧截取。

`isUrl` 使用 Rust URL 解析器验证带主机和允许协议的绝对 URL。链式条件接收类型化回调，正则拆分使用 Rust 正则语法，Base64 解码和解密返回字节。 通过 `hash::<D>()` 选用 Rust 摘要类型，返回小写十六进制文本。

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

## 格式扫描 {#format-scanning}

`Stringable::scan` 按格式串读出字段，支持宽度、跳过赋值、指定输出位置和字符集。匹配中断后，未匹配槽位为 `None`。首次转换前遇到输入末尾返回空集合。无效格式返回 `ScanError`。字段宽度与 `%n` 按字节计数，截断 UTF-8 的字段用 `ScanValue::Bytes` 保留。

```rust
use openrust::support::Str;
use serde_json::json;

let fields = Str::of("42 甲").scan("%2$d %1$s%n");
assert!(fields.is_err()); // Positional and sequential output cannot be mixed.
let fields = Str::of("42 甲").scan("%2$d %1$s")?.toArray()?;
assert_eq!(fields, json!(["甲", 42]));
assert_eq!(Str::of("17").scan("%d %s")?.toArray()?, json!([17, null]));
```
