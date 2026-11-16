# 辅助函数

## 数据路径 {#data-paths}

从 `openrust::support` 导入 `Arr` 和数据辅助函数访问 JSON 数组与对象。`Arr::get` 优先查找完整键，再遍历点路径。`data_get` 直接遍历路径，支持 `*`、`{first}` 和 `{last}`。路径缺失返回 `None`，显式空值返回 `Some(Value::Null)`。

```rust
use openrust::support::{Arr, data_fill, data_get, data_has};
use serde_json::json;

let mut users = json!([{"active": null}, {}]);
data_fill(&mut users, "*.active", json!(true))?;
assert!(data_has(&users, "0.active"));
assert_eq!(data_get(&users, Some("*.active")), Some(json!([null, true])));
assert_eq!(Arr::get(&users, "1.active"), Some(&json!(true)));
```

`data_has` 检查字面路径，`data_set` 覆盖值，`data_fill` 填补缺失值，`data_forget` 移除值。修改会校验嵌套深度，失败返回 `DataPathError`。用 `Arr::dot` 和 `Arr::undot` 展开和还原路径。类型化数组使用 Rust 切片和迭代器，集合流水线使用 `Collection` 或 `LazyCollection`。字符串操作见[字符串](./strings.md)。

## 链式 URI {#fluent-uris}

使用 `Uri::of` 解析绝对或相对引用。组件设置方法返回新的已验证 URI。`query()` 读取解码后的参数并保留字面的加号和带点键。用 `withQuery`、`withQueryIfMissing`、`replaceQuery`、`pushOntoQuery` 和 `withoutQuery` 修改查询串。用 JSON 映射传递具名参数。应用代码可用 `Uri::to`、`route`、`action`、`signedRoute` 或 `temporarySignedRoute` 生成地址，再用 `redirect` 构建经过验证的重定向响应。

```rust
use openrust::support::Uri;
use serde_json::json;

let uri = Uri::of("/items?label=a+b")?
    .withQuery(&json!({"page": 2}))?
    .withFragment("summary")?;
assert_eq!(uri.value(), "/items?label=a%2Bb&page=2#summary");
```

## 数组类型读取与查询编码 {#typed-arrays}

`Arr::array`、`boolean`、`float`、`integer` 和 `string` 检查已存储的 JSON 类型。路径缺失和类型错误分别返回不同错误。`Arr::add` 填充缺失或为 null 的路径，`pull` 读取并删除值。`wrap` 保留数组和对象，将 null 转为空列表，将标量包装为单元素列表。`Arr::query` 按插入顺序生成 RFC 3986 查询串。

```rust
use openrust::support::Arr;
use serde_json::json;

let mut data = json!({"filter": {"label": "a+b", "enabled": true}});
assert!(Arr::boolean(&data, "filter.enabled")?);
assert_eq!(Arr::pull(&mut data, "filter.enabled")?, Some(json!(true)));
assert_eq!(Arr::query(&data)?, "filter%5Blabel%5D=a%2Bb");
```
