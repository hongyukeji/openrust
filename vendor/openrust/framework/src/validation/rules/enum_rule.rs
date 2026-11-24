use crate::{
    contracts::validation::ValidationRule,
    support::{BackedEnum, EnumBacking},
    validation::{Rule, RuleContext, RuleFailure, ValidationFuture, ValidationResult, Value},
};

/// 使用应用的原生枚举映射，保留请求输入的弱标量转换规则。
pub struct Enum<E> {
    only: Vec<E>,
    except: Vec<E>,
}
impl<E> Default for Enum<E> {
    fn default() -> Self {
        Self {
            only: Vec::new(),
            except: Vec::new(),
        }
    }
}
impl<E: BackedEnum + PartialEq> Enum<E> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn only(mut self, cases: impl IntoIterator<Item = E>) -> Self {
        self.only = cases.into_iter().collect();
        self
    }
    pub fn except(mut self, cases: impl IntoIterator<Item = E>) -> Self {
        self.except = cases.into_iter().collect();
        self
    }
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
    pub fn passes(&self, value: &Value) -> bool {
        if value.is_null() {
            return false;
        }
        E::Backing::from_input(value)
            .ok()
            .and_then(E::try_from_backing)
            .is_some_and(|case| self.passes_case(&case))
    }
    pub fn passes_case(&self, case: &E) -> bool {
        if !self.only.is_empty() {
            self.only.contains(case)
        } else {
            !self.except.contains(case)
        }
    }
}
impl<E: BackedEnum + PartialEq + Send + Sync> ValidationRule for Enum<E> {
    fn validate<'a>(
        &'a self,
        _: &'a RuleContext<'_>,
        _: &'a str,
        value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            Ok(if value.is_some_and(|value| self.passes(value)) {
                Vec::new()
            } else {
                vec![RuleFailure::new("validation.enum")]
            })
        })
    }
}
impl<E: BackedEnum + PartialEq + Send + Sync + 'static> From<Enum<E>> for Rule {
    fn from(rule: Enum<E>) -> Self {
        Self::custom(rule)
    }
}
