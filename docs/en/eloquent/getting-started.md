# Eloquent: Getting Started

## Generating Model Classes

Run `./artisan make:model Flight` to generate `app/models/flight.rs`. Declare `mod flight;` in `app/models/mod.rs`, expose the `models` module from `app/mod.rs`, and rebuild. Use `--force` to replace the generated file. A Rust model declares a table in `ModelMetadata` and holds a `ModelState` for each instance.

```rust
use openrust::database::eloquent::{Model, ModelMetadata, ModelState};

pub struct Flight(ModelState);

impl Model for Flight {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("flights")
            .fillable(&["name", "active"])
            .softDeletes("deleted_at");
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }

    fn state(&self) -> &ModelState {
        &self.0
    }

    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}
```

`getFillable()` and `getGuarded()` expose the declared mass-assignment lists. `Model::isUnguarded()` reports the current bypass scope; `Model::unguarded(future)` restores guards when the future completes, fails, or is cancelled, and does not affect other tasks. `getDeletedAtColumn()` returns the configured soft-delete column as `Option<&str>`, or `None` when soft deletes are disabled.

## Eloquent Model Conventions

The defaults are the `id` primary key, an incrementing identifier, and `created_at` and `updated_at` timestamps. Use `primaryKey`, `incrementing`, `timestamps`, or `connection` on `ModelMetadata` to configure the model. Declare the columns that may be mass assigned with `fillable`.

### UUID and ULID keys {#uuid-and-ulid-keys}

Choose `UniqueId::Uuid` for UUIDv7 or `UniqueId::Ulid` for lowercase ULIDs. By default the primary key is generated and treated as a non-incrementing string. Use `uniqueIds` to choose the generated columns. On insertion, Eloquent fills empty values after `Saving` and before `Creating`, and preserves supplied non-empty values. Replication drops these columns so a new insert can generate fresh identifiers. Model binding returns 404 for malformed identifiers on these columns.

```rust
use openrust::database::eloquent::{ModelMetadata, UniqueId};

static TICKET: ModelMetadata = ModelMetadata::new("tickets")
    .uniqueId(UniqueId::Uuid)
    .uniqueIds(&["id", "reference"]);
```

### Refreshing attributes after writes {#refreshing-attributes-after-writes}

Declare database-managed columns with `ModelMetadata::refreshes`. After an insert or an update that writes changes, Eloquent reads those columns from the write connection before calling `Created` or `Updated` listeners. Explicit transactions read back on the same transaction. Replication omits these columns so the database can compute their values for the new row.

```rust
use openrust::database::eloquent::ModelMetadata;

static ARTICLE: ModelMetadata = ModelMetadata::new("articles")
    .fillable(&["title"])
    .refreshes(&["title_length"]);
```

## Retrieving Models

In an application request or command, use `Flight::query()?` for a model-aware builder, `Flight::all().await?` for all scoped models, or `Flight::find(id).await?` for one primary key. `findOrFail` returns `DatabaseError::ModelNotFound` for an absent model. Use `queryOn` to select an explicit database connection.

### Model Field References (Extension) {#model-field-references}

Generate model field references from an existing database table without declaring each field. Run your application migrations first, then create a model or refresh an existing one:

```sh
./artisan make:model User --fields
# Refresh fields while preserving existing model business code
./artisan model:fields User
# Custom table, connection, and nested model directory
./artisan model:fields Admin/User --table=accounts --database=pgsql
```

For a new model, declare its module in the parent `mod.rs` as instructed by `make:model`. The generated region includes a model type entry. After rebuilding, `model:fields` reads the table and connection through an empty model's `getTable()` and `resolveConnectionName()`. Single-model `--table` and `--database` options override this invocation without changing the model's business configuration. During creation, both options are saved in the new model metadata; `--database` requires `--fields`. Published custom model stubs use the `{{connection}}` placeholder to receive the connection setting.

