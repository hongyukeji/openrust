# Eloquent 序列化

## 序列化模型与集合

用 `toArray()` 得到可编码为 JSON 的映射，用 `toJson()` 得到 JSON 字符串。两者都会包含已加载的关系，输出中的关系名使用下划线命名。用 `attributesToArray()` 仅输出属性。集合输出各个模型组成的数组。

## 隐藏 JSON 字段

在 `ModelMetadata` 声明 `hidden` 或 `visible`。非空的 `visible` 只允许列出的字段，`hidden` 排除同名字段。通过 `makeVisible`、`makeHidden`、`setVisible` 或 `setHidden` 调整单个模型。配置关系时使用加载时的原始名称。

## 追加 JSON 字段

在 `ModelMetadata::appends` 声明访问器名称，或调用模型的 `append`。追加字段遵循相同的可见性规则。下例使用模型定义的 `summary` 访问器。

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub fn public_json(record: &mut impl Model) -> DatabaseResult<String> {
    record.makeHidden(&["label"]).append(&["summary"]);
    record.toJson()
}
```

## 日期序列化

日期默认输出为带微秒的 UTC RFC 3339 字符串。覆写 `serializeDate` 可调整模型输出。数据库存储格式由 `ModelMetadata::dateFormat` 控制。自定义转换可实现 `serialize` 调整转换值的输出。

使用 `AttributeCast::DateWithFormat("%Y/%m/%d")` 或 `DateTimeWithFormat("%H:%M %:z")` 设置单个字段的输出格式。格式采用 Chrono 语法，使用模型连接所属应用的时区。字段读取和数据库存储仍采用各自的格式。

## 从路由返回模型

路由可以直接返回模型、集合或数据库结果，响应使用模型的序列化规则。刚创建的模型返回 201，已有模型和集合返回 200，`findOrFail` 找不到模型时返回 404。

```rust
use openrust::{database::eloquent::Model, routing::RoutingError, support::facades::Route};

pub fn register_model_routes<M: Model>() -> Result<(), RoutingError> {
    Route::get("/records", M::all)?;
    Route::get("/records/1", || M::findOrFail(1))
}
```

## 条件可见性

`makeVisibleIf` 和 `makeHiddenIf` 接受布尔值或读取当前模型的条件函数。条件为假时保留现有可见性。对 Eloquent 集合调用 `makeVisible`、`makeHidden`、`setVisible`、`setHidden` 或 `append`，会应用到每个模型。

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub fn conditional_json(record: &mut impl Model, may_read: bool) -> DatabaseResult<String> {
    record.makeVisibleIf(may_read, &["label"]);
    record.makeHiddenIf(!may_read, &["label"]);
    record.toJson()
}
```
