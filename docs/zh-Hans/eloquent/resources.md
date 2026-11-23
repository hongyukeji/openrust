# API 资源

## 生成资源 {#generating-resources}

Artisan 将资源模块写到 `app/http/resources`。资源名使用 PascalCase，嵌套模块用 `/` 分隔。

```bash
./artisan make:resource UserResource
./artisan make:resource Admin/UserResource --json-api
./artisan make:resource UserCollection --collection
```

使用 `-j` 或 `--json-api` 生成 JSON:API 资源。使用 `-c`、`--collection` 或以 `Collection` 结尾的名称生成资源集合。两个选项同时给出时，优先选集合模板。

在上级 `mod.rs` 中声明生成的模块后重新构建。生成的集合是 `ResourceCollection` 的类型别名，可用 `new` 接收转换后的资源或分页器。

文件已存在时，Artisan 会报告冲突并保留文件。使用 `-f` 或 `--force` 可以覆盖。执行 `./artisan stub:publish` 后可自定义三种模板。

## 概念概览

实现 `JsonResource::to_array`，按当前 HTTP 请求转换数据。路由可直接返回资源，也可以调用 `to_response(request)`。响应默认使用 `data` 包装。可通过 `response().without_wrapping()` 或 `wrap` 为本次响应调整包装。`ModelResource` 沿用模型序列化规则，新建模型返回 201。

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

### 从控制器返回资源 {#responsable-resources}

路由可直接返回 `JsonResource`。当类型化控制器返回 `Responsable` 时，调用 `resource.into_responsable()`，并把控制器的响应类型声明为 `ResourceResponder<YourResource>`。例如，控制器可以返回 `Ok(StatusResource.into_responsable())`。适配器使用当前请求准备和转换资源，随后应用包装、元数据和 `with_response`。

`ResourceCollection` 及其分页形式也可调用此方法。转换前可调用 `preserve_query()` 或 `with_query(...)`，控制分页链接保留的查询参数。若资源实现了 `Sync`，`resource.response()` 返回的构建器也实现 `Responsable`。它渲染已经准备好的资源，并保留 `additional`、`without_wrapping` 等本次响应的选项。

### JSON 响应选项 {#json-response-options}

在资源上实现 `default_json_options`，即可为单资源、集合和空分页声明编码格式。按实例调整时覆盖 `json_options`。非空集合采用首个资源的选项。Serde 默认把 `3.0` 这类数字保留为带小数的写法。`JsonOptions` 可设置缩进、斜杠和 Unicode 转义，解码后的值保持一致。

用 `to_json(request)` 或 `to_pretty_json(request)` 直接序列化资源投影。HTTP 响应会对整个文档应用选项，包括分页链接和元数据。

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

### 应用级资源包装 {#application-resource-wrapping}

在应用容器中注册 `ResourceWrapping`，可以调整该应用的默认 `data` 包装。用 `ResourceWrapping::wrap("items")` 更换包装键，或用 `without_wrapping()` 关闭普通响应的包装。资源自定义包装键及单次响应的 `response().wrap(...)`、`without_wrapping()` 优先。

分页和附加元数据仍需包装。关闭默认包装时使用 `data`。实现 `JsonResource::force_wrapping` 可以在资源数据已含所选键时再包装一层。

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

### 模型默认资源 {#default-model-resources}

为模型实现 `UseResource`，声明 `type Resource = AccountResource`。资源实现 `JsonResource` 和 `From<Account>`。`toResource()` 与 `to_resource_collection()` 据此选择默认资源，关联关系在编译期检查。

可覆盖 `UseResource::resource_collection`，根据原始集合或分页器创建自定义集合响应。默认推断遇到空集合时输出 `data: []`，并保留分页信息。用 `toResourceWith` 或 `to_resource_collection_with` 显式选择元素资源，用 `to_resource_collection_using` 传入显式集合工厂。

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
    accounts.to_resource_collection()
}