On the first refresh of an unregistered existing model, the command infers a pluralized snake case table and uses the application default connection. Supply custom table and connection settings explicitly for that first refresh, then rebuild to enable configuration reuse. Rebuild the command program after changing model tables, connections, or module declarations. The command does not parse uncompiled Rust source.

```sh
# Select multiple models explicitly
./artisan model:fields User Admin/User
# Synchronize compiled application models with automatic fields enabled
./artisan model:fields --all
# CI check: 0 for matching output, 1 for stale output; no model source writes
./artisan model:fields --all --check
```

`--all` includes compiled models with generated regions under the conventional `app/models/` modules. It does not scan every database table or discover arbitrary trait implementations or package models. On first adoption, upgrades of old generated regions, or adding models, refresh each model, declare its module, and rebuild. Batch synchronization uses each model's settings and rejects shared table or connection overrides. All models are preflighted before individual atomic replacements. Preflight failure leaves models unchanged; an I/O failure during writing can leave some files updated, so fix the error and rerun. `--check` creates no source locks or source temporary files. Missing tables, invalid connections, and damaged markers also return nonzero with a specific error. Application startup logging and caching retain their usual behavior.

Then use IDE completion directly:

```rust
let mut query = User::query()?;
query.r#where(User::NAME, "=", "Zhang San");
let users = query.orderBy(User::ID, "asc").get().await?;
```

The current database columns are the source for the generated collection. `fillable` and `casts` continue to control mass assignment and attribute conversion independently. Commands write an `openrust:model-fields begin/end` region in the model file. Keep the generated source in version control: ordinary builds, requests, and deployment do not need a database connection. Do not edit the generated region manually. After migrations add, remove, or rename columns, synchronize again, fix affected references, and rebuild. Without synchronization, the compiler cannot detect external database changes. Dynamic models and string queries remain available. Plain `make:model` neither accesses the database nor generates field declarations.

Column names become uppercase snake case constants while retaining their actual database names: `display-name` becomes `DISPLAY_NAME`, and `1st` becomes `COLUMN_1ST`. Collisions, non-ASCII column names, or names that cannot form an identifier cause generation to fail without changing the original file. Missing tables, invalid connections, and damaged generation markers also leave the model intact. Special columns can use strings or explicit mappings through the advanced `model_fields!` macro. Remove duplicate handwritten declarations when migrating to generation; the command does not delete handwritten constants.

Constants have type `&'static str`. References to missing constants, including those removed by synchronization, fail to compile. **This does not enforce query model ownership, query value types, or database structure at build time.** NULL, numbers, dates, JSON, custom casts, and expressions retain their existing behavior without additional value type guarantees.

Column names are unqualified. Use `select([User::NAME])`, attribute maps such as `[(User::NAME.into(), value)].into()` for updates, and constants directly for relationship keys. For joins or aliases, pass `format!("u.{}", User::NAME)` through the existing query interface. The grammar still handles identifiers and values remain bound parameters. Do not concatenate constants into raw SQL such as `whereRaw`. Dynamic columns and raw expressions keep their existing entry points.

Database column comments are generated as Rust documentation attributes (`#[doc = "…"]`, equivalent to `///`), so IDE hover shows the field description. MySQL and PostgreSQL provide these comments through the existing schema interface; SQLite does not store native column comments. Missing or blank comments produce no description. Refresh after changing column comments; `--check` also detects these changes. Descriptions are read from the database, without guessing from field names or translating them during generation.

### Refreshing models {#refreshing-models}

