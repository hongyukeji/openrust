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

## Artisan translations {#artisan-translations}

Artisan uses `APP_LOCALE` for list headings, help and argument errors. Built-in text is available in English, Simplified Chinese and Traditional Chinese. An application can override text in `lang/<locale>/console.json`. Lookup uses the active locale and then `APP_FALLBACK_LOCALE` before built-in text. A command without a translated description uses its registered description.

```sh
APP_LOCALE=en ./artisan help list
APP_LOCALE=zh_CN ./artisan list make
APP_LOCALE=zh_Hant ./artisan help list
```

To customize help, use keys such as `options`, `commands.list.description` and `commands.list.arguments.namespace` inside the console group. See [Artisan output formats](./artisan.md#command-help-and-output-formats).

Production confirmations and cancellation messages use the same translation group. Override the message with `confirm.production` or `confirm.cancelled` in `console.json`. Answer with `y` or `yes` to proceed. An empty answer declines.
