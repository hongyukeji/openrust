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

## Eloquent Model Conventions

The defaults are the `id` primary key, an incrementing identifier, and `created_at` and `updated_at` timestamps. Use `primaryKey`, `incrementing`, `timestamps`, or `connection` on `ModelMetadata` to configure the model. Declare the columns that may be mass assigned with `fillable`.

## Retrieving Models

In an application request or command, use `Flight::query()?` for a model-aware builder, `Flight::all().await?` for all scoped models, or `Flight::find(id).await?` for one primary key. `findOrFail` returns `DatabaseError::ModelNotFound` for an absent model. Use `queryOn` to select an explicit database connection.

## Inserting and Updating Models

`create` applies mass-assignment rules and saves a new model. `setAttribute` updates an individual attribute and `save` writes dirty attributes using the original primary key. A successful write updates the model snapshot. Builder `update` and `delete` return affected-row counts.

## Deleting Models

Enable soft deletes with `softDeletes("deleted_at")`. Model queries exclude soft-deleted rows by default. Use `withTrashed` or `onlyTrashed` to include them, `restore` to clear the deletion timestamp, and `forceDelete` to remove a row permanently.

## Query Scopes

Implement `Scope<Model>` to reuse a query constraint. Apply it with `withScope`, or register it in the model’s `globalScopes`. Remove a named global scope with `withoutGlobalScope`. This example uses the `Flight` model above and a table with the declared columns and timestamps.

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
