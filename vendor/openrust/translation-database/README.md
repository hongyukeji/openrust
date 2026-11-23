# Database translations

`DatabaseLoader` reads the `language_lines` table with Spatie's `group`, `key`, and multilingual `text` JSON structure. It implements the existing OpenRust `Loader`, overlays file translations, and publishes through the existing translator. Applications manage writes and explicitly choose when to reload. Package namespaces remain file based.

See the [localization guide](../../../docs/en/digging-deeper/localization.md#database-translation-loader) for loading, limits, locale fallback, and error behavior. Database translation loading supports PostgreSQL 18, MySQL 8.4, and SQLite; other databases require separate behavior validation.

Create the table in an application migration:

```rust
use openrust::database::{Connection, DatabaseResult};

pub async fn create_translations(connection: &Connection) -> DatabaseResult<()> {
    connection.getSchemaBuilder().create("language_lines", |table| {
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
    translation::{FileLoader, TranslationResource, TranslationResult},
};
use openrust_translation_database::DatabaseLoader;

pub async fn load_database_translations(
    connection: Connection,
    language_directory: PathBuf,
) -> TranslationResult<()> {
    let source = DatabaseLoader::new(connection)
        .withTable("language_lines")?
        .withMaxRows(10_000)?
        .withFallback(Arc::new(FileLoader::new(language_directory)));
    Lang::loadFrom(
        &source,
        &[
            TranslationResource { locale: "fr", group: "messages", namespace: None },
            TranslationResource { locale: "en", group: "messages", namespace: None },
        ],
    ).await
}
```

The package does not register a provider or run migrations. Reads are explicit and asynchronous; translation lookups use the published catalog without SQL.

The schema and loading behavior follow `spatie/laravel-translation-loader` 2.8.3 (MIT). Upstream copyright and license are preserved in `LICENSE.spatie.md`. Locale fallback stays in OpenRust's translator; missing tables fail an explicit reload.
