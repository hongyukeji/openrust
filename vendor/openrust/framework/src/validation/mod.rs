//! Validation rules, results, and error reporting.

mod active_url;
mod current_password;
mod data;
mod database_presence_verifier;
mod date_rules;
mod extensions;
pub(crate) mod facade;
mod factory;
mod path;
mod rule;
mod rule_parameters;
mod rule_set;
pub mod rules;
mod scalar_rules;
pub(crate) mod unknown_fields;
mod validated_input;
mod validation_exception;
mod validation_service_provider;
mod validator;

use std::{error::Error, future::Future, pin::Pin};

pub use data::Data;
pub use database_presence_verifier::DatabasePresenceVerifier;
pub use extensions::{ValidationExtension, ValidationReplacer, ValidatorResolver};
pub use factory::Factory;
pub use rule::Rule;
pub use rule_set::RuleSet;
pub use validated_input::ValidatedInput;
pub use validation_exception::ValidationException;
pub use validation_service_provider::ValidationServiceProvider;
pub use validator::{ValidationExceptionFactory, Validator};

/// A validation operation that may perform asynchronous I/O.
pub type ValidationFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of a validation operation.
pub type ValidationResult<T> = Result<T, ValidationError>;

/// Invalid user input or a failure while running validation.
#[derive(Debug)]
pub enum ValidationError {
    Invalid(Box<ValidationException>),
    InvalidPath,
    InvalidRule,
    NotValidated,
    LimitsExceeded,
    Execution {
        source: Box<dyn Error + Send + Sync>,
    },
}

/// A validation input value. Missing input is represented separately.
pub type Value = serde_json::Value;

/// The data available to a custom validation rule.
pub struct RuleContext<'a> {
    pub data: &'a Data,
    pub(crate) translator: Option<&'a crate::translation::Translator>,
}

/// A custom rule failure with a translation key or literal message.
pub struct RuleFailure {
    pub message: String,
    pub replacements: std::collections::BTreeMap<String, String>,
    /// 已显式翻译的消息不再经过通用规则消息覆盖。
    pub translated: bool,
}

impl RuleFailure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            replacements: Default::default(),
            translated: false,
        }
    }

    /// 当场借用验证器的翻译器，不改变应用或请求语言。
    pub fn translate(
        mut self,
        context: &RuleContext<'_>,
        replacements: std::collections::BTreeMap<String, String>,
        locale: Option<&str>,
    ) -> Self {
        self.message = context
            .translator
            .and_then(|translator| translator.string(&self.message, &replacements, locale).ok())
            .unwrap_or_else(|| validator::replace(&self.message, &replacements));
        // 参数已在翻译阶段替换，最终只处理验证器字段占位符，避免二次展开。
        self.replacements.clear();
        self.translated = true;
        self
    }

    /// 当场选择复数分支，复用语言模块的选择与替换规则。
    pub fn translate_choice(
        mut self,
        context: &RuleContext<'_>,
        count: impl Into<crate::translation::PluralCount>,
        replacements: std::collections::BTreeMap<String, String>,
        locale: Option<&str>,
    ) -> Self {
        self.message = context
            .translator
            .and_then(|translator| {
                translator
                    .choose(&self.message, count, &replacements, locale)
                    .ok()
            })
            .unwrap_or_else(|| validator::replace(&self.message, &replacements));
        // 参数已在翻译阶段替换，最终只处理验证器字段占位符，避免二次展开。
        self.replacements.clear();
        self.translated = true;
        self
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(exception) => exception.fmt(f),
            Self::InvalidPath => f.write_str("invalid validation field path"),
            Self::InvalidRule => f.write_str("invalid validation rule"),
            Self::NotValidated => f.write_str("validation has not completed"),
            Self::LimitsExceeded => f.write_str("validation input exceeds its limits"),
            Self::Execution { .. } => f.write_str("validation could not be executed"),
        }
    }
}

impl Error for ValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Invalid(source) => Some(source),
            Self::Execution { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}

mod standard_rules;

/// 拥有回调的 Rust 适配，避免表单在验证期间的重复可变借用。
pub type AfterValidation = Box<dyn Fn(&mut Validator) -> ValidationResult<()> + Send + Sync>;
