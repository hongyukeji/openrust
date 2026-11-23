# Eloquent: Mutators & Casting

## Accessors and Mutators

Declare an `Attribute` in `Model::attribute` and use `get_attribute_value` and `set_attribute_value` to apply it. An accessor receives the raw value and the current raw attributes. A mutator returns the columns to store. Mass assignment through `fill` uses the same mutators after checking fillable fields.

```rust
fn attribute(name: &str) -> Option<Attribute> {
    if name == "label" {
        return Some(Attribute::get(|value, _| match value {
            Some(Value::Text(label)) => Ok(label.to_uppercase().into()),
            _ => Ok(Value::Null),
        }));
    }
    None
}
```

```rust
use openrust::database::{DatabaseResult, Value, eloquent::Model};

pub fn set_record_label(record: &mut impl Model, label: &str) -> DatabaseResult<Option<Value>> {
    record.set_attribute_value("label", label)?;
    record.get_attribute_value("label")
}
```

## Attribute Casting

Declare built-in conversions in `Model::casts`. Boolean, integer, float, string and JSON reads leave SQL null as null. Accessors take precedence over built-in casts. JSON assignment accepts `Value::Json` and encodes scalar values as JSON. Use `get_raw_original` to inspect the persisted snapshot. Existing `get_attribute` and `set_attribute` provide raw access.

### Exact Decimals and Dirty Attributes

Use `AttributeCast::Decimal(scale)` for a fixed-scale decimal string with half-up rounding. Pass decimal input as text to retain its digits. `is_dirty` and `get_dirty` on the model compare converted values. If the converted values are equivalent, `save` skips that column. Changes are written in their raw storage form.

```rust
use openrust::database::{DatabaseResult, Value, eloquent::Model};

pub fn adjust_amount(record: &mut impl Model, amount: &str) -> DatabaseResult<(Option<Value>, bool)> {
    record.set_attribute_value("amount", amount)?;
    Ok((record.get_attribute_value("amount")?, record.is_dirty()?))
}
```

### Date Casting

Use `AttributeCast::DateTime` for a date-time, `Date` for the start of the local day, and `Timestamp` to read Unix seconds. Date and date-time reads produce UTC RFC 3339 strings with microseconds. Date assignments use the application connection’s `app.timezone` and the model’s `date_format` storage format, expressed in Chrono syntax. The default storage format is `%Y-%m-%d %H:%M:%S`.

```rust
use openrust::database::{DatabaseResult, Value, eloquent::Model};

pub fn change_occurred_at(record: &mut impl Model, date: &str) -> DatabaseResult<Option<Value>> {
    record.set_attribute_value("occurred_at", date)?;
    record.get_attribute_value("occurred_at")
}
```

### Query Time Casting

Use `with_casts` to apply casts to models returned or created by a query. This is useful for selected aliases such as `selected_at`. An override takes precedence over the model's declared cast for the same attribute. Use `merge_casts` to change casts on a single model. Collection `fresh` uses the first model's casts and reads a new projection, so aliases from the original query must be selected again to be present.

```rust
use openrust::database::{
    Connection, DatabaseResult, Value,
    eloquent::{Model, casts::AttributeCast},
    query::Expression,
};

pub async fn selected_date<M: Model>(db: &Connection) -> DatabaseResult<Option<Value>> {
    let Some(record) = M::query_on(db)
        .select([Expression::new("occurred_at AS selected_at")])
        .with_casts([("selected_at", AttributeCast::DateTime)])
        .first()
        .await?
    else {
        return Ok(None);
    };
    record.get_attribute_value("selected_at")
}
```

### Enum and Custom Casting

Register an enum with `EnumCast::of` and return its case name, string backing, or integer backing from `into_value`. Use `get_enum::<Status>("status")` and `set_enum("status", Status::Published)` for typed access. `get_attribute_value` returns the canonical backing value or case name. Assign `Value::Null` to clear the attribute.

