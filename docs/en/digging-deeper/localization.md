# Localization

## Configuring the Locale

Set `APP_LOCALE` for the application default and `APP_FALLBACK_LOCALE` for missing group translations. Use `Lang::withLocale` around a request or task operation to select another locale. The previous locale is restored when the operation ends.

## Publishing Language Files {#publishing-the-language-files}

Run `./artisan lang:publish` to publish the English authentication, pagination, password reset, and validation JSON catalogs. The command writes to `lang/en` or to `resources/lang/en` when the application already uses `resources/lang`. It preserves existing files by default. Use `--force` to replace all four files, or `--existing` to replace only previously published files. When both flags are set, `--existing` determines the scope.

## Defining Translation Strings

Put grouped translations in files such as `lang/en/messages.json`. Retrieve nested entries with dot notation. Use `lang/en.json` for translations whose keys are complete phrases. When `resources/lang` exists, it is used as the application language directory.

```json
{
  "welcome": "Hello, :Name",
  "items": "{0} No items|{1} One item|[2,*] :count items"
}
```

### Registering Paths and Translation Lines {#registering-translation-sources}

Register directories through `Lang::addPath`, `Lang::addJsonPath`, or `Lang::addNamespace` in the application scope before the first lookup. Later paths override earlier paths. Grouped catalogs merge recursively, including list indexes. Literal JSON keys are replaced as a whole. Application roots are applied after JSON-only roots.

Use `Lang::addLines` with dotted group keys. A newly registered group starts with the supplied lines. To extend a file-backed group, call `Lang::load` first. Loaded groups are cached for the application, so later path registration applies to future loads.

```rust
use std::path::PathBuf;

use openrust::{
    support::facades::Lang,
    translation::{Line, TranslationResult},
};

pub fn register_translations(directory: PathBuf) -> TranslationResult<()> {
    Lang::addPath(directory)?;
    Lang::load(None, "messages", "en")?;
    Lang::addLines(
        [("messages.runtime".into(), Line::Text("Registered :name".into()))],
        "en",
        None,
    )
}
```

### Loading Groups From a Custom Source {#loading-custom-translation-sources}

Pass a `Loader` to `Lang::loadFrom` at an async application boundary. Each resource identifies one locale, group, and optional namespace. The returned lines replace the selected groups together. A loader error or a concurrent runtime write to one of those groups leaves the batch unpublished. Subsequent lookups stay synchronous.

```rust
use openrust::{
    contracts::translation::Loader,
    support::facades::Lang,
    translation::{TranslationResource, TranslationResult},
};

pub async fn load_notice(source: &dyn Loader) -> TranslationResult<()> {
    Lang::loadFrom(
        source,
        &[TranslationResource {
            locale: "en",
            group: "notices",
            namespace: None,
        }],
    )
    .await
}
```

### Database Translation Source (Extension) {#database-translation-loader}

Add `openrust-translation-database` to the application's Cargo dependencies using `path = "vendor/openrust/translation-database"`. `DatabaseLoader` accepts an explicit framework `Connection`; obtain it from the application's database manager or `DB::connection`. It reads translations from PostgreSQL 18, MySQL 8.4, and SQLite. Other database targets require separate behavior validation.

Create the table through an application migration before loading translations. This minimal schema is supported:

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

The schema follows Spatie’s Laravel Translation Loader: one row per group/key; `text` is a JSON object such as `{"en":"Hello :name","fr":"Bonjour :name"}`. Use canonical locale keys (`en_US`, `zh_CN`). A missing or null locale leaves the file value available. Translation values must be strings; empty strings are valid. Group `*` stores literal keys, including dots. Other groups expand dotted keys such as `nested.title` or `items.0`. Package namespaces use only the file source, matching Spatie. Duplicate or overlapping keys, invalid JSON, empty key segments, and keys deeper than 128 segments fail the load.

Call this function inside an asynchronous application scope, such as a provider's `boot`, after migrations are available:

```rust
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

The selected connection's table prefix applies. The table name is an unqualified identifier. The default table is `language_lines`, and the default maximum is 10,000 rows per group. Exceeding the configured limit fails the load without truncating the result.

`withFallback` loads the same locale/group/namespace from the file source and overlays database entries. Groups and list indexes merge recursively; literal JSON values are replaced as a whole. Empty database strings remain valid overrides. After deleting a row, explicitly reload its resource to restore the file value. The translator retains its configured language fallback; load database resources for both the selected and fallback languages when needed.

`Lang::loadFrom` publishes all requested groups together. Database, file, or parsing failures leave the previous published catalog available and return an error; missing rows produce an empty database overlay. Reads occur per group; publication atomicity does not provide a transaction spanning all database reads. Reloads are explicit. Applications manage writes, migrations, and refresh timing; the package does not automatically select a source or start background tasks.

Unlike Spatie’s PHP model, this source does not perform locale fallback while reading each row: OpenRust’s translator applies its configured fallback after loading exact locales. This also preserves file translations for the selected locale. A missing table returns an error so a failed explicit reload cannot silently replace a published database catalog with files.

## Retrieving Translation Strings

Use `__` to retrieve text. Placeholders support `:name`, `:Name` and `:NAME`. Use `Lang::array` for a group or list. A missing key is returned as text. Literal JSON keys are looked up in the selected locale. Grouped keys can fall back to the configured language.

### Fallback and Missing Keys {#translation-lookup-hooks}

Use `Lang::has(key, locale, true)` to check fallback translations and `Lang::hasForLocale(key, locale)` for an exact locale. Existence checks skip the missing-key handler. Register lookup hooks inside the application scope. The locale callback can reorder grouped lookups. A literal JSON match takes precedence. The missing-key handler can return replacement text, which then receives normal placeholder replacements. Returning `None` keeps the original key.

```rust
use openrust::{support::facades::Lang, translation::TranslationResult};

