# Seeding

## Writing Seeders

Use `make:seeder` to generate a Rust seeder in `database/seeders/`. The generator writes a named `ENTRY: SeederEntry` into the file. Handwritten seeders expose the same constant. During a build, OpenRust discovers these files, including files in subdirectories, and passes their entries to `withSeeders` in `bootstrap/app.rs`. Each Rust file in that tree supplies one entry, with `mod.rs` reserved for module declarations. The `run` method receives a `SeederContext` with the selected connection and access to application services.

Artisan temporarily disables Eloquent mass-assignment protection while a seeder and its child seeders run. Methods such as `Model::create` and `fill` can assign guarded attributes and still apply setters and casts. The scope ends on success or failure. Independent Tokio tasks, including tasks started with `tokio::spawn`, keep their own protection.

```sh
./artisan make:seeder CatalogSeeder
```

### Calling Additional Seeders

`DatabaseSeeder` is the default entry point. Use `call` to run child seeders in order. `callSilent` suppresses child progress output. `callOnce` skips a seeder that already completed in the current invocation. Use `callWith` to pass values that the child reads from `parameters()`.

Use `callWithMany(names, parameters)` to pass the same values to multiple seeders in order. Use `callSilentWith(names, parameters)` to suppress their progress lines, or `callOnceWith(names, silent, parameters)` to skip previously successful seeders and choose whether to show progress. Each seeder reads the values with `parameters()`. An error stops the list. A failed seeder can be retried because it has yet to complete successfully.

The following seeder assumes a `plans` table with a unique `code` column and a `label` column. Its upsert makes repeat execution update the same row.

Put the child seeder in `database/seeders/catalog_seeder.rs`.

```rust
use openrust::database::{
    DatabaseFuture, DatabaseResult, query::Record,
    seeder::{Seeder, SeederContext, SeederEntry},
};

pub(super) const ENTRY: SeederEntry = SeederEntry::new("CatalogSeeder", &CatalogSeeder);

pub struct CatalogSeeder;

impl Seeder for CatalogSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let label = context.parameters().get("label").cloned()
                .unwrap_or_else(|| "Standard".into());
            let row = Record::from([
                ("code".into(), "standard".into()),
                ("label".into(), label),
            ]);
            context.connection().table("plans")
                .upsert(&[row], &["code"], Some(&["label"]))
                .await?;
            Ok(())
        })
    }
}
```

In `database/seeders/database_seeder.rs`, call it by its registered name.

```rust
use openrust::database::{
    DatabaseFuture, DatabaseResult, query::Record,
    seeder::{Seeder, SeederContext, SeederEntry},
};

pub(super) const ENTRY: SeederEntry = SeederEntry::new("DatabaseSeeder", &DatabaseSeeder);

pub struct DatabaseSeeder;

impl Seeder for DatabaseSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(context.callOnceWith(
            &["CatalogSeeder"],
            true,
            Record::from([("label".into(), "Standard plan".into())]),
        ))
    }
}
```

## Running Seeders

Use `db:seed` to run `DatabaseSeeder`, which can call other seeders explicitly. Specify a class with `--class` or the positional class argument. For a seeder generated as `Admin/CatalogSeeder`, use `--class='Admin\CatalogSeeder'`. Use `--database` to select a connection. Within the seeder, `DB` resolves that connection by default without changing other tasks' connection selection.

```sh
./artisan db:seed
./artisan db:seed --class=CatalogSeeder
./artisan migrate:fresh --seed --seeder=CatalogSeeder
```

You can also seed after `migrate --seed` or `migrate:refresh --seed`. Use `--seeder` to choose a specific class. A seeder error stops the remaining calls and returns a failing exit code.

### Forcing Seeders to Run in Production

Production runs prompt for confirmation. Use `--force` when the deployment has already approved the data changes.

```sh
./artisan db:seed --force
```

## Using Model Factories {#using-model-factories}

See [Eloquent Factories](../eloquent/factories.md#factory-model-binding) for model binding, states, callbacks and relationship factories.

The application ships a default `UserFactory` in `database/factories`. After migrating an empty database, `./artisan db:seed` uses it to create the verified sample account `test@example.com` with the password `password`. The model hashes the password using the selected application configuration. The default seeder uses the selected database and mutes model events during the run. Repeating the default seeder against the same account returns a duplicate-email error.

Pass the seeder’s selected connection to `createOn`, including when running `db:seed --database=…`. The following function can be called from a seeder’s `run` method.

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

## Muting Model Events {#muting-model-events}

Call `withoutModelEvents()` on the file's `ENTRY` to mute Eloquent model events while it runs. Child seeders called from it inherit the scope. Model events resume when it returns, including on error. Seeder progress output and factory callbacks still run.

```rust
use openrust::database::seeder::SeederEntry;

pub(super) const ENTRY: SeederEntry =
    SeederEntry::new("DatabaseSeeder", &DatabaseSeeder).withoutModelEvents();
```