Implement `CastsInboundAttributes` for assignment and `CastsAttributes` for custom reads. Return `CastWrite::Value` for one column or `CastWrite::Patch` for several. The optional `compare` method compares stored values for dirty tracking. Return `CASTS` from `Model::casts` or pass its entries to `with_casts`.

```rust
use openrust::database::{
    DatabaseError, DatabaseResult, Value,
    eloquent::{
        Attributes,
        casts::{AttributeCast, CastWrite, CastableEnum, CastsInboundAttributes, EnumCast},
    },
};

#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    Draft,
    Published,
}

impl CastableEnum for Status {
    fn from_value(value: &Value) -> DatabaseResult<Self> {
        match value {
            Value::Text(name) if name == "Draft" => Ok(Self::Draft),
            Value::Text(name) if name == "Published" => Ok(Self::Published),
            _ => Err(DatabaseError::InvalidQuery("status case")),
        }
    }

    fn into_value(self) -> Value {
        match self {
            Self::Draft => "Draft".into(),
            Self::Published => "Published".into(),
        }
    }
}

struct Trimmed;

impl CastsInboundAttributes for Trimmed {
    fn set(&self, _: &str, value: Value, _: &Attributes) -> DatabaseResult<CastWrite> {
        match value {
            Value::Text(text) => Ok(CastWrite::Value(text.trim().into())),
            Value::Null => Ok(CastWrite::Value(Value::Null)),
            _ => Err(DatabaseError::InvalidQuery("code must be text")),
        }
    }
}

pub const CASTS: &[(&str, AttributeCast)] = &[
    ("status", AttributeCast::Enum(EnumCast::of::<Status>())),
    ("inbound_code", AttributeCast::Inbound(&Trimmed)),
];
```

### Incrementing custom casts {#incrementing-custom-casts}

Implement the optional `CastsInboundAttributes::increment` hook for a counter with custom storage units. Return the next logical value as `Some`, or `None` to use SQL arithmetic on the raw column. The framework passes the logical value and step through `set`. The stored step must be numeric and the counter column must support SQL addition.

```rust
use openrust::database::{
    DatabaseError, DatabaseResult, Value,
    eloquent::{Attributes, casts::{CastWrite, CastsInboundAttributes}},
};

pub struct Hundredths;

impl CastsInboundAttributes for Hundredths {
    fn set(&self, _: &str, value: Value, _: &Attributes) -> DatabaseResult<CastWrite> {
        let Value::Integer(value) = value else {
            return Err(DatabaseError::InvalidQuery("integer counter"));
        };
        let stored = value.checked_mul(100)
            .ok_or(DatabaseError::InvalidQuery("counter overflow"))?;
        Ok(CastWrite::Value(stored.into()))
    }

    fn increment(
        &self, name: &str, step: &Value, attributes: &Attributes,
    ) -> DatabaseResult<Option<Value>> {
        let (Some(Value::Integer(stored)), Value::Integer(step)) =
            (attributes.get(name), step) else {
                return Err(DatabaseError::InvalidQuery("integer counter"));
            };
        let next = (stored / 100).checked_add(*step)
            .ok_or(DatabaseError::InvalidQuery("counter overflow"))?;
        Ok(Some(next.into()))
    }
}
```

### Encrypted Casting

Return `CASTS` from `Model::casts`. The cast uses the encrypter from the model connection's application. Use a `TEXT` column for the ciphertext. Read and write plaintext through `get_attribute_value` and `set_attribute_value`. An SQL null stays null.

`Array`, `Json`, and `Object` expose JSON values. `Collection` can also be read through `get_encrypted_collection`. Configure `APP_PREVIOUS_KEYS` to read data written with earlier keys. Assigning an attribute and saving writes it with the current key. Reading an old value alone leaves its ciphertext in place.