`fresh()` reads a separate instance. `freshWithPaths(&["posts.comments"])` also preloads registered paths. `refresh()` updates the current instance and reloads its loaded registered relations. Both use the model’s original key and write connection, bypassing global scopes and including soft-deleted rows. An unsaved model yields `None` from `fresh()` and stays as it is after `refresh()`. For a missing database row, `refresh()` returns `ModelNotFound`. Register relationships in `namedRelations()` to allow `refresh()` to reload them. Intermediate Pivot projections are retained. If reloading a relationship fails, the current attributes contain the values just read and the original snapshot stays at its previous values.

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub async fn reload<M: Model>(model: &mut M) -> DatabaseResult<Option<M>> {
    let copy = model.fresh().await?;
    model.refresh().await?;
    Ok(copy)
}
```

Use `refreshForUpdate(&mut transaction)` to refresh a model while holding a MySQL row lock until the caller commits or rolls back. Start the transaction on the model’s connection. Loaded relationships on that connection and their retrieval events can see uncommitted changes in the transaction. Relationships on other connections use their own connection. Custom relation implementations can provide `matchResultsInTransaction` to participate. SQLite reads through the transaction and follows SQLite’s transaction locking rules.

```rust
use openrust::database::{DatabaseResult, Transaction, eloquent::Model};

pub async fn reload_locked<M: Model>(
    model: &mut M,
    transaction: &mut Transaction,
) -> DatabaseResult<()> {
    model.refreshForUpdate(transaction).await?;
    Ok(())
}
```

### Cursors {#cursors}

Call `cursor()` to hydrate one model per row as the stream is polled. Model scopes, casts and the `Retrieved` event still apply. For eager-loaded relations, use a batched query. The cursor holds a database connection while it is being read. If a retrieved listener makes a query, it needs a separate available connection.

```rust
use futures_util::TryStreamExt as _;
use openrust::database::{DatabaseResult, eloquent::Model};

async fn first_model<M: Model>() -> DatabaseResult<Option<M>> {
    let mut models = M::query()?.cursor();
    models.try_next().await
}
```

### Chunking Results {#chunking-results}

On an Eloquent query, `chunk(count, callback)` supplies a model collection for each page. If you provided no ordering, Eloquent orders by the primary key. Each batch runs model retrieval events, eager loads and the model collection factory before the callback. See the [query builder’s chunking rules](../database/queries.md#chunking-results).

Eloquent also supports `chunkById` and `chunkByIdDesc`. Passing `None` selects the model’s primary key. The seek value comes from the SQL projection, so include the key or its selected alias. Each batch still runs the Eloquent retrieval and eager-loading lifecycle.

With Eloquent `each`, the callback receives the model and its `ArrayKey` from the current batch, including custom collection keys. `eachById` adds the batch offset to the integer collection key and reports an error for keys that cannot be represented as signed integers.

Eloquent’s lazy methods hydrate each batch and run eager loading before yielding models. Each batch releases its database connection before consumption proceeds. `chunkMap` limits how many input models are read at a time, and its result collection contains every mapped value.

## Inserting and Updating Models

`create` applies mass-assignment rules and saves a new model. `setAttribute` updates an individual attribute and `save` writes dirty attributes using the original primary key. A successful write updates the model snapshot. Builder `update` and `delete` return affected-row counts.

### Updating counters {#updating-counters}

Use `increment`, `decrement`, `incrementEach`, or `decrementEach` to update numeric attributes atomically. The multi-column methods take a map of columns to amounts. They return the affected-row count. On a persisted model, the operation fires `Updating` and `Updated`, and refreshes written attributes from the database. An unpersisted instance applies the update to its scoped query.

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model}};

pub async fn record_visit<M: Model>(model: &mut M) -> DatabaseResult<u64> {
    model.incrementEach(
        Attributes::from([("visits".into(), 1.into()), ("views".into(), 2.into())]),
        Attributes::new(),
    ).await
}
```

Use the `Quietly` variants to suppress model events for an instance update. Eloquent Builder exposes the same four arithmetic methods for scoped bulk updates. Bulk updates write the model’s update timestamp unless you supply one, and run without per-model events.

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model}};

