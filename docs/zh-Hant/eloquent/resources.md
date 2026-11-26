# API 資源

明確回傳統一業務格式的用法見 [`ApiResult`](../the-basics/responses.md#business-responses)。

## 產生資源 {#generating-resources}

Artisan 將資源模組寫到 `app/http/resources`。資源名稱使用 PascalCase，巢狀模組以 `/` 分隔。

```bash
./artisan make:resource UserResource
./artisan make:resource Admin/UserResource --json-api
./artisan make:resource UserCollection --collection
```

使用 `-j` 或 `--json-api` 產生 JSON:API 資源。使用 `-c`、`--collection` 或以 `Collection` 結尾的名稱產生資源集合。兩個選項同時提供時，優先選集合範本。

在上層 `mod.rs` 宣告產生的模組後重新建置。產生的集合是 `ResourceCollection` 的型別別名，可用 `new` 接收轉換後的資源或分頁器。

檔案已存在時，Artisan 會回報衝突並保留檔案。使用 `-f` 或 `--force` 可以覆寫。執行 `./artisan stub:publish` 後可自訂三種範本。

## 概念概覽

實作 `JsonResource::to_array`，依目前 HTTP 請求轉換資料。路由可直接回傳資源，也可以呼叫 `to_response(request)`。回應預設使用 `data` 包裝。可透過 `response().without_wrapping()` 或 `wrap` 調整本次回應的包裝。`ModelResource` 沿用模型序列化規則，新建模型回傳 201。

```rust
use openrust::http::{
    Request,
    resources::json::{Conditional, JsonResource, ResourceResult},
};
use serde_json::{Value, json};

struct StatusResource;

impl JsonResource for StatusResource {
    fn to_array(&self, request: &Request) -> ResourceResult<Value> {
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

### 從控制器回傳資源 {#responsable-resources}

路由可以直接回傳 `JsonResource`。當型別化控制器回傳 `Responsable` 時，呼叫 `resource.into_responsable()`，並將控制器的回應型別宣告為 `ResourceResponder<YourResource>`。例如，控制器可以回傳 `Ok(StatusResource.into_responsable())`。轉接器使用目前的請求準備及轉換資源，再套用包裝、中繼資料與 `with_response`。

`ResourceCollection` 及其分頁形式也能呼叫此方法。轉換前可呼叫 `preserve_query()` 或 `with_query(...)`，控制分頁連結保留的查詢參數。若資源實作 `Sync`，`resource.response()` 回傳的建構器也實作 `Responsable`。它呈現已經準備好的資源，並保留 `additional`、`without_wrapping` 等本次回應的選項。

### JSON 回應選項 {#json-response-options}

在資源上實作 `default_json_options`，即可為單一資源、集合和空分頁宣告編碼格式。依執行個體調整時覆寫 `json_options`。非空集合採用第一個資源的選項。Serde 預設把 `3.0` 這類數字保留為帶小數的寫法。`JsonOptions` 可設定縮排、斜線和 Unicode 跳脫，解碼後的值保持一致。

用 `to_json(request)` 或 `to_pretty_json(request)` 直接序列化資源投影。HTTP 回應會對整份文件套用選項，包括分頁連結和中繼資料。

```rust
use openrust::http::{
    Request,
    resources::json::{JsonOptions, JsonResource, ResourceResult},
};
use serde_json::{Value, json};

pub struct ReadingResource(pub f64);

impl JsonResource for ReadingResource {
    fn to_array(&self, _: &Request) -> ResourceResult<Value> {
        Ok(json!({"reading_time": self.0, "path": "/café"}))
    }

    fn default_json_options() -> JsonOptions {
        JsonOptions::default().pretty_print().escape_slashes().escape_unicode()
    }
}
```

### 應用程式層級資源包裝 {#application-resource-wrapping}

在應用程式容器中註冊 `ResourceWrapping`，即可調整該應用程式的預設 `data` 包裝。用 `ResourceWrapping::wrap("items")` 更換包裝鍵，或用 `without_wrapping()` 關閉一般回應的包裝。資源自訂包裝鍵及單次回應的 `response().wrap(...)`、`without_wrapping()` 優先。

分頁和附加中繼資料仍需包裝。關閉預設包裝時使用 `data`。實作 `JsonResource::force_wrapping` 可在資源資料已有所選鍵時再包裝一層。

```rust
use std::sync::Arc;

use openrust::{
    container::Container,
    contracts::container::{BindingResolutionError, Container as _},
    http::resources::json::ResourceWrapping,
};

pub fn configure_resources(container: &Container) -> Result<(), BindingResolutionError> {
    container.instance(Arc::new(ResourceWrapping::without_wrapping()))?;
    Ok(())
}
```

### 模型預設資源 {#default-model-resources}

為模型實作 `UseResource`，宣告 `type Resource = AccountResource`。資源實作 `JsonResource` 和 `From<Account>`。`to_resource()` 與 `to_resource_collection()` 據此選擇預設資源，型別關聯在編譯期檢查。

可覆寫 `UseResource::resource_collection`，依原始集合或分頁器建立自訂集合回應。預設推斷遇到空集合時輸出 `data: []`，並保留分頁資訊。用 `to_resource_with` 或 `to_resource_collection_with` 明確選擇元素資源，用 `to_resource_collection_using` 傳入明確的集合工廠。

```rust
use openrust::{
    database::eloquent::Collection,
    http::resources::json::{JsonResource, TransformsToResourceCollection, UseResource},
    pagination::LengthAwarePaginator,
};

pub fn account_resource<M: UseResource>(account: M) -> M::Resource {
    account.to_resource()
}

pub fn account_resources<M: UseResource>(accounts: Collection<M>) -> impl JsonResource + Send {
    accounts.to_resource_collection()
}

pub fn account_page<M: UseResource>(page: LengthAwarePaginator<M>) -> impl JsonResource + Send {
    page.to_resource_collection()
}
```

## 資源集合

將資源向量傳給 `ResourceCollection::new`。集合依目前請求轉換每個項目，空集合回傳 `{"data":[]}`。項目的中繼資料與回應掛鉤在該項目作為最外層回應時生效。透過最外層資源的 `with`、`additional` 或 `with_response` 加入中繼資料或修改回應。

### 保留集合鍵 {#preserving-collection-keys}

先用 `ResourceItems::from` 接收 Support 或 Eloquent 集合，再轉換元素。集合只有數字鍵時預設重排為連續索引。只要有非數字鍵，回應就保留原有鍵和順序。在回應集合上用 `preserve_keys(true)`，或在元素資源上覆寫 `JsonResource::preserves_keys`，即可保留不連續的數字鍵。用 `preserve_keys(false)` 可以在該回應恢復數字鍵重排。

一般及簡易分頁保留來源集合鍵與分頁資訊。游標分頁使用連續索引，往前翻頁時也一樣。空結果輸出 `[]`。

```rust
use openrust::{
    http::resources::json::{JsonResource, ResourceCollection},
    support::Collection,
};

pub fn with_original_keys<R: JsonResource>(
    resources: Collection<R>,
) -> ResourceCollection<R> {
    ResourceCollection::new(resources).preserve_keys(true)
}
```

## 分頁回應

用 `ResourceItems::map` 轉換一般、簡易或游標分頁器。回應保留分頁器的 `links` 和 `meta`，停用包裝時分頁資料仍有包裝。用 `preserve_query` 或 `with_query` 保留跨頁篩選參數，用 `with_pagination_information` 自訂分頁中繼資料。

```rust
use openrust::{
    http::resources::json::{JsonResource, ResourceCollection, ResourceItems},
    pagination::LengthAwarePaginator,
};

fn page_resource<T, R: JsonResource>(
    page: LengthAwarePaginator<T>,
    resource: impl FnMut(T) -> R,
) -> ResourceCollection<R> {
    ResourceCollection::new(ResourceItems::from(page).map(resource)).preserve_query()
}
```

## 條件屬性與關聯

`Conditional::when` 僅在條件成立時執行閉包。將結果傳給 `Conditional::insert` 可以省略缺少的欄位並保留顯式 JSON null。`when_loaded` 讀取已載入的型別化關聯。`when_counted`、`when_aggregated` 和 `when_exists_loaded` 讀取已查詢的彙總屬性。

`when_loaded_one` 區分尚未載入的關聯與已載入但沒有模型的關聯。將內層 `None` 映射為 `Value::Null` 即可保留該欄位。用 `when_appended` 判斷存取器是否已加入附加清單，用 `when_null` 判斷顯式空值，用 `merge_when` 或 `merge_unless` 合併欄位，已有鍵保留原值。

`attributes` 從已序列化的模型選取欄位，遵循欄位可見性設定。`merge` 合併時保留已有欄位。`transform` 僅轉換非空白的 JSON 值，`0` 和 `false` 仍會參與轉換。

### 條件中介資料表欄位 {#conditional-pivot-information}

預設 `pivot` 存取名稱用 `when_pivot_loaded`，自訂存取名稱用 `when_pivot_loaded_as`。兩者都核對實際中介資料表並使用已載入的記錄。依 Rust 模型型別比對自訂 Pivot 時，用 `when_pivot_loaded_as_type`。

```rust
use openrust::{
    database::{Value as AttributeValue, eloquent::{Model, relations::Pivot}},
    http::resources::json::Conditional,
};
use serde_json::{Map, Value};

pub fn include_level(fields: &mut Map<String, Value>, model: &impl Model) {
    let level = Conditional::when_pivot_loaded(model, "memberships", || {
        let value = model.get_relation::<Pivot>("pivot")
            .and_then(|row| row.get_attribute("level"));
        match value {
            Some(AttributeValue::Text(level)) => Value::String(level.clone()),
            _ => Value::Null,
        }
    });
    Conditional::insert(fields, "level", level);
}
```

## JSON:API 資源 {#jsonapi-resources}

為模型投影實作 `JsonApiResource`。回應使用 JSON:API 的 `data` 文件、字串識別碼和 `application/vnd.api+json` 媒體型別。識別碼預設取模型主鍵，資源型別預設從 Rust 資源型別名稱推斷。可覆寫 `to_id` 和 `to_type` 明確設定。

用 `fields[people]=name` 選取屬性。`to_links` 和 `to_meta` 可提供資源層級資訊。集合和分頁使用陣列資料，並保留分頁連結。文件一律使用協定規定的 `data` 鍵。

```rust
use openrust::{
    database::eloquent::Model,
    http::resources::json_api::JsonApiResource,
};

pub struct PersonResource<M>(pub M);

impl<M: Model> JsonApiResource for PersonResource<M> {
    type Model = M;

    fn model(&self) -> &Self::Model {
        &self.0
    }
}
```

### JSON 物件資料來源 {#jsonapi-data-sources}

已有 JSON 物件時，可將資源的關聯型別 `Model` 宣告為 `serde_json::Map<String, Value>`。預設從字串或整數 `id` 取得識別值。需要限制公開欄位時，可以實作 `to_attributes` 自行投影。

```rust
use openrust::http::resources::json_api::JsonApiResource;
use serde_json::{Map, Value};

pub struct ThingResource(pub Map<String, Value>);

impl JsonApiResource for ThingResource {
    type Model = Map<String, Value>;

    fn model(&self) -> &Self::Model {
        &self.0
    }
}
```

物件資源可以單獨回傳、放入集合，或由 Eloquent 資源的關聯解析器回傳。

陣列或物件形式的 `include` 會被忽略。該型別的 `fields[type]` 若收到陣列或物件，則按空欄位集處理。

### 延遲屬性與關聯包含 {#jsonapi-includes}

在 `lazy_attributes` 中回傳 `JsonApiAttributes`，欄位會在稀疏欄位集篩選後計算。在 `to_relationships` 中用 `JsonApiRelationships::to_one` 或 `to_many` 宣告關聯。只有 `include` 請求的關聯才執行閉包。閉包可讀取查詢預載入或框架自動補載的 Eloquent 關聯。

`include=editor.organization` 會輸出關聯識別碼，並將關聯資源放入同一個頂層 `included` 陣列。同一型別和識別碼的資源只出現一次。稀疏欄位集可省略關聯欄位，同時保留明確包含的資源。關聯路徑最多五段。

```rust
use openrust::{
    database::eloquent::Model,
    http::resources::json_api::{JsonApiAttributes, JsonApiRelationships},
};
use serde_json::json;

pub fn attributes(display_name: &str) -> JsonApiAttributes<'_> {
    JsonApiAttributes::default().attribute("display_name", move || {
        Ok(json!(display_name.trim()))
    })
}

