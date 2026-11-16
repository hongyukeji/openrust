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

Implement `JsonResource::toArray` to transform data using the current HTTP request. Return the resource directly from a route or call `toResponse(request)`. The response defaults to a `data` wrapper. Use `response().withoutWrapping()` or `wrap` for a response-specific override. `ModelResource` uses the model’s serialization rules and returns 201 for newly created models.

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

### JSON Response Options {#json-response-options}

Set `defaultJsonOptions` on a resource to choose the wire format for single responses, collections, and empty pages. Override `jsonOptions` for an instance-specific format. A non-empty collection uses its first resource’s options. Serde keeps fractional zeroes such as `3.0` by default. `JsonOptions` can indent output or escape slashes and Unicode characters while leaving decoded values unchanged.

Use `toJson(request)` or `toPrettyJson(request)` to serialize the projected resource directly. HTTP responses apply the options to the whole document, including pagination links and metadata.

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

### Application Resource Wrapping {#application-resource-wrapping}

Register `ResourceWrapping` in the application container to change its default `data` wrapper. Use `ResourceWrapping::wrap("items")` for a different key, or `withoutWrapping()` to disable wrapping on ordinary responses. Named resource wrappers and explicit `response().wrap(...)` and `withoutWrapping()` choices take precedence.

Pagination and additional metadata still require a wrapper. With wrapping disabled, they use `data`. Implement `JsonResource::forceWrapping` to wrap even when the selected key already exists in the resource data.

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

### Default Model Resources {#default-model-resources}

Implement `UseResource` for a model and set `type Resource = AccountResource`. The resource implements `JsonResource` and `From<Account>`. This type declaration supplies the default used by `toResource()` and `toResourceCollection()`. Rust checks the association at compile time.

Override `UseResource::resourceCollection` to build a custom collection response from the original source. Inferred empty collections use the standard `data: []` response and retain any pagination metadata. Select an item resource explicitly with `toResourceWith` or `toResourceCollectionWith`. Use `toResourceCollectionUsing` to pass an explicit collection factory.

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

## Resource Collections

Pass a vector of resources to `ResourceCollection::new`. Each item is transformed for the current request, and an empty collection returns `{"data":[]}`. Item metadata and response hooks apply when the item is the outermost response. Use `with`, `additional`, or `withResponse` on the outer resource to add metadata or modify its response.

### Preserving Collection Keys {#preserving-collection-keys}

Pass a Support or Eloquent collection to `ResourceItems::from` before mapping its items. Numeric keys become sequential by default. If any key is non-numeric, the response keeps the original keys and order. Use `preserveKeys(true)` on the response collection or override `JsonResource::preservesKeys` on the item resource to retain sparse numeric keys. Use `preserveKeys(false)` to restore the default numeric behavior for a response.

Offset paginators retain their source collection keys and metadata. Cursor pages use sequential keys, including pages read backward. Empty results use `[]`.

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

## Paginated Responses

Map a length-aware, simple, or cursor paginator with `ResourceItems::map`. The response retains the paginator’s `links` and `meta`, and paginated data stays wrapped even when wrapping is disabled. Use `preserveQuery` or `withQuery` to carry filters across pages, and `withPaginationInformation` to customize the generated metadata.

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

## Conditional Attributes and Relationships

`Conditional::when` evaluates its closure only when the condition holds. Pass the result to `Conditional::insert` to omit missing fields while retaining explicit JSON null. `whenLoaded` reads a typed, already loaded relationship. `whenCounted`, `whenAggregated`, and `whenExistsLoaded` read selected aggregate attributes.

`whenLoadedOne` distinguishes an unloaded relationship from a loaded relationship with no model. Map the inner `None` to `Value::Null` to include it in the response. Use `whenAppended` for accessors on the model’s append list, `whenNull` for explicit nulls, and `mergeWhen` or `mergeUnless` to insert fields while retaining fields already present.

`attributes` selects fields from the serialized model and respects its visibility settings. `merge` adds them while retaining existing fields. `transform` runs only for a non-blank JSON value and preserves meaningful values such as `0` and `false`.

### Conditional Pivot Information {#conditional-pivot-information}

Use `whenPivotLoaded` for the default `pivot` accessor and `whenPivotLoadedAs` for a named accessor. Both check the actual intermediate table and use an already loaded row. `whenPivotLoadedAsType` matches a custom pivot by its Rust model type.

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

## JSON:API Resources {#jsonapi-resources}

Implement `JsonApiResource` for a model projection. The resource emits a JSON:API `data` document with a string identifier and the `application/vnd.api+json` content type. The default identifier is the model key, and the resource type is inferred from the resource’s Rust type name. Override `toId` and `toType` to use explicit values.

Use `fields[people]=name` to select attributes. `toLinks` and `toMeta` add per-resource information. Collections and pages keep JSON array data and pagination links. The document always uses the protocol’s `data` key.

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

For data you already have as a JSON object, set the resource's associated `Model` type to `serde_json::Map<String, Value>`. The default identifier reads a string or integer `id`. Return custom attributes from `toAttributes` when you need to restrict the fields exposed by the source.

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

Return a `JsonApiAttributes` declaration from `lazyAttributes` to calculate fields after applying the sparse fieldset. Declare related resources with `JsonApiRelationships::toOne` or `toMany` in `toRelationships`. Their closures run for requested `include` paths. Resolvers can use relationships loaded by the query or by automatic loading.

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
    JsonApiRelationships::default().toOne("editor", move || {
        Ok(editor.map(PersonResource))
    })
}
```

### Automatic Relationship Loading {#jsonapi-automatic-loading}

Map each allowed JSON:API path to a path registered in `Model::namedRelations` by implementing `includePath`. Return the model from `modelMut`. When a route returns a resource or a collection, including a page, the framework batches missing relationships before serialization. The resolvers registered in `toRelationships` read the loaded results. For manual response construction, call `toResponseAsync(request).await`.

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

    fn modelMut(&mut self) -> Option<&mut Self::Model> {
        Some(&mut self.0)
    }

    fn includePath(path: &str) -> Option<&'static str> {
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
    .maxRelationshipDepth(3);

    container.instance(Arc::new(settings)).expect("register JSON:API settings");
}
```

Relationship paths default to a depth of five. The framework truncates longer paths before loading and projecting relationships. A depth of zero or one keeps only direct relationships from the request.

On a resource, return `false` from `respectFieldsAndIncludesInQueryString` to ignore both query parameters. Return `true` from `includePreviouslyLoadedRelationships` to include declared direct relationships that are already loaded. In this mode, sparse fields are still applied unless disabled, and the query cannot trigger relationship loading.
