# API Resources

## Generating Resources {#generating-resources}

Artisan writes resource modules to `app/http/resources`. Use a PascalCase name and separate nested modules with `/`.

```bash
./artisan make:resource UserResource
./artisan make:resource Admin/UserResource --json-api
./artisan make:resource UserCollection --collection
```

Use `-j` or `--json-api` for a JSON:API resource. Use `-c`, `--collection`, or a name ending in `Collection` for a collection. The collection template takes precedence when both options are supplied.

Declare the generated modules in their parent `mod.rs` files and rebuild. A generated collection is a typed alias of `ResourceCollection`. Pass mapped resources or a paginator to its `new` method.

Artisan reports an existing file and leaves it in place unless you supply `-f` or `--force`. Publish and customize the three templates using `./artisan stub:publish`.

## Concept Overview

Implement `JsonResource::to_array` to transform data using the current HTTP request. Return the resource directly from a route or call `to_response(request)`. The response defaults to a `data` wrapper. Use `response().without_wrapping()` or `wrap` for a response-specific override. `ModelResource` uses the model’s serialization rules and returns 201 for newly created models.

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

### Returning Resources from Controllers {#responsable-resources}

Routes can return a `JsonResource` directly. When a typed controller returns a `Responsable`, call `resource.into_responsable()` and use `ResourceResponder<YourResource>` as the controller’s response type. For example, return `Ok(StatusResource.into_responsable())` from the controller. The adapter uses the current request for resource preparation and transformation, and applies wrapping, metadata, and `with_response` as usual.

Call the same method on a `ResourceCollection`, including one backed by a paginator. Choose `preserve_query()` or `with_query(...)` before converting to preserve selected query parameters in pagination links. A response builder from `resource.response()` also implements `Responsable` when it borrows a `Sync` resource. It renders an already prepared resource and keeps per-response options such as `additional` and `without_wrapping`.

### JSON Response Options {#json-response-options}

Set `default_json_options` on a resource to choose the wire format for single responses, collections, and empty pages. Override `json_options` for an instance-specific format. A non-empty collection uses its first resource’s options. Serde keeps fractional zeroes such as `3.0` by default. `JsonOptions` can indent output or escape slashes and Unicode characters while leaving decoded values unchanged.

Use `to_json(request)` or `to_pretty_json(request)` to serialize the projected resource directly. HTTP responses apply the options to the whole document, including pagination links and metadata.

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

### Application Resource Wrapping {#application-resource-wrapping}

Register `ResourceWrapping` in the application container to change its default `data` wrapper. Use `ResourceWrapping::wrap("items")` for a different key, or `without_wrapping()` to disable wrapping on ordinary responses. Named resource wrappers and explicit `response().wrap(...)` and `without_wrapping()` choices take precedence.

Pagination and additional metadata still require a wrapper. With wrapping disabled, they use `data`. Implement `JsonResource::force_wrapping` to wrap even when the selected key already exists in the resource data.

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

### Default Model Resources {#default-model-resources}

Implement `UseResource` for a model and set `type Resource = AccountResource`. The resource implements `JsonResource` and `From<Account>`. This type declaration supplies the default used by `toResource()` and `to_resource_collection()`. Rust checks the association at compile time.

Override `UseResource::resource_collection` to build a custom collection response from the original source. Inferred empty collections use the standard `data: []` response and retain any pagination metadata. Select an item resource explicitly with `toResourceWith` or `to_resource_collection_with`. Use `to_resource_collection_using` to pass an explicit collection factory.

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

## Resource Collections

Pass a vector of resources to `ResourceCollection::new`. Each item is transformed for the current request, and an empty collection returns `{"data":[]}`. Item metadata and response hooks apply when the item is the outermost response. Use `with`, `additional`, or `with_response` on the outer resource to add metadata or modify its response.

### Preserving Collection Keys {#preserving-collection-keys}

Pass a Support or Eloquent collection to `ResourceItems::from` before mapping its items. Numeric keys become sequential by default. If any key is non-numeric, the response keeps the original keys and order. Use `preserve_keys(true)` on the response collection or override `JsonResource::preserves_keys` on the item resource to retain sparse numeric keys. Use `preserve_keys(false)` to restore the default numeric behavior for a response.

Offset paginators retain their source collection keys and metadata. Cursor pages use sequential keys, including pages read backward. Empty results use `[]`.

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

## Paginated Responses

