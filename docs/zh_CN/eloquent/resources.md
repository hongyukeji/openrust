# API 资源

## 概念概览

实现 `JsonResource::toArray`，按当前 HTTP 请求转换数据。路由可直接返回资源，也可以调用 `toResponse(request)`。响应默认使用 `data` 包装。可通过 `response().withoutWrapping()` 或 `wrap` 为本次响应调整包装。`ModelResource` 沿用模型序列化规则，新建模型返回 201。

```rust
use openrust::http::{
    Request,
    resources::json::{Conditional, JsonResource, ResourceResult},
};
use serde_json::{Value, json};

struct StatusResource;

impl JsonResource for StatusResource {
    fn toArray(&self, request: &Request) -> ResourceResult<Value> {
        let mut fields = serde_json::Map::from_iter([("status".into(), json!("ready"))]);
        Conditional::insert(
            &mut fields,
            "detail",
            Conditional::when(request.query(Some("detail"))?.is_some(), || json!("available")),
        );
        Ok(Value::Object(fields))
    }
}
```

## 资源集合

将资源向量传给 `ResourceCollection::new`。集合按当前请求转换每个条目，空集合返回 `{"data":[]}`。条目的元数据和响应钩子在该条目作为最外层响应时生效。通过最外层资源的 `with`、`additional` 或 `withResponse` 添加元数据或修改响应。

## 分页响应

用 `ResourceItems::map` 转换普通、简易或游标分页器。响应保留分页器的 `links` 和 `meta`，关闭包装时分页数据仍有包装。用 `preserveQuery` 或 `withQuery` 保留跨页筛选参数，用 `withPaginationInformation` 自定义分页元数据。

```rust
use openrust::{
    http::resources::json::{JsonResource, ResourceCollection, ResourceItems},
    pagination::LengthAwarePaginator,
};

fn page_resource<T, R: JsonResource>(
    page: LengthAwarePaginator<T>,
    resource: impl FnMut(T) -> R,
) -> ResourceCollection<R> {
    ResourceCollection::new(ResourceItems::from(page).map(resource)).preserveQuery()
}
```

## 条件属性与关系

`Conditional::when` 仅在条件成立时执行闭包。将结果传给 `Conditional::insert` 可以省略缺失字段并保留显式 JSON null。`whenLoaded` 读取已加载的类型化关系。`whenCounted`、`whenAggregated` 和 `whenExistsLoaded` 读取已查询的聚合属性。
