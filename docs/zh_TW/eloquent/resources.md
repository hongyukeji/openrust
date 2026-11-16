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

### JSON 回應選項 {#json-response-options}

在資源上實作 `defaultJsonOptions`，即可為單一資源、集合和空分頁宣告編碼格式。依執行個體調整時覆寫 `jsonOptions`。非空集合採用第一個資源的選項。Serde 預設把 `3.0` 這類數字保留為帶小數的寫法。`JsonOptions` 可設定縮排、斜線和 Unicode 跳脫，解碼後的值保持一致。

用 `toJson(request)` 或 `toPrettyJson(request)` 直接序列化資源投影。HTTP 回應會對整份文件套用選項，包括分頁連結和中繼資料。

```rust
use openrust::http::{
    Request,
    resources::json::{JsonOptions, JsonResource, ResourceResult},
};
use serde_json::{Value, json};

pub struct ReadingResource(pub f64);

impl JsonResource for ReadingResource {
    fn toArray(&self, _: &Request) -> ResourceResult<Value> {
        Ok(json!({"reading_time": self.0, "path": "/café"}))
    }

    fn defaultJsonOptions() -> JsonOptions {
        JsonOptions::default().prettyPrint().escapeSlashes().escapeUnicode()
    }
}
```

### 應用程式層級資源包裝 {#application-resource-wrapping}

在應用程式容器中註冊 `ResourceWrapping`，即可調整該應用程式的預設 `data` 包裝。用 `ResourceWrapping::wrap("items")` 更換包裝鍵，或用 `withoutWrapping()` 關閉一般回應的包裝。資源自訂包裝鍵及單次回應的 `response().wrap(...)`、`withoutWrapping()` 優先。

分頁和附加中繼資料仍需包裝。關閉預設包裝時使用 `data`。實作 `JsonResource::forceWrapping` 可在資源資料已有所選鍵時再包裝一層。

```rust
use std::sync::Arc;

use openrust::{
    container::Container,
    contracts::container::{BindingResolutionError, Container as _},
    http::resources::json::ResourceWrapping,
};

pub fn configure_resources(container: &Container) -> Result<(), BindingResolutionError> {
    container.instance(Arc::new(ResourceWrapping::withoutWrapping()))?;
    Ok(())
}
```

### 模型預設資源 {#default-model-resources}

為模型實作 `UseResource`，宣告 `type Resource = AccountResource`。資源實作 `JsonResource` 和 `From<Account>`。`toResource()` 與 `toResourceCollection()` 據此選擇預設資源，型別關聯在編譯期檢查。

可覆寫 `UseResource::resourceCollection`，依原始集合或分頁器建立自訂集合回應。預設推斷遇到空集合時輸出 `data: []`，並保留分頁資訊。用 `toResourceWith` 或 `toResourceCollectionWith` 明確選擇元素資源，用 `toResourceCollectionUsing` 傳入明確的集合工廠。

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

## 資源集合

將資源向量傳給 `ResourceCollection::new`。集合依目前請求轉換每個項目，空集合回傳 `{"data":[]}`。項目的中繼資料與回應掛鉤在該項目作為最外層回應時生效。透過最外層資源的 `with`、`additional` 或 `withResponse` 加入中繼資料或修改回應。

### 保留集合鍵 {#preserving-collection-keys}

先用 `ResourceItems::from` 接收 Support 或 Eloquent 集合，再轉換元素。集合只有數字鍵時預設重排為連續索引。只要有非數字鍵，回應就保留原有鍵和順序。在回應集合上用 `preserveKeys(true)`，或在元素資源上覆寫 `JsonResource::preservesKeys`，即可保留不連續的數字鍵。用 `preserveKeys(false)` 可以在該回應恢復數字鍵重排。

一般及簡易分頁保留來源集合鍵與分頁資訊。游標分頁使用連續索引，往前翻頁時也一樣。空結果輸出 `[]`。

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

`whenLoadedOne` 區分尚未載入的關聯與已載入但沒有模型的關聯。將內層 `None` 映射為 `Value::Null` 即可保留該欄位。用 `whenAppended` 判斷存取器是否已加入附加清單，用 `whenNull` 判斷顯式空值，用 `mergeWhen` 或 `mergeUnless` 合併欄位，已有鍵保留原值。

`attributes` 從已序列化的模型選取欄位，遵循欄位可見性設定。`merge` 合併時保留已有欄位。`transform` 僅轉換非空白的 JSON 值，`0` 和 `false` 仍會參與轉換。

### 條件中介資料表欄位 {#conditional-pivot-information}

預設 `pivot` 存取名稱用 `whenPivotLoaded`，自訂存取名稱用 `whenPivotLoadedAs`。兩者都核對實際中介資料表並使用已載入的記錄。依 Rust 模型型別比對自訂 Pivot 時，用 `whenPivotLoadedAsType`。

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
