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