```rust
use openrust::database::{
    DatabaseResult, Value,
    eloquent::{Model, casts::{AttributeCast, EncryptedCast}},
};

pub const CASTS: &[(&str, AttributeCast)] = &[
    ("secret", AttributeCast::Encrypted(EncryptedCast::String)),
    ("settings", AttributeCast::Encrypted(EncryptedCast::Array)),
];

pub async fn change_secret(
    model: &mut impl Model,
    secret: &str,
) -> DatabaseResult<Option<Value>> {
    model.set_attribute_value("secret", secret)?;
    model.save().await?;
    model.get_attribute_value("secret")
}
```

### Enum Collections and Value Objects

Declare `AsEnumCollection::of::<Status>()` to preserve keys or `AsEnumArrayObject::of::<Status>()` to reindex values on assignment. `get_enum_collection` reads typed cases, and `set_enum_collection` writes their backing values or names. SQL null remains null. Use `edit_enum_collection` to modify a typed collection before writing it back.

Register `ValueObjectCast::of::<T>()` for a type that implements `CastableObject`. `get_value_object` borrows the cached object. Use `edit_value_object` to change a copy and apply the resulting column patch after validation. The model persists the changes on `save`. This example assumes the model declares casts for `left_code` and `enum_list`.

```rust
use openrust::{
    database::{
        DatabaseResult,
        eloquent::{Model, casts::{CastableEnum, CastableObject}},
    },
    support::Collection,
};

pub async fn update_codes<M: Model, T: CastableObject>(
    record: &mut M,
    edit: impl FnOnce(&mut T) -> DatabaseResult<()>,
) -> DatabaseResult<bool> {
    record.edit_value_object("left_code", edit)?;
    record.save().await
}

pub async fn update_enum_list<M: Model, E: CastableEnum>(
    record: &mut M,
    values: Collection<E>,
) -> DatabaseResult<bool> {
    record.set_enum_collection("enum_list", values)?;
    record.save().await
}
```

### Encrypted Classes and JSON Paths

Register `AsEncryptedArrayObject::cast()` or `AsEncryptedCollection::cast()` for an encrypted JSON column. Read the typed collection with `get_encrypted_collection` or update it with `edit_encrypted_collection`. Array objects require an array or keyed object when decoded.

Use `AsEncryptedCollection::of::<Member>()` to project each item through a Serde type, `map` for an item callback, or `using` to build the collection before applying an optional callback. The projection runs after decryption and supplies the collection and model JSON while preserving keys during item mapping. Direct assignments encrypt the supplied JSON. `edit_encrypted_collection` writes the edited projection back, so use it with a mapping that accepts the edited values on subsequent reads.

```rust
use openrust::database::eloquent::casts::{AsEncryptedCollection, AttributeCast};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct Member {
    name: String,
}

pub const MEMBER_CAST: AttributeCast = AsEncryptedCollection::of::<Member>();
```

Assign nested values with `setAttributeValue("settings->notifications->enabled", true)` or `fill_json_attribute`. The model updates the whole column while preserving sibling fields. For encrypted columns it decrypts first and writes new ciphertext. List each accepted path in `fillable` when using mass assignment.

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub async fn enable_notifications(record: &mut impl Model) -> DatabaseResult<bool> {
    record.fill_json_attribute("settings->notifications->enabled", true)?;
    record.save().await
}
```

### Custom JSON Codecs {#custom-json-codecs}

Register `Json` in the application's container before using models. Set the encoder with `encode_using` and the decoder with `decode_using`. The codec applies to stored JSON attributes, collections, enums, `AsFluent`, and JSON values inside encrypted attributes. It also applies to nested JSON updates. SQL `NULL` retains its usual cast behavior. Model `to_json()` and HTTP responses serialize the decoded values with the regular JSON serializer.

Callbacks receive a Serde JSON value or UTF-8 text. A custom encoder must produce valid JSON for a native JSON column. The database may normalize that text before the decoder reads it.

```rust
use std::sync::Arc;

