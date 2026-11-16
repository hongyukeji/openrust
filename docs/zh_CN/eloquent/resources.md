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

### 模型默认资源 {#default-model-resources}

为模型实现 `UseResource`，声明 `type Resource = AccountResource`。资源实现 `JsonResource` 和 `From<Account>`。`toResource()` 与 `toResourceCollection()` 据此选择默认资源，关联关系在编译期检查。

可覆盖 `UseResource::resourceCollection`，根据原始集合或分页器创建自定义集合响应。默认推断遇到空集合时输出 `data: []`，并保留分页信息。用 `toResourceWith` 或 `toResourceCollectionWith` 显式选择元素资源，用 `toResourceCollectionUsing` 传入显式集合工厂。

```rust
use openrust::{
    database::eloquent::Collection,
    http::resources::json::{JsonResource, TransformsToResourceCollection, UseResource},
    pagination::LengthAwarePaginator,
};

pub fn account_resource<M: UseResource>(account: M) -> M::Resource {
    account.toResource()
}

pub fn account_resources<M: UseResource>(accounts: Collection<M>) -> impl JsonResource + Send {
    accounts.toResourceCollection()
}

pub fn account_page<M: UseResource>(page: LengthAwarePaginator<M>) -> impl JsonResource + Send {
    page.toResourceCollection()
}
```

## 资源集合

将资源向量传给 `ResourceCollection::new`。集合按当前请求转换每个条目，空集合返回 `{"data":[]}`。条目的元数据和响应钩子在该条目作为最外层响应时生效。通过最外层资源的 `with`、`additional` 或 `withResponse` 添加元数据或修改响应。

### 保留集合键 {#preserving-collection-keys}

先用 `ResourceItems::from` 接收 Support 或 Eloquent 集合，再转换元素。集合只有数字键时默认重排为连续索引。只要有非数字键，响应就保留原有键和顺序。在响应集合上用 `preserveKeys(true)`，或在元素资源上覆盖 `JsonResource::preservesKeys`，即可保留不连续的数字键。用 `preserveKeys(false)` 可以在该响应恢复数字键重排。

普通及简易分页保留源集合键与分页信息。游标分页使用连续索引，向前翻页时也一样。空结果输出 `[]`。

```rust
use openrust::{
    http::resources::json::{JsonResource, ResourceCollection},
    support::Collection,
};

pub fn with_original_keys<R: JsonResource>(
    resources: Collection<R>,
) -> ResourceCollection<R> {
    ResourceCollection::new(resources).preserveKeys(true)
}
```

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

`whenLoadedOne` 区分尚未加载的关系和已加载但没有模型的关系。将内层 `None` 映射成 `Value::Null` 即可保留该字段。用 `whenAppended` 判断访问器是否已加入追加列表，用 `whenNull` 判断显式空值，用 `mergeWhen` 或 `mergeUnless` 合并字段，已有键保留原值。

### 条件中间表字段 {#conditional-pivot-information}

默认 `pivot` 访问名用 `whenPivotLoaded`，自定义访问名用 `whenPivotLoadedAs`。两者都核对实际中间表并使用已加载的记录。按 Rust 模型类型匹配自定义 Pivot 时，用 `whenPivotLoadedAsType`。

```rust
use openrust::{
    database::{Value as AttributeValue, eloquent::{Model, relations::Pivot}},
    http::resources::json::Conditional,
};
use serde_json::{Map, Value};

pub fn include_level(fields: &mut Map<String, Value>, model: &impl Model) {
    let level = Conditional::whenPivotLoaded(model, "memberships", || {
        let value = model.getRelation::<Pivot>("pivot")
            .and_then(|row| row.getAttribute("level"));
        match value {
            Some(AttributeValue::Text(level)) => Value::String(level.clone()),
            _ => Value::Null,
        }
    });
    Conditional::insert(fields, "level", level);
}
```
