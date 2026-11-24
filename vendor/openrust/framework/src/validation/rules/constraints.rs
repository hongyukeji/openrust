use crate::validation::{Rule, RuleSet, ValidationResult};

#[derive(Clone)]
pub(super) struct Constraints(
    pub(super) Vec<String>,
    Option<crate::validation::rule_compiler::CompilationError>,
);
impl Constraints {
    pub(super) fn new(rule: &str) -> Self {
        Self(vec![rule.to_owned()], None)
    }
    pub(super) fn push(&mut self, rule: impl Into<String>) {
        let rule = rule.into();
        if !self.0.contains(&rule) {
            self.0.push(rule);
        }
    }
    pub(super) fn fail(&mut self, error: impl std::error::Error + Send + Sync + 'static) {
        self.1 = Some(
            crate::validation::ValidationError::Execution {
                source: Box::new(error),
            }
            .into(),
        );
    }
    pub(super) fn compile(&self, attribute: &str) -> ValidationResult<RuleSet> {
        if let Some(error) = &self.1 {
            return Err(error.error());
        }
        RuleSet::new().field(
            attribute,
            self.0
                .iter()
                .map(|rule| Rule::parse(rule))
                .collect::<ValidationResult<Vec<_>>>()?,
        )
    }
}
pub(super) fn parameters(values: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    values
        .into_iter()
        .map(|value| format!("\"{}\"", value.as_ref().replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",")
}
pub(super) fn joined_parameters(values: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    values
        .into_iter()
        .map(|value| value.as_ref().to_owned())
        .collect::<Vec<_>>()
        .join(",")
}
macro_rules! rule_builder {
    ($name:ident) => {
        impl $name {
            pub fn when(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
                if condition { callback(self) } else { self }
            }
            pub fn unless(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
                self.when(!condition, callback)
            }
            pub fn when_else(
                self,
                condition: bool,
                yes: impl FnOnce(Self) -> Self,
                no: impl FnOnce(Self) -> Self,
            ) -> Self {
                if condition { yes(self) } else { no(self) }
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.constraints.0.join("|"))
            }
        }
        impl $crate::contracts::validation::CompilableRules for $name {
            fn compile(
                &self,
                attribute: &str,
                _: Option<&$crate::validation::Value>,
                _: &serde_json::Map<String, $crate::validation::Value>,
                _: Option<&$crate::validation::Value>,
            ) -> $crate::validation::ValidationResult<$crate::validation::RuleSet> {
                self.constraints.compile(attribute)
            }
        }
        impl From<$name> for $crate::validation::Rule {
            fn from(rule: $name) -> Self {
                Self::compilable(rule)
            }
        }
    };
}
pub(super) use rule_builder;
