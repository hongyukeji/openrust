use super::{Data, Rule, ValidationResult, rule::Kind};
use std::sync::Arc;

type Condition = dyn Fn(&Data) -> ValidationResult<bool> + Send + Sync;
/// 条件在规则编译时求值，不在每次字段验证时重复执行。
#[derive(Clone)]
pub struct RuleCondition(Arc<Condition>);
impl RuleCondition {
    pub fn callback(
        callback: impl Fn(&Data) -> ValidationResult<bool> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(callback))
    }
    pub fn lazy(callback: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self::callback(move |_| Ok(callback()))
    }
    pub(super) fn evaluate(&self, data: &Data) -> ValidationResult<bool> {
        (self.0)(data)
    }
}
impl From<bool> for RuleCondition {
    fn from(value: bool) -> Self {
        Self::callback(move |_| Ok(value))
    }
}
#[derive(Clone)]
pub struct ConditionalRules {
    condition: RuleCondition,
    yes: Vec<Rule>,
    no: Vec<Rule>,
    invert: bool,
}
impl ConditionalRules {
    pub fn new(condition: impl Into<RuleCondition>, rules: Vec<Rule>, default: Vec<Rule>) -> Self {
        Self {
            condition: condition.into(),
            yes: rules,
            no: default,
            invert: false,
        }
    }
    pub fn unless(
        condition: impl Into<RuleCondition>,
        rules: Vec<Rule>,
        default: Vec<Rule>,
    ) -> Self {
        Self {
            invert: true,
            ..Self::new(condition, rules, default)
        }
    }
    pub fn passes(&self, data: &Data) -> ValidationResult<bool> {
        Ok(self.condition.evaluate(data)? != self.invert)
    }
    pub fn rules(&self) -> &[Rule] {
        &self.yes
    }
    pub fn default_rules(&self) -> &[Rule] {
        &self.no
    }
    pub(super) fn resolve(&self, data: &Data) -> ValidationResult<&[Rule]> {
        Ok(if self.passes(data)? {
            &self.yes
        } else {
            &self.no
        })
    }
}
impl From<ConditionalRules> for Rule {
    fn from(rule: ConditionalRules) -> Self {
        Self::new(Kind::ConditionalRules(rule))
    }
}
