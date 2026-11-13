# Eloquent Serialization

## Serializing Models and Collections

Use `toArray()` for a JSON-compatible map and `toJson()` for a JSON string. Both include loaded relationships. Relationship names use snake case in the result. Unloaded relationships stay out of the result. Use `attributesToArray()` to include only attributes. A collection produces an array of its serialized models.

## Hiding Attributes From JSON

Declare `hidden` or `visible` in `ModelMetadata`. A nonempty `visible` list limits the output to the listed names. Hidden names are excluded. Use `makeVisible`, `makeHidden`, `setVisible` or `setHidden` to change one model. For a relationship, use its original name in these lists.

## Appending Values to JSON

Declare accessor names in `ModelMetadata::appends` or call `append` on a model. Appended names follow the same visibility rules. This example assumes the model defines a `summary` accessor.

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub fn public_json(record: &mut impl Model) -> DatabaseResult<String> {
    record.makeHidden(&["label"]).append(&["summary"]);
    record.toJson()
}
```

## Date Serialization

Date casts serialize to UTC RFC 3339 strings with microseconds by default. Override `serializeDate` to change model output. `ModelMetadata::dateFormat` controls database storage. A custom cast can implement `serialize` to project its converted value.

Use `AttributeCast::DateWithFormat("%Y/%m/%d")` or `DateTimeWithFormat("%H:%M %:z")` for an individual attribute. These formats use Chrono syntax and the model connection's application time zone. The format applies to serialized output and leaves attribute reads and database storage formats independent.

## Returning Models from Routes

A route can return a model, collection, or database result directly. The router uses the model's serialization rules. A newly created model returns 201. An existing model or a collection returns 200, and `findOrFail` maps a missing model to 404.

```rust
use openrust::{database::eloquent::Model, routing::RoutingError, support::facades::Route};

pub fn register_model_routes<M: Model>() -> Result<(), RoutingError> {
    Route::get("/records", M::all)?;
    Route::get("/records/1", || M::findOrFail(1))
}
```

## Conditional Visibility

Use `makeVisibleIf` and `makeHiddenIf` with a boolean or a predicate that receives the current model. A false condition leaves visibility as it was.

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub fn conditional_json(record: &mut impl Model, may_read: bool) -> DatabaseResult<String> {
    record.makeVisibleIf(may_read, &["label"]);
    record.makeHiddenIf(!may_read, &["label"]);
    record.toJson()
}
```

## Collection Visibility and Appends

On an Eloquent collection, visibility and append methods apply to each model. `mergeHidden` and `mergeVisible` extend the corresponding lists. `setAppends` replaces the append list, and `withoutAppends` clears it, including model defaults. Appended fields still follow each model’s visibility rules.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, Model},
};

pub fn public_json<M: Model>(records: &mut Collection<M>) -> DatabaseResult<String> {
    records.mergeHidden(&["internal_note"]).withoutAppends().toJson()
}
```
