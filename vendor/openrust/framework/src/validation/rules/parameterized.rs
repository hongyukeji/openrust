use super::constraints::parameters;
use crate::{
    contracts::validation::{CompilableRules, ParameterizedRule},
    validation::{Rule, RuleSet, ValidationResult, Value},
};

pub(in crate::validation) struct Parameterized<T>(pub(in crate::validation) T);
impl<T: ParameterizedRule> CompilableRules for Parameterized<T> {
    fn compile(
        &self,
        attribute: &str,
        _: Option<&Value>,
        _: &serde_json::Map<String, Value>,
        _: Option<&Value>,
    ) -> ValidationResult<RuleSet> {
        let values = self.0.to_array();
        let Some((name, values)) = values.split_first() else {
            return Err(crate::validation::ValidationError::InvalidRule);
        };
        let rule = if values.is_empty() {
            name.to_owned()
        } else if name == "regex" || name == "not_regex" {
            format!("{name}:{}", values[0])
        } else {
            format!("{name}:{}", parameters(values))
        };
        RuleSet::new().field(attribute, [Rule::parse(&rule)?])
    }
}
macro_rules! typed_parameterized {
    ($name:ident) => {
        impl From<$name> for Rule {
            fn from(rule: $name) -> Self {
                Self::parameterized(rule)
            }
        }
    };
}
#[derive(Clone, Default)]
pub struct Url {
    protocols: Vec<String>,
}
impl Url {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn protocols(mut self, protocols: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.protocols = protocols.into_iter().map(Into::into).collect();
        self
    }
}
impl ParameterizedRule for Url {
    fn to_array(&self) -> Vec<String> {
        std::iter::once("url".to_owned())
            .chain(self.protocols.clone())
            .collect()
    }
}
typed_parameterized!(Url);
#[derive(Clone, Default)]
pub struct Uuid {
    version: Option<String>,
}
impl Uuid {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn version(mut self, version: impl ToString) -> Self {
        self.version = Some(version.to_string());
        self
    }
}
impl ParameterizedRule for Uuid {
    fn to_array(&self) -> Vec<String> {
        std::iter::once("uuid".to_owned())
            .chain(self.version.clone())
            .collect()
    }
}
typed_parameterized!(Uuid);
#[derive(Clone, Default)]
pub struct IpAddress {
    version: Option<u8>,
}
impl IpAddress {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn version(mut self, version: u8) -> ValidationResult<Self> {
        if ![4, 6].contains(&version) {
            return Err(crate::validation::ValidationError::InvalidRule);
        }
        self.version = Some(version);
        Ok(self)
    }
}
impl ParameterizedRule for IpAddress {
    fn to_array(&self) -> Vec<String> {
        vec![
            self.version
                .map_or_else(|| "ip".to_owned(), |v| format!("ipv{v}")),
        ]
    }
}
typed_parameterized!(IpAddress);
#[derive(Clone, Default)]
pub struct Distinct {
    strict: bool,
    ignore_case: bool,
}
impl Distinct {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn strict(mut self) -> Self {
        self.strict = true;
        self
    }
    pub fn ignore_case(mut self) -> Self {
        self.ignore_case = true;
        self
    }
}
impl ParameterizedRule for Distinct {
    fn to_array(&self) -> Vec<String> {
        let mut values = vec!["distinct".to_owned()];
        if self.strict {
            values.push("strict".to_owned())
        }
        if self.ignore_case {
            values.push("ignore_case".to_owned())
        }
        values
    }
}
typed_parameterized!(Distinct);
#[derive(Clone, Default)]
pub struct Timezone {
    arguments: Vec<String>,
}
impl Timezone {
    pub fn new(arguments: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            arguments: arguments.into_iter().map(Into::into).collect(),
        }
    }
}
impl ParameterizedRule for Timezone {
    fn to_array(&self) -> Vec<String> {
        std::iter::once("timezone".to_owned())
            .chain(self.arguments.clone())
            .collect()
    }
}
typed_parameterized!(Timezone);
