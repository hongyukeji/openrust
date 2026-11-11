# Localization

## Configuring the Locale

Set `APP_LOCALE` for the application default and `APP_FALLBACK_LOCALE` for missing group translations. Use `Lang::withLocale` around a request or task operation to select another locale. The previous locale is restored when the operation ends.

## Defining Translation Strings

Put grouped translations in files such as `lang/en/messages.json`. Retrieve nested entries with dot notation. Use `lang/en.json` for translations whose keys are complete phrases. When `resources/lang` exists, it is used as the application language directory.

```json
{
  "welcome": "Hello, :Name",
  "items": "{0} No items|{1} One item|[2,*] :count items"
}
```

## Retrieving Translation Strings

Use `__` to retrieve text. Placeholders support `:name`, `:Name` and `:NAME`. Use `Lang::array` for a group or list. A missing key is returned as text. Literal JSON keys are looked up in the selected locale. Grouped keys can fall back to the configured language.

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
