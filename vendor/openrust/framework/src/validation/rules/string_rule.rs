use super::constraints::{Constraints, joined_parameters as parameters, rule_builder};
#[derive(Clone)]
pub struct StringRule {
    constraints: Constraints,
}
impl Default for StringRule {
    fn default() -> Self {
        Self::new()
    }
}
impl StringRule {
    pub(in crate::validation) fn into_constraints(mut self) -> crate::validation::Rule {
        self.constraints.0.remove(0);
        crate::validation::Rule::compilable(self)
    }
    pub fn new() -> Self {
        Self {
            constraints: Constraints::new("string"),
        }
    }
    pub fn alpha(mut self, ascii: bool) -> Self {
        self.constraints
            .push(if ascii { "alpha:ascii" } else { "alpha" });
        self
    }
    pub fn alpha_dash(mut self, ascii: bool) -> Self {
        self.constraints.push(if ascii {
            "alpha_dash:ascii"
        } else {
            "alpha_dash"
        });
        self
    }
    pub fn alpha_numeric(mut self, ascii: bool) -> Self {
        self.constraints.push(if ascii {
            "alpha_num:ascii"
        } else {
            "alpha_num"
        });
        self
    }
    pub fn ascii(mut self) -> Self {
        self.constraints.push("ascii");
        self
    }
    pub fn lowercase(mut self) -> Self {
        self.constraints.push("lowercase");
        self
    }
    pub fn uppercase(mut self) -> Self {
        self.constraints.push("uppercase");
        self
    }
    pub fn min(mut self, value: usize) -> Self {
        self.constraints.push(format!("min:{value}"));
        self
    }
    pub fn max(mut self, value: usize) -> Self {
        self.constraints.push(format!("max:{value}"));
        self
    }
    pub fn exactly(mut self, value: usize) -> Self {
        self.constraints.push(format!("size:{value}"));
        self
    }
    pub fn between(mut self, min: usize, max: usize) -> Self {
        self.constraints.push(format!("between:{min},{max}"));
        self
    }
    pub fn starts_with(mut self, values: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.constraints
            .push(format!("starts_with:{}", parameters(values)));
        self
    }
    pub fn ends_with(mut self, values: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.constraints
            .push(format!("ends_with:{}", parameters(values)));
        self
    }
    pub fn doesnt_start_with(mut self, values: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.constraints
            .push(format!("doesnt_start_with:{}", parameters(values)));
        self
    }
    pub fn doesnt_end_with(mut self, values: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.constraints
            .push(format!("doesnt_end_with:{}", parameters(values)));
        self
    }
}
rule_builder!(StringRule);
