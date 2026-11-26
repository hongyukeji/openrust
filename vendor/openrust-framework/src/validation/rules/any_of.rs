use crate::{
    contracts::validation::{ValidationRule, Validator as _},
    support::facades::Validator,
    validation::{
        Rule, RuleContext, RuleFailure, RuleSet, ValidationFuture, ValidationResult, Value,
    },
};

/// 一个备选组：同一标量的规则列表，或关联值的字段规则。
#[derive(Clone)]
pub enum AnyOfRules {
    Value(Vec<Rule>),
    Fields(RuleSet),
}
impl From<Vec<Rule>> for AnyOfRules {
    fn from(rules: Vec<Rule>) -> Self {
        Self::Value(rules)
    }
}
impl From<RuleSet> for AnyOfRules {
    fn from(rules: RuleSet) -> Self {
        Self::Fields(rules)
    }
}

/// 任意一组完整通过即可；各组复用当前应用的验证器工厂。
#[derive(Clone)]
pub struct AnyOf {
    rules: Vec<AnyOfRules>,
}
impl AnyOf {
    pub fn new<T: Into<AnyOfRules>>(rules: impl IntoIterator<Item = T>) -> Self {
        Self {
            rules: rules.into_iter().map(Into::into).collect(),
        }
    }
}
impl ValidationRule for AnyOf {
    fn validate<'a>(
        &'a self,
        context: &'a RuleContext<'_>,
        _attribute: &'a str,
        value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            let value = value.unwrap_or(&Value::Null);
            for alternative in &self.rules {
                let (input, rules, wrapped) = match alternative {
                    AnyOfRules::Value(rules) => {
                        let set = if rules.is_empty() {
                            RuleSet::new()
                        } else {
                            RuleSet::new().field("0", rules.clone())?
                        };
                        (Value::Array(vec![value.clone()]), set, true)
                    }
                    AnyOfRules::Fields(rules) => {
                        let keyed = rules
                            .fields
                            .iter()
                            .enumerate()
                            .any(|(i, (field, _))| field != &i.to_string());
                        if keyed {
                            let associative = value.as_object().is_some_and(|fields| {
                                fields
                                    .keys()
                                    .enumerate()
                                    .any(|(i, key)| key != &i.to_string())
                            });
                            let input = if associative {
                                value.clone()
                            } else {
                                Value::Array(vec![value.clone()])
                            };
                            (input, rules.clone(), !associative)
                        } else {
                            let list: Vec<_> = rules
                                .fields
                                .iter()
                                .flat_map(|(_, rules)| rules.iter().cloned())
                                .collect();
                            let set = if list.is_empty() {
                                RuleSet::new()
                            } else {
                                RuleSet::new().field("0", list)?
                            };
                            (Value::Array(vec![value.clone()]), set, true)
                        }
                    }
                };
                let mut validator = Validator::make_with(
                    context.data.scope(context.attribute, input, wrapped)?,
                    rules,
                    context.messages.clone(),
                    context.attributes.clone(),
                )?;
                if !validator.fails().await? {
                    return Ok(Vec::new());
                }
            }
            Ok(vec![RuleFailure::new("validation.any_of")])
        })
    }
}
impl From<AnyOf> for Rule {
    fn from(rule: AnyOf) -> Self {
        Self::custom(rule)
    }
}
