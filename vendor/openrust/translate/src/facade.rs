use std::sync::Arc;

use openrust::{container::ServiceKey, support::facades::Facade};

use crate::{
    TranslateManager, Translation, TranslationError, TranslationRequest, TranslationResult,
    Translator,
};

pub struct Translate;

impl Facade for Translate {
    type Service = TranslateManager;

    fn get_facade_accessor() -> ServiceKey {
        "translator".into()
    }
}

impl Translate {
    pub fn manager() -> TranslationResult<Arc<TranslateManager>> {
        Self::get_facade_root().map_err(|_| TranslationError::Unavailable)
    }

    pub fn driver(name: &str) -> TranslationResult<Arc<dyn Translator>> {
        Self::manager()?.driver(name)
    }

    pub async fn translate(
        text: impl Into<String>,
        target: &str,
    ) -> TranslationResult<Translation> {
        Self::translate_request(TranslationRequest::new(text, target)?).await
    }

    pub async fn translate_request(request: TranslationRequest) -> TranslationResult<Translation> {
        Self::manager()?.translate_request(request).await
    }
}