Map a length-aware, simple, or cursor paginator with `ResourceItems::map`. The response retains the paginator’s `links` and `meta`, and paginated data stays wrapped even when wrapping is disabled. Use `preserve_query` or `with_query` to carry filters across pages, and `with_pagination_information` to customize the generated metadata.

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

## Conditional Attributes and Relationships

`Conditional::when` evaluates its closure only when the condition holds. Pass the result to `Conditional::insert` to omit missing fields while retaining explicit JSON null. `when_loaded` reads a typed, already loaded relationship. `when_counted`, `when_aggregated`, and `when_exists_loaded` read selected aggregate attributes.

`when_loaded_one` distinguishes an unloaded relationship from a loaded relationship with no model. Map the inner `None` to `Value::Null` to include it in the response. Use `when_appended` for accessors on the model’s append list, `when_null` for explicit nulls, and `merge_when` or `merge_unless` to insert fields while retaining fields already present.

`attributes` selects fields from the serialized model and respects its visibility settings. `merge` adds them while retaining existing fields. `transform` runs only for a non-blank JSON value and preserves meaningful values such as `0` and `false`.

### Conditional Pivot Information {#conditional-pivot-information}

Use `when_pivot_loaded` for the default `pivot` accessor and `when_pivot_loaded_as` for a named accessor. Both check the actual intermediate table and use an already loaded row. `when_pivot_loaded_as_type` matches a custom pivot by its Rust model type.

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

## JSON:API Resources {#jsonapi-resources}

Implement `JsonApiResource` for a model projection. The resource emits a JSON:API `data` document with a string identifier and the `application/vnd.api+json` content type. The default identifier is the model key, and the resource type is inferred from the resource’s Rust type name. Override `to_id` and `to_type` to use explicit values.

Use `fields[people]=name` to select attributes. `to_links` and `to_meta` add per-resource information. Collections and pages keep JSON array data and pagination links. The document always uses the protocol’s `data` key.

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

### JSON Object Sources {#jsonapi-data-sources}

For data you already have as a JSON object, set the resource's associated `Model` type to `serde_json::Map<String, Value>`. The default identifier reads a string or integer `id`. Return custom attributes from `to_attributes` when you need to restrict the fields exposed by the source.

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

Object-backed resources can be returned on their own, in collections, or from a relationship resolver alongside Eloquent resources.

An array or object supplied as `include` is ignored. An array or object supplied as `fields[type]` behaves like an empty fieldset for that type.

### Lazy Attributes and Included Relationships {#jsonapi-includes}

Return a `JsonApiAttributes` declaration from `lazy_attributes` to calculate fields after applying the sparse fieldset. Declare related resources with `JsonApiRelationships::to_one` or `to_many` in `to_relationships`. Their closures run for requested `include` paths. Resolvers can use relationships loaded by the query or by automatic loading.

For `include=editor.organization`, the response emits relationship identifiers and a single top-level `included` array. Resources with the same type and ID appear once. A sparse fieldset can omit the relationship member while keeping explicitly included resources. Include paths accept up to five segments.

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

### Collection Relationships {#jsonapi-collection-relationships}

Use `JsonApiRelationships::to_collection` when a resolver returns a `ResourceCollection`. Nested include paths apply to the items selected by that resolver. Filter the collection before returning it to exclude both an item and its descendants from the document.

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

### Automatic Relationship Loading {#jsonapi-automatic-loading}

Map each allowed JSON:API path to a path registered in `Model::namedRelations` by implementing `include_path`. Return the model from `model_mut`. When a route returns a resource or a collection, including a page, the framework batches missing relationships before serialization. The resolvers registered in `to_relationships` read the loaded results. For manual response construction, call `to_response_async(request).await`.

An alias such as `reports.reports` can map to `directReports.directReports`. Every supported include path should be explicitly mapped.

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

### Document Configuration and Query Controls {#jsonapi-configuration}

Register `JsonApi` in the application container, for example in a service provider. Configured information appears in the top-level `jsonapi` object of JSON:API responses, including collections and empty pages.

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

Relationship paths default to a depth of five. The framework truncates longer paths before loading and projecting relationships. A depth of zero or one keeps only direct relationships from the request.

On a resource, return `false` from `respect_fields_and_includes_in_query_string` to ignore both query parameters. Return `true` from `include_previously_loaded_relationships` to include declared direct relationships that are already loaded. In this mode, sparse fields are still applied unless disabled, and the query cannot trigger relationship loading.
