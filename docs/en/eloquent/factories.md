# Eloquent: Factories

## Defining Model Factories

Use `Factory::new` to define the model’s default attributes. Implement `HasFactory` on the model if you want to call `Flight::factory()`. Generate a factory under `database/factories` with the command below, then declare its Rust module and supply the definition.

```sh
./artisan make:factory FlightFactory --model=Flight
```

## Creating Models Using Factories

Apply `state` in declaration order. A `sequence` receives the zero-based index for the current batch. `make` builds the model and runs `afterMaking`. `create` saves it, creates its declared children, then runs `afterCreating`. Calling `count` switches the result to an Eloquent collection. Factory attributes bypass mass-assignment filtering and still pass through the model’s setters and casts.

For a counted batch, all models are constructed before `afterMaking` runs. `create` then saves each model and its declared relations. The batch runs `afterCreating` after those writes finish. If `afterMaking` fails, the batch itself has yet to be saved. Owner factories resolved during construction may already have written records.

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

Use `has` with `HasOne`, `HasMany`, `MorphOne` or `MorphMany`. Use `r#for` to generate a `BelongsTo` or `MorphTo` owner, or `forExisting` to associate an existing model. A batch resolves each generated owner once, including when using `make`. A new invocation resolves a new owner. A `MorphTo` descriptor must include the owner target and its mapped alias. `hasAttached` accepts a `BelongsToMany` or `MorphToMany` descriptor and writes the pivot attributes. Writes are sequential. If a later relation fails, earlier writes can remain.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, factories::Factory, relations::MorphTo},
    seeder::SeederContext,
};

pub async fn seed_comments<C: Model, P: Model>(
    context: &SeederContext<'_>,
    comments: Factory<C>,
    posts: Factory<P>,
    subject: MorphTo<C>,
) -> DatabaseResult<()> {
    comments.r#for(subject, posts).count(3).createOn(context.connection()).await?;
    Ok(())
}
```

Child factories apply relation query defaults after the definition and before explicit states. Direct and polymorphic child relations then supply the foreign key and discriminator before `afterMaking`. Attached model factories also see the relation query defaults before their explicit states.

### Recycling Existing Models {#recycling-existing-models}

Call `recycle` with a saved model or `recycleMany` with an iterable of saved models of one type. Call either method before or after `count`. The pool follows nested factory relationships. For each generated owner, the factory randomly selects a matching model of the requested type on the same connection, or creates one if the pool has no match. A batch reuses its selected owner.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, Model, factories::Factory, relations::BelongsTo},
    seeder::SeederContext,
};

pub async fn reuse_owner<C: Model, P: Model>(
    context: &SeederContext<'_>,
    children: Factory<C>,
    owner: P,
    relation: BelongsTo<C, P>,
    owners: Factory<P>,
) -> DatabaseResult<Collection<C>> {
    children.r#for(relation, owners).count(3).recycle(owner)
        .createOn(context.connection()).await
}
```