pub async fn count_visit_quietly<M: Model>(model: &mut M) -> DatabaseResult<u64> {
    model.incrementQuietly("visits", 1_i64, Attributes::new()).await
}
```

Use `fillAndInsert(rows)` on an Eloquent query to apply model defaults, pending query attributes, setters and casts before a bulk insert. The builder generates missing unique IDs and timestamps, preserves explicit timestamps, and bypasses mass-assignment guards and model persistence events. Use `fillAndInsertOrIgnore(rows)` for the driver’s insert-ignore behavior and `fillAndInsertGetId(row)` for a single generated database ID. `fillForInsert(rows)` returns the prepared attributes without inserting them.

## Deleting Models

Enable soft deletes with `softDeletes("deleted_at")`. Model queries exclude soft-deleted rows by default. Use `withTrashed` or `onlyTrashed` to include them, `restore` to clear the deletion timestamp, and `forceDelete` to remove a row permanently.

## Replicating Models {#replicating-models}

Call `replicate(&[]).await?` to create an unsaved copy. The primary key, configured timestamps, and internal through key are excluded. Pass additional column names to omit them. Loaded relations are available on the copy and can be removed independently. The copy keeps the source database connection and receives the `Replicating` lifecycle event. Use `replicateQuietly` to skip that event. Call `save` to insert the copy.

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

async fn copy_without_secret<M: Model>(source: &M) -> DatabaseResult<M> {
    source.replicate(&["secret"]).await
}
```

### Strict Attribute Policies

Configure a model's policies for an application at startup with `Flight::shouldBeStrict(&app, true)?`. Use `preventLazyLoading(&app, enabled)`, `preventSilentlyDiscardingAttributes(&app, enabled)`, and `preventAccessingMissingAttributes(&app, enabled)` to configure them individually. Defaults are disabled. Policies belong to an application and model type; a detached model requires an application connection from `newInstanceOn` or an active application context to use them.

Strict mass assignment returns `DatabaseError::MassAssignment` when an attribute is discarded. Fields already assigned remain on the instance. `forceFill`, `forceFillAsync`, and `unguarded` retain their bypass behavior. Override `handleDiscardedAttributeViolationUsing` to handle the discarded names; returning success permits discarding them.

Use `getAttributeValue` for checked attribute reads. A missing attribute on a retrieved model returns `DatabaseError::MissingAttribute` when enabled; an explicitly loaded SQL NULL, an accessor, an unsaved model, or a recently created model follows its normal behavior. Override `handleMissingAttributeViolationUsing` to supply a value for an unknown attribute. For a primitive or enum cast, a successful handler permits the normal null cast. `getAttribute` and `getRawOriginal` remain raw `Option` reads. The lazy loading policy is captured when hydrating a result containing multiple models and is checked by implicit relationship access. Changing the default affects subsequent hydration; explicit model overrides of `preventsLazyLoading` still apply.

### Temporarily Disabling Relationship Touches

`Owner::withoutTouching(work).await` suppresses updates to owners of that model type during the future. To select multiple types, use `Owner::withoutTouchingOn(&[TypeId::of::<Owner>(), TypeId::of::<Team>()], work).await` after importing `std::any::TypeId`. `isIgnoringTouch()` reports whether the model's relationship timestamp updates are disabled, including models without an update timestamp. Saving the child still writes its own attributes and dispatches model events; related Saved events and further owner traversal keep their normal behavior. Nested scopes restore the previous setting after success, errors, panics, or cancellation and are isolated from other tasks.

## Query Scopes

Implement `Scope<Model>` to reuse a query constraint. Apply it with `withScope`, or register it in the model’s `globalScopes`. Remove a named global scope with `withoutGlobalScope`. This example uses the `Flight` model above and a table with the declared columns and timestamps.

`withGlobalScope(name, scope)` registers or replaces a scope on one query. It accepts a `Scope<M>` implementation or a `Send + Sync + 'static` closure returning `DatabaseResult<()>`. The scope's `extend` hook runs at registration; `apply` runs when scopes are applied. `withoutGlobalScopes()` removes the currently registered scopes; scopes registered afterwards still run. `withoutGlobalScopesExcept(&["tenant"])` retains only those names. `removedScopes()` preserves the order and repeated names of removal calls. Registrations belong to a query and do not change another query or model type.

