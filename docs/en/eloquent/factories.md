# Eloquent: Factories

## Defining Model Factories

Use `Factory::new` to define the model’s default attributes. Implement `HasFactory` on the model if you want to call `Flight::factory()`. Generate a factory under `database/factories` with the command below, then declare its Rust module and supply the definition.

```sh
./artisan make:factory FlightFactory --model=Flight
```

## Creating Models Using Factories

Apply `state` in declaration order. A `sequence` receives the zero-based index for the current batch. `make` builds the model and runs `afterMaking`. `create` saves it, creates its declared children, then runs `afterCreating`. Calling `count` switches the result to an Eloquent collection. Factory attributes bypass mass-assignment filtering and still pass through the model’s setters and casts.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, factories::Factory},
    seeder::SeederContext,
};

async fn seed_flights<M: Model>(context: &SeederContext<'_>) -> DatabaseResult<()> {
    Factory::<M>::new(|| Attributes::from([("name".into(), "Flight".into())]))
        .sequence(|index| Attributes::from([("name".into(), format!("Flight {index}").into())]))
        .count(3)
        .createOn(context.connection())
        .await?;
    Ok(())
}
```

## Factory Relationships

Use `has` for a typed `HasMany`, `r#for` for a new `BelongsTo` owner, `forExisting` for a previously created owner, and `hasAttached` for many-to-many records and their pivot attributes. A factory for a new owner runs once per generated child, including during `make`. Writes are sequential. If a later relation fails, earlier writes can remain.
