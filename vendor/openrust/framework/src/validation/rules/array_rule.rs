use super::constraints::{Constraints, joined_parameters as parameters, rule_builder};
#[derive(Clone)]
pub struct ArrayRule {
    constraints: Constraints,
}
impl Default for ArrayRule {
    fn default() -> Self {
        Self::new(std::iter::empty::<&str>())
    }
}
impl ArrayRule {
    pub fn new(keys: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let keys = parameters(keys);
        Self {
            constraints: Constraints::new(&if keys.is_empty() {
                "array".to_owned()
            } else {
                format!("array:{keys}")
            }),
        }
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
}
rule_builder!(ArrayRule);