pub fn register_lookup_hooks() -> TranslationResult<()> {
    Lang::determineLocalesUsing(|locales| {
        let mut locales = locales.to_vec();
        if locales.first().is_some_and(|locale| locale == "fr_CA") {
            locales.insert(1, "fr".into());
        }
        locales
    })?;

    Lang::handleMissingKeysUsing(|missing| {
        (missing.key == "messages.welcome").then(|| "Hello, :name".into())
    })
}
```

### Replacing Parameters in Translation Strings {#replacing-parameters-in-translation-strings}

Use `Lang::getWith` to pass an ordered list of text, tags, and objects. A callback receives the contents of each matching `<name>…</name>` on the same line. Tag callbacks run in argument order before text placeholders are substituted. Use `Lang::arrayWith` to replace values within a group or list. `Lang::choiceWith` picks the plural form before applying replacements.

#### Object Replacement Formatting {#object-replacement-formatting}

Register a formatter for the concrete Rust type with `Lang::stringable` in the application scope, typically in `AppServiceProvider::boot`. Use `Replacement::object` when registration is required, or `Replacement::display` to use the type’s `Display` implementation when no formatter is registered. To use a Rust enum, pass the desired variant name or value explicitly, or register a formatter for the enum.

For example, define `payments.notice` in `lang/en/payments.json`.

```json
{"notice": "<mark>Paid</mark> :amount"}
```

```rust
use openrust::{
    support::facades::Lang,
    translation::{Replacement, TranslationResult},
};

pub struct Amount(pub u32);

pub fn register_formatters() -> TranslationResult<()> {
    Lang::stringable(|amount: &Amount| format!("{} USD", amount.0))
}

pub fn payment_notice(amount: &Amount) -> TranslationResult<String> {
    Lang::getWith(
        "payments.notice",
        &[
            ("amount", Replacement::object(amount)),
            ("mark", Replacement::tag(|text| format!("[{text}]"))),
        ],
        None,
    )
}
```

## Pluralization

Use `trans_choice` to select a form. Explicit conditions such as `{0}` or `[2,*]` are tried first. Otherwise, the selector uses the matching locale’s plural rules. It provides `:count` automatically.

```rust
use std::collections::BTreeMap;

use openrust::{
    support::facades::Lang,
    translation::{TranslationResult, __, trans_choice},
};

pub async fn summary(locale: &str) -> TranslationResult<(String, String)> {
    Lang::withLocale(locale, async {
        let replacements = BTreeMap::from([("name".into(), "ada".into())]);
        Ok((
            __("messages.welcome", &replacements)?,
            trans_choice("messages.items", 2_i64, &replacements)?,
        ))
    })
    .await?
}
```

Use lowercase language codes and underscores in application configuration, catalog names, and documentation paths. Use uppercase region codes and title-case script codes. Examples include `en`, `en_GB`, `fr_CA`, `es_419`, `sr_Latn_RS`, `zh_CN`, and `zh_TW`. Locale input accepts hyphens, underscores, and mixed case. `zh-Hans` resolves to `zh_CN` and `zh-Hant` resolves to `zh_TW`. Explicit locales such as `zh_HK` and `zh_Hant_HK` keep their own catalogs. HTML uses language tags such as `zh-CN` and `zh-TW`. Rename any existing catalogs to match the normalized identifiers.

## Overriding Package Language Files {#overriding-package-language-files}

To override a package translation such as `shop::cart.summary.heading`, define the same key in `lang/vendor/shop/en/cart.json`. Use the package namespace, locale, and group name from the original translation. Only the entries provided by the application are overridden. Other entries still come from the package.

## Artisan translations {#artisan-translations}

Artisan uses `APP_LOCALE` for list headings, help and argument errors. Built-in text is available in English, Simplified Chinese and Traditional Chinese. An application can override text in `lang/<locale>/console.json`. Lookup uses the active locale and then `APP_FALLBACK_LOCALE` before built-in text. A command without a translated description uses its registered description.

```sh
APP_LOCALE=en ./artisan help list
APP_LOCALE=zh_CN ./artisan list make
APP_LOCALE=zh_Hant ./artisan help list
```

To customize help, use keys such as `options`, `commands.list.description` and `commands.list.arguments.namespace` inside the console group. See [Artisan output formats](./artisan.md#command-help-and-output-formats).

Production confirmations and cancellation messages use the same translation group. Override the message with `confirm.production` or `confirm.cancelled` in `console.json`. Answer with `y` or `yes` to proceed. An empty answer declines.

## Locale contract methods

The Translator contract, concrete translator, and Lang facade expose `getLocale`, `setLocale`, `getFallback`, and `setFallback`. Boot-time changes configure application defaults; request/queue changes stay in the current scope. Nested `withLocale` restores its caller’s locale; fallback is set separately. Migrate old snake_case contract calls to these names.

## Locale Ownership

Locale and fallback getters return owned `String` snapshots. Setters take `&self` and return `TranslationResult<()>`; invalid locale identifiers return an error. Locale identifiers are canonicalized, for example `zh-CN` becomes `zh_CN`. Boot-time values form the defaults; changes during a request or queue task stay in that container scope. `withLocale` temporarily overrides only the active task and restores the enclosing locale after completion.
