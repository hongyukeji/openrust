use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use super::{RuleFailure, ValidationResult, Validator, Value};

/// 注册回调可读取当前验证器；异步应用规则继续使用 ValidationRule。
pub type ValidationExtension = Arc<
    dyn Fn(&str, Option<&Value>, &[String], &Validator) -> ValidationResult<bool> + Send + Sync,
>;
pub type ValidationReplacer =
    Arc<dyn Fn(&str, &str, &str, &[String], &Validator) -> String + Send + Sync>;
/// Rust 使用拥有验证器的回调表达创建定制，不动态构造 PHP 子类。
pub type ValidatorResolver = Arc<dyn Fn(Validator) -> ValidationResult<Validator> + Send + Sync>;

#[derive(Clone, Default)]
pub(super) struct Extensions {
    pub callbacks: BTreeMap<String, ValidationExtension>,
    pub implicit: BTreeSet<String>,
    pub dependent: BTreeSet<String>,
    pub replacers: BTreeMap<String, ValidationReplacer>,
    pub fallback_messages: BTreeMap<String, String>,
}

pub(super) fn name(rule: &str) -> String {
    crate::support::Str::snake(rule, "_")
}

impl Validator {
    pub(super) fn extension_parameters(
        &self,
        rule: &str,
        parameters: &[String],
        pattern: &str,
        attribute: &str,
    ) -> ValidationResult<Vec<String>> {
        if self.extensions.dependent.contains(rule) {
            parameters
                .iter()
                .map(|parameter| super::validator::dependent_path(pattern, attribute, parameter))
                .collect()
        } else {
            Ok(parameters.to_vec())
        }
    }

    pub(super) fn evaluate_extension(
        &self,
        rule: &str,
        parameters: &[String],
        attribute: &str,
        value: Option<&Value>,
        nullable: bool,
    ) -> ValidationResult<Vec<RuleFailure>> {
        let implicit = self.extensions.implicit.contains(rule);
        if !implicit
            && (value.is_none()
                || (nullable && value == Some(&Value::Null))
                || value.is_some_and(|value| {
                    value.as_str().is_some_and(|value| {
                        value
                            .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                            .is_empty()
                    })
                }))
        {
            return Ok(Vec::new());
        }
        let callback = self
            .extensions
            .callbacks
            .get(rule)
            .ok_or(super::ValidationError::InvalidRule)?;
        Ok(
            if callback(
                &super::validator::unescape(attribute),
                value,
                parameters,
                self,
            )? {
                Vec::new()
            } else {
                vec![RuleFailure::new(format!("validation.{rule}"))]
            },
        )
    }
}

#[allow(non_snake_case)]
impl Validator {
    pub fn addExtension(
        &mut self,
        rule: &str,
        extension: impl Fn(&str, Option<&Value>, &[String], &Validator) -> ValidationResult<bool>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        let rule = name(rule);
        self.extensions
            .callbacks
            .insert(rule.clone(), Arc::new(extension));
        self.invalidate();
        self
    }
    pub fn addExtensions(
        &mut self,
        extensions: impl IntoIterator<Item = (String, ValidationExtension)>,
    ) -> &mut Self {
        for (rule, callback) in extensions {
            let rule = name(&rule);
            self.extensions.callbacks.insert(rule.clone(), callback);
        }
        self.invalidate();
        self
    }
    pub fn addImplicitExtension(
        &mut self,
        rule: &str,
        extension: impl Fn(&str, Option<&Value>, &[String], &Validator) -> ValidationResult<bool>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        let rule = name(rule);
        self.extensions
            .callbacks
            .insert(rule.clone(), Arc::new(extension));
        self.extensions.implicit.insert(rule);
        self.invalidate();
        self
    }
    pub fn addImplicitExtensions(
        &mut self,
        extensions: impl IntoIterator<Item = (String, ValidationExtension)>,
    ) -> &mut Self {
        for (rule, callback) in extensions {
            let rule = name(&rule);
            self.extensions.callbacks.insert(rule.clone(), callback);
            self.extensions.implicit.insert(rule);
        }
        self.invalidate();
        self
    }
    pub fn addDependentExtension(
        &mut self,
        rule: &str,
        extension: impl Fn(&str, Option<&Value>, &[String], &Validator) -> ValidationResult<bool>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        let rule = name(rule);
        self.extensions
            .callbacks
            .insert(rule.clone(), Arc::new(extension));
        self.extensions.dependent.insert(rule);
        self.invalidate();
        self
    }
    pub fn addDependentExtensions(
        &mut self,
        extensions: impl IntoIterator<Item = (String, ValidationExtension)>,
    ) -> &mut Self {
        for (rule, callback) in extensions {
            let rule = name(&rule);
            self.extensions.callbacks.insert(rule.clone(), callback);
            self.extensions.dependent.insert(rule);
        }
        self.invalidate();
        self
    }
    pub fn addReplacer(
        &mut self,
        rule: &str,
        replacer: impl Fn(&str, &str, &str, &[String], &Validator) -> String + Send + Sync + 'static,
    ) -> &mut Self {
        self.extensions
            .replacers
            .insert(name(rule), Arc::new(replacer));
        self.invalidate();
        self
    }
    pub fn addReplacers(
        &mut self,
        replacers: impl IntoIterator<Item = (String, ValidationReplacer)>,
    ) -> &mut Self {
        self.extensions.replacers.extend(
            replacers
                .into_iter()
                .map(|(rule, callback)| (name(&rule), callback)),
        );
        self.invalidate();
        self
    }
}