use openrust::{
    container::Container,
    contracts::container::{BindingResolutionError, Container as _},
    database::{DatabaseError, eloquent::casts::Json},
};

pub fn configure_json(container: &Container) -> Result<(), BindingResolutionError> {
    let codec = Json::default()
        .encode_using(|value| {
            serde_json::to_string_pretty(value)
                .map_err(|_| DatabaseError::InvalidQuery("model JSON encoding"))
        })
        .decode_using(|text| Json::default().decode(text));

    container.instance(Arc::new(codec))?;
    Ok(())
}
```

### Castables

Implement `Castable::cast_using` to construct the caster from typed arguments. Register `ClassCast::of::<C>(&ARGUMENTS)` for a two-way cast or `ClassCast::inbound::<C>(&ARGUMENTS)` for assignment only. The arguments are static model declarations and can also be selected with `with_casts` or `merge_casts`.

Set `CastableObject::WITHOUT_OBJECT_CACHING` to `true` when each read should construct a new object. `get_value_object` returns a borrowed or owned value through `Cow`.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, casts::{Castable, CastsAttributes, ClassCast}},
};

pub async fn save_scaled<C: Castable<Arguments = i64>>(
    record: &mut impl Model,
    factor: &'static i64,
    value: i64,
) -> DatabaseResult<bool>
where
    C::Caster: CastsAttributes,
{
    record.merge_casts([("factory_value", ClassCast::of::<C>(factor))]);
    record.set_attribute_value("factory_value", value)?;
    record.save().await
}
```

### Array Object and Collection Casting

Declare `AsArrayObject::cast()` or `AsCollection::cast()` for a JSON array or keyed object. Use `get_json_collection`, `set_json_collection`, and `edit_json_collection` to work with ordered keys. By default, assigning null stores the JSON literal `null`. Use `nullable()` to store SQL NULL.

Use `AsCollection::of::<Item>()` to decode and project each item through Serde. Use `AsCollection::map` for a callback or `AsCollection::using` to build the collection before optional mapping. `get_json_collection_as::<Item>` reads the projected items as Rust values.

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub async fn add_option(record: &mut impl Model) -> DatabaseResult<bool> {
    record.edit_json_collection("json_collection", |items| {
        items.put("enabled", serde_json::json!(true));
        Ok(())
    })?;
    record.save().await
}
```

### Binary and Vector Casting

Declare `AsBinary::uuid()` or `AsBinary::ulid()` for a binary column. The database stores 16 bytes and model serialization returns the canonical identifier string. To declare a custom format, pass `BinaryFormat::Custom(BinaryCodec::new(encode, decode))` to `AsBinary::of`.

Declare `AsVector::cast()` to store a numeric vector as JSON text on SQLite or MySQL. The cast returns a numeric JSON array and can decode little-endian float32 bytes from a binary column. Native vector SQL operations require a driver with vector support.

```rust
use openrust::database::{DatabaseResult, Value, eloquent::Model};

pub async fn store_embedding(
    record: &mut impl Model,
    uuid: &str,
    embedding: serde_json::Value,
) -> DatabaseResult<bool> {
    record.set_attribute_value("binary_uuid", uuid)?;
    record.set_attribute_value("embedding", Value::Json(embedding))?;
    record.save().await
}
```

### Hashed Casting

Declare `AttributeCast::Hashed` for `password` in the model's `casts()`. `create` and `update` apply the cast when they assign attributes. To assign a plaintext value directly, use `set_attribute_value_async` so the application's hash service can run the work off the async executor.

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub async fn change_password(record: &mut impl Model, password: &str) -> DatabaseResult<bool> {
    record.set_attribute_value_async("password", password).await?;
    record.save().await
}
```

Existing hashes that pass the configured driver's validation are preserved. `null` passes through without calling the hash service. The synchronous setter accepts validated hashes and `null`. Declare the attribute as `hidden` to omit it from model JSON.