pub fn relationships<M: Model>(editor: Option<M>) -> JsonApiRelationships<'static> {
    JsonApiRelationships::default().to_one("editor", move || {
        Ok(editor.map(PersonResource))
    })
}
```

### 集合關聯 {#jsonapi-collection-relationships}

關聯解析器回傳 `ResourceCollection` 時使用 `JsonApiRelationships::to_collection`。巢狀 include 只套用到解析器回傳的結果。回傳集合前篩除記錄，可同時將該記錄及其後代排除在文件之外。

```rust
use openrust::http::resources::{
    json::ResourceCollection,
    json_api::{JsonApiRelationships, JsonApiResource},
};

pub fn relationships<R: JsonApiResource + 'static>(visible: Vec<R>) -> JsonApiRelationships<'static> {
    JsonApiRelationships::default()
        .to_collection("reports", move || Ok(ResourceCollection::new(visible)))
}
```

### 自動載入關聯 {#jsonapi-automatic-loading}

實作 `include_path`，將允許的 JSON:API 路徑映射至 `Model::named_relations` 登記的路徑，並透過 `model_mut` 傳回模型。路由回傳單一資源、集合或分頁時，框架會在序列化前批次補載缺少的關聯。`to_relationships` 宣告的閉包讀取載入結果。手動建立回應時，使用 `to_response_async(request).await`。

例如 `reports.reports` 可映射成 `directReports.directReports`。請明確映射每條允許自動載入的路徑。

```rust
use openrust::{
    database::eloquent::Model,
    http::resources::json_api::JsonApiResource,
};

