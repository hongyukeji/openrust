use super::constraints::{Constraints, joined_parameters as parameters, rule_builder};
#[derive(Clone)]
pub struct Numeric {
    constraints: Constraints,
}
impl Default for Numeric {
    fn default() -> Self {
        Self::new(false)
    }
}
impl Numeric {
    pub fn new(strict: bool) -> Self {
        Self {
            constraints: Constraints::new(if strict { "numeric:strict" } else { "numeric" }),
        }
    }
    pub fn min(mut self, value: impl Into<bigdecimal::BigDecimal>) -> Self {
        self.constraints.push(format!("min:{}", value.into()));
        self
    }
    pub fn max(mut self, value: impl Into<bigdecimal::BigDecimal>) -> Self {
        self.constraints.push(format!("max:{}", value.into()));
        self
    }
    pub fn multiple_of(mut self, value: impl Into<bigdecimal::BigDecimal>) -> Self {
        self.constraints
            .push(format!("multiple_of:{}", value.into()));
        self
    }
    pub fn digits(mut self, value: usize) -> Self {
        self.constraints.push("integer");
        self.constraints.push(format!("digits:{value}"));
        self
    }
    pub fn max_digits(mut self, value: usize) -> Self {
        self.constraints.push(format!("max_digits:{value}"));
        self
    }
    pub fn min_digits(mut self, value: usize) -> Self {
        self.constraints.push(format!("min_digits:{value}"));
        self
    }
    pub fn exactly(mut self, value: usize) -> Self {
        self.constraints.push("integer");
        self.constraints.push(format!("size:{value}"));
        self
    }
    pub fn between(
        mut self,
        min: impl Into<bigdecimal::BigDecimal>,
        max: impl Into<bigdecimal::BigDecimal>,
    ) -> Self {
        self.constraints
            .push(format!("between:{},{}", min.into(), max.into()));
        self
    }
    pub fn digits_between(mut self, min: usize, max: usize) -> Self {
        self.constraints.push("integer");
        self.constraints.push(format!("digits_between:{min},{max}"));
        self
    }
    pub fn decimal(mut self, min: usize, max: Option<usize>) -> Self {
        self.constraints.push(max.map_or_else(
            || format!("decimal:{min}"),
            |max| format!("decimal:{min},{max}"),
        ));
        self
    }
    pub fn integer(mut self, strict: bool) -> Self {
        self.constraints
            .push(if strict { "integer:strict" } else { "integer" });
        self
    }
    pub fn different(mut self, field: &str) -> Self {
        self.constraints
            .push(format!("different:{}", parameters([field])));
        self
    }
    pub fn same(mut self, field: &str) -> Self {
        self.constraints
            .push(format!("same:{}", parameters([field])));
        self
    }
    pub fn greater_than(mut self, field: &str) -> Self {
        self.constraints.push(format!("gt:{}", parameters([field])));
        self
    }
    pub fn greater_than_or_equal_to(mut self, field: &str) -> Self {
        self.constraints
            .push(format!("gte:{}", parameters([field])));
        self
    }
    pub fn less_than(mut self, field: &str) -> Self {
        self.constraints.push(format!("lt:{}", parameters([field])));
        self
    }
    pub fn less_than_or_equal_to(mut self, field: &str) -> Self {
        self.constraints
            .push(format!("lte:{}", parameters([field])));
        self
    }
}
rule_builder!(Numeric);
