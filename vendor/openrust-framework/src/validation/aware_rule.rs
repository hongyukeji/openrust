use super::{
    Data, RuleContext, RuleFailure, ValidationFuture, ValidationResult, Validator, ValidatorHandle,
    Value, validator_handle,
};
use crate::{
    contracts::validation::{DataAwareRule, ValidationRule, ValidatorAwareRule},
    localization::Localizer,
};
use std::{collections::BTreeMap, sync::Arc};

pub(super) struct Snapshot {
    pub data: Arc<Data>,
    pub localizer: Option<Arc<Localizer>>,
    pub messages: BTreeMap<String, String>,
    pub attributes: BTreeMap<String, String>,
}
impl Snapshot {
    fn context<'a>(&'a self, attribute: &'a str, original_data: &'a Data) -> RuleContext<'a> {
        RuleContext {
            data: &self.data,
            original_data: Some(original_data),
            localizer: self.localizer.as_deref(),
            attribute,
            messages: &self.messages,
            attributes: &self.attributes,
        }
    }
}

pub(super) trait AwareValidationRule: Send + Sync {
    fn is_implicit(&self) -> bool;
    fn validate<'a>(
        &'a self,
        validator: &'a mut Validator,
        attribute: &'a str,
        value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>>;
}

type Setter<R> = fn(&mut R, &Data, ValidatorHandle) -> ValidationResult<()>;
pub(super) struct Aware<R> {
    rule: tokio::sync::Mutex<R>,
    setter: Setter<R>,
    implicit: bool,
}
impl<R: ValidationRule> Aware<R> {
    fn new(rule: R, setter: Setter<R>) -> Self {
        let implicit = rule.is_implicit();
        Self {
            rule: tokio::sync::Mutex::new(rule),
            setter,
            implicit,
        }
    }
}
impl<R: ValidationRule + DataAwareRule> Aware<R> {
    pub fn data(rule: R) -> Self {
        Self::new(rule, |rule, data, _| rule.set_data(data))
    }
}
impl<R: ValidationRule + ValidatorAwareRule> Aware<R> {
    pub fn validator(rule: R) -> Self {
        Self::new(rule, |rule, _, validator| rule.set_validator(validator))
    }
}
impl<R: ValidationRule + DataAwareRule + ValidatorAwareRule> Aware<R> {
    pub fn both(rule: R) -> Self {
        Self::new(rule, |rule, data, validator| {
            rule.set_data(data)?;
            rule.set_validator(validator)
        })
    }
}
impl<R: ValidationRule> AwareValidationRule for Aware<R> {
    fn is_implicit(&self) -> bool {
        self.implicit
    }
    fn validate<'a>(
        &'a self,
        validator: &'a mut Validator,
        attribute: &'a str,
        value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            let mut rule = self.rule.lock().await;
            validator_handle::resolving(validator, |handle| async move {
                let data = handle.with(|validator| Ok(validator.aware_data()))?;
                (self.setter)(&mut rule, &data, handle.clone())?;
                let snapshot = handle.with(|validator| Ok(validator.aware_snapshot()))?;
                rule.validate(
                    &snapshot.context(attribute, &data),
                    &super::validator::custom_rule_attribute(attribute),
                    value,
                )
                .await
            })
            .await
        })
    }
}
