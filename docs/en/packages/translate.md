# Text Translation

Enable the `translate` feature in the application’s default Cargo features, keeping its existing defaults. The package adds the `Translate` facade and asynchronous `Translator` contract for single plain-text requests. Application strings continue to use `Lang` and local language resources.

## Configuring profiles

Use the `translate` key in the application’s configuration loader. `default` selects a named profile. Each profile’s `driver` selects a registered factory. `TRANSLATE_DRIVER` can select the `default` while building configuration. The example below declares a profile that adds a label for testing.

```rust
let profiles = serde_json::json!({
    "default": "preview",
    "drivers": {
        "preview": {"driver":"example","label":"Preview:"}
    }
});
```

## Registering a driver

Implement `Translator` in your application or a dependency. Register an instance of the provider below in the application’s provider list. The example driver echoes labeled text to demonstrate the contract.

```rust
use std::sync::Arc;

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use openrust_translate::{
    TranslateManager, Translation, TranslationError, TranslationFuture,
    TranslationRequest, Translator,
};

struct Preview(String);

impl Translator for Preview {
    fn translate(&self, request: TranslationRequest) -> TranslationFuture<'_> {
        Box::pin(async move {
            Ok(Translation::new(format!("{} {}", self.0, request.text())))
        })
    }
}

struct AppTranslationProvider;

impl ServiceProvider for AppTranslationProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().make::<TranslateManager>()?.extend("example", |profile| {
                let label = profile["label"]
                    .as_str()
                    .ok_or(TranslationError::InvalidConfiguration("label"))?;
                Ok(Arc::new(Preview(label.to_owned())))
            });
            Ok(())
        })
    }
}
```

## Translating text

Omit `with_source` to ask the driver to detect the source language. Pass canonical BCP 47 tags such as `en-US` or `zh-Hans`. The request preserves surrounding whitespace in nonblank text. Results contain translated text and, when available, the language reported by the driver.

```rust
use openrust_translate::{Translate, TranslationRequest, TranslationResult};

async fn preview() -> TranslationResult<String> {
    let request = TranslationRequest::new("你好", "en")?.with_source("zh-Hans")?;
    let default = Translate::translate("你好", "en").await?;
    let named = Translate::driver("preview")?.translate(request).await?;
    assert_eq!(default.text(), named.text());
    Ok(named.into_text())
}
```

Each application caches one instance per profile. After changing a configuration or replacing a factory, call `Translate::manager()?.forget_drivers()` to resolve future handles from the new configuration. Existing handles keep their previous settings. Rebuild the application’s configuration cache after changing its environment or configuration loader.
