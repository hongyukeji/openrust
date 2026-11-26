use crate::validation::{RuleContext, RuleFailure, ValidationFuture, ValidationResult, Value};

/// Checks an attribute and returns its violations, if any.
pub trait ValidationRule: Send + Sync {
    /// 隐式规则也检查缺失、空值和空字符串；是否拒绝由规则自行决定。
    fn is_implicit(&self) -> bool {
        false
    }

    fn validate<'a>(
        &'a self,
        context: &'a RuleContext<'_>,
        attribute: &'a str,
        value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>>;
}

/// 一次性同步规则无需声明额外类型；失败和异步规则使用同一执行管线。
impl<F> ValidationRule for F
where
    F: Fn(&RuleContext<'_>, &str, Option<&Value>) -> ValidationResult<Vec<RuleFailure>>
        + Send
        + Sync,
{
    fn validate<'a>(
        &'a self,
        context: &'a RuleContext<'_>,
        attribute: &'a str,
        value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move { self(context, attribute, value) })
    }
}
