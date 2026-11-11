use std::fmt;

use crate::support::MessageBag;

/// A validation rejection with the ordered field errors.
#[derive(Clone, Debug)]
pub struct ValidationException {
    errors: MessageBag,
    message: String,
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
        let mut message = errors.first().map(str::to_owned).unwrap_or_else(|| {
            translated(DEFAULT, &Default::default()).unwrap_or_else(|| DEFAULT.into())
        });
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
        Self { errors, message }
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
