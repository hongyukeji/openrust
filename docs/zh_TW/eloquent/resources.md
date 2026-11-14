# API 資源

## 概念概覽

實作 `JsonResource::toArray`，依目前 HTTP 請求轉換資料。路由可直接回傳資源，也可以呼叫 `toResponse(request)`。回應預設使用 `data` 包裝。可透過 `response().withoutWrapping()` 或 `wrap` 調整本次回應的包裝。`ModelResource` 沿用模型序列化規則，新建模型回傳 201。

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

## 資源集合

將資源向量傳給 `ResourceCollection::new`。集合依目前請求轉換每個項目，空集合回傳 `{"data":[]}`。項目的中繼資料與回應掛鉤在該項目作為最外層回應時生效。透過最外層資源的 `with`、`additional` 或 `withResponse` 加入中繼資料或修改回應。

## 分頁回應

用 `ResourceItems::map` 轉換一般、簡易或游標分頁器。回應保留分頁器的 `links` 和 `meta`，停用包裝時分頁資料仍有包裝。用 `preserveQuery` 或 `withQuery` 保留跨頁篩選參數，用 `withPaginationInformation` 自訂分頁中繼資料。

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

## 條件屬性與關聯

`Conditional::when` 僅在條件成立時執行閉包。將結果傳給 `Conditional::insert` 可以省略缺少的欄位並保留顯式 JSON null。`whenLoaded` 讀取已載入的型別化關聯。`whenCounted`、`whenAggregated` 和 `whenExistsLoaded` 讀取已查詢的彙總屬性。
