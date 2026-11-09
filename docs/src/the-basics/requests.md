# HTTP 请求

## 读取与转换输入

控制器通过公开 `Request` 读取当前请求输入。`integer_or`、`float_or` 提供显式缺失默认值；字段存在且为 null 时仍执行转换，不采用这两个方法的缺失默认值。

```rust
use openrust::{Result, http::Request};

pub fn numeric_input(request: &Request) -> Result<(i64, f64)> {
    let quantity = request.integer_or("quantity", 1)?;
    let ratio = request.float("ratio")?;
    Ok((quantity, ratio))
}
```

`integer` 和 `float` 遵循 PHP 的宽松转换。例如字符串 `"2items"` 的整数结果是 2，`"1.25suffix"` 的浮点结果是 1.25。若要求整个字段均为合法数字，应另行校验；转换方法不能代替业务规则。

float／integer 的数字字符串解析与对应 Model 读取、Session 数值字符串算术共用同一内核。显式指数先按 PHP 的解析规则处理，再与尾数和小数位组合，因此大尾数与反向大指数相消时不能直接套用 Rust 字符串解析结果。浮点转换可能产生非有限值，业务代码应在计算和响应序列化前明确处理。

`clamp` 使用 `support::InputNumber` 保留整数与浮点类别，并按 `min(max(value, min), max)` 选择结果。它要求完整数字字符串，不接受整数转换的尾缀规则。`clamp_or` 在缺失、null 或无法作为完整数值读取时采用默认值，再执行相同夹界运算；不能把它的默认规则与 `integer_or` 混用。唯一模块的 `/api/example/input` 使用这些公开方法读取 `typed.quantity`、`typed.amount` 并约束数量。

整数验证的固定对照为 `validateInteger`，普通规则沿 PHP `FILTER_VALIDATE_INT` 的值边界。 [Laravel ValidatesAttributes](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Validation/Concerns/ValidatesAttributes.php)

## 输入类别与当前边界

JSON 中的字符串仍可保留完整数字文本；已经解码为 JSON Number 的值则已历经解析。不能由字符串转换的一致性推断任意超精度原始 JSON 数字也与 PHP 解码完全一致。字段校验、金额等领域精度要求和数据库类型需要各自显式建模。

本章先说明已接入的数值读取流程。完整请求章节、原始 JSON 数字的全部解析边界及其他尚未对应的行为仍按框架计划逐项交付，不以此页代表全章完成。路由装配见[路由](routing.md)，模型原始属性与 cast 阶段见[模型基础](../eloquent/getting-started.md)。

固定来源：[Laravel 13.x Requests](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md) 与框架 `InteractsWithData`；Rust 的显式方法名和 `Result` 返回适配静态类型与异常传播。
