use super::{Rule, ValidationError, ValidationResult, path};

/// Field rules in declaration order.
#[derive(Clone, Default)]
pub struct RuleSet(pub(super) Vec<(String, Vec<Rule>)>);

impl RuleSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn field(
        mut self,
        attribute: impl Into<String>,
        rules: impl IntoIterator<Item = Rule>,
    ) -> ValidationResult<Self> {
        let attribute = attribute.into();
        path::parse(&attribute)?;
        if self.0.len() >= 1024 || self.0.iter().any(|(field, _)| field == &attribute) {
            return Err(ValidationError::InvalidRule);
        }
        let rules: Vec<_> = rules.into_iter().take(65).collect();
        if rules.is_empty() || rules.len() > 64 {
            return Err(ValidationError::InvalidRule);
        }
        self.0.push((attribute, rules));
        Ok(self)
    }

    pub fn parse<'a>(
        declarations: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> ValidationResult<Self> {
        declarations
            .into_iter()
            .try_fold(Self::new(), |rules, (attribute, spec)| {
                if spec.len() > 4096 {
                    return Err(ValidationError::LimitsExceeded);
                }
                let field = spec
                    .split('|')
                    .map(Rule::parse)
                    .collect::<ValidationResult<Vec<_>>>()?;
                rules.field(attribute, field)
            })
    }
}
