# Text translation

Enable the optional package and configure a default profile in the application's configuration loader under the translate key. Register external translators with the manager during provider registration.

    use openrust_translate::{Translate, TranslationRequest};

    async fn example() -> openrust_translate::TranslationResult<String> {
        let request = TranslationRequest::new("Bonjour", "en")?.with_source("fr")?;
        Ok(Translate::translate_request(request).await?.into_text())
    }

The independent Lang vocabulary lookup is available through the application even when this package is disabled.