pub fn account_page<M: UseResource>(page: LengthAwarePaginator<M>) -> impl JsonResource + Send {
    page.to_resource_collection()
}
```

## 资源集合

将资源向量传给 `ResourceCollection::new`。集合按当前请求转换每个条目，空集合返回 `{"data":[]}`。条目的元数据和响应钩子在该条目作为最外层响应时生效。通过最外层资源的 `with`、`additional` 或 `with_response` 添加元数据或修改响应。

### 保留集合键 {#preserving-collection-keys}

先用 `ResourceItems::from` 接收 Support 或 Eloquent 集合，再转换元素。集合只有数字键时默认重排为连续索引。只要有非数字键，响应就保留原有键和顺序。在响应集合上用 `preserve_keys(true)`，或在元素资源上覆盖 `JsonResource::preserves_keys`，即可保留不连续的数字键。用 `preserve_keys(false)` 可以在该响应恢复数字键重排。

普通及简易分页保留源集合键与分页信息。游标分页使用连续索引，向前翻页时也一样。空结果输出 `[]`。

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

## 分页响应

用 `ResourceItems::map` 转换普通、简易或游标分页器。响应保留分页器的 `links` 和 `meta`，关闭包装时分页数据仍有包装。用 `preserve_query` 或 `with_query` 保留跨页筛选参数，用 `with_pagination_information` 自定义分页元数据。

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

## 条件属性与关系

`Conditional::when` 仅在条件成立时执行闭包。将结果传给 `Conditional::insert` 可以省略缺失字段并保留显式 JSON null。`when_loaded` 读取已加载的类型化关系。`when_counted`、`when_aggregated` 和 `when_exists_loaded` 读取已查询的聚合属性。

`when_loaded_one` 区分尚未加载的关系和已加载但没有模型的关系。将内层 `None` 映射成 `Value::Null` 即可保留该字段。用 `when_appended` 判断访问器是否已加入追加列表，用 `when_null` 判断显式空值，用 `merge_when` 或 `merge_unless` 合并字段，已有键保留原值。

`attributes` 从已序列化的模型中选取字段，遵循字段可见性设置。`merge` 合并时保留已有字段。`transform` 仅转换非空白的 JSON 值，`0` 和 `false` 仍会参与转换。

### 条件中间表字段 {#conditional-pivot-information}

默认 `pivot` 访问名用 `when_pivot_loaded`，自定义访问名用 `when_pivot_loaded_as`。两者都核对实际中间表并使用已加载的记录。按 Rust 模型类型匹配自定义 Pivot 时，用 `when_pivot_loaded_as_type`。

```rust
use openrust::{
    database::{Value as AttributeValue, eloquent::{Model, relations::Pivot}},
    http::resources::json::Conditional,
};
use serde_json::{Map, Value};

pub fn include_level(fields: &mut Map<String, Value>, model: &impl Model) {
    let level = Conditional::when_pivot_loaded(model, "memberships", || {
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

## JSON:API 资源 {#jsonapi-resources}

为模型投影实现 `JsonApiResource`。响应使用 JSON:API 的 `data` 文档、字符串标识和 `application/vnd.api+json` 媒体类型。标识默认取模型主键，资源类型默认从 Rust 资源类型名推断。可覆盖 `to_id` 和 `to_type` 以显式设定。

用 `fields[people]=name` 选取属性。`to_links` 和 `to_meta` 可提供资源级信息。集合和分页使用数组数据，并保留分页链接。文档始终使用协议规定的 `data` 键。

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

### JSON 对象数据源 {#jsonapi-data-sources}

已有 JSON 对象时，可将资源的关联类型 `Model` 声明为 `serde_json::Map<String, Value>`。默认从字符串或整数 `id` 取标识。需要限制公开字段时，可以实现 `to_attributes` 自行投影。

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

对象资源可以单独返回，也可以放进集合，或作为 Eloquent 资源关联解析的结果。

数组或对象形式的 `include` 会被忽略。该类型的 `fields[type]` 若收到数组或对象，则按空字段集处理。

### 延迟属性与关联包含 {#jsonapi-includes}

在 `lazy_attributes` 中返回 `JsonApiAttributes`，字段会在稀疏字段集筛选后计算。在 `to_relationships` 中用 `JsonApiRelationships::to_one` 或 `to_many` 声明关联。只有 `include` 请求的关联才执行闭包。闭包可读取查询预加载或框架自动补载的 Eloquent 关联。

`include=editor.organization` 会输出关联标识，并将关联资源放入同一个顶层 `included` 数组。同一类型和标识的资源只出现一次。稀疏字段集可省略关联字段，同时保留显式包含的资源。关联路径最多五段。

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

### 集合关联 {#jsonapi-collection-relationships}

关联解析器返回 `ResourceCollection` 时使用 `JsonApiRelationships::to_collection`。嵌套 include 只作用于解析器返回的结果。返回集合前筛掉记录，可同时将该记录及其后代排除在文档之外。

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

### 自动加载关联 {#jsonapi-automatic-loading}

实现 `include_path`，把允许的 JSON:API 路径映射到 `Model::namedRelations` 登记的路径，并通过 `model_mut` 返回模型。路由返回单资源、集合或分页时，框架会在序列化前批量补载缺失关联。`to_relationships` 中声明的闭包读取加载结果。手动构造响应时，使用 `to_response_async(request).await`。

例如 `reports.reports` 可映射为 `directReports.directReports`。请显式映射每条允许自动加载的路径。

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

### 文档配置与查询控制 {#jsonapi-configuration}

在服务提供者等应用装配位置向容器注册 `JsonApi`。配置信息出现在 JSON:API 响应的顶层 `jsonapi` 对象中，集合和空页同样包含该信息。

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

关系路径默认最多五层。超出深度的路径会先截断，再加载和输出关联。将深度设为零或一时，请求只能包含直接关联。

资源的 `respect_fields_and_includes_in_query_string` 返回 `false` 时忽略两个查询参数。让 `include_previously_loaded_relationships` 返回 `true` 可包含已声明且已加载的直接关联。该模式仍应用稀疏字段筛选，除非显式关闭。查询参数不会触发关联加载。
