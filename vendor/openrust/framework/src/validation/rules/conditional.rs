use crate::{
    contracts::validation::CompilableRules,
    validation::{
        Data, NestedRuleContext, Rule, RuleCondition, RuleSet, ValidationResult, Value, path,
        rule::Kind,
    },
};
#[derive(Clone)]
pub struct RequiredIf(RuleCondition);
impl RequiredIf {
    pub fn new(condition: impl Into<RuleCondition>) -> Self {
        Self(condition.into())
    }
    fn resolve(&self, attribute: &str, data: &Data) -> ValidationResult<RuleSet> {
        let enabled = self.0.evaluate(data)?;
        crate::validation::rule_compiler::bind(
            attribute,
            (if enabled {
                vec![Rule::new(Kind::Required)]
            } else {
                Vec::new()
            })
            .into(),
        )
    }
}
impl CompilableRules for RequiredIf {
    fn compile(
        &self,
        attribute: &str,
        _: Option<&Value>,
        data: &serde_json::Map<String, Value>,
        _: Option<&Value>,
    ) -> ValidationResult<RuleSet> {
        self.resolve(attribute, &Data::new(path::from_flat(data)?)?)
    }
    fn compile_context(&self, context: &NestedRuleContext<'_>) -> ValidationResult<RuleSet> {
        self.resolve(context.attribute, context.data)
    }
}
impl From<RequiredIf> for Rule {
    fn from(rule: RequiredIf) -> Self {
        Rule::compilable(rule)
    }
}
#[derive(Clone)]
pub struct RequiredUnless(RuleCondition);
impl RequiredUnless {
    pub fn new(condition: impl Into<RuleCondition>) -> Self {
        Self(condition.into())
    }
    fn resolve(&self, attribute: &str, data: &Data) -> ValidationResult<RuleSet> {
        let enabled = !self.0.evaluate(data)?;
        crate::validation::rule_compiler::bind(
            attribute,
            (if enabled {
                vec![Rule::new(Kind::Required)]
            } else {
                Vec::new()
            })
            .into(),
        )
    }
}
impl CompilableRules for RequiredUnless {
    fn compile(
        &self,
        attribute: &str,
        _: Option<&Value>,
        data: &serde_json::Map<String, Value>,
        _: Option<&Value>,
    ) -> ValidationResult<RuleSet> {
        self.resolve(attribute, &Data::new(path::from_flat(data)?)?)
    }
    fn compile_context(&self, context: &NestedRuleContext<'_>) -> ValidationResult<RuleSet> {
        self.resolve(context.attribute, context.data)
    }
}
impl From<RequiredUnless> for Rule {
    fn from(rule: RequiredUnless) -> Self {
        Rule::compilable(rule)
    }
}
#[derive(Clone)]
pub struct ExcludeIf(RuleCondition);
impl ExcludeIf {
    pub fn new(condition: impl Into<RuleCondition>) -> Self {
        Self(condition.into())
    }
    fn resolve(&self, attribute: &str, data: &Data) -> ValidationResult<RuleSet> {
        let enabled = self.0.evaluate(data)?;
        crate::validation::rule_compiler::bind(
            attribute,
            (if enabled {
                vec![Rule::new(Kind::Exclude)]
            } else {
                Vec::new()
            })
            .into(),
        )
    }
}
impl CompilableRules for ExcludeIf {
    fn compile(
        &self,
        attribute: &str,
        _: Option<&Value>,
        data: &serde_json::Map<String, Value>,
        _: Option<&Value>,
    ) -> ValidationResult<RuleSet> {
        self.resolve(attribute, &Data::new(path::from_flat(data)?)?)
    }
    fn compile_context(&self, context: &NestedRuleContext<'_>) -> ValidationResult<RuleSet> {
        self.resolve(context.attribute, context.data)
    }
}
impl From<ExcludeIf> for Rule {
    fn from(rule: ExcludeIf) -> Self {
        Rule::compilable(rule)
    }
}
#[derive(Clone)]
pub struct ExcludeUnless(RuleCondition);
impl ExcludeUnless {
    pub fn new(condition: impl Into<RuleCondition>) -> Self {
        Self(condition.into())
    }
    fn resolve(&self, attribute: &str, data: &Data) -> ValidationResult<RuleSet> {
        let enabled = !self.0.evaluate(data)?;
        crate::validation::rule_compiler::bind(
            attribute,
            (if enabled {
                vec![Rule::new(Kind::Exclude)]
            } else {
                Vec::new()
            })
            .into(),
        )
    }
}
impl CompilableRules for ExcludeUnless {
    fn compile(
        &self,
        attribute: &str,
        _: Option<&Value>,
        data: &serde_json::Map<String, Value>,
        _: Option<&Value>,
    ) -> ValidationResult<RuleSet> {
        self.resolve(attribute, &Data::new(path::from_flat(data)?)?)
    }
    fn compile_context(&self, context: &NestedRuleContext<'_>) -> ValidationResult<RuleSet> {
        self.resolve(context.attribute, context.data)
    }
}
impl From<ExcludeUnless> for Rule {
    fn from(rule: ExcludeUnless) -> Self {
        Rule::compilable(rule)
    }
}
#[derive(Clone)]
pub struct ProhibitedIf(RuleCondition);
impl ProhibitedIf {
    pub fn new(condition: impl Into<RuleCondition>) -> Self {
        Self(condition.into())
    }
    fn resolve(&self, attribute: &str, data: &Data) -> ValidationResult<RuleSet> {
        let enabled = self.0.evaluate(data)?;
        crate::validation::rule_compiler::bind(
            attribute,
            (if enabled {
                vec![Rule::new(Kind::Prohibited)]
            } else {
                Vec::new()
            })
            .into(),
        )
    }
}
impl CompilableRules for ProhibitedIf {
    fn compile(
        &self,
        attribute: &str,
        _: Option<&Value>,
        data: &serde_json::Map<String, Value>,
        _: Option<&Value>,
    ) -> ValidationResult<RuleSet> {
        self.resolve(attribute, &Data::new(path::from_flat(data)?)?)
    }
    fn compile_context(&self, context: &NestedRuleContext<'_>) -> ValidationResult<RuleSet> {
        self.resolve(context.attribute, context.data)
    }
}
impl From<ProhibitedIf> for Rule {
    fn from(rule: ProhibitedIf) -> Self {
        Rule::compilable(rule)
    }
}
#[derive(Clone)]
pub struct ProhibitedUnless(RuleCondition);
impl ProhibitedUnless {
    pub fn new(condition: impl Into<RuleCondition>) -> Self {
        Self(condition.into())
    }
    fn resolve(&self, attribute: &str, data: &Data) -> ValidationResult<RuleSet> {
        let enabled = !self.0.evaluate(data)?;
        crate::validation::rule_compiler::bind(
            attribute,
            (if enabled {
                vec![Rule::new(Kind::Prohibited)]
            } else {
                Vec::new()
            })
            .into(),
        )
    }
}
impl CompilableRules for ProhibitedUnless {
    fn compile(
        &self,
        attribute: &str,
        _: Option<&Value>,
        data: &serde_json::Map<String, Value>,
        _: Option<&Value>,
    ) -> ValidationResult<RuleSet> {
        self.resolve(attribute, &Data::new(path::from_flat(data)?)?)
    }
    fn compile_context(&self, context: &NestedRuleContext<'_>) -> ValidationResult<RuleSet> {
        self.resolve(context.attribute, context.data)
    }
}
impl From<ProhibitedUnless> for Rule {
    fn from(rule: ProhibitedUnless) -> Self {
        Rule::compilable(rule)
    }
}
