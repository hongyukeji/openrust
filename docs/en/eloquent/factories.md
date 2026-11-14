# Eloquent: Factories

## Defining Model Factories

Use `Factory::new` to define the model’s default attributes. Implement `HasFactory` on the model if you want to call `Flight::factory()`. Generate a factory under `database/factories` with the command below, then declare its Rust module and supply the definition.

```sh
./artisan make:factory FlightFactory --model=Flight
```

### Localized Fake Data {#localized-fake-data}

Use `Factory::withFaker` to define generated model attributes. It reads `app.faker_locale` from the application owning the selected connection. Use `fake(Some("fr_FR"))` to select a locale explicitly, or `fake(None)` to use the current application. The Rust generator uses fake-rs data and exposes `name`, `safeEmail`, `city`, and `sentence`. Unsupported locales use English data.

```rust
use openrust::database::eloquent::{Attributes, Model, factories::Factory};

pub fn profile_factory<M: Model>() -> Factory<M> {
    Factory::withFaker(|faker| {
        Attributes::from([
            ("name".into(), faker.name().into()),
            ("email".into(), faker.safeEmail().into()),
        ])
    })
}
```


Use `faker.unique(false)` to avoid duplicates per formatter. `unique(true)` clears that generator’s history. With `Factory::tryWithFaker`, a batch shares one generator, and an exhausted formatter returns an error before the batch is saved. Separate factory calls start with fresh history. For example, use `faker.unique(false).safeEmail()?` inside the definition.

Other built-in formatters include `firstName`, `lastName`, `freeEmail`, `userName`, `streetName`, `postcode`, `phoneNumber`, `company`, and `word`. The same methods are available through `unique`. The selected locale supplies sample data where fake-rs has it.

Use `numberBetween(first, second)` for an inclusive integer range in either order. `randomDigit()` returns 0 through 9, and `randomDigitNotNull()` returns 1 through 9. `dateTimeBetween(start, end)` takes explicit UTC date-times, samples at whole-second precision including both endpoints, and returns an error when the start second follows the end second. All four formatters support `unique`.

```rust
use chrono::DateTime;
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, factories::Factory},
};

pub fn activity_factory<M: Model>() -> Factory<M> {
    Factory::tryWithFaker(|faker| -> DatabaseResult<Attributes> {
        let start = DateTime::UNIX_EPOCH;
        let end = DateTime::from_timestamp(86_400, 0).unwrap();
        let happened_at = faker.dateTimeBetween(start, end)?;
        Ok(Attributes::from([
            ("score".into(), faker.numberBetween(1, 100).into()),
            ("happened_at".into(), happened_at.to_rfc3339().into()),
        ]))
    })
}
```

## Creating Models Using Factories

Apply `state` in declaration order. A `sequence` receives the zero-based index for the current batch. `make` builds the model and runs `afterMaking`. `create` saves it, creates its declared children, then runs `afterCreating`. Calling `count` switches the result to an Eloquent collection. Factory attributes bypass mass-assignment filtering and still pass through the model’s setters and casts.

Use `prependState` to run a transformation before all currently registered states. Each transformation sees the attributes produced so far. Use `set(name, value)` to append a single attribute override. Both work before or after `count`. Attributes passed directly to a creation method are applied after the registered states.

For a counted batch, all models are constructed before `afterMaking` runs. `create` then saves each model and its declared relations. The batch runs `afterCreating` after those writes finish. If `afterMaking` fails, the batch itself has yet to be saved. Owner factories resolved during construction may already have written records.

Use `raw()` or `rawOn(connection)` to get expanded attributes before model setters and casts. A counted factory returns a vector of attribute maps. Raw expansion skips the target factory’s callbacks and child relationships. It can still persist a declared parent to resolve its key. Use `createQuietly()` or `createQuietlyOn(connection)` to persist models and relationships while suppressing Eloquent model events. Factory callbacks still run.

Use `createOne(attributes)` to create one model even on a counted factory. Use `createMany(records)` to create records one at a time, in order. Pass `FactoryRecords::Default` for the configured count (one on an uncounted factory), `FactoryRecords::Count(n)` for an explicit count, or `FactoryRecords::Attributes(vec![…])` for per-record overrides. The overrides apply after factory states. Each record runs both factory callbacks before the next one starts. If a later record fails, earlier writes can remain. Use `createOneQuietly` and `createManyQuietly` to suppress model events, or the corresponding `On` methods to select a connection.

Use `makeOne(attributes)` to build one unsaved model, or `makeMany(records)` to build unsaved models in order. Both run `afterMaking` and accept explicit attributes. The `On` variants take a connection. `lazy(attributes)` returns an async callback. Await a call to create the model or counted batch using the connection resolved at that time. Calling it again creates a fresh result.

Use `forEachSequence(states)` to apply one attribute map per model and set the batch count to the number of maps. `crossJoinSequence(sequences)` combines the maps from each input sequence in order, with the last sequence changing fastest. Later maps override earlier maps on matching keys. Set the count separately to select how many combinations to use. The sequence repeats if the count exceeds the number of combinations. An empty input sequence returns an error.

Use `insert(attributes)` on a single or counted factory to bulk-insert the generated rows, or `insertOn(connection, attributes)` to select the connection explicitly. The factory runs `afterMaking` and resolves any required parent keys. The bulk write skips target model persistence events, child creation and `afterCreating`. It writes storage attributes, including hidden columns, without adding appended values or applying output accessors.

Use `withoutAfterMaking()` or `withoutAfterCreating()` on a single or counted factory to clear the corresponding callbacks already registered on that factory. You can register new callbacks afterward. The other callback phase and Eloquent model events still run.

Use `connection(Some("archive"))` to select a named connection on a factory or counted batch. `getConnectionName()` returns the configured override, and `connection(None)` clears it. Operations resolve the override first, then the model connection, then the current application or Seeder default. The `On` methods use the connection supplied for the target factory. Nested factories use their own override or model connection before inheriting the enclosing connection. Relationship connection checks still apply.

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
    subject: &str,
) -> DatabaseResult<()> {
    comments.forNamed::<_, MorphTo<C>>(subject, posts).count(3)
        .createOn(context.connection()).await?;
    Ok(())
}
```

Child factories apply relation query defaults after the definition and before explicit states. Direct and polymorphic child relations then supply the foreign key and discriminator before `afterMaking`. Attached model factories also see the relation query defaults before their explicit states.

Use `hasNamed`, `forNamed`, or `hasAttachedNamed` to resolve a relationship by name. The factory uses a descriptor registered through `Model::namedRelations` or the application’s `resolveRelationUsing`. Pass the Rust descriptor type explicitly, for example `forNamed::<_, MorphTo<Comment>>("subject", posts)`. Unknown names and incompatible descriptor types return errors.

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

Pass a vector of attribute maps to `hasAttached` or `hasAttachedNamed` to apply pivot attributes in related-model order. The sequence cycles when the related factory count exceeds its length. Use `FactoryPivot::resolve` to compute attributes for each attachment from the saved parent model. An attribute map applies the same values to all attachments.