Soft deletion uses the same registry. To retain its filter when removing other scopes, include `std::any::type_name::<SoftDeletingScope>()` in the retained names after importing `eloquent::SoftDeletingScope`. Conditions explicitly added by `onlyTrashed()` or `withoutTrashed()` remain even if all registered scopes are removed.

`applyScopes()` consumes the model builder and returns the builder with its scopes applied. Repeating it applies the registered scopes again. `getQuery()` consumes the model builder and returns its current base query without applying pending scopes; call `applyScopes()?.getQuery()` when those constraints are required. `setQuery(query)` replaces the base query and its previous conditions while retaining the model scopes and eager loads. `getEagerLoads()` borrows the typed `EagerLoads<M>` configuration, whose `names()` method lists registered names; `setEagerLoads(loads)` replaces that configuration, including an empty configuration. Override `Model::newEloquentBuilder(query)` and construct with `Builder::fromQuery(query)` to configure the builder used by public model queries. The factory returns the framework's typed builder; additional application methods use Rust traits or wrappers rather than runtime class names.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Attributes, Builder, Scope},
};

struct Active;

impl Scope<Flight> for Active {
    fn apply(&self, query: &mut Builder<'_, Flight>) -> DatabaseResult<()> {
        query.r#where("active", "=", true);
        Ok(())
    }
}

pub async fn save_flight(connection: &Connection) -> DatabaseResult<()> {
    let mut flight = Flight::queryOn(connection)
        .create(Attributes::from([
            ("name".into(), "Flight 42".into()),
            ("active".into(), true.into()),
        ]))
        .await?;
    flight.setAttribute("name", "Flight 42A");
    flight.save().await?;

    let mut flights = Flight::queryOn(connection);
    flights.withScope(&Active)?;
    assert_eq!(flights.get().await?.len(), 1);

    flight.delete().await?;
    assert!(flight.trashed());
    flight.restore().await?;
    Ok(())
}
```

## Writing models in a transaction {#model-transactions}

Pass the same mutable transaction to `saveInTransaction`, `updateInTransaction`, `deleteInTransaction`, `forceDeleteInTransaction` or `restoreInTransaction`. The caller controls commit and rollback. A model already bound to a connection must use a transaction from that connection. To make listener SQL participate in the transaction, override `ModelEventListener::handleInTransaction` and issue SQL through the supplied transaction. Existing connection-based callbacks continue to receive the pooled connection. A rollback reverts database writes. Discard or reload model instances that reflected rolled-back writes.

```rust
use openrust::database::{Connection, DatabaseResult, eloquent::Model};

async fn save_in_transaction<M: Model>(
    connection: &Connection,
    model: &mut M,
) -> DatabaseResult<bool> {
    let mut transaction = connection.beginTransaction().await?;
    match model.saveInTransaction(&mut transaction).await {
        Ok(true) => {
            transaction.commit().await?;
            Ok(true)
        }
        Ok(false) => {
            transaction.rollBack(None).await?;
            Ok(false)
        }
        Err(error) => {
            transaction.rollBack(None).await?;
            Err(error)
        }
    }
}
```

## Model events {#model-events}

Register a typed listener with `Model::registerModelEvent` during application boot. Listeners can change the model before the write or return `ModelEventResponse::Cancel` to cancel it. A successful save emits `Saved` even when the attributes are unchanged. Bulk builder updates and deletes do not emit instance events. Use `saveQuietly`, `deleteQuietly`, `forceDeleteQuietly`, `restoreQuietly`, or `Model::withoutEvents` to suppress model events in the current async call chain. An error from a post-write listener is returned to the caller. The database write has already run.

```rust
use openrust::{
    database::{
        Connection, DatabaseFuture, DatabaseResult,
        eloquent::{
            Model,
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
        },
    },
    foundation::Application,
};

struct RequireTitle;

