# API Resources

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

## Resource Collections

Pass a vector of resources to `ResourceCollection::new`. Each item is transformed for the current request, and an empty collection returns `{"data":[]}`. Item metadata and response hooks apply when the item is the outermost response. Use `with`, `additional`, or `withResponse` on the outer resource to add metadata or modify its response.

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
