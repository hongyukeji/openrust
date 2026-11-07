# NumericDate 公开日期接口

只使用 `openrust::support::calendar`，展示 JSON 数值/数值字符串解析、负数的 `U.u` 秒与微秒字段、六位 half-up 舍入及非法输入。接口不读取时钟，解析固定 UTC；不将 PHP 字段规则当作数学负小数时间戳。

`format_php_numeric_timestamp(f64)` 返回六位小数文本，拒绝 NaN/Infinity；`parse_php_numeric_timestamp(&serde_json::Value)` 接受数字或合法数值字符串，拒绝 bool、null、数组、对象及超秒范围。大整数先经 `f64`，会有精度边界。完整 Model 日期与 JWT Parser/ResourceServer 不属于这两个函数。

在仓库根目录可执行：

```sh
cargo check --manifest-path tests/consumers/calendar-numeric/Cargo.toml
cargo test --manifest-path tests/consumers/calendar-numeric/Cargo.toml
```