impl<M: Model> ModelEventListener<M> for RequireTitle {
    fn handle<'a>(
        &'a self,
        _: ModelEvent,
        model: &'a mut M,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            Ok(match model.getAttribute("title") {
                Some(openrust::database::Value::Text(title)) if !title.trim().is_empty() => {
                    ModelEventResponse::Continue
                }
                _ => ModelEventResponse::Cancel,
            })
        })
    }
}

fn register<M: Model>(app: &Application) -> DatabaseResult<()> {
    M::registerModelEvent(app, ModelEvent::Saving, RequireTitle)
}
```

### Mapping model events {#mapping-model-events}

Override `dispatchesEvents` to build an event from the model at the current lifecycle point. Register listeners for the returned event through the [application dispatcher](../digging-deeper/events.md). A pre-write listener can return `ListenerResponse::Value(Box::new(false))` to cancel the write. An absent response falls through to regular model listeners and observers. A handled response takes precedence. For a transaction-aware save, the dispatcher receives that transaction. After-commit listeners receive the owned event payload captured when the lifecycle event fired.

```rust
use std::sync::Arc;
use openrust::{
    contracts::events::Event,
    database::{Value, eloquent::{Model, ModelMetadata, ModelState, concerns::ModelEvent}},
    events::NamedEvent,
};

struct Flight(ModelState);

impl Model for Flight {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("flights").fillable(&["name"]);
        &META
    }

    fn from_state(state: ModelState) -> Self { Self(state) }
    fn state(&self) -> &ModelState { &self.0 }
    fn state_mut(&mut self) -> &mut ModelState { &mut self.0 }

    #[allow(non_snake_case)]
    fn dispatchesEvents(&self, event: ModelEvent) -> Option<Arc<dyn Event>> {
        let (ModelEvent::Saved, Some(Value::Text(name))) = (event, self.getAttribute("name")) else {
            return None;
        };
        Some(Arc::new(NamedEvent::new("flight.saved", name.clone())))
    }
}
```

### Observers {#observers}

Run `./artisan make:observer FlightObserver --model=Flight` to generate `app/observers/flight_observer.rs` with the five post-write lifecycle methods. Declare the module and register the observer during provider boot. Omit `--model` to generate a plain observer struct, or use `--force` to replace an existing file. Use `./artisan stub:publish` to customize `observer.rs.stub` and `observer.plain.rs.stub`.

Group lifecycle methods in an `Observer<M>` and register it with `M::observe` during provider boot. List the lifecycle events in `EVENTS`; the framework registers only those methods. Observer handlers receive the model and the operation’s database connection. A pre-write handler can return `ModelEventResponse::Cancel`. The registry belongs to the application.

```rust
use openrust::{
    database::{
        Connection, DatabaseFuture, DatabaseResult,
        eloquent::{Model, Observer, concerns::{ModelEvent, ModelEventResponse}},
    },
    foundation::Application,
};

struct NormalizeTitle;

impl<M: Model> Observer<M> for NormalizeTitle {
    const EVENTS: &'static [ModelEvent] = &[ModelEvent::Saving];

    fn saving<'a>(
        &'a self,
        model: &'a mut M,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            if let Some(openrust::database::Value::Text(title)) = model.getAttribute("title") {
                let title = title.trim().to_owned();
                model.setAttribute("title", title);
            }
            Ok(ModelEventResponse::Continue)
        })
    }
}

fn register<M: Model>(app: &Application) -> DatabaseResult<()> {
    M::observe(app, NormalizeTitle)
}
```

#### Observers and database transactions {#observers-and-database-transactions}

Implement `ShouldHandleEventsAfterCommit` and register the observer with `Model::observeAfterCommit`. With explicit model transactions, the six pre-write callbacks still run immediately and can cancel the write. Other callbacks run after the outermost successful commit. A rollback discards the callbacks from its transaction or savepoint. Without a transaction the observer runs immediately. Rust requires a cloneable model and captures its state when the event fires. If a deferred callback fails, commit returns `DatabaseError::AfterCommit`; the database write is already committed.

```rust
use openrust::{
    contracts::events::ShouldHandleEventsAfterCommit,
    database::{
        Connection, DatabaseFuture, DatabaseResult,
        eloquent::{Model, Observer, concerns::{ModelEvent, ModelEventResponse}},
    },
    foundation::Application,
};