pub struct ReportResource<M>(pub M);

impl<M: Model> JsonApiResource for ReportResource<M> {
    type Model = M;

    fn model(&self) -> &Self::Model {
        &self.0
    }

    fn model_mut(&mut self) -> Option<&mut Self::Model> {
        Some(&mut self.0)
    }

    fn include_path(path: &str) -> Option<&'static str> {
        match path {
            "reports" => Some("directReports"),
            "reports.reports" => Some("directReports.directReports"),
            _ => None,
        }
    }
}
```

### 文件設定與查詢控制 {#jsonapi-configuration}

在服務提供者等應用程式組裝位置向容器註冊 `JsonApi`。設定資訊出現在 JSON:API 回應的頂層 `jsonapi` 物件中，集合與空頁同樣包含該資訊。

```rust
use openrust::{
    contracts::container::Container,
    http::resources::json_api::JsonApi,
};
use std::sync::Arc;

pub fn register(container: &openrust::container::Container) {
    let settings = JsonApi::configure(
        Some("1.1".into()),
        Vec::new(),
        Vec::new(),
        serde_json::Map::new(),
    )
    .max_relationship_depth(3);

    container.instance(Arc::new(settings)).expect("register JSON:API settings");
}
```

關聯路徑預設最多五層。超出深度的路徑會先截斷，再載入與輸出關聯。將深度設為零或一時，請求只能包含直接關聯。

資源的 `respect_fields_and_includes_in_query_string` 傳回 `false` 時忽略兩個查詢參數。讓 `include_previously_loaded_relationships` 傳回 `true` 可包含已宣告且已載入的直接關聯。此模式仍套用稀疏欄位篩選，除非明確停用。查詢參數不會觸發關聯載入。
