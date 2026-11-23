use std::fmt;

use crate::support::MessageBag;

/// A validation rejection with the ordered field errors.
#[derive(Clone, Debug)]
pub struct ValidationException {
    errors: MessageBag,
    message: String,
    error_bag: Option<String>,
    redirect_target: Option<String>,
    status: crate::http::StatusCode,
}

impl ValidationException {
    pub fn new(errors: MessageBag) -> Self {
        Self::localized(errors, None)
    }

    pub(super) fn localized(
        errors: MessageBag,
        translator: Option<&crate::translation::Translator>,
    ) -> Self {
        const DEFAULT: &str = "The given data was invalid.";
        let translated = |key, replacements: &std::collections::BTreeMap<String, String>| {
            translator.and_then(|translator| translator.string(key, replacements, None).ok())
        };
        let mut message = errors.first(None, None);
        if message.is_empty() {
            message = translated(DEFAULT, &Default::default()).unwrap_or_else(|| DEFAULT.into());
        }
        let more = errors.len().saturating_sub(1);
        if more > 0 {
            let key = if more == 1 {
                "(and :count more error)"
            } else {
                "(and :count more errors)"
            };
            let replacements =
                std::collections::BTreeMap::from([("count".into(), more.to_string())]);
            let suffix = translated(key, &replacements)
                .unwrap_or_else(|| key.replace(":count", &more.to_string()));
            message.push(' ');
            message.push_str(&suffix);
        }
        Self {
            errors,
            message,
            error_bag: None,
            redirect_target: None,
            status: crate::http::StatusCode::UNPROCESSABLE_ENTITY,
        }
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn errorBag(&mut self, name: impl Into<String>) -> &mut Self {
        self.error_bag = Some(name.into());
        self
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn redirectTo(&mut self, path: impl Into<String>) -> &mut Self {
        self.redirect_target = Some(path.into());
        self
    }

    pub(crate) fn error_bag(&self) -> &str {
        self.error_bag.as_deref().unwrap_or("default")
    }

    pub(crate) fn redirect_target(&self) -> Option<&str> {
        self.redirect_target.as_deref()
    }

    /// Selects the status used by the JSON validation response.
    pub fn status(&mut self, status: crate::http::StatusCode) -> &mut Self {
        self.status = status;
        self
    }

    #[allow(non_snake_case, reason = "Matches the validation response API")]
    pub fn statusCode(&self) -> crate::http::StatusCode {
        self.status
    }

    pub fn errors(&self) -> &MessageBag {
        &self.errors
    }
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ValidationException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ValidationException {}
