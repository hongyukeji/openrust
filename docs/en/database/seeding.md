# Seeding

## Writing Seeders

Use `make:seeder` to generate a Rust seeder in `database/seeders`. Register its `SeederEntry` in `database/seeders/mod.rs` and rebuild the application. The `run` method receives a `SeederContext` with the selected connection and access to application services.

```sh
./artisan make:seeder CatalogSeeder
```

### Calling Additional Seeders

`DatabaseSeeder` is the default entry point. Use `call` to run child seeders in order. `callSilent` suppresses child progress output. `callOnce` skips a seeder that already completed in the current invocation. Use `callWith` to pass values that the child reads from `parameters()`.

The following seeder assumes a `plans` table with a unique `code` column and a `label` column. Its upsert makes repeat execution update the same row.

```rust
use openrust::database::{
    DatabaseFuture, DatabaseResult, query::Record,
    seeder::{Seeder, SeederContext, SeederEntry},
};

pub struct CatalogSeeder;

impl Seeder for CatalogSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let row = Record::from([
                ("code".into(), "standard".into()),
                ("label".into(), "Standard plan".into()),
            ]);
            context.connection().table("plans")
                .upsert(&[row], &["code"], Some(&["label"]))
                .await?;
            Ok(())
        })
    }
}

pub struct DatabaseSeeder;

impl Seeder for DatabaseSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(context.call(&["CatalogSeeder"]))
    }
}

pub const ENTRIES: &[SeederEntry] = &[
    SeederEntry::new("DatabaseSeeder", &DatabaseSeeder),
    SeederEntry::new("CatalogSeeder", &CatalogSeeder),
];
```

## Running Seeders

Use `db:seed` to run the default seeder. Specify a class with `--class` or the positional class argument. Use `--database` to select a connection. Within the seeder, `DB` resolves that connection by default without changing other tasks' connection selection.

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