`force_fill(attributes)?` bypasses mass-assignment guards while still applying mutators and casts. For plaintext hashed attributes use `force_fill_async(attributes).await?`. Both retain the owning application's hash configuration; the default assignment path leaves attributes unchanged on failure and restores the guard state. The bypass is local to the current call/task. Pass only attributes you intend to assign without mass-assignment protection.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model},
};

pub async fn force_change_password(
    record: &mut impl Model,
    password: &str,
) -> DatabaseResult<bool> {
    record
        .force_fill_async(Attributes::from([("password".into(), password.into())]))
        .await?;
    record.save().await
}
```

### Stringable Casting

Declare `AsStringable::cast()` for `label` in the model's `casts()`. A loaded value is available as `openrust::support::Stringable` through `get_value_object`. Assign a plain string with `set_attribute_value` or a typed string with `set_value_object`.

```rust
use openrust::{
    database::{DatabaseResult, eloquent::Model},
    support::Stringable,
};

pub async fn trim_label(record: &mut impl Model) -> DatabaseResult<bool> {
    let Some(label) = record.get_value_object::<Stringable>("label")? else {
        return Ok(false);
    };
    let label = label.into_owned().trim();
    record.set_value_object("label", Some(label))?;
    record.save().await
}
```

The cast preserves whitespace during assignment and reads SQL `NULL` as `None`. Model JSON contains a string or `null`. The example explicitly trims the typed value before saving it.

### Fluent Casting

Declare `AsFluent::cast()` for `options` in the model's `casts()`. Assign an array or object as `Value::Json` and read it as `openrust::support::Fluent` through `get_value_object`. `get` reads dot paths, `value` reads exact keys, and `has` recognizes keys containing explicit `null`.

```rust
use openrust::{
    database::{DatabaseError, DatabaseResult, eloquent::Model},
    support::Fluent,
};

pub async fn enable_feature(record: &mut impl Model) -> DatabaseResult<bool> {
    record.edit_value_object::<Fluent>("options", |options| {
        options
            .set("features.preview", serde_json::json!(true))
            .map_err(|_| DatabaseError::InvalidQuery("options path"))?;
        Ok(())
    })?;
    record.save().await
}
```

Use `edit_value_object` to apply an edit and write its result to the model before saving. SQL `NULL` reads as `None`. An existing JSON scalar or malformed document returns an error when read as `Fluent`. Text columns retain the written key order. Native JSON columns follow the order returned by the database.

### HTML String Casting {#html-string-casting}

Declare `AsHtmlString::cast()` in `Model::casts()` to read an HTML attribute as an `HtmlString`. `to_html()` returns the original markup. The database and model JSON store it as text, and SQL `NULL` reads as `None`. Assign a typed `HtmlString` with `set_value_object`, or assign plain text with `set_attribute_value`. Treat markup from untrusted sources as text when rendering.

```rust
use openrust::{
    database::{DatabaseResult, eloquent::Model},
    support::HtmlString,
};

fn update_body(model: &mut impl Model) -> DatabaseResult<()> {
    model.set_value_object("body", Some(HtmlString::new("<strong>OpenRust</strong>")))?;
    Ok(())
}
```

### URI Casting

Declare an attribute with `AsUri::cast()` in `Model::casts()`. Text assignments keep their original database value. Reading and serializing the attribute parse and normalize the URI. SQL NULL remains null. Both absolute URIs and relative references are supported. Invalid text can be corrected with another assignment.

```rust
use openrust::{
    database::{DatabaseResult, eloquent::Model},
    support::Uri,
};

fn update_link(model: &mut impl Model) -> DatabaseResult<Option<String>> {
    let previous = model
        .get_value_object::<Uri>("link")?
        .map(|uri| uri.to_string());
    model.set_value_object("link", Some(Uri::of("../orders?state=open").unwrap()))?;
    Ok(previous)
}
```
