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

## Built-in drivers

Set the `driver` field of a profile as shown below. The `api_key_env` field holds an environment variable name. With a cached configuration, provide the secret in the environment of the HTTP server, Artisan, and each Worker. Each profile uses a 10-second timeout unless `timeout_ms` is set.

| Driver | Profile settings | Language mapping |
| --- | --- | --- |
| `deepl` | Set `plan` to `free` (default) or `pro` and provide `api_key_env`. An optional `base_url` overrides the official Free or Pro base address. The `driver` appends `/v2/translate`. | Regional and script-specific targets include `en-GB`, `en-US`, `pt-BR`, `pt-PT`, `zh-Hans`, and `zh-Hant`. Use a base language for an explicit source. |
| `libretranslate` | Provide the self-hosted `base_url` and optionally `api_key_env`. The `driver` appends `/translate`. | The traditional Chinese and Brazilian Portuguese models use the `zt` and `pb` protocol codes for `zh-Hant` and `pt-BR`. Installed models determine the available pairs. |
| `tencent` | Set `api_key_env` and the TokenHub translation model, such as `hy-mt3-plus`. An optional `base_url` selects a different TokenHub site. | Uses `/v1/api/translations` and sends `stream=false`. Maps `zh-Hant` to `zh-TR` and omits source for automatic detection. |
| `aliyun` | Set `access_key_id_env` and `access_key_secret_env`. Use `security_token_env` with temporary credentials. Select `region` if needed (default `cn-hangzhou`). | Calls `TranslateGeneral` with an ACS3-signed plain-text form. Maps `zh-Hant` to `zh-tw` and uses auto for automatic detection. |
| `baidu` | Set `app_id_env` and `api_key_env` for the general text translation service. | Signs the original UTF-8 text before form encoding. Maps `zh-Hant` to `cht`, ja to `jp`, and ko to `kor`. Joins returned segments in order with newlines. |
| `azure` | Set `api_key_env` for the subscription key. Set `region` for regional and multi-service resources. `base_url` may include the path prefix of a custom resource endpoint. | Requests `/translate` with `api-version=3.0` and one `to`. Omits `from` for detection. Preserves `zh-Hans` and `zh-Hant`. |
| `google` | Set `project` and `location` (default `global`). `quota_project` and `base_url` are optional. Use Google Application Default Credentials in the process environment. | Uses Cloud Translation Advanced v3 with `text/plain`. Maps `zh-Hans` and `zh-Hant` `to` `zh-CN` and `zh-TW`. Omits `sourceLanguageCode` for detection. |

```json
{
    "default": "office",
    "drivers": {
        "office": {"driver": "deepl", "plan": "pro", "api_key_env": "DEEPL_API_KEY"},
        "local": {"driver": "libretranslate", "base_url": "http://127.0.0.1:5000"}
    }
}
```

Use HTTPS for remote endpoints. HTTP is available on loopback for a local service. Each translation sends one POST and leaves redirects untouched. After a timeout the supplier may have processed the request.

The Google driver uses the official Rust authentication library. Provide a trusted `GOOGLE_APPLICATION_CREDENTIALS` file or an attached identity with the Cloud Translation permissions required by your application. Keep credential files outside app configuration. The library caches OAuth access tokens and refreshes them when needed.
