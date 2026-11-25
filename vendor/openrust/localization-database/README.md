# Database localization

`DatabaseLoader` reads the `language_lines` table with Spatie's `group`, `key`, and multilingual `text` JSON structure. It implements the existing OpenRust `Loader`, overlays file translations, and publishes through the current localizer. Applications manage writes and explicitly choose when to reload. Package namespaces remain file based.

See the [localization guide](../../../docs/en/digging-deeper/localization.md#database-translation-loader) for loading, limits, locale fallback, and error behavior. Database translation loading supports PostgreSQL 18, MySQL 8.4, and SQLite; other databases require separate behavior validation.

Create the table in an application migration:

```rust
use openrust::database::{Connection, DatabaseResult};

pub async fn create_translations(connection: &Connection) -> DatabaseResult<()> {
    connection.get_schema_builder().create("language_lines", |table| {
        table.id();
        table.string("group", None).index();
        table.string("key", None);
        table.json("text");
        table.timestamps();
    }).await
}
```

Write a multilingual row with the existing query builder:

```no_run
use openrust::database::{Connection, DatabaseResult, Value};
use serde_json::json;

async fn store(connection: &Connection) -> DatabaseResult<()> {
    connection.table("language_lines").insert(&[[
        ("group".into(), Value::from("messages")),
        ("key".into(), Value::from("welcome")),
        ("text".into(), Value::Json(json!({"en":"Hello :name","fr":"Bonjour :name"}))),
    ].into()]).await?;
    Ok(())
}
```

```no_run
use std::{path::PathBuf, sync::Arc};
use openrust::{
    database::Connection,
    support::facades::Lang,
    localization::{FileLoader, TranslationResource, TranslationResult},
};
use openrust_localization_database::DatabaseLoader;

pub async fn load_database_translations(
    connection: Connection,
    language_directory: PathBuf,
) -> TranslationResult<()> {
    let source = DatabaseLoader::new(connection)
        .with_table("language_lines")?
        .with_max_rows(10_000)?
        .with_fallback(Arc::new(FileLoader::new(language_directory)));
    Lang::load_from(
        &source,
        &[
            TranslationResource { locale: "fr", group: "messages", namespace: None },
            TranslationResource { locale: "en", group: "messages", namespace: None },
        ],
    ).await
}
```

The package provider exposes the `localization-database-migrations` publication tag. Run the published migration before enabling the database source. Existing applications keep their original migration and table. Register your `DatabaseLoader` as `lang.loader` in the application provider and declare its exact groups in `lang.resources`. The application reads these groups at startup and on `Lang::refresh_default_source().await`. Synchronous lookups use the published catalog without SQL.

The schema and loading behavior follow `spatie/laravel-translation-loader` 2.8.3 (MIT). Upstream copyright and license are preserved in `LICENSE.spatie.md`. Locale fallback stays in OpenRust's localizer; missing tables fail an explicit reload.
