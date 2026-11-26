use std::{fmt, future::Future, pin::Pin};

use openrust::localization::canonical_locale;

use crate::{TranslationError, TranslationResult};

pub type TranslationFuture<'a> =
    Pin<Box<dyn Future<Output = TranslationResult<Translation>> + Send + 'a>>;

/// 一次单条纯文本翻译。
pub trait Translator: Send + Sync {
    fn translate(&self, request: TranslationRequest) -> TranslationFuture<'_>;
}

#[derive(Clone, PartialEq, Eq)]
pub struct TranslationRequest {
    text: String,
    source: Option<String>,
    target: String,
}

impl TranslationRequest {
    pub fn new(text: impl Into<String>, target: &str) -> TranslationResult<Self> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(TranslationError::EmptyText);
        }
        let target = canonical_locale(target).map_err(|_| TranslationError::InvalidLanguage)?;
        Ok(Self {
            text,
            source: None,
            target,
        })
    }

    /// 指定源语言。未指定时请求服务商自动识别。
    pub fn with_source(mut self, source: &str) -> TranslationResult<Self> {
        self.source =
            Some(canonical_locale(source).map_err(|_| TranslationError::InvalidLanguage)?);
        Ok(self)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn target(&self) -> &str {
        &self.target
    }
}

impl fmt::Debug for TranslationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TranslationRequest")
            .field("source", &self.source)
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Translation {
    text: String,
    detected_source: Option<String>,
}

impl Translation {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            detected_source: None,
        }
    }

    pub fn with_detected_source(mut self, source: &str) -> TranslationResult<Self> {
        self.detected_source =
            Some(canonical_locale(source).map_err(|_| TranslationError::InvalidResponse)?);
        Ok(self)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn into_text(self) -> String {
        self.text
    }

    pub fn detected_source(&self) -> Option<&str> {
        self.detected_source.as_deref()
    }
}

impl fmt::Debug for Translation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Translation")
            .field("detected_source", &self.detected_source)
            .finish_non_exhaustive()
    }
}
