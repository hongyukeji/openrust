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