struct ArticleCommitted;

impl ShouldHandleEventsAfterCommit for ArticleCommitted {}

impl<M: Model> Observer<M> for ArticleCommitted {
    const EVENTS: &'static [ModelEvent] = &[ModelEvent::Created];

    fn created<'a>(
        &'a self,
        _model: &'a mut M,
        connection: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            connection.insert("INSERT INTO article_audit (action) VALUES (?)", &["created".into()]).await?;
            Ok(ModelEventResponse::Continue)
        })
    }
}

fn register<M: Model + Clone>(app: &Application) -> DatabaseResult<()> {
    M::observeAfterCommit(app, ArticleCommitted)
}
```

## Primary Keys and Single Values {#keys-and-single-values}

`whereKey`, `whereKeyNot`, `orWhereKey` and `orWhereKeyNot` accept a scalar, a `Vec` or an array. Integer-key lists use integer conditions; string-key lists use bound values. Empty positive lists match nothing and empty negative lists match everything. Key columns are qualified with the model table, and OR conditions remain grouped under global scopes. Models with string keys override `getKeyType()` to return `ModelKeyType::String`; scalar keys then use string conversion. To filter by a model instance, explicitly pass its `getKey()` value. `eloquent::Key` represents the scalar/list alternative when building dynamic conditions.

```rust
let mut query = User::query()?;
query.whereKey([1, 2, 3]).orWhereKeyNot(4);
let users = query.get().await?;
```

`value(column)` returns the first model's attribute or `Value::Null` for no row. `valueOrFail(column)` returns a model-not-found error for no row. `sole()` and `soleValue(column)` require exactly one row; zero rows return a model-not-found error and multiple rows return `DatabaseError::MultipleModelsFound`. These methods hydrate models and retain accessors, casts, retrieved events and global scopes. Existing selected columns are preserved; selecting only `id` does not implicitly fetch another requested attribute. A present row containing SQL NULL still returns `Value::Null` from `valueOrFail`.

## Saving and Deleting Variants {#write-variants}

`saveOrFail()` starts a transaction on the model's connection. `updateOrFail(attributes)` fills an existing model and saves it in a transaction; `deleteOrFail()` deletes an existing model in a transaction. A canceled event returns `false`; SQL or listener errors roll back writes and propagate. A canceled async task also releases the transaction through the existing rollback mechanism. Database rollback does not restore the model's in-memory attributes or existence flag; discard or reload affected instances. When sharing an outer transaction, use `saveInTransaction`, `updateInTransaction` or `deleteInTransaction` with that mutable transaction. Custom transaction behavior belongs in those typed hooks.

`updateQuietly(attributes)` fills an existing model, then saves with instance events suppressed. A model that does not exist returns `false` before filling for both update variants and before starting a transaction for `deleteOrFail`.

`saveOrIgnore(None)` inserts a new model and ignores unique conflicts; `Some(&["email"])` targets the named conflict columns. A conflict returns `false` without marking the model as existing or dispatching `Created` / `Saved`. Successful inserts receive their generated key and emit normal creation events. Calling it on an existing model returns an error. This method uses the driver's insert-ignore-returning support: PostgreSQL and SQLite support it; MySQL and SQL Server return an unsupported-operation error for nonempty inserts. It does not start a transaction or suppress listener errors. An empty model with timestamps disabled can return `true` without inserting a row.

`original` is synchronized only after successful post-save events and owner touches, using the final model attributes. A post-save listener failure leaves pending changes visible; a successful listener's attribute changes are included in that synchronization.
